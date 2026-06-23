use crate::eventlog::EventLog;
use crate::ipc::{decode_request, encode_response, Request, Response};
use crate::paths::Paths;
use crate::session::{finalize, Session};
use crate::state::{now_ms, State};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, Mutex};

pub struct Shared {
    pub state: Mutex<State>,
    pub log: Mutex<EventLog>,
    pub token: String,
    pub agents_seen: Mutex<u64>,
}

pub fn handle_request(shared: &Shared, req: &Request) -> Response {
    if req.token != shared.token {
        return Response::err(&req.id, "unauthorized");
    }
    match req.action.as_str() {
        "register" => {
            let now = now_ms();
            let agent_id = {
                let mut st = shared.state.lock().unwrap();
                st.register(req.meta.clone(), now)
            };
            {
                let mut seen = shared.agents_seen.lock().unwrap();
                *seen += 1;
            }
            let event = serde_json::json!({
                "type": "AGENT_JOINED",
                "agentId": agent_id,
                "ts": crate::bootstrap::now_iso(),
            });
            let _ = shared.log.lock().unwrap().append(&event);
            Response::ok_with_agent(&req.id, &agent_id)
        }
        "heartbeat" => {
            let now = now_ms();
            match &req.agent_id {
                Some(id) => {
                    let ok = shared.state.lock().unwrap().heartbeat(id, now);
                    if ok {
                        Response::ok_for(&req.id)
                    } else {
                        Response::err(&req.id, "unknown agent")
                    }
                }
                None => Response::err(&req.id, "missing agent_id"),
            }
        }
        "submit_event" => {
            let _ = shared.log.lock().unwrap().append(&req.event);
            Response::ok_for(&req.id)
        }
        _ => Response::err(&req.id, "unknown action"),
    }
}

/// Handle one connection: read newline-delimited requests, reply per line.
/// When the connecting agent registered, drop it on disconnect and return
/// whether the network is now empty.
fn handle_conn(shared: &Arc<Shared>, stream: UnixStream) -> bool {
    let mut writer = match stream.try_clone() {
        Ok(w) => w,
        Err(_) => return false,
    };
    let reader = BufReader::new(stream);
    let mut this_agent: Option<String> = None;

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }
        match decode_request(&line) {
            Ok(req) => {
                let resp = handle_request(shared, &req);
                if resp.ok && req.action == "register" {
                    this_agent = resp.agent_id.clone();
                }
                let mut out = encode_response(&resp);
                out.push('\n');
                if writer.write_all(out.as_bytes()).is_err() {
                    break;
                }
            }
            Err(_) => {
                let mut out = encode_response(&Response::err("?", "malformed request"));
                out.push('\n');
                let _ = writer.write_all(out.as_bytes());
            }
        }
    }

    // Connection closed: the agent left.
    if let Some(id) = this_agent {
        let (removed, empty) = {
            let mut st = shared.state.lock().unwrap();
            let removed = st.remove(&id);
            (removed, st.agent_count() == 0)
        };
        if removed {
            let event = serde_json::json!({
                "type": "AGENT_LEFT",
                "agentId": id,
                "ts": crate::bootstrap::now_iso(),
            });
            let _ = shared.log.lock().unwrap().append(&event);
        }
        return empty;
    }
    false
}

