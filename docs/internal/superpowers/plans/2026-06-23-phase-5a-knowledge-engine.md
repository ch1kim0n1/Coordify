# Phase 5a — Knowledge Engine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Derive a hotzone map + coupling graph from accepted events, accumulate them (in-memory + persisted across sessions), and feed them into live heat so `historicalHotzoneRisk`/`historicalCoupling` finally fire.

**Architecture:** A new `KnowledgeStore` holds integer counts; a saturating curve `n/(n+K)` derives the `[0,1]` scores that the existing `heat::Knowledge` feeds into `compute_heat`. `Shared.knowledge` becomes a `Mutex<KnowledgeStore>`, snapshotted (cloned to scores) for the pure heat compute and updated on conflict-open / claim-create. Counts load at startup and atomic-write (with `.prev` rotation) at finalize.

**Tech Stack:** Rust (edition 2021). Dependencies limited to `serde` + derive, `serde_json`, `chrono`. No new crates.

## Global Constraints

- No new dependencies; only `serde`, `serde_json`, `chrono`.
- Determinism: counts→scores via `n / (n + K)` (K default 5.0); same counts → identical scores. No clock/random in scoring.
- Lock discipline: locks are `{state, heat, conflict, waitgraph, knowledge, log}`. NEVER hold two across a log append. `knowledge` is locked alone and briefly — snapshot for compute, update separately. Collect data inside other locks' scopes, then take `knowledge` after they drop (mirrors the existing `conflict_events` collect-then-log pattern). A same-thread re-lock of a `std::Mutex` deadlocks the suite.
- Accrue on transitions only: `record_conflict(paths)` once per NEW conflict open; `record_claim_files(files)` once per `CLAIM_CREATED`. Never accrue per-recompute (would inflate counts).
- Knowledge files use atomic writes (temp + rename) with `.prev` rotation. Counts (u64), not scores, are persisted.
- Knowledge is derived from accepted events; Core is the only writer. No LLM.
- Run cargo from `packages/coordify-core/` (the crate has its own manifest; there is no workspace root). Keep `cargo test` and `cargo clippy -- -D warnings` clean before each commit.

---

### Task 1: `knowledge.rs` — KnowledgeStore (counts, scores, load/save, quarantine)

**Files:**
- Create: `packages/coordify-core/src/knowledge.rs`
- Modify: `packages/coordify-core/src/lib.rs` (add `pub mod knowledge;`)
- Modify: `packages/coordify-core/src/paths.rs` (add knowledge dir paths)

**Interfaces:**
- Consumes: `crate::heat::Knowledge` (the score struct: `{ hotzones: HashMap<String,f64>, coupling: HashMap<(String,String),f64> }`).
- Produces:
  - `pub struct KnowledgeStore` (default-constructible, `new()`).
  - `record_conflict(&mut self, paths: &[String])` — +1 hotzone per path, +1 coupling per unordered pair.
  - `record_claim_files(&mut self, files: &[String])` — +1 coupling per unordered pair.
  - `snapshot(&self, k: f64) -> heat::Knowledge` — scores via `n/(n+k)`.
  - `hotzone_count(&self, path) -> u64`, `coupling_count(&self, a, b) -> u64` (test accessors).
  - `save_atomic(&self, dir: &Path) -> std::io::Result<()>` — rotate `.prev`, temp+rename.
  - `load(dir: &Path) -> (KnowledgeStore, Vec<String>)` — second tuple element is the list of quarantined file paths.
  - `Paths::knowledge_dir(&self) -> PathBuf`.

- [ ] **Step 1: Add the module declaration and path helper, write failing tests**

In `packages/coordify-core/src/lib.rs`, add alongside the other `pub mod` lines:
```rust
pub mod knowledge;
```

In `packages/coordify-core/src/paths.rs`, add to `impl Paths` (after `sessions`):
```rust
    pub fn knowledge_dir(&self) -> PathBuf {
        self.coordify().join("knowledge")
    }
```

