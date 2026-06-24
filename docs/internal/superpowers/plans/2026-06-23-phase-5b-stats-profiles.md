# Phase 5b — Stats, Profiles & Entertainment Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** At finalize, derive session stats, a session summary, heat history, cross-session per-agent profiles, coordination overhead, and a rich set of deterministic, color-coded (no-emoji) entertainment metrics from `events.log`.

**Architecture:** Pure aggregation over the session's `events.log` (the recoverable source) invoked once at finalize. `stats.rs` produces `SessionStats` + a cross-session `ProfileStore`; `entertainment.rs` produces leaderboards/badges/superlatives/streaks/narrative. `persist_stats` (after `persist_knowledge`, both finalize sites) reads the log, summarizes, and atomic-writes the outputs. No per-event wiring, no new hot-path lock, no live-heat coupling.

**Tech Stack:** Rust (edition 2021). Deps limited to serde + derive, serde_json, chrono. No new crates.

## Global Constraints

- No new dependencies (serde, serde_json, chrono).
- Determinism: `summarize` and `build_entertainment` are pure — no clock, no randomness; timestamps come only from event `ts` fields. Same events → identical reports. Ties broken deterministically (leaderboards/badges: lowest agent-id; peak heat: earliest ts via order-preserving max).
- NO EMOJIS in any output. Facts are highlighted via a named `color` field from the fixed palette: `red`(heat/danger), `cyan`(calm), `blue`(cooperative), `yellow`(drama), `green`(success), `gray`(neutral/absent), `magenta`(superlative).
- Lock discipline: locks `{state, heat, conflict, waitgraph, knowledge, log}`; never two across a log append. `persist_stats` locks `knowledge` once (snapshot) in a closed scope, then does file IO; the failure-log lock is a separate scope.
- Atomic writes (temp+rename) with `.prev` rotation for the cross-session profile files. Counts are u64.
- 5b is pure reporting: live heat / conflicts / hot path are unchanged.
- Run cargo from `packages/coordify-core/`. `cargo test` + `cargo clippy --all-targets -- -D warnings` clean before each commit.

---

### Task 1: `stats.rs` — SessionStats + summarize + heat_history; expose atomic-IO helpers

**Files:**
- Modify: `packages/coordify-core/src/knowledge.rs` (make `write_atomic` + `quarantine` `pub(crate)`; add `KnowledgeStore::summary_json`)
- Create: `packages/coordify-core/src/stats.rs`
- Modify: `packages/coordify-core/src/lib.rs` (`pub mod stats;`)

**Interfaces:**
- Consumes: `serde_json::Value`; `chrono` for ISO parsing; the now-`pub(crate)` `knowledge::{write_atomic, with_suffix, quarantine}`.
- Produces:
  - `pub struct AgentTally { claims_made, tasks_completed, ghost_claims, conflicts_involved, heat_generated_sum, heat_generated_count, arbitrations_involved, deadlocks_involved : u64 }` (Debug, Default, Clone, Serialize).
  - `pub struct PeakHeat { heat: u64, pair: Vec<String>, ts: String }` (Default, Serialize).
  - `pub struct SessionStats { ...counts..., peak_heat: PeakHeat, duration_ms: i64, agents: BTreeMap<String, AgentTally> }` (Default, Serialize) with `pub fn to_summary(&self, narrative: &str, knowledge_snapshot: Value) -> Value`.
  - `pub fn summarize(events: &[Value]) -> SessionStats`.
  - `pub fn heat_history(events: &[Value]) -> Vec<Value>` — the `HEAT_UPDATED` series `[{pair, heat, band, ts}]`.

- [ ] **Step 1: Make the knowledge IO helpers reusable + add a JSON-safe snapshot**

In `packages/coordify-core/src/knowledge.rs`, change these two free fns from private to `pub(crate)` (bodies unchanged; leave `with_suffix` private — it is only used internally by `write_atomic`):
```rust
pub(crate) fn write_atomic(path: &Path, contents: &str) -> std::io::Result<()> {
```
```rust
pub(crate) fn quarantine(path: &Path, out: &mut Vec<String>) {
```