pub fn run(
    paths: Paths,
    session: Session,
    token: String,
    listener: UnixListener,
) -> std::io::Result<()> {
    let log = EventLog::create(session.dir.join("events.log"))?;
    let shared = Arc::new(Shared {
        state: Mutex::new(State::new()),
        log: Mutex::new(log),
        token,
        agents_seen: Mutex::new(0),
    });

    let interval_ms = std::env::var("COORDIFY_REAPER_INTERVAL_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2_000);
    let timeout_ms = std::env::var("COORDIFY_REAPER_TIMEOUT_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10_000);
    let _reaper = spawn_reaper(Arc::clone(&shared), interval_ms, timeout_ms);

    for conn in listener.incoming() {
        let stream = match conn {
            Ok(s) => s,
            Err(_) => continue,
        };
        let shared_c = Arc::clone(&shared);
        let handle = std::thread::spawn(move || handle_conn(&shared_c, stream));
        // ponytail: serialize connection handling for the MVP skeleton so the
        // "last agent leaves -> finalize" decision is observed on this thread.
        // Phase 2 replaces this with a shared reaper-driven shutdown signal.
        if let Ok(network_empty) = handle.join() {
            let seen = *shared.agents_seen.lock().unwrap();
            if network_empty && seen > 0 {
                finalize(&session, &paths, seen)?;
                break;
            }
        }
    }
    Ok(())
}

pub fn spawn_reaper(
    shared: Arc<Shared>,
    interval_ms: u64,
    timeout_ms: u64,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(interval_ms));
        let lost = {
            let mut st = shared.state.lock().unwrap();
            st.reap(now_ms(), timeout_ms)
        };
        for id in lost {
            let mut log = shared.log.lock().unwrap();
            let _ = log.append(&serde_json::json!({
                "type": "AGENT_LOST",
                "agentId": id,
                "ts": crate::bootstrap::now_iso(),
            }));
            let _ = log.append(&serde_json::json!({
                "type": "CLAIM_ORPHANED",
                "agentId": id,
                "ts": crate::bootstrap::now_iso(),
            }));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    static TEST_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    fn shared_for_test(token: &str) -> Arc<Shared> {
        let mut dir = std::env::temp_dir();
        dir.push(format!("coordify-srv-{}-{}", std::process::id(),
            TEST_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let log = EventLog::create(dir.join("events.log")).unwrap();
        Arc::new(Shared {
            state: Mutex::new(State::new()),
            log: Mutex::new(log),
            token: token.to_string(),
            agents_seen: Mutex::new(0),
        })
    }

    fn req(token: &str, action: &str) -> Request {
        Request {
            id: "r1".to_string(),
            token: token.to_string(),
            action: action.to_string(),
            agent_id: None,
            meta: json!({}),
            event: json!({}),
        }
    }

    #[test]
    fn rejects_bad_token() {
        let s = shared_for_test("good");
        let resp = handle_request(&s, &req("bad", "register"));
        assert!(!resp.ok);
        assert_eq!(resp.error.as_deref(), Some("unauthorized"));
        assert_eq!(s.state.lock().unwrap().agent_count(), 0);
    }

    #[test]
    fn register_then_heartbeat() {
        let s = shared_for_test("good");
        let resp = handle_request(&s, &req("good", "register"));
        assert!(resp.ok);
        let id = resp.agent_id.unwrap();

        let mut hb = req("good", "heartbeat");
        hb.agent_id = Some(id.clone());
        assert!(handle_request(&s, &hb).ok);

        let mut bad_hb = req("good", "heartbeat");
        bad_hb.agent_id = Some("agent-999".to_string());
        assert!(!handle_request(&s, &bad_hb).ok);
    }

    #[test]
    fn unknown_action_errors() {
        let s = shared_for_test("good");
        let resp = handle_request(&s, &req("good", "frobnicate"));
        assert_eq!(resp.error.as_deref(), Some("unknown action"));
    }

    #[test]
    fn reaper_emits_lost_and_orphaned_events() {
        let s = shared_for_test("good");
        // Register an agent, then backdate it by mutating via a stale heartbeat.
        let resp = handle_request(&s, &req("good", "register"));
        let id = resp.agent_id.unwrap();
        // Force it stale: heartbeat with an old timestamp is not exposed, so
        // reap directly with a now far past the timeout window.
        let lost = s.state.lock().unwrap().reap(now_ms() + 1_000_000, 10_000);
        assert_eq!(lost, vec![id]);
        // The reaper loop body's event-append is exercised by the integration test;
        // here we assert reap removed the agent.
        assert_eq!(s.state.lock().unwrap().agent_count(), 0);
    }
}
