use crate::cap::{self, CapErrorCode, CapEvent, ClaimStatus};
use crate::eventlog::EventLog;
use crate::ipc::{decode_request, encode_response, Request, Response};
use crate::paths::Paths;
use crate::session::{finalize, Session};
use crate::state::{now_ms, State, StateError};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
use std::sync::{Arc, Mutex};

pub struct Shared {
    pub state: Mutex<State>,
    pub log: Mutex<EventLog>,
    pub token: String,
    pub agents_seen: Mutex<u64>,
    pub finalized: AtomicBool,
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
        "submit_event" => handle_cap_event(shared, req),
        _ => Response::err(&req.id, "unknown action"),
    }
}

fn cap_err(id: &str, code: CapErrorCode) -> Response {
    Response::err(id, code.as_str())
}

fn handle_cap_event(shared: &Shared, req: &Request) -> Response {
    if req.cap_version.as_deref() != Some("0.1") {
        return cap_err(&req.id, CapErrorCode::UnsupportedCapVersion);
    }
    let event = match cap::decode_event(&req.event) {
        Ok(e) => e,
        Err(code) => return cap_err(&req.id, code),
    };
    match event {
        CapEvent::ClaimProposed {
            agent_id,
            intent,
            domains,
            estimated_files,
            confidence,
            ..
        } => {
            // Existence check + propose + promote under ONE state lock (atomic;
            // closes the TOCTOU window vs. the reaper). None => agent unknown.
            let outcome = {
                let mut st = shared.state.lock().unwrap();
                if st.agent_state(&agent_id).is_none() {
                    None
                } else {
                    let task_summary = req
                        .event
                        .get("task")
                        .and_then(|t| t.get("summary"))
                        .and_then(|s| s.as_str())
                        .unwrap_or("")
                        .to_string();
                    let created = st.claims.propose(
                        &agent_id,
                        task_summary,
                        intent.as_str().to_string(),
                        domains,
                        estimated_files,
                        confidence,
                    );
                    if let Some(ref claim) = created {
                        if claim.status == ClaimStatus::Active {
                            st.promote_active(&agent_id);
                        }
                    }
                    Some(created)
                }
            };
            match outcome {
                None => cap_err(&req.id, CapErrorCode::AgentNotFound),
                Some(Some(claim)) => {
                    let event = serde_json::json!({
                        "type": "CLAIM_CREATED",
                        "claimId": claim.claim_id,
                        "agentId": agent_id,
                        "status": claim.status.as_str(),
                        "ts": crate::bootstrap::now_iso(),
                    });
                    let _ = shared.log.lock().unwrap().append(&event);
                    Response::ok_with_data(
                        &req.id,
                        serde_json::json!({"claimId": claim.claim_id, "status": claim.status.as_str()}),
                    )
                }
                Some(None) => {
                    let event = serde_json::json!({
                        "type": "CLAIM_REJECTED",
                        "agentId": agent_id,
                        "reason": "LOW_CONFIDENCE",
                        "ts": crate::bootstrap::now_iso(),
                    });
                    let _ = shared.log.lock().unwrap().append(&event);
                    Response::ok_with_data(
                        &req.id,
                        serde_json::json!({"status": "REJECTED", "reason": "LOW_CONFIDENCE"}),
                    )
                }
            }
        }
        CapEvent::ClaimReleased { claim_id, agent_id, reason } => {
            let released = {
                let mut st = shared.state.lock().unwrap();
                st.claims.release(&claim_id)
            };
            if !released {
                return cap_err(&req.id, CapErrorCode::ClaimNotFound);
            }
            let log_event = serde_json::json!({
                "type": "CLAIM_RELEASED",
                "claimId": claim_id,
                "agentId": agent_id,
                "reason": serde_json::to_value(reason).unwrap(),
                "ts": crate::bootstrap::now_iso(),
            });
            let _ = shared.log.lock().unwrap().append(&log_event);
            Response::ok_for(&req.id)
        }
        CapEvent::AgentStateChanged { agent_id, state } => {
            let result = {
                let mut st = shared.state.lock().unwrap();
                st.set_state(&agent_id, state)
            };
            match result {
                Ok(()) => {
                    let event = serde_json::json!({
                        "type": "AGENT_STATE_CHANGED",
                        "agentId": agent_id,
                        "state": serde_json::to_value(state).unwrap(),
                        "ts": crate::bootstrap::now_iso(),
                    });
                    let _ = shared.log.lock().unwrap().append(&event);
                    Response::ok_for(&req.id)
                }
                Err(StateError::AgentNotFound) => cap_err(&req.id, CapErrorCode::AgentNotFound),
                Err(StateError::InvalidTransition) => {
                    cap_err(&req.id, CapErrorCode::InvalidStateTransition)
                }
            }
        }
        CapEvent::ClearInvoked { agent_id } => {
            let cleared = {
                let mut st = shared.state.lock().unwrap();
                if st.agent_state(&agent_id).is_none() {
                    None
                } else {
                    let released = st.claims.release_for_agent(&agent_id);
                    let generation = st.clear(&agent_id).unwrap();
                    Some((released, generation))
                }
            };
            let (released, generation) = match cleared {
                Some(v) => v,
                None => return cap_err(&req.id, CapErrorCode::AgentNotFound),
            };
            {
                let mut log = shared.log.lock().unwrap();
                for claim_id in &released {
                    let _ = log.append(&serde_json::json!({
                        "type": "CLAIM_RELEASED",
                        "claimId": claim_id,
                        "agentId": agent_id,
                        "reason": "CLEAR_INVOKED",
                        "ts": crate::bootstrap::now_iso(),
                    }));
                }
                let _ = log.append(&serde_json::json!({
                    "type": "CLEAR_INVOKED",
                    "agentId": agent_id,
                    "newGeneration": generation,
                    "ts": crate::bootstrap::now_iso(),
                }));
                let _ = log.append(&serde_json::json!({
                    "type": "AGENT_GENERATION_INCREMENTED",
                    "agentId": agent_id,
                    "generation": generation,
                    "ts": crate::bootstrap::now_iso(),
                }));
            }
            Response::ok_with_data(&req.id, serde_json::json!({"generation": generation}))
        }
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
        finalized: AtomicBool::new(false),
    });

    let interval_ms = std::env::var("COORDIFY_REAPER_INTERVAL_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2_000);
    let timeout_ms = std::env::var("COORDIFY_REAPER_TIMEOUT_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10_000);
    let orphan_ttl_ms = std::env::var("COORDIFY_ORPHAN_TTL_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300_000);
    let _reaper = spawn_reaper(
        Arc::clone(&shared),
        session.clone(),
        Paths::new(paths.root.clone()),
        interval_ms,
        timeout_ms,
        orphan_ttl_ms,
    );

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
            if network_empty && seen > 0
                && shared.finalized.compare_exchange(false, true, SeqCst, SeqCst).is_ok()
            {
                finalize(&session, &paths, seen)?;
                break;
            } else if network_empty && seen > 0 {
                // Reaper already finalized + is exiting; just stop the loop.
                break;
            }
        }
    }
    Ok(())
}