Also add a JSON-safe snapshot method to `impl KnowledgeStore` (the `heat::Knowledge` `coupling` map has tuple keys that cannot serialize to JSON, so build a portable shape here). Add `use std::collections::BTreeMap;` to knowledge.rs imports if not present:
```rust
    /// JSON-portable view of the knowledge for the session summary:
    /// { "hotzones": {path: score}, "coupling": [{a,b,score}] }, scores = n/(n+k).
    pub fn summary_json(&self, k: f64) -> serde_json::Value {
        let score = |n: u64| (n as f64) / (n as f64 + k);
        let hotzones: std::collections::BTreeMap<&String, f64> =
            self.hotzone_counts.iter().map(|(p, &n)| (p, score(n))).collect();
        let coupling: Vec<serde_json::Value> = self
            .coupling_counts
            .iter()
            .map(|((a, b), &n)| serde_json::json!({ "a": a, "b": b, "score": score(n) }))
            .collect();
        serde_json::json!({ "hotzones": hotzones, "coupling": coupling })
    }
```
(Note: `hotzone_counts`/`coupling_counts` are the existing private fields of `KnowledgeStore`; the method lives in the same module so it can read them. `coupling_counts` is iterated in sorted order — use a `BTreeMap` if the field is a `HashMap` and determinism of the array matters; if it is already a `HashMap`, collect into a `BTreeMap<(String,String),u64>` first or sort the resulting Vec by `(a,b)` for deterministic output.)

- [ ] **Step 2: Write the failing tests** (create `stats.rs` with the test module first)

```rust
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
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p coordify-core --lib stats:: 2>&1 | tail -20`
Expected: FAIL — `summarize`/`SessionStats` not found.

- [ ] **Step 4: Implement** (prepend to `stats.rs`, above the tests)

```rust
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
```

Add `pub mod stats;` to `lib.rs`.

- [ ] **Step 5: Run tests + clippy, commit**

Run: `cargo test -p coordify-core --lib stats:: 2>&1 | tail -15` (PASS)
Run: `cargo clippy -p coordify-core --lib -- -D warnings 2>&1 | tail -5` (clean)
```bash
git add packages/coordify-core/src/stats.rs packages/coordify-core/src/lib.rs packages/coordify-core/src/knowledge.rs
git commit -m "feat(core): stats.rs — pure summarize(events) + heat_history + session summary"
```

---

### Task 2: `stats.rs` — cross-session ProfileStore

**Files:**
- Modify: `packages/coordify-core/src/stats.rs`

**Interfaces:**
- Consumes: `AgentTally` (Task 1); `knowledge::{write_atomic, with_suffix, quarantine}` (now pub(crate)).
- Produces:
  - `pub struct AgentProfile { sessions, claims_made, tasks_completed, ghost_claims, conflicts_involved, heat_generated_sum, heat_generated_count, arbitrations_involved, deadlocks_involved : u64 }` (Debug, Default, Clone, Serialize, Deserialize; camelCase).
  - `pub struct ProfileStore { agents: BTreeMap<String, AgentProfile> }`.
  - `ProfileStore::load(dir) -> (Self, Vec<String> quarantined)`, `merge_session(&mut self, &BTreeMap<String, AgentTally>)`, `save_atomic(&self, dir) -> io::Result<()>` (writes `agent-profiles.json` + derived `velocity-profiles.json` + `coordination-overhead.json`, each with `.prev` rotation).

- [ ] **Step 1: Write the failing tests** (add to `stats.rs` tests)

```rust
    #[test]
    fn profile_merge_accumulates_across_sessions() {
        let dir = std::env::temp_dir().join(format!("cp-{}-{}", std::process::id(), 1));
        let _ = std::fs::remove_dir_all(&dir);
        let mut tallies: BTreeMap<String, AgentTally> = BTreeMap::new();
        tallies.insert("agent-1".into(), AgentTally { claims_made: 2, tasks_completed: 2, heat_generated_sum: 100, heat_generated_count: 4, conflicts_involved: 1, ..Default::default() });

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
        let ovh: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("coordination-overhead.json")).unwrap()).unwrap();
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
```