Create `packages/coordify-core/src/knowledge.rs` with the test module first (implementation in Step 2):
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn record_conflict_accrues_hotzone_and_coupling() {
        let mut s = KnowledgeStore::new();
        s.record_conflict(&["a.rs".into(), "b.rs".into()]);
        assert_eq!(s.hotzone_count("a.rs"), 1);
        assert_eq!(s.hotzone_count("b.rs"), 1);
        assert_eq!(s.coupling_count("a.rs", "b.rs"), 1);
        // direction-independent
        assert_eq!(s.coupling_count("b.rs", "a.rs"), 1);
        s.record_conflict(&["a.rs".into(), "b.rs".into()]);
        assert_eq!(s.hotzone_count("a.rs"), 2);
        assert_eq!(s.coupling_count("a.rs", "b.rs"), 2);
    }

    #[test]
    fn record_claim_files_accrues_coupling_only_pairs() {
        let mut s = KnowledgeStore::new();
        s.record_claim_files(&["x".into(), "y".into(), "z".into()]);
        // 3 files -> 3 unordered pairs, each count 1; no hotzone accrual
        assert_eq!(s.coupling_count("x", "y"), 1);
        assert_eq!(s.coupling_count("x", "z"), 1);
        assert_eq!(s.coupling_count("y", "z"), 1);
        assert_eq!(s.hotzone_count("x"), 0);
        // single-file claim -> no pairs
        let mut s2 = KnowledgeStore::new();
        s2.record_claim_files(&["solo".into()]);
        assert_eq!(s2.coupling_count("solo", "solo"), 0);
    }

    #[test]
    fn snapshot_uses_saturating_curve() {
        let mut s = KnowledgeStore::new();
        for _ in 0..5 { s.record_conflict(&["f".into()]); } // count 5
        let k = s.snapshot(5.0);
        // 5/(5+5) = 0.5
        assert!((k.hotzone_risk("f") - 0.5).abs() < 1e-9);
        // unseen file -> 0
        assert_eq!(k.hotzone_risk("missing"), 0.0);
        // n=0 path absent
        let empty = KnowledgeStore::new().snapshot(5.0);
        assert!(empty.hotzones.is_empty());
    }

    #[test]
    fn save_then_load_round_trips_counts() {
        let dir = std::env::temp_dir().join(format!("ck-{}-{}", std::process::id(), 1));
        let _ = std::fs::remove_dir_all(&dir);
        let mut s = KnowledgeStore::new();
        s.record_conflict(&["a".into(), "b".into()]);
        s.record_claim_files(&["a".into(), "c".into()]);
        s.save_atomic(&dir).unwrap();
        let (loaded, quarantined) = KnowledgeStore::load(&dir);
        assert!(quarantined.is_empty());
        assert_eq!(loaded.hotzone_count("a"), 1);
        assert_eq!(loaded.coupling_count("a", "b"), 1);
        assert_eq!(loaded.coupling_count("a", "c"), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_rotates_prev() {
        let dir = std::env::temp_dir().join(format!("ck-{}-{}", std::process::id(), 2));
        let _ = std::fs::remove_dir_all(&dir);
        let mut s = KnowledgeStore::new();
        s.record_conflict(&["a".into()]);
        s.save_atomic(&dir).unwrap();          // first write, no prev
        let mut s2 = KnowledgeStore::new();
        s2.record_conflict(&["a".into()]);
        s2.record_conflict(&["a".into()]);      // count 2
        s2.save_atomic(&dir).unwrap();          // rotates prior (count 1) to .prev
        assert!(dir.join("hotzones.json.prev").exists());
        let prev: HashMap<String, u64> =
            serde_json::from_str(&std::fs::read_to_string(dir.join("hotzones.json.prev")).unwrap()).unwrap();
        assert_eq!(prev.get("a").copied(), Some(1));
        let cur: HashMap<String, u64> =
            serde_json::from_str(&std::fs::read_to_string(dir.join("hotzones.json")).unwrap()).unwrap();
        assert_eq!(cur.get("a").copied(), Some(2));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_file_is_quarantined_and_map_starts_empty() {
        let dir = std::env::temp_dir().join(format!("ck-{}-{}", std::process::id(), 3));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("hotzones.json"), b"{ not json").unwrap();
        let (loaded, quarantined) = KnowledgeStore::load(&dir);
        assert_eq!(quarantined.len(), 1);
        assert_eq!(loaded.hotzone_count("anything"), 0);
        // original corrupt file moved out of the way
        assert!(!dir.join("hotzones.json").exists());
        assert!(dir.join("quarantine").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p coordify-core --lib knowledge:: 2>&1 | tail -20`
Expected: FAIL — `KnowledgeStore` not found (compile error).

- [ ] **Step 3: Implement `knowledge.rs`**

Prepend to `packages/coordify-core/src/knowledge.rs` (above the test module):
```rust
use crate::heat::Knowledge;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Ordered key so (a,b) and (b,a) map to the same coupling entry.
fn pair(a: &str, b: &str) -> (String, String) {
    if a <= b {
        (a.to_string(), b.to_string())
    } else {
        (b.to_string(), a.to_string())
    }
}

/// Append a literal suffix to a path (e.g. ".prev", ".tmp") without losing the
/// existing extension — `with_extension` would replace `.json`.
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

#[derive(Serialize, Deserialize)]
struct CouplingEdge {
    a: String,
    b: String,
    count: u64,
}

/// Persisted integer counts. Scores are derived on demand (see `snapshot`).
#[derive(Default)]
pub struct KnowledgeStore {
    hotzone_counts: HashMap<String, u64>,
    coupling_counts: HashMap<(String, String), u64>,
}

impl KnowledgeStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_conflict(&mut self, paths: &[String]) {
        for p in paths {
            *self.hotzone_counts.entry(p.clone()).or_insert(0) += 1;
        }
        self.accrue_pairs(paths);
    }

    pub fn record_claim_files(&mut self, files: &[String]) {
        self.accrue_pairs(files);
    }

    fn accrue_pairs(&mut self, files: &[String]) {
        for i in 0..files.len() {
            for j in (i + 1)..files.len() {
                let k = pair(&files[i], &files[j]);
                *self.coupling_counts.entry(k).or_insert(0) += 1;
            }
        }
    }

    /// Derive `heat::Knowledge` scores via the saturating curve `n / (n + k)`.
    pub fn snapshot(&self, k: f64) -> Knowledge {
        let score = |n: u64| (n as f64) / (n as f64 + k);
        Knowledge {
            hotzones: self
                .hotzone_counts
                .iter()
                .map(|(p, &n)| (p.clone(), score(n)))
                .collect(),
            coupling: self
                .coupling_counts
                .iter()
                .map(|(k2, &n)| (k2.clone(), score(n)))
                .collect(),
        }
    }

    pub fn hotzone_count(&self, path: &str) -> u64 {
        self.hotzone_counts.get(path).copied().unwrap_or(0)
    }

    pub fn coupling_count(&self, a: &str, b: &str) -> u64 {
        self.coupling_counts.get(&pair(a, b)).copied().unwrap_or(0)
    }

    /// Atomic write of both knowledge files: rotate the existing file to `.prev`,
    /// write a temp file, rename over the canonical name.
    pub fn save_atomic(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;

        let hz_path = dir.join("hotzones.json");
        let hz = serde_json::to_string_pretty(&self.hotzone_counts).unwrap_or_else(|_| "{}".into());
        write_atomic(&hz_path, &hz)?;

        let cp_path = dir.join("coupling-graph.json");
        let edges: Vec<CouplingEdge> = self
            .coupling_counts
            .iter()
            .map(|((a, b), &count)| CouplingEdge { a: a.clone(), b: b.clone(), count })
            .collect();
        let cp = serde_json::to_string_pretty(&edges).unwrap_or_else(|_| "[]".into());
        write_atomic(&cp_path, &cp)?;
        Ok(())
    }

    /// Load counts from `dir`. A file that fails to parse is moved into
    /// `dir/quarantine/` and its map starts empty; the returned Vec lists the
    /// quarantined paths (for the caller to log).
    pub fn load(dir: &Path) -> (Self, Vec<String>) {
        let mut store = Self::default();
        let mut quarantined = Vec::new();

        let hz_path = dir.join("hotzones.json");
        if hz_path.exists() {
            match std::fs::read_to_string(&hz_path)
                .ok()
                .and_then(|s| serde_json::from_str::<HashMap<String, u64>>(&s).ok())
            {
                Some(m) => store.hotzone_counts = m,
                None => quarantine(&hz_path, &mut quarantined),
            }
        }

        let cp_path = dir.join("coupling-graph.json");
        if cp_path.exists() {
            match std::fs::read_to_string(&cp_path)
                .ok()
                .and_then(|s| serde_json::from_str::<Vec<CouplingEdge>>(&s).ok())
            {
                Some(edges) => {
                    for e in edges {
                        store.coupling_counts.insert(pair(&e.a, &e.b), e.count);
                    }
                }
                None => quarantine(&cp_path, &mut quarantined),
            }
        }

        (store, quarantined)
    }
}

fn write_atomic(path: &Path, contents: &str) -> std::io::Result<()> {
    if path.exists() {
        // Rotate the last-good file to .prev (best-effort).
        let _ = std::fs::rename(path, with_suffix(path, ".prev"));
    }
    let tmp = with_suffix(path, ".tmp");
    std::fs::write(&tmp, contents)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn quarantine(path: &Path, out: &mut Vec<String>) {
    let dir = path.parent().map(|p| p.join("quarantine"));
    if let Some(qdir) = dir {
        let _ = std::fs::create_dir_all(&qdir);
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "unknown".into());
        let stamp = crate::bootstrap::now_iso().replace(':', "-");
        let dest = qdir.join(format!("{name}.{stamp}"));
        if std::fs::rename(path, &dest).is_ok() {
            out.push(dest.to_string_lossy().to_string());
        } else {
            out.push(path.to_string_lossy().to_string());
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p coordify-core --lib knowledge:: 2>&1 | tail -20`
Then: `cargo clippy -p coordify-core --lib -- -D warnings 2>&1 | tail -5`
Expected: PASS (all 6 `knowledge::tests`), clippy clean.

- [ ] **Step 5: Commit**

```bash
git add packages/coordify-core/src/knowledge.rs packages/coordify-core/src/lib.rs packages/coordify-core/src/paths.rs
git commit -m "feat(core): KnowledgeStore — counts, saturating scores, atomic persist + quarantine"
```

---

### Task 2: Wire knowledge into Shared + live heat

**Files:**
- Modify: `packages/coordify-core/src/server.rs`

**Interfaces:**
- Consumes: `crate::knowledge::KnowledgeStore` (Task 1); `crate::heat::Knowledge`.
- Produces: `Shared.knowledge: Mutex<KnowledgeStore>` (replaces `knowledge: Knowledge`); `Shared.knowledge_k: f64`; knowledge snapshotted for `compute_heat`; counts updated on conflict-open and claim-create.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `packages/coordify-core/src/server.rs` (uses the existing `open_conflict_between_two` helper and `shared_for_test`):
```rust
    #[test]
    fn conflict_open_accrues_hotzone_and_feeds_live_heat() {
        let s = shared_for_test("good");
        let (_a, _b, _id) = open_conflict_between_two(&s);
        // The conflict opened on src/auth/session.ts -> hotzone count >= 1.
        let count = s.knowledge.lock().unwrap().hotzone_count("src/auth/session.ts");
        assert!(count >= 1, "expected hotzone accrual, got {count}");
        // A snapshot now scores that file > 0, so live heat would include it.
        let k = s.knowledge.lock().unwrap().snapshot(s.knowledge_k);
        assert!(k.hotzone_risk("src/auth/session.ts") > 0.0, "hotzone risk should be live");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p coordify-core --lib server::tests::conflict_open_accrues 2>&1 | tail -20`
Expected: FAIL — `s.knowledge.lock()` does not compile (field is `Knowledge`, not `Mutex<KnowledgeStore>`) / `knowledge_k` missing.

- [ ] **Step 3: Implement the wiring**

In `packages/coordify-core/src/server.rs`:

Update imports — add `KnowledgeStore`, and DROP `Knowledge` from the heat import (after this task `Knowledge` is no longer named in `server.rs` — the snapshot's type is inferred — so leaving it imported fails `clippy -D warnings` as an unused import):
```rust
use crate::knowledge::KnowledgeStore;
use crate::heat::{self, HeatBand, HeatConfig};
```
(Keep `self`, `HeatBand`, `HeatConfig`; remove `Knowledge`. If the compiler later reports `Knowledge` IS still referenced somewhere, re-add it — but the wiring below names only the local snapshot variable, never the type.)

In `struct Shared`, replace the `pub knowledge: Knowledge,` field with:
```rust
    pub knowledge: Mutex<KnowledgeStore>,
    pub knowledge_k: f64,
```

In `run()`, before constructing `Shared`, load the store and read K:
```rust
    let knowledge_k = std::env::var("COORDIFY_KNOWLEDGE_K")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5.0);
    let (knowledge_store, quarantined) = KnowledgeStore::load(&paths.knowledge_dir());
```
Then in the `Shared { .. }` literal, replace `knowledge: Knowledge::default(),` with:
```rust
        knowledge: Mutex::new(knowledge_store),
        knowledge_k,
```
Immediately after `let shared = Arc::new(Shared { .. });` in `run()`, log any quarantined files:
```rust
    for f in &quarantined {
        let _ = shared.log.lock().unwrap().append(&serde_json::json!({
            "type": "KNOWLEDGE_QUARANTINED",
            "file": f,
            "reason": "PARSE_FAILED",
            "ts": crate::bootstrap::now_iso(),
        }));
    }
```

In the test constructor `shared_for_test`, replace `knowledge: Knowledge::default(),` with:
```rust
            knowledge: Mutex::new(KnowledgeStore::new()),
            knowledge_k: 5.0,
```

In `predicted_heat`, snapshot once at the top and use it instead of `&shared.knowledge`:
```rust
fn predicted_heat(shared: &Shared, proposed: &heat::HeatInputs) -> Vec<PredictedEdge> {
    let knowledge = shared.knowledge.lock().unwrap().snapshot(shared.knowledge_k);
    let others = {
        // ... unchanged ...
    };
    others
        .iter()
        .map(|other| {
            let r = heat::compute_heat(proposed, other, &knowledge, &shared.heat_cfg);
            // ... unchanged ...
        })
        .collect()
}
```
(Replace the `&shared.knowledge` argument with `&knowledge`.)

In `recompute_current_heat`, snapshot knowledge before the compute loop:
```rust
    // Snapshot knowledge (scores) under a short lock for the pure compute.
    let knowledge = shared.knowledge.lock().unwrap().snapshot(shared.knowledge_k);

    // Compute (pure). Keep each other's inputs for conflict metadata.
    let mut updates: Vec<(heat::HeatInputs, heat::HeatResult)> = Vec::new();
    for other in others {
        let result = heat::compute_heat(&mine, &other, &knowledge, &shared.heat_cfg);
        updates.push((other, result));
    }
```
(Replace the `&shared.knowledge` argument with `&knowledge`.)

In the conflict-decision block of `recompute_current_heat`, collect the paths of NEWLY opened conflicts so they can be accrued after the lock drops. Add a vector before the `let mut cstore = ...` block and push inside the `if let Some(c) = cstore.open(...)` arm:
```rust
    let mut conflict_events: Vec<serde_json::Value> = Vec::new();
    let mut accrue_paths: Vec<Vec<String>> = Vec::new();
    {
        let mut cstore = shared.conflicts.lock().unwrap();
        for (other, result) in &updates {
            let other_id = &other.agent_id;
            if result.band == HeatBand::ConflictCandidate {
                if !cstore.has_open(agent_id, other_id) {
                    let paths: Vec<String> =
                        mine.files.intersection(&other.files).cloned().collect();
                    let domains: Vec<String> =
                        mine.domains.union(&other.domains).cloned().collect();
                    let intents = vec![mine.intent.clone(), other.intent.clone()];
                    if let Some(c) = cstore.open(agent_id, other_id, result.heat, now_ms(), paths, domains, intents) {
                        accrue_paths.push(c.paths.clone());
                        let ts = crate::bootstrap::now_iso();
                        conflict_events.push(serde_json::json!({
                            // ... unchanged CONFLICT_OPENED event ...
                        }));
                    }
                }
            } else if cstore.has_open(agent_id, other_id) {
                // ... unchanged resolve arm ...
            }
        }
    }
    // Accrue knowledge from newly-opened conflicts (knowledge lock alone).
    if !accrue_paths.is_empty() {
        let mut k = shared.knowledge.lock().unwrap();
        for paths in &accrue_paths {
            k.record_conflict(paths);
        }
    }
```
(The `conflict_events` / log block that follows is unchanged.)

In `handle_cap_event`, the `CapEvent::ClaimProposed` arm: accrue claim-file coupling after a successful `CLAIM_CREATED`. The `estimated_files` are moved into `claims.propose`, so capture a clone BEFORE the propose call and use it after. In the existing arm, before the `let outcome = { ... }` block that calls `propose`, add:
```rust
            let claim_files = estimated_files.clone();
```
Then in the `Some(Some(claim)) => { ... }` branch, after the `recompute_current_heat(shared, &agent_id);` call, add:
```rust
                    if claim_files.len() >= 2 {
                        shared.knowledge.lock().unwrap().record_claim_files(&claim_files);
                    }
```

- [ ] **Step 4: Run tests + clippy**

Run: `cargo test -p coordify-core --lib 2>&1 | tail -15`
Run: `cargo clippy -p coordify-core --all-targets -- -D warnings 2>&1 | tail -5`
Expected: PASS (including the new `conflict_open_accrues_hotzone_and_feeds_live_heat`), clippy clean.

- [ ] **Step 5: Commit**

```bash
git add packages/coordify-core/src/server.rs
git commit -m "feat(core): KnowledgeStore in Shared, live-heat snapshot + conflict/claim accrual"
```

---

### Task 3: Persist knowledge at finalize + integration test

**Files:**
- Modify: `packages/coordify-core/src/server.rs`
- Modify: `packages/coordify-core/tests/integration.rs`

**Interfaces:**
- Consumes: `KnowledgeStore::save_atomic`, `Paths::knowledge_dir` (Task 1); the two finalize sites in `run()` and `spawn_reaper`.
- Produces: a `persist_knowledge(shared, paths)` helper called immediately before each `finalize`; an end-to-end test asserting the knowledge files appear after a session that opened a conflict.

- [ ] **Step 1: Write the integration test**

Add to `packages/coordify-core/tests/integration.rs`, modelled on `high_overlap_claims_open_conflict` (raw JSON strings, two registers on one stream, drop, poll the sessions dir — but here poll the **knowledge dir** under the project root):
```rust
#[test]
fn knowledge_files_written_after_conflict_session() {
    let core = spawn_core("know");
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
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p coordify-core --test integration knowledge_files_written 2>&1 | tail -20`
Expected: FAIL — the knowledge dir/files are never written (persist not wired yet).

- [ ] **Step 3: Implement `persist_knowledge` and call it before both finalize sites**

In `packages/coordify-core/src/server.rs`, add a free helper (near the other free fns, e.g. after `sweep_proposal_timeouts`):
```rust
/// Atomically persist the knowledge counts to the project's knowledge dir.
/// Best-effort: a write failure is swallowed (finalize must not be blocked).
fn persist_knowledge(shared: &Shared, paths: &Paths) {
    let store = shared.knowledge.lock().unwrap();
    if let Err(e) = store.save_atomic(&paths.knowledge_dir()) {
        let _ = shared.log.lock().unwrap().append(&serde_json::json!({
            "type": "KNOWLEDGE_PERSIST_FAILED",
            "error": e.to_string(),
            "ts": crate::bootstrap::now_iso(),
        }));
    }
}
```
NOTE: `store` (the knowledge guard) is dropped at the end of the `if let` only if not held across the log lock. To respect lock discipline, restructure so the knowledge guard is released before the log lock is taken:
```rust
fn persist_knowledge(shared: &Shared, paths: &Paths) {
    let result = {
        let store = shared.knowledge.lock().unwrap();
        store.save_atomic(&paths.knowledge_dir())
    };
    if let Err(e) = result {
        let _ = shared.log.lock().unwrap().append(&serde_json::json!({
            "type": "KNOWLEDGE_PERSIST_FAILED",
            "error": e.to_string(),
            "ts": crate::bootstrap::now_iso(),
        }));
    }
}
```
(Use this second form — the knowledge guard is dropped before the log lock.)

In `run()`, in the run-loop finalize branch, call it just before `finalize`:
```rust
            if network_empty && seen > 0
                && shared.finalized.compare_exchange(false, true, SeqCst, SeqCst).is_ok()
            {
                persist_knowledge(&shared, &paths);
                finalize(&session, &paths, seen)?;
                break;
            } else if network_empty && seen > 0 {
```

In `spawn_reaper`, in the reaper finalize branch, call it just before `finalize`:
```rust
        if empty
            && seen > 0
            && shared.finalized.compare_exchange(false, true, SeqCst, SeqCst).is_ok()
        {
            persist_knowledge(&shared, &paths);
            let _ = finalize(&session, &paths, seen);
            std::process::exit(0);
        }
```
(`paths` is the `Paths` already in scope in each function. `Paths` does not implement `Copy`; both call sites pass `&shared` and `&paths` by reference, so no move occurs.)

- [ ] **Step 4: Run the full suite + clippy, check flakiness**

Run: `cargo test -p coordify-core 2>&1 | tail -15`
Run: `cargo clippy -p coordify-core --all-targets -- -D warnings 2>&1 | tail -5`
Then confirm the new integration test is stable: `for i in 1 2 3; do cargo test -p coordify-core --test integration knowledge_files_written 2>&1 | grep "test result"; done`
Expected: ALL PASS; 3/3 stable.

- [ ] **Step 5: Commit**

```bash
git add packages/coordify-core/src/server.rs packages/coordify-core/tests/integration.rs
git commit -m "feat(core): persist knowledge at finalize (.prev rotation) + e2e test"
```

---

## Notes for the Final Whole-Branch Review

- Lock discipline: confirm `knowledge` is never held across a log append and never nested with `heat`/`conflict`/`state`. The accrual collects `accrue_paths` inside the conflict-lock scope, then takes `knowledge` after that scope closes; `persist_knowledge` drops the knowledge guard before the log lock.
- Determinism: `snapshot` is pure `n/(n+K)`; counts accrue only on transitions (new conflict open, claim create), not per-recompute — confirm no path inflates counts on repeated recomputes.
- One-event lag: heat reads the snapshot taken before the same recompute's accrual; confirm the snapshot precedes the conflict-open accrual.
- Persistence: `.prev` rotation precedes the temp write; the rename is atomic; a corrupt file at load is quarantined and never aborts startup.
- Coverage ≥90% (target ≥95%); uncovered limited to IO-fault paths.