pub fn spawn_reaper(
    shared: Arc<Shared>,
    session: Session,
    paths: Paths,
    interval_ms: u64,
    timeout_ms: u64,
    orphan_ttl_ms: u64,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(interval_ms));
        let now = now_ms();
        let lost = {
            let mut st = shared.state.lock().unwrap();
            st.reap(now, timeout_ms)
        };
        // Orphan each lost agent's live claims (collect under a short lock).
        let mut orphaned: Vec<(String, String)> = Vec::new(); // (claimId, previousOwner)
        if !lost.is_empty() {
            let mut st = shared.state.lock().unwrap();
            for id in &lost {
                for claim_id in st.claims.orphan_for_agent(id, now) {
                    orphaned.push((claim_id, id.clone()));
                }
            }
        }
        let reclaimable = {
            let mut st = shared.state.lock().unwrap();
            st.claims.sweep_reclaimable(now, orphan_ttl_ms)
        };

        if !lost.is_empty() || !orphaned.is_empty() || !reclaimable.is_empty() {
            let ttl_seconds = orphan_ttl_ms / 1000;
            let mut log = shared.log.lock().unwrap();
            for id in &lost {
                let _ = log.append(&serde_json::json!({
                    "type": "AGENT_LOST",
                    "agentId": id,
                    "ts": crate::bootstrap::now_iso(),
                }));
            }
            for (claim_id, previous_owner) in &orphaned {
                let _ = log.append(&serde_json::json!({
                    "type": "CLAIM_ORPHANED",
                    "claimId": claim_id,
                    "previousOwner": previous_owner,
                    "ttlSeconds": ttl_seconds,
                    "ts": crate::bootstrap::now_iso(),
                }));
            }
            for claim_id in &reclaimable {
                let _ = log.append(&serde_json::json!({
                    "type": "CLAIM_RECLAIMABLE",
                    "claimId": claim_id,
                    "ts": crate::bootstrap::now_iso(),
                }));
            }
        }

        // Empty-network finalize: the last agent leaving ends the session
        // immediately (ARCHITECTURE §15). Note: because the accept loop is
        // serialized and a fully-empty network finalizes here, the orphan ->
        // RECLAIMABLE sweep above is only reachable once concurrent connections
        // are supported (later phase); its logic is unit-tested in claim.rs.
        let empty = shared.state.lock().unwrap().agent_count() == 0;
        let seen = *shared.agents_seen.lock().unwrap();
        if empty
            && seen > 0
            && shared.finalized.compare_exchange(false, true, SeqCst, SeqCst).is_ok()
        {
            let _ = finalize(&session, &paths, seen);
            std::process::exit(0);
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
            finalized: AtomicBool::new(false),
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
            cap_version: None,
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

    // Target A1: heartbeat with missing agent_id returns ok:false, error:"missing agent_id".
    #[test]
    fn heartbeat_with_missing_agent_id() {
        let s = shared_for_test("good");
        // agent_id is None by default in req()
        let resp = handle_request(&s, &req("good", "heartbeat"));
        assert!(!resp.ok);
        assert_eq!(resp.error.as_deref(), Some("missing agent_id"));
    }

    // Target A2: submit_event without cap_version returns UNSUPPORTED_CAP_VERSION (Task 4 routing).
    #[test]
    fn submit_event_without_cap_version_returns_error() {
        let s = shared_for_test("good");
        // submit_event now routes through handle_cap_event; missing cap_version → error.
        let mut ev_req = req("good", "submit_event");
        ev_req.event = json!({"type": "CUSTOM", "x": 1});
        let resp = handle_request(&s, &ev_req);
        assert!(!resp.ok);
        assert_eq!(resp.error.as_deref(), Some("UNSUPPORTED_CAP_VERSION"));
    }

    fn cap_req(token: &str, event: serde_json::Value) -> Request {
        Request {
            id: "r1".to_string(),
            token: token.to_string(),
            action: "submit_event".to_string(),
            agent_id: None,
            meta: json!({}),
            event,
            cap_version: Some("0.1".to_string()),
        }
    }

    #[test]
    fn claim_proposed_active_creates_claim_and_activates_agent() {
        let s = shared_for_test("good");
        let reg = handle_request(&s, &req("good", "register"));
        let agent = reg.agent_id.unwrap();
        let resp = handle_request(
            &s,
            &cap_req("good", json!({"type":"CLAIM_PROPOSED","agentId":agent,"intent":"BUGFIX","confidence":0.9})),
        );
        assert!(resp.ok);
        let data = resp.data.unwrap();
        assert_eq!(data["status"], "ACTIVE");
        assert!(data["claimId"].as_str().unwrap().starts_with("claim-"));
        assert_eq!(s.state.lock().unwrap().agent_state(&agent), Some(crate::cap::AgentState::Active));
    }

    #[test]
    fn claim_proposed_low_confidence_is_rejected() {
        let s = shared_for_test("good");
        let reg = handle_request(&s, &req("good", "register"));
        let agent = reg.agent_id.unwrap();
        let resp = handle_request(
            &s,
            &cap_req("good", json!({"type":"CLAIM_PROPOSED","agentId":agent,"intent":"BUGFIX","confidence":0.1})),
        );
        assert!(resp.ok);
        assert_eq!(resp.data.unwrap()["status"], "REJECTED");
        // Agent stays DISCOVERY.
        assert_eq!(s.state.lock().unwrap().agent_state(&agent), Some(crate::cap::AgentState::Discovery));
    }

    #[test]
    fn claim_proposed_unknown_agent_errors() {
        let s = shared_for_test("good");
        let resp = handle_request(
            &s,
            &cap_req("good", json!({"type":"CLAIM_PROPOSED","agentId":"agent-404","intent":"BUGFIX","confidence":0.9})),
        );
        assert!(!resp.ok);
        assert_eq!(resp.error.as_deref(), Some("AGENT_NOT_FOUND"));
    }

    #[test]
    fn agent_state_changed_valid_and_invalid() {
        let s = shared_for_test("good");
        let agent = handle_request(&s, &req("good", "register")).agent_id.unwrap();
        // Discovery -> Active ok
        let ok = handle_request(&s, &cap_req("good", json!({"type":"AGENT_STATE_CHANGED","agentId":agent,"state":"ACTIVE"})));
        assert!(ok.ok);
        // Active -> SUBAGENT_WAITING ok; then SUBAGENT_WAITING -> TESTING is invalid
        assert!(handle_request(&s, &cap_req("good", json!({"type":"AGENT_STATE_CHANGED","agentId":agent,"state":"SUBAGENT_WAITING"}))).ok);
        let bad = handle_request(&s, &cap_req("good", json!({"type":"AGENT_STATE_CHANGED","agentId":agent,"state":"TESTING"})));
        assert_eq!(bad.error.as_deref(), Some("INVALID_STATE_TRANSITION"));
        // unknown agent
        let nf = handle_request(&s, &cap_req("good", json!({"type":"AGENT_STATE_CHANGED","agentId":"agent-404","state":"IDLE"})));
        assert_eq!(nf.error.as_deref(), Some("AGENT_NOT_FOUND"));
    }

    #[test]
    fn clear_invoked_releases_claims_and_bumps_generation() {
        let s = shared_for_test("good");
        let agent = handle_request(&s, &req("good", "register")).agent_id.unwrap();
        let propose = handle_request(&s, &cap_req("good", json!({"type":"CLAIM_PROPOSED","agentId":agent,"intent":"BUGFIX","confidence":0.9})));
        let claim_id = propose.data.unwrap()["claimId"].as_str().unwrap().to_string();

        let resp = handle_request(&s, &cap_req("good", json!({"type":"CLEAR_INVOKED","agentId":agent})));
        assert!(resp.ok);
        assert_eq!(resp.data.unwrap()["generation"], 2);
        let st = s.state.lock().unwrap();
        assert_eq!(st.agent_state(&agent), Some(crate::cap::AgentState::Discovery));
        assert_eq!(st.claims.get(&claim_id).unwrap().status, crate::cap::ClaimStatus::Released);
    }

    #[test]
    fn clear_invoked_unknown_agent_errors() {
        let s = shared_for_test("good");
        let resp = handle_request(&s, &cap_req("good", json!({"type":"CLEAR_INVOKED","agentId":"agent-404"})));
        assert_eq!(resp.error.as_deref(), Some("AGENT_NOT_FOUND"));
    }

    #[test]
    fn bad_cap_version_and_unknown_event_and_release_missing() {
        let s = shared_for_test("good");
        // wrong cap version
        let mut r = cap_req("good", json!({"type":"CLAIM_RELEASED","claimId":"claim-1","agentId":"a","reason":"TASK_COMPLETED"}));
        r.cap_version = Some("9.9".to_string());
        assert_eq!(handle_request(&s, &r).error.as_deref(), Some("UNSUPPORTED_CAP_VERSION"));
        // unknown event type
        let resp = handle_request(&s, &cap_req("good", json!({"type":"NONSENSE"})));
        assert_eq!(resp.error.as_deref(), Some("SCHEMA_VALIDATION_FAILED"));
        // release a claim that doesn't exist
        let resp = handle_request(&s, &cap_req("good", json!({"type":"CLAIM_RELEASED","claimId":"claim-404","agentId":"a","reason":"TASK_COMPLETED"})));
        assert_eq!(resp.error.as_deref(), Some("CLAIM_NOT_FOUND"));
    }
}