- [ ] **Step 2: Run to verify fail**

Run: `cargo test -p coordify-core --lib stats::tests::profile 2>&1 | tail -15`
Expected: FAIL — `ProfileStore` not found.

- [ ] **Step 3: Implement** (add to `stats.rs`, before the tests; add `use serde::Deserialize;` and `use std::path::Path;` to the imports)

```rust
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
        let profiles = serde_json::to_string_pretty(&self.agents).unwrap_or_else(|_| "{}".into());
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
            &serde_json::to_string_pretty(&velocity).unwrap_or_else(|_| "{}".into()),
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
            &serde_json::to_string_pretty(&overhead).unwrap_or_else(|_| "{}".into()),
        )?;
        Ok(())
    }
}
```

- [ ] **Step 4: Run tests + clippy, commit**

Run: `cargo test -p coordify-core --lib stats:: 2>&1 | tail -15` (PASS)
Run: `cargo clippy -p coordify-core --lib -- -D warnings 2>&1 | tail -5` (clean)
```bash
git add packages/coordify-core/src/stats.rs
git commit -m "feat(core): cross-session ProfileStore (agent/velocity/overhead, .prev rotation)"
```

---

### Task 3: `entertainment.rs` — leaderboards, badges, superlatives, streaks, narrative (color-coded, no emoji)

**Files:**
- Create: `packages/coordify-core/src/entertainment.rs`
- Modify: `packages/coordify-core/src/lib.rs` (`pub mod entertainment;`)

**Interfaces:**
- Consumes: `serde_json::Value`; `crate::stats::{SessionStats, AgentTally}`.
- Produces: `pub fn build_entertainment(events: &[Value], stats: &SessionStats) -> Entertainment` and the serializable `Entertainment { leaderboards, badges, superlatives, streaks, narrative }`.

**Exact rules (the contract; the tests below pin them).** All deterministic. Tie-break = lowest agent-id. A badge/leaderboard entry is emitted only for a non-trivial winner (value > 0 unless noted). Colors from the fixed palette.

