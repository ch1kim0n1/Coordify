use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

fn ev_type(e: &Value) -> &str {
    e.get("type").and_then(|v| v.as_str()).unwrap_or("")
}
fn ev_str(e: &Value, k: &str) -> String {
    e.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string()
}
fn ev_pair(e: &Value, key: &str) -> Option<(String, String)> {
    let arr = e.get(key)?.as_array()?;
    if arr.len() == 2 {
        Some((arr[0].as_str()?.to_string(), arr[1].as_str()?.to_string()))
    } else {
        None
    }
}
fn parse_ms(ts: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(ts)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

#[derive(Debug, Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTally {
    pub claims_made: u64,
    pub tasks_completed: u64,
    pub ghost_claims: u64,
    pub conflicts_involved: u64,
    pub heat_generated_sum: u64,
    pub heat_generated_count: u64,
    pub arbitrations_involved: u64,
    pub deadlocks_involved: u64,
}

#[derive(Debug, Default, Serialize)]
pub struct PeakHeat {
    pub heat: u64,
    pub pair: Vec<String>,
    pub ts: String,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStats {
    pub agents_seen: u64,
    pub claims_created: u64,
    pub claims_released: u64,
    pub claims_rejected: u64,
    pub files_touched: u64,
    pub heat_updates: u64,
    pub conflicts_opened: u64,
    pub auto_resolved_heat_dropped: u64,
    pub negotiated_resolved: u64,
    pub user_arbitrated: u64,
    pub escalated: u64,
    pub timed_out: u64,
    pub aborted: u64,
    pub deadlocks: u64,
    pub arbitrations_requested: u64,
    pub peak_heat: PeakHeat,
    pub duration_ms: i64,
    pub agents: BTreeMap<String, AgentTally>,
}

impl SessionStats {
    pub fn to_summary(&self, narrative: &str, knowledge_snapshot: Value) -> Value {
        serde_json::json!({
            "session": { "durationMs": self.duration_ms, "agentsSeen": self.agents_seen },
            "agents": self.agents,
            "claims": {
                "created": self.claims_created,
                "released": self.claims_released,
                "rejected": self.claims_rejected,
                "filesTouched": self.files_touched
            },
            "heat": { "updates": self.heat_updates, "peak": self.peak_heat },
            "conflicts": {
                "opened": self.conflicts_opened,
                "autoResolvedHeatDropped": self.auto_resolved_heat_dropped,
                "negotiatedResolved": self.negotiated_resolved,
                "userArbitrated": self.user_arbitrated,
                "escalated": self.escalated,
                "timedOut": self.timed_out,
                "aborted": self.aborted,
                "deadlocks": self.deadlocks,
                "arbitrationsRequested": self.arbitrations_requested
            },
            "knowledgeSnapshot": knowledge_snapshot,
            "narrative": narrative,
        })
    }
}

pub fn heat_history(events: &[Value]) -> Vec<Value> {
    events
        .iter()
        .filter(|e| ev_type(e) == "HEAT_UPDATED")
        .map(|e| {
            serde_json::json!({
                "pair": e.get("pair").cloned().unwrap_or(Value::Null),
                "heat": e.get("heat").cloned().unwrap_or(Value::Null),
                "band": e.get("band").cloned().unwrap_or(Value::Null),
                "ts": e.get("ts").cloned().unwrap_or(Value::Null),
            })
        })
        .collect()
}

pub fn summarize(events: &[Value]) -> SessionStats {
    let mut s = SessionStats::default();
    let mut agents_set: BTreeSet<String> = BTreeSet::new();
    let mut files_set: BTreeSet<String> = BTreeSet::new();
    let mut first_ts: Option<i64> = None;
    let mut last_ts: Option<i64> = None;

    for e in events {
        if let Some(ms) = e.get("ts").and_then(|v| v.as_str()).and_then(parse_ms) {
            first_ts = Some(first_ts.map_or(ms, |f| f.min(ms)));
            last_ts = Some(last_ts.map_or(ms, |l| l.max(ms)));
        }
        match ev_type(e) {
            "AGENT_JOINED" => {
                let id = ev_str(e, "agentId");
                if !id.is_empty() {
                    agents_set.insert(id);
                }
            }
            "CLAIM_CREATED" => {
                s.claims_created += 1;
                s.agents
                    .entry(ev_str(e, "agentId"))
                    .or_default()
                    .claims_made += 1;
            }
            "CLAIM_REJECTED" => s.claims_rejected += 1,
            "CLAIM_RELEASED" => {
                s.claims_released += 1;
                if ev_str(e, "reason") == "TASK_COMPLETED" {
                    s.agents
                        .entry(ev_str(e, "agentId"))
                        .or_default()
                        .tasks_completed += 1;
                }
            }
            "CLAIM_ORPHANED" => {
                let id = ev_str(e, "previousOwner");
                if !id.is_empty() {
                    s.agents.entry(id).or_default().ghost_claims += 1;
                }
            }
            "FILE_TOUCHED" => {
                if let Some(arr) = e.get("files").and_then(|v| v.as_array()) {
                    for f in arr {
                        if let Some(p) = f.as_str() {
                            files_set.insert(p.to_string());
                        }
                    }
                }
            }
            "HEAT_UPDATED" => {
                s.heat_updates += 1;
                let heat = e.get("heat").and_then(|v| v.as_u64()).unwrap_or(0);
                if let Some((a, b)) = ev_pair(e, "pair") {
                    for id in [a.clone(), b.clone()] {
                        let t = s.agents.entry(id).or_default();
                        t.heat_generated_sum += heat;
                        t.heat_generated_count += 1;
                    }
                    // Strictly-greater keeps the earliest occurrence of the max (events are in order).
                    if heat > s.peak_heat.heat {
                        s.peak_heat = PeakHeat {
                            heat,
                            pair: vec![a, b],
                            ts: ev_str(e, "ts"),
                        };
                    }
                }
            }
            "CONFLICT_OPENED" => {
                s.conflicts_opened += 1;
                if let Some((a, b)) = ev_pair(e, "agents") {
                    for id in [a, b] {
                        s.agents.entry(id).or_default().conflicts_involved += 1;
                    }
                }
            }
            "CONFLICT_RESOLVED" => match ev_str(e, "resolution").as_str() {
                "AUTO_RESOLVED_HEAT_DROPPED" => s.auto_resolved_heat_dropped += 1,
                "USER_ARBITRATED" => s.user_arbitrated += 1,
                "PARTICIPANT_STEPPED_ASIDE" | "QUEUED" | "SCOPE_SPLIT" | "CO_OWNERSHIP" => {
                    s.negotiated_resolved += 1
                }
                _ => {}
            },
            "CONFLICT_ESCALATED" => s.escalated += 1,
            "CONFLICT_TIMEOUT" => s.timed_out += 1,
            "CONFLICT_ABORTED" => s.aborted += 1,
            "USER_ARBITRATION_REQUIRED" => {
                s.arbitrations_requested += 1;
                if let Some((a, b)) = ev_pair(e, "agents") {
                    for id in [a, b] {
                        s.agents.entry(id).or_default().arbitrations_involved += 1;
                    }
                }
            }
            "DEADLOCK_DETECTED" => {
                s.deadlocks += 1;
                if let Some(arr) = e.get("agents").and_then(|v| v.as_array()) {
                    for id in arr {
                        if let Some(x) = id.as_str() {
                            s.agents
                                .entry(x.to_string())
                                .or_default()
                                .deadlocks_involved += 1;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    s.agents_seen = agents_set.len() as u64;
    s.files_touched = files_set.len() as u64;
    s.duration_ms = match (first_ts, last_ts) {
        (Some(f), Some(l)) => l - f,
        _ => 0,
    };
    s
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProfile {
    pub sessions: u64,
    pub claims_made: u64,
    pub tasks_completed: u64,
    pub ghost_claims: u64,
    pub conflicts_involved: u64,
    pub heat_generated_sum: u64,
    pub heat_generated_count: u64,
    pub arbitrations_involved: u64,
    pub deadlocks_involved: u64,
}

#[derive(Default)]
pub struct ProfileStore {
    pub agents: BTreeMap<String, AgentProfile>,
}

impl ProfileStore {
    pub fn load(dir: &Path) -> (Self, Vec<String>) {
        let mut store = Self::default();
        let mut quarantined = Vec::new();
        let f = dir.join("agent-profiles.json");
        if f.exists() {
            match std::fs::read_to_string(&f)
                .ok()
                .and_then(|s| serde_json::from_str::<BTreeMap<String, AgentProfile>>(&s).ok())
            {
                Some(m) => store.agents = m,
                None => crate::knowledge::quarantine(&f, &mut quarantined),
            }
        }
        (store, quarantined)
    }

    pub fn merge_session(&mut self, tallies: &BTreeMap<String, AgentTally>) {
        for (id, t) in tallies {
            let p = self.agents.entry(id.clone()).or_default();
            p.sessions += 1;
            p.claims_made += t.claims_made;
            p.tasks_completed += t.tasks_completed;
            p.ghost_claims += t.ghost_claims;
            p.conflicts_involved += t.conflicts_involved;
            p.heat_generated_sum += t.heat_generated_sum;
            p.heat_generated_count += t.heat_generated_count;
            p.arbitrations_involved += t.arbitrations_involved;
            p.deadlocks_involved += t.deadlocks_involved;
        }
    }

    pub fn save_atomic(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let profiles = serde_json::to_string_pretty(&self.agents).unwrap();
        crate::knowledge::write_atomic(&dir.join("agent-profiles.json"), &profiles)?;

        let velocity: BTreeMap<&String, Value> = self
            .agents
            .iter()
            .map(|(id, p)| {
                (id, serde_json::json!({
                    "tasksPerSession": if p.sessions > 0 { p.tasks_completed as f64 / p.sessions as f64 } else { 0.0 },
                    "meanHeatGenerated": if p.heat_generated_count > 0 { p.heat_generated_sum as f64 / p.heat_generated_count as f64 } else { 0.0 },
                }))
            })
            .collect();
        crate::knowledge::write_atomic(
            &dir.join("velocity-profiles.json"),
            &serde_json::to_string_pretty(&velocity).unwrap(),
        )?;

        let overhead: BTreeMap<&String, Value> = self
            .agents
            .iter()
            .map(|(id, p)| {
                (id, serde_json::json!({
                    "conflictsInvolved": p.conflicts_involved,
                    "arbitrationsInvolved": p.arbitrations_involved,
                    "deadlocksInvolved": p.deadlocks_involved,
                    "overheadScore": p.conflicts_involved + p.arbitrations_involved + p.deadlocks_involved,
                }))
            })
            .collect();
        crate::knowledge::write_atomic(
            &dir.join("coordination-overhead.json"),
            &serde_json::to_string_pretty(&overhead).unwrap(),
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> Vec<serde_json::Value> {
        vec![
            json!({"type":"AGENT_JOINED","agentId":"agent-1","ts":"2026-06-23T00:00:00Z"}),
            json!({"type":"AGENT_JOINED","agentId":"agent-2","ts":"2026-06-23T00:00:01Z"}),
            json!({"type":"CLAIM_CREATED","claimId":"claim-1","agentId":"agent-1","ts":"2026-06-23T00:00:02Z"}),
            json!({"type":"CLAIM_CREATED","claimId":"claim-2","agentId":"agent-2","ts":"2026-06-23T00:00:03Z"}),
            json!({"type":"FILE_TOUCHED","agentId":"agent-1","files":["src/x.rs"],"ts":"2026-06-23T00:00:04Z"}),
            json!({"type":"HEAT_UPDATED","pair":["agent-1","agent-2"],"heat":40,"band":"MONITOR","ts":"2026-06-23T00:00:05Z"}),
            json!({"type":"HEAT_UPDATED","pair":["agent-1","agent-2"],"heat":82,"band":"CONFLICT_CANDIDATE","ts":"2026-06-23T00:00:06Z"}),
            json!({"type":"CONFLICT_OPENED","conflictId":"conflict-1","agents":["agent-1","agent-2"],"paths":["src/x.rs"],"ts":"2026-06-23T00:00:06Z"}),
            json!({"type":"CONFLICT_RESOLVED","conflictId":"conflict-1","resolution":"PARTICIPANT_STEPPED_ASIDE","ts":"2026-06-23T00:00:07Z"}),
            json!({"type":"CLAIM_RELEASED","claimId":"claim-1","agentId":"agent-1","reason":"TASK_COMPLETED","ts":"2026-06-23T00:00:08Z"}),
            json!({"type":"CLAIM_ORPHANED","claimId":"claim-2","previousOwner":"agent-2","ts":"2026-06-23T00:00:09Z"}),
        ]
    }

    #[test]
    fn summarize_counts_and_peak_and_agents() {
        let s = summarize(&sample());
        assert_eq!(s.agents_seen, 2);
        assert_eq!(s.claims_created, 2);
        assert_eq!(s.claims_released, 1);
        assert_eq!(s.files_touched, 1);
        assert_eq!(s.heat_updates, 2);
        assert_eq!(s.conflicts_opened, 1);
        assert_eq!(s.negotiated_resolved, 1);
        assert_eq!(s.peak_heat.heat, 82);
        assert_eq!(s.peak_heat.pair, vec!["agent-1", "agent-2"]);
        assert_eq!(s.duration_ms, 9000); // 00:00:00 -> 00:00:09
        let a1 = s.agents.get("agent-1").unwrap();
        assert_eq!(a1.claims_made, 1);
        assert_eq!(a1.tasks_completed, 1);
        assert_eq!(a1.conflicts_involved, 1);
        assert_eq!(a1.heat_generated_count, 2);
        assert_eq!(a1.heat_generated_sum, 122);
        let a2 = s.agents.get("agent-2").unwrap();
        assert_eq!(a2.ghost_claims, 1);
        assert_eq!(a2.tasks_completed, 0);
    }

    #[test]
    fn peak_heat_tie_keeps_earliest() {
        let evs = vec![
            json!({"type":"HEAT_UPDATED","pair":["a","b"],"heat":50,"band":"OVERLAP","ts":"2026-06-23T00:00:01Z"}),
            json!({"type":"HEAT_UPDATED","pair":["c","d"],"heat":50,"band":"OVERLAP","ts":"2026-06-23T00:00:02Z"}),
        ];
        let s = summarize(&evs);
        assert_eq!(s.peak_heat.pair, vec!["a", "b"]); // earliest of the tie
    }

    #[test]
    fn empty_events_zeroed() {
        let s = summarize(&[]);
        assert_eq!(s.agents_seen, 0);
        assert_eq!(s.duration_ms, 0);
        assert_eq!(s.peak_heat.heat, 0);
        assert!(s.agents.is_empty());
    }

    #[test]
    fn heat_history_extracts_series() {
        let h = heat_history(&sample());
        assert_eq!(h.len(), 2);
        assert_eq!(h[0]["heat"], 40);
        assert_eq!(h[1]["band"], "CONFLICT_CANDIDATE");
        assert_eq!(h[1]["pair"][0], "agent-1");
    }

    #[test]
    fn to_summary_has_sections() {
        let s = summarize(&sample());
        let doc = s.to_summary("recap", json!({"hotzones":{}}));
        assert_eq!(doc["claims"]["created"], 2);
        assert_eq!(doc["conflicts"]["opened"], 1);
        assert_eq!(doc["heat"]["peak"]["heat"], 82);
        assert_eq!(doc["narrative"], "recap");
        assert!(doc["knowledgeSnapshot"].is_object());
    }

    #[test]
    fn summarize_rare_event_types() {
        let evs = vec![
            json!({"type":"CLAIM_REJECTED","agentId":"agent-1","ts":"2026-06-23T00:00:00Z"}),
            json!({"type":"CONFLICT_TIMEOUT","conflictId":"c-1","ts":"2026-06-23T00:00:01Z"}),
            json!({"type":"CONFLICT_ABORTED","conflictId":"c-2","reason":"TIMEOUT","ts":"2026-06-23T00:00:02Z"}),
            json!({"type":"USER_ARBITRATION_REQUIRED","agents":["agent-1","agent-2"],"ts":"2026-06-23T00:00:03Z"}),
            json!({"type":"DEADLOCK_DETECTED","agents":["agent-1","agent-2","agent-3"],"ts":"2026-06-23T00:00:04Z"}),
        ];
        let s = summarize(&evs);
        assert_eq!(s.claims_rejected, 1);
        assert_eq!(s.timed_out, 1);
        assert_eq!(s.aborted, 1);
        assert_eq!(s.arbitrations_requested, 1);
        assert_eq!(s.deadlocks, 1);
        let a1 = s.agents.get("agent-1").unwrap();
        assert_eq!(a1.arbitrations_involved, 1);
        assert_eq!(a1.deadlocks_involved, 1);
        let a2 = s.agents.get("agent-2").unwrap();
        assert_eq!(a2.arbitrations_involved, 1);
        assert_eq!(a2.deadlocks_involved, 1);
        let a3 = s.agents.get("agent-3").unwrap();
        assert_eq!(a3.deadlocks_involved, 1);
        assert_eq!(a3.arbitrations_involved, 0);
    }

    #[test]
    fn profile_merge_accumulates_across_sessions() {
        let dir = std::env::temp_dir().join(format!("cp-{}-{}", std::process::id(), 1));
        let _ = std::fs::remove_dir_all(&dir);
        let mut tallies: BTreeMap<String, AgentTally> = BTreeMap::new();
        tallies.insert(
            "agent-1".into(),
            AgentTally {
                claims_made: 2,
                tasks_completed: 2,
                heat_generated_sum: 100,
                heat_generated_count: 4,
                conflicts_involved: 1,
                ..Default::default()
            },
        );

        let (mut store, q) = ProfileStore::load(&dir);
        assert!(q.is_empty());
        store.merge_session(&tallies);
        store.save_atomic(&dir).unwrap();

        // second session
        let (mut store2, _) = ProfileStore::load(&dir);
        store2.merge_session(&tallies);
        store2.save_atomic(&dir).unwrap();

        let (final_store, _) = ProfileStore::load(&dir);
        let p = final_store.agents.get("agent-1").unwrap();
        assert_eq!(p.sessions, 2);
        assert_eq!(p.claims_made, 4);
        assert_eq!(p.tasks_completed, 4);

        // derived velocity + overhead written
        let vel = std::fs::read_to_string(dir.join("velocity-profiles.json")).unwrap();
        assert!(vel.contains("tasksPerSession"));
        let ovh: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.join("coordination-overhead.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(ovh["agent-1"]["overheadScore"], 2); // conflicts 1+1, arb 0, deadlock 0
                                                        // prev rotation of agent-profiles
        assert!(dir.join("agent-profiles.json.prev").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn profile_corrupt_is_quarantined() {
        let dir = std::env::temp_dir().join(format!("cp-{}-{}", std::process::id(), 2));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("agent-profiles.json"), b"{bad").unwrap();
        let (store, q) = ProfileStore::load(&dir);
        assert_eq!(q.len(), 1);
        assert!(store.agents.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn summarize_edge_cases() {
        let evs = vec![
            // no ts → first_ts/last_ts None branch
            json!({"type":"AGENT_JOINED","agentId":"agent-x"}),
            // empty agentId → if !id.is_empty() false branch
            json!({"type":"AGENT_JOINED","agentId":"","ts":"2026-06-23T00:00:00Z"}),
            // empty previousOwner → if !id.is_empty() false branch in CLAIM_ORPHANED
            json!({"type":"CLAIM_ORPHANED","claimId":"c-1","previousOwner":"","ts":"2026-06-23T00:00:01Z"}),
            // FILE_TOUCHED with non-string element → if let Some(p) false branch
            json!({"type":"FILE_TOUCHED","agentId":"agent-x","files":[42],"ts":"2026-06-23T00:00:02Z"}),
            // HEAT_UPDATED with 3-element pair → ev_pair returns None (arr.len() != 2)
            json!({"type":"HEAT_UPDATED","pair":["a","b","c"],"heat":50,"band":"OVERLAP","ts":"2026-06-23T00:00:03Z"}),
            // HEAT_UPDATED: no pair key → e.get("pair")? = None (line 13, first ?)
            json!({"type":"HEAT_UPDATED","heat":30,"band":"SAFE","ts":"2026-06-23T00:00:03Z"}),
            // HEAT_UPDATED: pair is object (not array) → .as_array()? = None (line 13, second ?)
            json!({"type":"HEAT_UPDATED","pair":{"a":1},"heat":30,"band":"SAFE","ts":"2026-06-23T00:00:03Z"}),
            // HEAT_UPDATED: first element non-string → arr[0].as_str()? = None (line 15, first ?)
            json!({"type":"HEAT_UPDATED","pair":[42,"b"],"heat":30,"band":"SAFE","ts":"2026-06-23T00:00:03Z"}),
            // HEAT_UPDATED: second element non-string → arr[1].as_str()? = None (line 15, second ?)
            json!({"type":"HEAT_UPDATED","pair":["a",42],"heat":30,"band":"SAFE","ts":"2026-06-23T00:00:03Z"}),
            // CONFLICT_OPENED with 3-element agents → ev_pair returns None
            json!({"type":"CONFLICT_OPENED","conflictId":"c-x","agents":["a","b","c"],"paths":["x.rs"],"ts":"2026-06-23T00:00:04Z"}),
            // CONFLICT_RESOLVED: AUTO_RESOLVED_HEAT_DROPPED arm
            json!({"type":"CONFLICT_RESOLVED","conflictId":"c-1","resolution":"AUTO_RESOLVED_HEAT_DROPPED","ts":"2026-06-23T00:00:05Z"}),
            // CONFLICT_RESOLVED: USER_ARBITRATED arm
            json!({"type":"CONFLICT_RESOLVED","conflictId":"c-2","resolution":"USER_ARBITRATED","ts":"2026-06-23T00:00:06Z"}),
            // CONFLICT_RESOLVED: CO_OWNERSHIP arm (multi-pattern)
            json!({"type":"CONFLICT_RESOLVED","conflictId":"c-3","resolution":"CO_OWNERSHIP","ts":"2026-06-23T00:00:07Z"}),
            // CONFLICT_RESOLVED: unknown resolution → _ => {}
            json!({"type":"CONFLICT_RESOLVED","conflictId":"c-4","resolution":"UNKNOWN_RES","ts":"2026-06-23T00:00:08Z"}),
            // CONFLICT_ESCALATED
            json!({"type":"CONFLICT_ESCALATED","conflictId":"c-5","reason":"TIMEOUT","ts":"2026-06-23T00:00:09Z"}),
            // USER_ARBITRATION_REQUIRED with 1-element agents → ev_pair None (no per-agent tally)
            json!({"type":"USER_ARBITRATION_REQUIRED","agents":["agent-1"],"ts":"2026-06-23T00:00:10Z"}),
            // DEADLOCK_DETECTED without agents field → if let Some(arr) false branch
            json!({"type":"DEADLOCK_DETECTED","ts":"2026-06-23T00:00:11Z"}),
            // DEADLOCK_DETECTED with non-string element → id.as_str() None branch
            json!({"type":"DEADLOCK_DETECTED","agents":[42],"ts":"2026-06-23T00:00:12Z"}),
        ];
        let s = summarize(&evs);
        assert_eq!(s.auto_resolved_heat_dropped, 1);
        assert_eq!(s.user_arbitrated, 1);
        assert_eq!(s.negotiated_resolved, 1); // CO_OWNERSHIP
        assert_eq!(s.escalated, 1);
        assert_eq!(s.heat_updates, 5); // all five HEAT_UPDATED events increment heat_updates
        assert_eq!(s.deadlocks, 2); // both DEADLOCK_DETECTED events
        assert_eq!(s.arbitrations_requested, 1); // still incremented with malformed agents
        assert_eq!(s.files_touched, 0); // non-string file element skipped
    }

    #[test]
    fn profile_save_atomic_zero_counters() {
        // Agent with sessions=0, heat_generated_count=0 → covers the `else 0.0` branches
        // in save_atomic's velocity-profiles computation.
        let dir = std::env::temp_dir().join(format!("cp-{}-{}", std::process::id(), 3));
        let _ = std::fs::remove_dir_all(&dir);
        let mut store = ProfileStore::default();
        store
            .agents
            .insert("zero-agent".into(), AgentProfile::default());
        store.save_atomic(&dir).unwrap();
        let vel: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.join("velocity-profiles.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(vel["zero-agent"]["tasksPerSession"], 0.0);
        assert_eq!(vel["zero-agent"]["meanHeatGenerated"], 0.0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn profile_save_atomic_io_errors() {
        use std::os::unix::fs::PermissionsExt;

        let base = std::env::temp_dir().join(format!("cp-{}-io", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();

        // create_dir_all failure: write a regular file at the target dir path
        let blocked_dir = base.join("blocked");
        std::fs::write(&blocked_dir, b"not-a-dir").unwrap();
        let store = ProfileStore::default();
        assert!(store.save_atomic(&blocked_dir).is_err());

        // write_atomic failure: dir exists but is read-only
        let ro_dir = base.join("readonly");
        std::fs::create_dir_all(&ro_dir).unwrap();
        std::fs::set_permissions(&ro_dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        assert!(store.save_atomic(&ro_dir).is_err());
        std::fs::set_permissions(&ro_dir, std::fs::Permissions::from_mode(0o755)).unwrap();

        let _ = std::fs::remove_dir_all(&base);
    }
}
