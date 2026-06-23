// Integration test: build+spawn the real binary, talk to it over the socket.
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

fn temp_root(tag: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!("coordify-it-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn wait_for(path: &PathBuf, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if path.exists() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

fn read_token(root: &PathBuf) -> String {
    let p = root.join(".coordify/runtime/session.token");
    assert!(wait_for(&p, Duration::from_secs(5)), "token never written");
    std::fs::read_to_string(p).unwrap()
}

struct Spawned {
    child: Child,
    root: PathBuf,
}

impl Drop for Spawned {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn spawn_core(tag: &str) -> Spawned {
    let root = temp_root(tag);
    let child = Command::new(env!("CARGO_BIN_EXE_coordify-core"))
        .arg("--root")
        .arg(&root)
        .spawn()
        .expect("failed to spawn coordify-core");
    let sock = root.join(".coordify/runtime/core.sock");
    assert!(wait_for(&sock, Duration::from_secs(5)), "socket never appeared");
    Spawned { child, root }
}

fn spawn_core_fast_reaper(tag: &str) -> Spawned {
    let root = temp_root(tag);
    let child = Command::new(env!("CARGO_BIN_EXE_coordify-core"))
        .arg("--root")
        .arg(&root)
        .env("COORDIFY_REAPER_INTERVAL_MS", "100")
        .env("COORDIFY_REAPER_TIMEOUT_MS", "300")
        .spawn()
        .expect("failed to spawn coordify-core");
    let sock = root.join(".coordify/runtime/core.sock");
    assert!(wait_for(&sock, Duration::from_secs(5)), "socket never appeared");
    Spawned { child, root }
}

fn send_line(stream: &mut UnixStream, line: &str) -> serde_json::Value {
    stream.write_all(line.as_bytes()).unwrap();
    stream.write_all(b"\n").unwrap();
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut resp = String::new();
    reader.read_line(&mut resp).unwrap();
    serde_json::from_str(&resp).unwrap()
}

#[test]
fn register_and_heartbeat_over_socket() {
    let core = spawn_core("reg");
    let token = read_token(&core.root);
    let sock = core.root.join(".coordify/runtime/core.sock");
    let mut stream = UnixStream::connect(&sock).unwrap();

    let reg = format!(
        r#"{{"id":"1","token":"{}","action":"register","meta":{{"task":"auth"}}}}"#,
        token
    );
    let resp = send_line(&mut stream, &reg);
    assert_eq!(resp["ok"], true);
    let agent_id = resp["agent_id"].as_str().unwrap().to_string();
    assert!(agent_id.starts_with("agent-"));

    let hb = format!(
        r#"{{"id":"2","token":"{}","action":"heartbeat","agent_id":"{}"}}"#,
        token, agent_id
    );
    let resp = send_line(&mut stream, &hb);
    assert_eq!(resp["ok"], true);
}

#[test]
fn rejects_bad_token_over_socket() {
    let core = spawn_core("badtok");
    let sock = core.root.join(".coordify/runtime/core.sock");
    let mut stream = UnixStream::connect(&sock).unwrap();
    let reg = r#"{"id":"1","token":"WRONG","action":"register","meta":{}}"#;
    let resp = send_line(&mut stream, reg);
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["error"], "unauthorized");
}

#[test]
fn last_agent_leaving_finalizes_session() {
    let core = spawn_core("final");
    let token = read_token(&core.root);
    let sock = core.root.join(".coordify/runtime/core.sock");

    {
        let mut stream = UnixStream::connect(&sock).unwrap();
        let reg = format!(
            r#"{{"id":"1","token":"{}","action":"register","meta":{{}}}}"#,
            token
        );
        let resp = send_line(&mut stream, &reg);
        assert_eq!(resp["ok"], true);
        // Drop the stream -> agent leaves -> network empty -> finalize.
    }

    // network-final.json should appear under some session dir.
    let sessions = core.root.join(".coordify/sessions");
    let start = Instant::now();
    let mut found = false;
    while start.elapsed() < Duration::from_secs(5) {
        if let Ok(entries) = std::fs::read_dir(&sessions) {
            for e in entries.flatten() {
                if e.path().join("network-final.json").exists() {
                    found = true;
                }
            }
        }
        if found {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(found, "session was not finalized after last agent left");
    // Lock should be gone after finalize.
    assert!(!core.root.join(".coordify/runtime/core.lock").exists());
}

#[test]
fn reaper_logs_agent_lost_for_silent_agent() {
    let core = spawn_core_fast_reaper("reap");
    let token = read_token(&core.root);
    let sock = core.root.join(".coordify/runtime/core.sock");

    // Register, then keep the connection OPEN but send no heartbeats so the
    // reaper times the agent out while it is still "connected".
    let mut stream = UnixStream::connect(&sock).unwrap();
    let reg = format!(
        r#"{{"id":"1","token":"{}","action":"register","meta":{{}}}}"#,
        token
    );
    let resp = send_line(&mut stream, &reg);
    assert_eq!(resp["ok"], true);

    // Wait past the 300ms timeout + a reaper tick.
    std::thread::sleep(Duration::from_millis(700));

    // Find the events.log and assert AGENT_LOST + CLAIM_ORPHANED are present.
    let sessions = core.root.join(".coordify/sessions");
    let mut log_contents = String::new();
    if let Ok(entries) = std::fs::read_dir(&sessions) {
        for e in entries.flatten() {
            let log = e.path().join("events.log");
            if log.exists() {
                log_contents = std::fs::read_to_string(log).unwrap();
            }
        }
    }
    assert!(log_contents.contains("AGENT_LOST"), "no AGENT_LOST event logged");
    assert!(log_contents.contains("CLAIM_ORPHANED"), "no CLAIM_ORPHANED event logged");

    // Keep the stream alive until assertions are done.
    drop(stream);
}

#[test]
fn reaper_finalizes_when_last_silent_agent_times_out() {
    let core = spawn_core_fast_reaper("rfin");
    let token = read_token(&core.root);
    let sock = core.root.join(".coordify/runtime/core.sock");
    let mut stream = UnixStream::connect(&sock).unwrap();
    let reg = format!(
        r#"{{"id":"1","token":"{}","action":"register","meta":{{}}}}"#,
        token
    );
    let resp = send_line(&mut stream, &reg);
    assert_eq!(resp["ok"], true);

    // Keep the stream OPEN and send no heartbeats. The reaper (300ms timeout)
    // should reap the silent agent, empty the network, finalize, and exit.
    let sessions = core.root.join(".coordify/sessions");
    let start = Instant::now();
    let mut finalized = false;
    while start.elapsed() < Duration::from_secs(5) {
        if let Ok(entries) = std::fs::read_dir(&sessions) {
            for e in entries.flatten() {
                if e.path().join("network-final.json").exists() {
                    finalized = true;
                }
            }
        }
        if finalized {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(finalized, "reaper did not finalize after last silent agent timed out");
    assert!(
        !core.root.join(".coordify/runtime/core.lock").exists(),
        "lock not removed by finalize"
    );
    drop(stream);
}