- **Leaderboards** (each: top entries sorted by value desc, ties by agent-id asc; include only agents with value > 0): `most_claims` (claims_made, green), `most_tasks_completed` (tasks_completed, green), `most_files_touched` — derive per-agent touched-file counts from `FILE_TOUCHED` events (gray), `most_heat_generated` (heat_generated_sum, red), `lowest_avg_heat` (mean heat where count>0, ascending, cyan), `most_conflicts_involved` (conflicts_involved, yellow).
- **Badges** (`{id,label,color,agent}`, plain labels): `firestarter` ("Firestarter", red) = max heat_generated_sum>0; `sprinter` ("Sprinter", green) = max tasks_completed>0; `ghost` ("Ghost", gray) = max ghost_claims>0; `conflict_magnet` ("Conflict Magnet", yellow) = max conflicts_involved>0; `diplomat` ("Diplomat", blue) = max count of `CONFLICT_PROPOSAL_RECEIVED` with kind in {YIELD_CLAIM, CO_OWNERSHIP}... use the event's `kind` field values `YIELD_CLAIM`/`CO_OWN`/... by `from` (>0); `hotzone_hero` ("Hotzone Hero", red) = lowest-id agent who `FILE_TOUCHED` the battleground file (if a battleground exists); `lone_wolf` ("Lone Wolf", gray) = lowest-id agent with claims_made>0 and conflicts_involved==0; `pacifist` ("Pacifist", cyan) = lowest-id agent with conflicts_involved>0 and arbitrations_involved==0; `sniper` ("Sniper", green) = lowest-id agent with claims_made>0, tasks_completed==claims_made, ghost_claims==0; `speed_demon` ("Speed Demon", green) = agent with the smallest mean (release_ts − create_ts) over claims matched by `claimId` (CLAIM_CREATED→CLAIM_RELEASED), requires ≥1 matched claim.
- **Superlatives** (`{key,label,color,value}`): `the_battleground` (red) = the file with the most occurrences across `CONFLICT_OPENED.paths` + `FILE_TOUCHED.files` (value = {file, count}); `peak_heat_moment` (red) = stats.peak_heat (value = {heat,pair,ts}); `biggest_spike` (magenta) = max positive delta between consecutive `HEAT_UPDATED.heat` on the same `pair` (value = {pair, from, to, delta}); `mexican_standoffs` (yellow) = {count: stats.deadlocks}; `court_cases` (yellow) = {count: stats.arbitrations_requested}; `longest_negotiation` (magenta) = max (close_ts − open_ts) span matching `CONFLICT_OPENED.conflictId` to the first subsequent `CONFLICT_RESOLVED`/`CONFLICT_ESCALATED` with the same conflictId (value = {conflictId, ms}); `bloodiest_minute` (magenta) = the maximum number of events whose ts falls in any 60_000ms window, computed by bucketing each event ts to `floor(ms/60000)` and taking the largest bucket (value = {count}).
- **Streaks** (`{longest_auto_resolve_streak, longest_completion_streak}`): iterate events in order. Auto-resolve streak = longest run of consecutive conflict-outcome events (`CONFLICT_RESOLVED`/`CONFLICT_ESCALATED`/`CONFLICT_TIMEOUT`/`CONFLICT_ABORTED`) that are `CONFLICT_RESOLVED`. Completion streak = longest run of consecutive `CLAIM_RELEASED` events whose reason is `TASK_COMPLETED`.
- **Narrative** = a deterministic plain-text, multi-line `String` built from stats + the winners above. No emojis. Empty/quiet session → "Quiet session — no conflicts, no drama." plus the basic counts. Example shape:
  `"Session recap: {agentsSeen} agents, {claimsCreated} claims, {conflictsOpened} conflicts ({autoResolvedHeatDropped+negotiatedResolved} resolved, {escalated} escalated), {deadlocks} deadlocks.\n"` then one line per present badge (`"{label}: {agent}."`) and `"The battleground was {file}."` when present.

