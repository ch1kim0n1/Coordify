// Integration test: build+spawn the real binary, talk to it over the socket.
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

/// Write raw bytes to the stream (no newline appended).
fn write_raw(stream: &mut UnixStream, bytes: &[u8]) {
    stream.write_all(bytes).unwrap();
}

fn temp_root(tag: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!("coordify-it-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn wait_for(path: &Path, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if path.exists() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// Connect to the socket, retrying until the listener is actually accepting.
/// `UnixListener::bind` creates the socket file (bind syscall) a moment before
/// it calls listen, so the file existing is not sufficient readiness — a
/// connect in that window gets ECONNREFUSED. Retry the connect itself.
fn connect_retry(sock: &Path) -> UnixStream {
    let start = Instant::now();
    loop {
        match UnixStream::connect(sock) {
            Ok(s) => return s,
            Err(e) => {
                if start.elapsed() > Duration::from_secs(5) {
                    panic!("could not connect to {} within 5s: {e}", sock.display());
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }
}

fn read_token(root: &Path) -> String {
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

fn spawn_core_fast_proposal_timeout(tag: &str) -> Spawned {
    let root = temp_root(tag);
    let child = Command::new(env!("CARGO_BIN_EXE_coordify-core"))
        .arg("--root")
        .arg(&root)
        .env("COORDIFY_REAPER_INTERVAL_MS", "100")
        .env("COORDIFY_REAPER_TIMEOUT_MS", "60000") // keep agents alive past the proposal timeout
        .env("COORDIFY_PROPOSAL_TIMEOUT_MS", "200") // conflicts time out fast
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
    let mut stream = connect_retry(&sock);

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
    let mut stream = connect_retry(&sock);
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
        let mut stream = connect_retry(&sock);
        let reg = format!(
            r#"{{"id":"1","token":"{}","action":"register","meta":{{}}}}"#,
            token
        );
        let resp = send_line(&mut stream, &reg);
        assert_eq!(resp["ok"], true);
        // Drop the stream -> agent leaves -> network empty -> finalize.
    }

    // network-final.json should appear under some session dir, and the lock
    // should be removed. finalize() writes the summary before removing the
    // lock, so poll for BOTH to avoid observing the in-between window.
    let sessions = core.root.join(".coordify/sessions");
    let lock = core.root.join(".coordify/runtime/core.lock");
    let start = Instant::now();
    let mut found = false;
    let mut lock_gone = false;
    while start.elapsed() < Duration::from_secs(5) {
        if let Ok(entries) = std::fs::read_dir(&sessions) {
            for e in entries.flatten() {
                if e.path().join("network-final.json").exists() {
                    found = true;
                }
            }
        }
        lock_gone = !lock.exists();
        if found && lock_gone {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(found, "session was not finalized after last agent left");
    assert!(lock_gone, "lock not removed after finalize");
}

#[test]
fn reaper_logs_agent_lost_and_orphans_claim() {
    let core = spawn_core_fast_reaper("reap");
    let token = read_token(&core.root);
    let sock = core.root.join(".coordify/runtime/core.sock");

    let mut stream = connect_retry(&sock);
    let reg = format!(r#"{{"id":"1","token":"{}","action":"register","meta":{{}}}}"#, token);
    let agent = send_line(&mut stream, &reg)["agent_id"].as_str().unwrap().to_string();
    let propose = format!(
        r#"{{"id":"2","token":"{}","action":"submit_event","capVersion":"0.1","event":{{"type":"CLAIM_PROPOSED","agentId":"{}","intent":"BUGFIX","confidence":0.9}}}}"#,
        token, agent
    );
    assert_eq!(send_line(&mut stream, &propose)["ok"], true);

    // Keep the connection open, send no heartbeats; reaper times the agent out.
    std::thread::sleep(Duration::from_millis(700));

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
    assert!(log_contents.contains("AGENT_LOST"), "no AGENT_LOST logged");
    assert!(log_contents.contains("CLAIM_ORPHANED"), "no CLAIM_ORPHANED logged");
    drop(stream);
}

#[test]
fn reaper_finalizes_when_last_silent_agent_times_out() {
    let core = spawn_core_fast_reaper("rfin");
    let token = read_token(&core.root);
    let sock = core.root.join(".coordify/runtime/core.sock");
    let mut stream = connect_retry(&sock);
    let reg = format!(
        r#"{{"id":"1","token":"{}","action":"register","meta":{{}}}}"#,
        token
    );
    let resp = send_line(&mut stream, &reg);
    assert_eq!(resp["ok"], true);

    // Keep the stream OPEN and send no heartbeats. The reaper (300ms timeout)
    // should reap the silent agent, empty the network, finalize, and exit.
    let sessions = core.root.join(".coordify/sessions");
    let lock = core.root.join(".coordify/runtime/core.lock");
    let start = Instant::now();
    let mut finalized = false;
    let mut lock_gone = false;
    // finalize() writes network-final.json FIRST, then removes the runtime
    // files (lock among them). Poll for BOTH so we never observe the window
    // where the summary exists but the lock is not yet removed.
    while start.elapsed() < Duration::from_secs(5) {
        if let Ok(entries) = std::fs::read_dir(&sessions) {
            for e in entries.flatten() {
                if e.path().join("network-final.json").exists() {
                    finalized = true;
                }
            }
        }
        lock_gone = !lock.exists();
        if finalized && lock_gone {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(finalized, "reaper did not finalize after last silent agent timed out");
    assert!(lock_gone, "lock not removed by finalize");
    drop(stream);
}

// ---------------------------------------------------------------------------
// Target B3: blank / empty line is silently skipped; a subsequent valid
// register still succeeds.
// ---------------------------------------------------------------------------
#[test]
fn blank_line_is_skipped_then_register_succeeds() {
    let core = spawn_core("blnk");
    let token = read_token(&core.root);
    let sock = core.root.join(".coordify/runtime/core.sock");
    let mut stream = connect_retry(&sock);

    // Send a bare newline (the blank line that should be skipped).
    write_raw(&mut stream, b"\n");

    // Now send a valid register and read back its response.
    let reg = format!(
        r#"{{"id":"1","token":"{}","action":"register","meta":{{}}}}"#,
        token
    );
    let resp = send_line(&mut stream, &reg);
    assert_eq!(resp["ok"], true, "register after blank line should succeed");
    assert!(resp["agent_id"].as_str().unwrap_or("").starts_with("agent-"));
}

// ---------------------------------------------------------------------------
// Target B4: malformed (non-JSON) line causes server to reply ok:false with
// error "malformed request".
// ---------------------------------------------------------------------------
#[test]
fn malformed_request_returns_error() {
    let core = spawn_core("malf");
    let sock = core.root.join(".coordify/runtime/core.sock");
    let mut stream = connect_retry(&sock);

    // Send something that is not valid JSON.
    stream.write_all(b"{not json\n").unwrap();

    // Read the single response line.
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut resp_line = String::new();
    reader.read_line(&mut resp_line).unwrap();
    let resp: serde_json::Value = serde_json::from_str(&resp_line).unwrap();
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["error"], "malformed request");
}

// ---------------------------------------------------------------------------
// Target B5: a connection that sends a bad-token register (never successfully
// registers) and then closes leaves the daemon alive; subsequent connections
// still work and the lock still exists.
// ---------------------------------------------------------------------------
#[test]
fn unregistered_connection_leaves_daemon_alive() {
    let core = spawn_core("noreg");
    let sock = core.root.join(".coordify/runtime/core.sock");

    // Connect, send a bad-token register, then drop the stream.
    {
        let mut stream = connect_retry(&sock);
        let bad_reg = r#"{"id":"1","token":"WRONG","action":"register","meta":{}}"#;
        let resp = send_line(&mut stream, bad_reg);
        assert_eq!(resp["ok"], false);
        // stream is dropped here (connection closed).
    }

    // Lock file must still be present — daemon did not finalize.
    assert!(
        core.root.join(".coordify/runtime/core.lock").exists(),
        "lock was removed even though no agent ever registered"
    );

    // A fresh connection must still work.
    let token = read_token(&core.root);
    let mut stream2 = connect_retry(&sock);
    let reg = format!(
        r#"{{"id":"2","token":"{}","action":"register","meta":{{}}}}"#,
        token
    );
    let resp2 = send_line(&mut stream2, &reg);
    assert_eq!(resp2["ok"], true, "daemon should still accept connections after unregistered drop");
}

// ---------------------------------------------------------------------------
// Target E9: if the lock is already held, a second instance prints a message
// and exits with code 0.
// ---------------------------------------------------------------------------
#[test]
fn second_instance_exits_zero_when_lock_held() {
    let core = spawn_core("held");
    // Give instance A a moment to finish writing the lock.
    let lock = core.root.join(".coordify/runtime/core.lock");
    assert!(wait_for(&lock, Duration::from_secs(5)), "lock never appeared");

    // Spawn instance B on the SAME root — it should detect the held lock and exit 0.
    let output = Command::new(env!("CARGO_BIN_EXE_coordify-core"))
        .arg("--root")
        .arg(&core.root)
        .output()
        .expect("failed to run second instance");
    assert!(
        output.status.success(),
        "second instance should exit 0 when lock is held, got {:?}",
        output.status
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("already running"),
        "expected 'already running' in stderr, got: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// Target E10: `--root` with no path argument → exit code 1 + stderr contains
// "requires a path".
// ---------------------------------------------------------------------------
#[test]
fn root_flag_without_value_exits_one() {
    let output = Command::new(env!("CARGO_BIN_EXE_coordify-core"))
        .arg("--root")
        .output()
        .expect("failed to run binary");
    assert_eq!(
        output.status.code(),
        Some(1),
        "expected exit code 1, got {:?}",
        output.status
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("requires a path"),
        "expected 'requires a path' in stderr, got: {stderr}"
    );
}

#[test]
fn claim_proposed_and_released_over_socket() {
    let core = spawn_core("claim");
    let token = read_token(&core.root);
    let sock = core.root.join(".coordify/runtime/core.sock");
    let mut stream = connect_retry(&sock);

    let reg = format!(r#"{{"id":"1","token":"{}","action":"register","meta":{{}}}}"#, token);
    let agent = send_line(&mut stream, &reg)["agent_id"].as_str().unwrap().to_string();

    let propose = format!(
        r#"{{"id":"2","token":"{}","action":"submit_event","capVersion":"0.1","event":{{"type":"CLAIM_PROPOSED","agentId":"{}","intent":"BUGFIX","confidence":0.9}}}}"#,
        token, agent
    );
    let resp = send_line(&mut stream, &propose);
    assert_eq!(resp["ok"], true);
    let claim_id = resp["data"]["claimId"].as_str().unwrap().to_string();
    assert_eq!(resp["data"]["status"], "ACTIVE");

    let release = format!(
        r#"{{"id":"3","token":"{}","action":"submit_event","capVersion":"0.1","event":{{"type":"CLAIM_RELEASED","claimId":"{}","agentId":"{}","reason":"TASK_COMPLETED"}}}}"#,
        token, claim_id, agent
    );
    assert_eq!(send_line(&mut stream, &release)["ok"], true);
}

#[test]
fn agent_state_change_over_socket() {
    let core = spawn_core("state");
    let token = read_token(&core.root);
    let sock = core.root.join(".coordify/runtime/core.sock");
    let mut stream = connect_retry(&sock);
    let reg = format!(r#"{{"id":"1","token":"{}","action":"register","meta":{{}}}}"#, token);
    let agent = send_line(&mut stream, &reg)["agent_id"].as_str().unwrap().to_string();

    let good = format!(
        r#"{{"id":"2","token":"{}","action":"submit_event","capVersion":"0.1","event":{{"type":"AGENT_STATE_CHANGED","agentId":"{}","state":"ACTIVE"}}}}"#,
        token, agent
    );
    assert_eq!(send_line(&mut stream, &good)["ok"], true);

    // Active -> WAITING_USER is not a legal direct transition.
    let bad = format!(
        r#"{{"id":"3","token":"{}","action":"submit_event","capVersion":"0.1","event":{{"type":"AGENT_STATE_CHANGED","agentId":"{}","state":"WAITING_USER"}}}}"#,
        token, agent
    );
    let resp = send_line(&mut stream, &bad);
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["error"], "INVALID_STATE_TRANSITION");
}

#[test]
fn clear_invoked_over_socket_releases_and_bumps_generation() {
    let core = spawn_core("clear");
    let token = read_token(&core.root);
    let sock = core.root.join(".coordify/runtime/core.sock");
    let mut stream = connect_retry(&sock);
    let reg = format!(r#"{{"id":"1","token":"{}","action":"register","meta":{{}}}}"#, token);
    let agent = send_line(&mut stream, &reg)["agent_id"].as_str().unwrap().to_string();

    let propose = format!(
        r#"{{"id":"2","token":"{}","action":"submit_event","capVersion":"0.1","event":{{"type":"CLAIM_PROPOSED","agentId":"{}","intent":"BUGFIX","confidence":0.9}}}}"#,
        token, agent
    );
    assert_eq!(send_line(&mut stream, &propose)["data"]["status"], "ACTIVE");

    let clear = format!(
        r#"{{"id":"3","token":"{}","action":"submit_event","capVersion":"0.1","event":{{"type":"CLEAR_INVOKED","agentId":"{}"}}}}"#,
        token, agent
    );
    let resp = send_line(&mut stream, &clear);
    assert_eq!(resp["ok"], true);
    assert_eq!(resp["data"]["generation"], 2);
}

#[test]
fn overlapping_claims_emit_heat_updated() {
    let core = spawn_core("heat");
    let token = read_token(&core.root);
    let sock = core.root.join(".coordify/runtime/core.sock");

    // The Phase-1 server accepts connections serially (join-per-connection), so
    // two agents must be registered on a single connection to be simultaneously
    // live.  Agent B is registered after A so that when A claims, B is already
    // in state; when B then claims, recompute_current_heat sees A's live claim
    // and emits HEAT_UPDATED + HEAT_THRESHOLD_EXCEEDED for the A<->B edge.
    let mut stream = connect_retry(&sock);

    let reg_a = format!(
        r#"{{"id":"1","token":"{}","action":"register","meta":{{"branch":"main"}}}}"#,
        token
    );
    let agent_a = send_line(&mut stream, &reg_a)["agent_id"]
        .as_str()
        .unwrap()
        .to_string();

    let reg_b = format!(
        r#"{{"id":"2","token":"{}","action":"register","meta":{{"branch":"main"}}}}"#,
        token
    );
    let agent_b = send_line(&mut stream, &reg_b)["agent_id"]
        .as_str()
        .unwrap()
        .to_string();

    // A claims — no heat edge yet (B has no live claim).
    let claim_a = format!(
        r#"{{"id":"3","token":"{}","action":"submit_event","capVersion":"0.1","event":{{"type":"CLAIM_PROPOSED","agentId":"{}","intent":"BUGFIX","domains":["AUTH"],"estimatedFiles":["src/auth/session.ts"],"task":{{"summary":"fix session expiry"}},"confidence":0.9}}}}"#,
        token, agent_a
    );
    assert_eq!(send_line(&mut stream, &claim_a)["ok"], true);

    // B claims — overlapping with A → heat edge A<->B emitted.
    let claim_b = format!(
        r#"{{"id":"4","token":"{}","action":"submit_event","capVersion":"0.1","event":{{"type":"CLAIM_PROPOSED","agentId":"{}","intent":"BUGFIX","domains":["AUTH"],"estimatedFiles":["src/auth/session.ts"],"task":{{"summary":"fix session expiry"}},"confidence":0.9}}}}"#,
        token, agent_b
    );
    assert_eq!(send_line(&mut stream, &claim_b)["ok"], true);

    drop(stream); // connection closed.

    // events.log should contain HEAT_UPDATED for the A<->B pair.
    let sessions = core.root.join(".coordify/sessions");
    let mut log_contents = String::new();
    // Poll until log is flushed (finalize can lag a moment).
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(3) {
        if let Ok(entries) = std::fs::read_dir(&sessions) {
            for e in entries.flatten() {
                let log = e.path().join("events.log");
                if log.exists() {
                    log_contents = std::fs::read_to_string(log).unwrap();
                }
            }
        }
        if log_contents.contains("HEAT_UPDATED") {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(log_contents.contains("HEAT_UPDATED"), "no HEAT_UPDATED logged");
    assert!(
        log_contents.contains("HEAT_THRESHOLD_EXCEEDED"),
        "expected threshold exceeded for high overlap"
    );
}

#[test]
fn predicted_heat_calculated_logged_on_second_overlapping_claim() {
    let core = spawn_core("pheat");
    let token = read_token(&core.root);
    let sock = core.root.join(".coordify/runtime/core.sock");

    // Both agents must share one connection so both remain live simultaneously
    // (the server finalizes when the last connected agent leaves; two separate
    // connections would cause the server to exit after A disconnects).
    let mut stream = connect_retry(&sock);

    let reg_a = format!(r#"{{"id":"1","token":"{}","action":"register","meta":{{"branch":"main"}}}}"#, token);
    let agent_a = send_line(&mut stream, &reg_a)["agent_id"].as_str().unwrap().to_string();

    let claim_a = format!(
        r#"{{"id":"2","token":"{}","action":"submit_event","capVersion":"0.1","event":{{"type":"CLAIM_PROPOSED","agentId":"{}","intent":"BUGFIX","domains":["AUTH"],"estimatedFiles":["src/auth/session.ts"],"task":{{"summary":"fix session expiry"}},"confidence":0.9}}}}"#,
        token, agent_a
    );
    assert_eq!(send_line(&mut stream, &claim_a)["ok"], true);

    let reg_b = format!(r#"{{"id":"3","token":"{}","action":"register","meta":{{"branch":"main"}}}}"#, token);
    let agent_b = send_line(&mut stream, &reg_b)["agent_id"].as_str().unwrap().to_string();
    let claim_b = format!(
        r#"{{"id":"4","token":"{}","action":"submit_event","capVersion":"0.1","event":{{"type":"CLAIM_PROPOSED","agentId":"{}","intent":"BUGFIX","domains":["AUTH"],"estimatedFiles":["src/auth/session.ts"],"task":{{"summary":"fix session expiry"}},"confidence":0.9}}}}"#,
        token, agent_b
    );
    let resp = send_line(&mut stream, &claim_b);
    assert_eq!(resp["ok"], true);
    assert_eq!(resp["data"]["recommendation"], "NEGOTIATE_BEFORE_CLAIM");

    drop(stream);

    let sessions = core.root.join(".coordify/sessions");
    let mut log_contents = String::new();
    let start = std::time::Instant::now();
    while start.elapsed() < std::time::Duration::from_secs(3) {
        if let Ok(entries) = std::fs::read_dir(&sessions) {
            for e in entries.flatten() {
                let log = e.path().join("events.log");
                if log.exists() {
                    log_contents = std::fs::read_to_string(log).unwrap();
                }
            }
        }
        if log_contents.contains("PREDICTED_HEAT_CALCULATED") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(log_contents.contains("PREDICTED_HEAT_CALCULATED"), "no PREDICTED_HEAT_CALCULATED logged");
}

// ---------------------------------------------------------------------------
// Target E11: a file at <root>/.coordify/runtime prevents create_dir_all from
// succeeding → acquire_lock errors → process exits 1.
// ---------------------------------------------------------------------------
#[test]
fn lock_acquisition_error_exits_one() {
    let root = temp_root("rtfile");
    // Create <root>/.coordify/ then write a plain file at
    // <root>/.coordify/runtime so create_dir_all(runtime) fails.
    std::fs::create_dir_all(root.join(".coordify")).unwrap();
    std::fs::write(root.join(".coordify/runtime"), b"I am a file").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_coordify-core"))
        .arg("--root")
        .arg(&root)
        .output()
        .expect("failed to run binary");
    assert_eq!(
        output.status.code(),
        Some(1),
        "expected exit code 1, got {:?}",
        output.status
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn high_overlap_claims_open_conflict() {
    let core = spawn_core("conflict");
    let token = read_token(&core.root);
    let sock = core.root.join(".coordify/runtime/core.sock");
    let mut stream = connect_retry(&sock);

    let reg_a = format!(r#"{{"id":"1","token":"{}","action":"register","meta":{{"branch":"main"}}}}"#, token);
    let a = send_line(&mut stream, &reg_a)["agent_id"].as_str().unwrap().to_string();
    let reg_b = format!(r#"{{"id":"2","token":"{}","action":"register","meta":{{"branch":"main"}}}}"#, token);
    let b = send_line(&mut stream, &reg_b)["agent_id"].as_str().unwrap().to_string();

    let mk = |id: &str, agent: &str| format!(
        r#"{{"id":"{}","token":"{}","action":"submit_event","capVersion":"0.1","event":{{"type":"CLAIM_PROPOSED","agentId":"{}","intent":"BUGFIX","domains":["AUTH"],"estimatedFiles":["src/auth/session.ts"],"task":{{"summary":"fix session expiry"}},"confidence":0.9}}}}"#,
        id, token, agent
    );
    assert_eq!(send_line(&mut stream, &mk("3", &a))["ok"], true);
    assert_eq!(send_line(&mut stream, &mk("4", &b))["ok"], true);

    drop(stream); // connection closed.

    let sessions = core.root.join(".coordify/sessions");
    let mut log_contents = String::new();
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(3) {
        if let Ok(entries) = std::fs::read_dir(&sessions) {
            for e in entries.flatten() {
                let log = e.path().join("events.log");
                if log.exists() {
                    log_contents = std::fs::read_to_string(log).unwrap();
                }
            }
        }
        if log_contents.contains("CONFLICT_OPENED") {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(log_contents.contains("CONFLICT_OPENED"), "no CONFLICT_OPENED logged for high-overlap pair");
}

#[test]
fn releasing_participant_aborts_conflict_over_socket() {
    let core = spawn_core("cabort");
    let token = read_token(&core.root);
    let sock = core.root.join(".coordify/runtime/core.sock");
    let mut stream = connect_retry(&sock);

    let reg_a = format!(r#"{{"id":"1","token":"{}","action":"register","meta":{{"branch":"main"}}}}"#, token);
    let a = send_line(&mut stream, &reg_a)["agent_id"].as_str().unwrap().to_string();
    let reg_b = format!(r#"{{"id":"2","token":"{}","action":"register","meta":{{"branch":"main"}}}}"#, token);
    let b = send_line(&mut stream, &reg_b)["agent_id"].as_str().unwrap().to_string();

    let mk = |id: &str, agent: &str| format!(
        r#"{{"id":"{}","token":"{}","action":"submit_event","capVersion":"0.1","event":{{"type":"CLAIM_PROPOSED","agentId":"{}","intent":"BUGFIX","domains":["AUTH"],"estimatedFiles":["src/auth/session.ts"],"task":{{"summary":"fix session expiry"}},"confidence":0.9}}}}"#,
        id, token, agent
    );
    assert_eq!(send_line(&mut stream, &mk("3", &a))["ok"], true);
    let rb = send_line(&mut stream, &mk("4", &b));
    let b_claim = rb["data"]["claimId"].as_str().unwrap().to_string();

    // Release b's claim -> conflict aborted.
    let release = format!(
        r#"{{"id":"5","token":"{}","action":"submit_event","capVersion":"0.1","event":{{"type":"CLAIM_RELEASED","claimId":"{}","agentId":"{}","reason":"TASK_COMPLETED"}}}}"#,
        token, b_claim, b
    );
    assert_eq!(send_line(&mut stream, &release)["ok"], true);

    drop(stream);

    let sessions = core.root.join(".coordify/sessions");
    let mut log_contents = String::new();
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(3) {
        if let Ok(entries) = std::fs::read_dir(&sessions) {
            for e in entries.flatten() {
                let log = e.path().join("events.log");
                if log.exists() {
                    log_contents = std::fs::read_to_string(log).unwrap();
                }
            }
        }
        if log_contents.contains("CONFLICT_ABORTED") {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(log_contents.contains("CONFLICT_OPENED"), "expected CONFLICT_OPENED");
    assert!(log_contents.contains("CONFLICT_ABORTED"), "expected CONFLICT_ABORTED after release");
}

#[test]
fn negotiation_resolves_conflict_over_socket() {
    let core = spawn_core("neg");
    let token = read_token(&core.root);
    let sock = core.root.join(".coordify/runtime/core.sock");
    let mut stream = connect_retry(&sock);

    let reg_a = format!(r#"{{"id":"1","token":"{}","action":"register","meta":{{"branch":"main"}}}}"#, token);
    let a = send_line(&mut stream, &reg_a)["agent_id"].as_str().unwrap().to_string();
    let reg_b = format!(r#"{{"id":"2","token":"{}","action":"register","meta":{{"branch":"main"}}}}"#, token);
    let b = send_line(&mut stream, &reg_b)["agent_id"].as_str().unwrap().to_string();

    let mk = |id: &str, agent: &str| format!(
        r#"{{"id":"{}","token":"{}","action":"submit_event","capVersion":"0.1","event":{{"type":"CLAIM_PROPOSED","agentId":"{}","intent":"BUGFIX","domains":["AUTH"],"estimatedFiles":["src/auth/session.ts"],"task":{{"summary":"fix session expiry"}},"confidence":0.9}}}}"#,
        id, token, agent
    );
    assert_eq!(send_line(&mut stream, &mk("3", &a))["ok"], true);
    assert_eq!(send_line(&mut stream, &mk("4", &b))["ok"], true); // conflict-1 opens

    // One agent yields, the other co-owns -> Core auto-resolves (PARTICIPANT_STEPPED_ASIDE).
    let prop = |id: &str, agent: &str, kind: &str| format!(
        r#"{{"id":"{}","token":"{}","action":"submit_event","capVersion":"0.1","event":{{"type":"CONFLICT_PROPOSAL_SUBMITTED","conflictId":"conflict-1","from":"{}","proposal":{{"kind":"{}","summary":"{} proposes"}}}}}}"#,
        id, token, agent, kind, agent
    );
    assert_eq!(send_line(&mut stream, &prop("5", &a, "YIELD_CLAIM"))["ok"], true);
    assert_eq!(send_line(&mut stream, &prop("6", &b, "CO_OWN"))["ok"], true);

    drop(stream);

    let sessions = core.root.join(".coordify/sessions");
    let mut log_contents = String::new();
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(3) {
        if let Ok(entries) = std::fs::read_dir(&sessions) {
            for e in entries.flatten() {
                let log = e.path().join("events.log");
                if log.exists() {
                    log_contents = std::fs::read_to_string(log).unwrap();
                }
            }
        }
        if log_contents.contains("CONFLICT_RESOLVED") {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(log_contents.contains("CONFLICT_OPENED"), "expected CONFLICT_OPENED");
    assert!(log_contents.contains("CONFLICT_PROPOSAL_RECEIVED"), "expected proposals logged");
    assert!(log_contents.contains("PARTICIPANT_STEPPED_ASIDE"), "expected negotiated resolution");
}

#[test]
fn knowledge_files_written_after_conflict_session() {
    // Use fast reaper so the orphaned first-registered agent times out quickly
    // after the connection closes, leaving the network empty and triggering finalize.
    let core = spawn_core_fast_reaper("know");
    let token = read_token(&core.root);
    let sock = core.root.join(".coordify/runtime/core.sock");
    let mut stream = connect_retry(&sock);

    let reg_a = format!(r#"{{"id":"1","token":"{}","action":"register","meta":{{"branch":"main"}}}}"#, token);
    let a = send_line(&mut stream, &reg_a)["agent_id"].as_str().unwrap().to_string();
    let reg_b = format!(r#"{{"id":"2","token":"{}","action":"register","meta":{{"branch":"main"}}}}"#, token);
    let b = send_line(&mut stream, &reg_b)["agent_id"].as_str().unwrap().to_string();

    let mk = |id: &str, agent: &str| format!(
        r#"{{"id":"{}","token":"{}","action":"submit_event","capVersion":"0.1","event":{{"type":"CLAIM_PROPOSED","agentId":"{}","intent":"BUGFIX","domains":["AUTH"],"estimatedFiles":["src/auth/session.ts","src/auth/tokens.ts"],"task":{{"summary":"fix session expiry"}},"confidence":0.9}}}}"#,
        id, token, agent
    );
    assert_eq!(send_line(&mut stream, &mk("3", &a))["ok"], true);
    assert_eq!(send_line(&mut stream, &mk("4", &b))["ok"], true); // conflict opens on the shared files

    drop(stream); // last agent leaves -> finalize -> knowledge persisted

    let hz = core.root.join(".coordify/knowledge/hotzones.json");
    let cp = core.root.join(".coordify/knowledge/coupling-graph.json");
    let start = std::time::Instant::now();
    let mut hz_contents = String::new();
    while start.elapsed() < Duration::from_secs(3) {
        if hz.exists() && cp.exists() {
            hz_contents = std::fs::read_to_string(&hz).unwrap();
            if hz_contents.contains("src/auth/session.ts") { break; }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(hz.exists(), "hotzones.json should be written at finalize");
    assert!(cp.exists(), "coupling-graph.json should be written at finalize");
    assert!(hz_contents.contains("src/auth/session.ts"), "hotzone for the conflict file:\n{hz_contents}");
    let cp_contents = std::fs::read_to_string(&cp).unwrap();
    assert!(cp_contents.contains("src/auth/tokens.ts"), "coupling edge present:\n{cp_contents}");
}

#[test]
fn file_touched_over_socket_raises_heat() {
    let core = spawn_core("ftouch");
    let token = read_token(&core.root);
    let sock = core.root.join(".coordify/runtime/core.sock");
    let mut stream = connect_retry(&sock);

    let reg_a = format!(r#"{{"id":"1","token":"{}","action":"register","meta":{{"branch":"main"}}}}"#, token);
    let a = send_line(&mut stream, &reg_a)["agent_id"].as_str().unwrap().to_string();
    let reg_b = format!(r#"{{"id":"2","token":"{}","action":"register","meta":{{"branch":"main"}}}}"#, token);
    let b = send_line(&mut stream, &reg_b)["agent_id"].as_str().unwrap().to_string();

    // Claims with NO estimated files (like the real adapter).
    let claim = |id: &str, agent: &str| format!(
        r#"{{"id":"{}","token":"{}","action":"submit_event","capVersion":"0.1","event":{{"type":"CLAIM_PROPOSED","agentId":"{}","intent":"BUGFIX","domains":["AUTH"],"task":{{"summary":"work"}},"confidence":0.9}}}}"#,
        id, token, agent
    );
    assert_eq!(send_line(&mut stream, &claim("3", &a))["ok"], true);
    assert_eq!(send_line(&mut stream, &claim("4", &b))["ok"], true);

    let touch = |id: &str, agent: &str| format!(
        r#"{{"id":"{}","token":"{}","action":"submit_event","capVersion":"0.1","event":{{"type":"FILE_TOUCHED","agentId":"{}","files":["src/auth/session.ts"]}}}}"#,
        id, token, agent
    );
    assert_eq!(send_line(&mut stream, &touch("5", &a))["ok"], true);
    assert_eq!(send_line(&mut stream, &touch("6", &b))["ok"], true);

    drop(stream);

    let sessions = core.root.join(".coordify/sessions");
    let mut log = String::new();
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(3) {
        if let Ok(entries) = std::fs::read_dir(&sessions) {
            for e in entries.flatten() {
                let f = e.path().join("events.log");
                if f.exists() { log = std::fs::read_to_string(f).unwrap(); }
            }
        }
        if log.contains("FILE_TOUCHED") && log.contains("\"pair\"") { break; }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(log.contains("FILE_TOUCHED"), "FILE_TOUCHED logged");
    assert!(log.contains("src/auth/session.ts"), "touched file in log:\n{log}");
    assert!(log.contains("HEAT_UPDATED"), "FILE_TOUCHED should trigger heat recompute:\n{log}");
}

#[test]
fn reaper_escalates_timed_out_conflict_over_socket() {
    let core = spawn_core_fast_proposal_timeout("ptmo");
    let token = read_token(&core.root);
    let sock = core.root.join(".coordify/runtime/core.sock");
    let mut stream = connect_retry(&sock);

    let reg_a = format!(r#"{{"id":"1","token":"{}","action":"register","meta":{{"branch":"main"}}}}"#, token);
    let a = send_line(&mut stream, &reg_a)["agent_id"].as_str().unwrap().to_string();
    let reg_b = format!(r#"{{"id":"2","token":"{}","action":"register","meta":{{"branch":"main"}}}}"#, token);
    let b = send_line(&mut stream, &reg_b)["agent_id"].as_str().unwrap().to_string();

    let mk = |id: &str, agent: &str| format!(
        r#"{{"id":"{}","token":"{}","action":"submit_event","capVersion":"0.1","event":{{"type":"CLAIM_PROPOSED","agentId":"{}","intent":"BUGFIX","domains":["AUTH"],"estimatedFiles":["src/auth/session.ts"],"task":{{"summary":"fix session expiry"}},"confidence":0.9}}}}"#,
        id, token, agent
    );
    assert_eq!(send_line(&mut stream, &mk("3", &a))["ok"], true);
    assert_eq!(send_line(&mut stream, &mk("4", &b))["ok"], true); // conflict-1 opens

    // No proposals submitted: the reaper proposal-timeout sweep (§18.6) escalates it.
    // Keep the connection open (agents alive) while polling the live log.
    let sessions = core.root.join(".coordify/sessions");
    let mut log_contents = String::new();
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(4) {
        if let Ok(entries) = std::fs::read_dir(&sessions) {
            for e in entries.flatten() {
                let log = e.path().join("events.log");
                if log.exists() {
                    log_contents = std::fs::read_to_string(log).unwrap();
                }
            }
        }
        // Poll on the LAST event the sweep emits: CONFLICT_TIMEOUT and
        // USER_ARBITRATION_REQUIRED are two separate appends, so reading on the
        // first risks a mid-write snapshot missing the second.
        if log_contents.contains("USER_ARBITRATION_REQUIRED") {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    drop(stream);
    assert!(log_contents.contains("CONFLICT_OPENED"), "expected CONFLICT_OPENED");
    assert!(log_contents.contains("CONFLICT_TIMEOUT"), "expected CONFLICT_TIMEOUT from reaper sweep:\n{log_contents}");
    assert!(log_contents.contains("USER_ARBITRATION_REQUIRED"), "expected arbitration after timeout");
}
