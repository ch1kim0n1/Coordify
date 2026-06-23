use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

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
    chrono::DateTime::parse_from_rfc3339(ts).ok().map(|dt| dt.timestamp_millis())
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
                s.agents.entry(ev_str(e, "agentId")).or_default().claims_made += 1;
            }
            "CLAIM_REJECTED" => s.claims_rejected += 1,
            "CLAIM_RELEASED" => {
                s.claims_released += 1;
                if ev_str(e, "reason") == "TASK_COMPLETED" {
                    s.agents.entry(ev_str(e, "agentId")).or_default().tasks_completed += 1;
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
                        s.peak_heat = PeakHeat { heat, pair: vec![a, b], ts: ev_str(e, "ts") };
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
                            s.agents.entry(x.to_string()).or_default().deadlocks_involved += 1;
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev(v: serde_json::Value) -> serde_json::Value { v }

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
        assert_eq!(s.peak_heat.pair, vec!["agent-1","agent-2"]);
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
        assert_eq!(s.peak_heat.pair, vec!["a","b"]); // earliest of the tie
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

    // Suppress unused-import warning for `ev` helper (used as a no-op wrapper
    // in case the brief's tests use it directly).
    #[allow(dead_code)]
    fn _use_ev() { let _ = ev(serde_json::Value::Null); }
}