- [ ] **Step 1: Write the failing tests** (create `entertainment.rs` with the test module first)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::summarize;
    use serde_json::json;

    fn drama() -> Vec<serde_json::Value> {
        vec![
            json!({"type":"AGENT_JOINED","agentId":"agent-1","ts":"2026-06-23T00:00:00Z"}),
            json!({"type":"AGENT_JOINED","agentId":"agent-2","ts":"2026-06-23T00:00:00Z"}),
            json!({"type":"CLAIM_CREATED","claimId":"claim-1","agentId":"agent-1","ts":"2026-06-23T00:00:01Z"}),
            json!({"type":"CLAIM_CREATED","claimId":"claim-2","agentId":"agent-2","ts":"2026-06-23T00:00:01Z"}),
            json!({"type":"FILE_TOUCHED","agentId":"agent-1","files":["src/hot.rs"],"ts":"2026-06-23T00:00:02Z"}),
            json!({"type":"FILE_TOUCHED","agentId":"agent-2","files":["src/hot.rs"],"ts":"2026-06-23T00:00:02Z"}),
            json!({"type":"HEAT_UPDATED","pair":["agent-1","agent-2"],"heat":20,"band":"SAFE","ts":"2026-06-23T00:00:03Z"}),
            json!({"type":"HEAT_UPDATED","pair":["agent-1","agent-2"],"heat":82,"band":"CONFLICT_CANDIDATE","ts":"2026-06-23T00:00:04Z"}),
            json!({"type":"CONFLICT_OPENED","conflictId":"conflict-1","agents":["agent-1","agent-2"],"paths":["src/hot.rs"],"ts":"2026-06-23T00:00:04Z"}),
            json!({"type":"CONFLICT_PROPOSAL_RECEIVED","conflictId":"conflict-1","from":"agent-1","kind":"YIELD_CLAIM","ts":"2026-06-23T00:00:05Z"}),
            json!({"type":"CONFLICT_RESOLVED","conflictId":"conflict-1","resolution":"PARTICIPANT_STEPPED_ASIDE","ts":"2026-06-23T00:00:06Z"}),
            json!({"type":"CLAIM_RELEASED","claimId":"claim-1","agentId":"agent-1","reason":"TASK_COMPLETED","ts":"2026-06-23T00:00:07Z"}),
        ]
    }

    const PALETTE: &[&str] = &["red","cyan","blue","yellow","green","gray","magenta"];

    #[test]
    fn badges_awarded_to_right_agents_no_emoji() {
        let evs = drama();
        let stats = summarize(&evs);
        let e = build_entertainment(&evs, &stats);
        // firestarter = whoever generated most heat; both share the pair so equal -> tie to agent-1
        let fire = e.badges.iter().find(|b| b.id == "firestarter").unwrap();
        assert_eq!(fire.agent, "agent-1");
        assert_eq!(fire.color, "red");
        // diplomat = agent-1 (only YIELD proposer)
        assert_eq!(e.badges.iter().find(|b| b.id == "diplomat").unwrap().agent, "agent-1");
        // sprinter = agent-1 (only completer)
        assert_eq!(e.badges.iter().find(|b| b.id == "sprinter").unwrap().agent, "agent-1");
        // every badge color is in the palette; every label is plain ASCII (no emoji)
        for b in &e.badges {
            assert!(PALETTE.contains(&b.color.as_str()), "bad color {}", b.color);
            assert!(b.label.chars().all(|c| (c as u32) < 0x7F), "non-ASCII label {}", b.label);
        }
    }

    #[test]
    fn battleground_and_peak_superlatives() {
        let evs = drama();
        let stats = summarize(&evs);
        let e = build_entertainment(&evs, &stats);
        let bg = e.superlatives.iter().find(|s| s.key == "the_battleground").unwrap();
        assert_eq!(bg.value["file"], "src/hot.rs");
        let peak = e.superlatives.iter().find(|s| s.key == "peak_heat_moment").unwrap();
        assert_eq!(peak.value["heat"], 82);
        let spike = e.superlatives.iter().find(|s| s.key == "biggest_spike").unwrap();
        assert_eq!(spike.value["delta"], 62); // 82 - 20 on the same pair
    }

    #[test]
    fn leaderboards_sorted_and_tie_broken() {
        let evs = drama();
        let stats = summarize(&evs);
        let e = build_entertainment(&evs, &stats);
        let lb = e.leaderboards.iter().find(|l| l.metric == "most_tasks_completed").unwrap();
        assert_eq!(lb.entries[0].agent, "agent-1");
    }

    #[test]
    fn streaks_counted() {
        let evs = drama();
        let stats = summarize(&evs);
        let e = build_entertainment(&evs, &stats);
        assert_eq!(e.streaks.longest_auto_resolve_streak, 1);
        assert_eq!(e.streaks.longest_completion_streak, 1);
    }

    #[test]
    fn narrative_plain_text_no_emoji() {
        let evs = drama();
        let stats = summarize(&evs);
        let e = build_entertainment(&evs, &stats);
        assert!(!e.narrative.is_empty());
        assert!(e.narrative.chars().all(|c| (c as u32) < 0x7F), "narrative has non-ASCII/emoji");
        assert!(e.narrative.contains("recap") || e.narrative.contains("Session"));
    }

    #[test]
    fn quiet_session_graceful() {
        let stats = summarize(&[]);
        let e = build_entertainment(&[], &stats);
        assert!(!e.narrative.is_empty());
        assert!(e.badges.is_empty());
        assert_eq!(e.streaks.longest_auto_resolve_streak, 0);
    }
}
```

- [ ] **Step 2: Run to verify fail**

Run: `cargo test -p coordify-core --lib entertainment:: 2>&1 | tail -20`
Expected: FAIL — module/`build_entertainment` not found. (Add `pub mod entertainment;` to `lib.rs` first so the test compiles to the not-found state.)

- [ ] **Step 3: Implement `entertainment.rs`**

Prepend the implementation above the test module. Implement exactly the rules listed in the Interfaces section. Use these serializable types and a `color` palette of plain strings:

```rust
use crate::stats::SessionStats;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Serialize)]
pub struct LeaderEntry { pub agent: String, pub value: f64 }
#[derive(Serialize)]
pub struct Leaderboard { pub metric: String, pub color: String, pub entries: Vec<LeaderEntry> }
#[derive(Serialize)]
pub struct Badge { pub id: String, pub label: String, pub color: String, pub agent: String }
#[derive(Serialize)]
pub struct Superlative { pub key: String, pub label: String, pub color: String, pub value: Value }
#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Streaks { pub longest_auto_resolve_streak: u64, pub longest_completion_streak: u64 }
#[derive(Serialize)]
pub struct Entertainment {
    pub leaderboards: Vec<Leaderboard>,
    pub badges: Vec<Badge>,
    pub superlatives: Vec<Superlative>,
    pub streaks: Streaks,
    pub narrative: String,
}
```

Implementation notes (follow exactly):
- Helper `fn ev_type/ev_str/parse_ms` — mirror `stats.rs` (or `pub(crate)` reuse from `stats.rs`; if reusing, mark those fns `pub(crate)` in stats.rs and import — your choice, keep it DRY without over-engineering).
- `leaderboard(metric, color, pairs: Vec<(agent,f64)>)`: filter value>0 (for `lowest_avg_heat` filter count>0 and sort ascending), sort by value (desc, except lowest_avg ascending) then agent-id asc; map to `LeaderEntry`.
- `badge winner` helpers: `max_by(tally_fn)` returning the lowest-id agent with the max value when max>0; `first_matching(pred)` returning the lowest-id agent satisfying a predicate.
- Battleground: count occurrences per file across `CONFLICT_OPENED.paths` (each path +1) and `FILE_TOUCHED.files` (each file +1); pick max count (tie → lexicographically smallest file). Emit superlative + enable `hotzone_hero`.
- `biggest_spike`: keep `BTreeMap<(String,String), u64>` of last heat per ordered pair; for each `HEAT_UPDATED`, if the pair was seen, delta = new − last (only positive); track max.
- `longest_negotiation`: `BTreeMap<conflictId, open_ms>`; on `CONFLICT_RESOLVED`/`CONFLICT_ESCALATED` with a known conflictId, span = close_ms − open_ms; track max; remove the entry.
- `bloodiest_minute`: `BTreeMap<i64, u64>` bucket = `parse_ms(ts)/60000`; max bucket count.
- Streaks: single pass; maintain current run + max for each rule per the spec definitions.
- Narrative: `format!` the recap line + per-present-badge lines + battleground line. ASCII only.
- Emit badges/superlatives only when non-trivial (winner value>0 / battleground exists / spike found). Quiet session → empty badges, narrative = a fixed "Quiet session — no conflicts, no drama." prefixed recap.

- [ ] **Step 4: Run tests + clippy, commit**

Run: `cargo test -p coordify-core --lib entertainment:: 2>&1 | tail -15` (PASS)
Run: `cargo clippy -p coordify-core --lib -- -D warnings 2>&1 | tail -5` (clean)
```bash
git add packages/coordify-core/src/entertainment.rs packages/coordify-core/src/lib.rs
git commit -m "feat(core): entertainment.rs — color-coded leaderboards, badges, superlatives, narrative"
```

---

### Task 4: `persist_stats` wiring + integration test

**Files:**
- Modify: `packages/coordify-core/src/server.rs`
- Modify: `packages/coordify-core/tests/integration.rs`

**Interfaces:**
- Consumes: `stats::{summarize, heat_history, ProfileStore}`, `entertainment::build_entertainment`, `Session`, `Paths::knowledge_dir`, `KnowledgeStore::snapshot` (Tasks 1-3).
- Produces: `persist_stats(shared, session, paths)` called after `persist_knowledge` at both finalize sites.

- [ ] **Step 1: Write the integration test** (`tests/integration.rs`, modelled on `knowledge_files_written_after_conflict_session`, using `spawn_core_fast_proposal_timeout` or `spawn_core_fast_reaper` so the silent agent is reaped and finalize fires within the poll window)

```rust
#[test]
fn stats_files_written_after_session() {
    let core = spawn_core_fast_reaper("stat");
    let token = read_token(&core.root);
    let sock = core.root.join(".coordify/runtime/core.sock");
    let mut stream = connect_retry(&sock);

    let reg = |id: &str| format!(r#"{{"id":"{}","token":"{}","action":"register","meta":{{"branch":"main"}}}}"#, id, token);
    let a = send_line(&mut stream, &reg("1"))["agent_id"].as_str().unwrap().to_string();
    let b = send_line(&mut stream, &reg("2"))["agent_id"].as_str().unwrap().to_string();
    let mk = |id: &str, agent: &str| format!(
        r#"{{"id":"{}","token":"{}","action":"submit_event","capVersion":"0.1","event":{{"type":"CLAIM_PROPOSED","agentId":"{}","intent":"BUGFIX","domains":["AUTH"],"estimatedFiles":["src/auth/session.ts"],"task":{{"summary":"fix"}},"confidence":0.9}}}}"#,
        id, token, agent);
    assert_eq!(send_line(&mut stream, &mk("3", &a))["ok"], true);
    assert_eq!(send_line(&mut stream, &mk("4", &b))["ok"], true);

    drop(stream); // both agents go silent -> reaper finalizes

    let sdir = core.root.join(".coordify/sessions");
    let kdir = core.root.join(".coordify/knowledge");
    let start = std::time::Instant::now();
    let mut summary = String::new();
    while start.elapsed() < Duration::from_secs(4) {
        if let Ok(entries) = std::fs::read_dir(&sdir) {
            for e in entries.flatten() {
                let f = e.path().join("session-summary.json");
                if f.exists() { summary = std::fs::read_to_string(f).unwrap(); }
            }
        }
        if !summary.is_empty() && kdir.join("agent-profiles.json").exists() { break; }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!summary.is_empty(), "session-summary.json written");
    assert!(summary.contains("\"conflicts\""), "summary has conflicts section:\n{summary}");
    assert!(summary.contains("\"narrative\""), "summary has narrative");
    // sibling per-session files
    let any_session = std::fs::read_dir(&sdir).unwrap().flatten().map(|e| e.path()).find(|p| p.join("stats.json").exists()).expect("a session dir with stats.json");
    assert!(any_session.join("stats.json").exists());
    assert!(any_session.join("heat-history.json").exists());
    assert!(any_session.join("entertainment.json").exists());
    // cross-session profiles
    assert!(kdir.join("agent-profiles.json").exists(), "agent-profiles.json written");
    let prof = std::fs::read_to_string(kdir.join("agent-profiles.json")).unwrap();
    assert!(prof.contains(&a) || prof.contains("claimsMade"), "profiles populated:\n{prof}");
}
```

- [ ] **Step 2: Run to verify fail**

Run: `cargo test -p coordify-core --test integration stats_files_written 2>&1 | tail -20`
Expected: FAIL — stats files never written (persist_stats not wired).

- [ ] **Step 3: Implement `persist_stats` and wire both sites**

In `packages/coordify-core/src/server.rs`, add the free helper near `persist_knowledge`:
```rust
/// Derive and atomically persist the session's stats, summary, heat-history,
/// entertainment, and cross-session profiles from events.log. Best-effort:
/// failures log STATS_PERSIST_FAILED, never block finalize.
fn persist_stats(shared: &Shared, session: &Session, paths: &Paths) {
    let result = (|| -> std::io::Result<()> {
        let raw = std::fs::read_to_string(session.dir.join("events.log"))?;
        let events: Vec<serde_json::Value> = raw
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();

        let stats = crate::stats::summarize(&events);
        let ent = crate::entertainment::build_entertainment(&events, &stats);
        let snapshot = {
            let store = shared.knowledge.lock().unwrap();
            store.summary_json(shared.knowledge_k)
        };

        // Per-session files.
        crate::knowledge::write_atomic(
            &session.dir.join("stats.json"),
            &serde_json::to_string_pretty(&stats).unwrap_or_else(|_| "{}".into()),
        )?;
        crate::knowledge::write_atomic(
            &session.dir.join("session-summary.json"),
            &serde_json::to_string_pretty(&stats.to_summary(&ent.narrative, snapshot)).unwrap_or_else(|_| "{}".into()),
        )?;
        crate::knowledge::write_atomic(
            &session.dir.join("heat-history.json"),
            &serde_json::to_string_pretty(&crate::stats::heat_history(&events)).unwrap_or_else(|_| "[]".into()),
        )?;
        crate::knowledge::write_atomic(
            &session.dir.join("entertainment.json"),
            &serde_json::to_string_pretty(&ent).unwrap_or_else(|_| "{}".into()),
        )?;

        // Cross-session profiles (merge + derived views).
        let (mut profiles, _q) = crate::stats::ProfileStore::load(&paths.knowledge_dir());
        profiles.merge_session(&stats.agents);
        profiles.save_atomic(&paths.knowledge_dir())?;
        Ok(())
    })();
    if let Err(e) = result {
        let _ = shared.log.lock().unwrap().append(&serde_json::json!({
            "type": "STATS_PERSIST_FAILED",
            "error": e.to_string(),
            "ts": crate::bootstrap::now_iso(),
        }));
    }
}
```
(The snapshot uses `KnowledgeStore::summary_json` from Task 1 — JSON-safe, no dependency on `heat::Knowledge` being `Serialize`, so `heat.rs` is NOT modified by this task.)

Wire it after `persist_knowledge` at BOTH finalize sites:

Run-loop branch (currently):
```rust
                persist_knowledge(&shared, &paths);
                finalize(&session, &paths, seen)?;
```
becomes:
```rust
                persist_knowledge(&shared, &paths);
                persist_stats(&shared, &session, &paths);
                finalize(&session, &paths, seen)?;
```

Reaper branch (currently):
```rust
            persist_knowledge(&shared, &paths);
            let _ = finalize(&session, &paths, seen);
```
becomes:
```rust
            persist_knowledge(&shared, &paths);
            persist_stats(&shared, &session, &paths);
            let _ = finalize(&session, &paths, seen);
```

- [ ] **Step 4: Full suite + clippy + flakiness, commit**

Run: `cargo test -p coordify-core 2>&1 | tail -15` (all PASS)
Run: `cargo clippy -p coordify-core --all-targets -- -D warnings 2>&1 | tail -5` (clean)
Run: `for i in 1 2 3; do cargo test -p coordify-core --test integration stats_files_written 2>&1 | grep "test result"; done` (3/3 stable)
```bash
git add packages/coordify-core/src/server.rs packages/coordify-core/tests/integration.rs
git commit -m "feat(core): persist_stats — write stats/summary/heat-history/entertainment + profiles at finalize"
```

---

## Notes for the Final Whole-Branch Review

- Lock discipline: `persist_stats` locks `knowledge` once (snapshot) in a closed scope, then file IO; the `STATS_PERSIST_FAILED` log lock is separate. No two of `{state, heat, conflict, waitgraph, knowledge, log}` co-held. It runs only in the `finalized` CAS-won branch (exactly once) at shutdown.
- Determinism: `summarize` / `build_entertainment` use no clock/random; peak-heat tie keeps earliest (order-preserving strict-greater); leaderboard/badge ties → lowest agent-id. No emojis (tests assert ASCII labels + narrative; colors from the fixed palette).
- Pure-reporting: confirm no change to heat/conflict/claim hot paths — only two `persist_stats` lines added at finalize, plus `KnowledgeStore::summary_json` (new method) and the two `pub(crate)` visibility bumps. `heat.rs` is untouched.
- Reporting reads `events.log` at shutdown when the network is empty (no concurrent writers).
- Coverage ≥90% (target ≥95%); uncovered limited to IO-fault paths.
