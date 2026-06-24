# FILE_TOUCHED Ingestion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Core ingests `FILE_TOUCHED`, folds actual touched files into heat's file-overlap and the knowledge coupling graph, and the adapter forwards `FILE_TOUCHED` instead of recording it.

**Architecture:** A new `FILE_TOUCHED` CAP event adds files to the agent's live claim's `actual_files` set; `heat_inputs_for` feeds heat the union `estimated ∪ actual`; co-touched files accrue coupling (new-pairs-only). The adapter's `PostToolUse(Edit/Write)` mapping flips from recorded-only to forwarded.

**Tech Stack:** Rust Core (edition 2021; serde, serde_json, chrono only) + Node adapter (`packages/coordify-hook/`, stdlib only, `node:test`).

## Global Constraints

- No new dependencies (Rust: serde/serde_json/chrono; Node: stdlib only).
- Lock discipline (Core): locks `{state, heat, conflict, waitgraph, knowledge, log}`; NEVER hold two across a log append. The FILE_TOUCHED handler: state lock (add + capture new files) → drop → `recompute_current_heat` → knowledge lock alone (coupling) → log — each scope closes before the next.
- Coupling accrues on transitions only (new co-touch pairs), never re-accruing historical pairs — no count inflation.
- Hotzone stays conflict-driven; a plain touched file gets NO hotzone increment.
- Determinism preserved; Core is the only writer; no LLM.
- Adapter hooks stay emit-only and crash-safe (the flip changes only the mapping result).
- Run Core cargo from `packages/coordify-core/`; run adapter tests from `packages/coordify-hook/` (`npm test`). Keep `cargo test` + `cargo clippy -- -D warnings` clean (Core) and the adapter suite green before each commit.

---

### Task 1: Data model + pure helpers (cap, claim, state, knowledge)

**Files:**
- Modify: `packages/coordify-core/src/cap.rs`
- Modify: `packages/coordify-core/src/claim.rs`
- Modify: `packages/coordify-core/src/state.rs`
- Modify: `packages/coordify-core/src/knowledge.rs`

**Interfaces:**
- Produces:
  - `CapEvent::FileTouched { agent_id: String, files: Vec<String> }` (serde camelCase: `agentId`, `files`; `files` defaults to `[]`).
  - `Claim.actual_files: std::collections::BTreeSet<String>`.
  - `ClaimStore::record_touched(&mut self, agent_id: &str, files: &[String]) -> Option<Vec<String>>` — inserts into the agent's live claim's `actual_files`, returns the newly-inserted subset (empty if all present), `None` if no live claim.
  - `state::heat_inputs_for` builds `HeatInputs.files` from `estimated_files ∪ actual_files`.
  - `KnowledgeStore::record_cotouch(&mut self, all_files: &[String], new_files: &[String])` — +1 coupling for every unordered pair of `all_files` with at least one endpoint in `new_files`.

- [ ] **Step 1: Write the failing tests**

In `packages/coordify-core/src/claim.rs` tests module, add:
```rust
    #[test]
    fn record_touched_adds_dedups_and_returns_new() {
        let mut s = ClaimStore::new();
        let c = s.propose("agent-1", "t".into(), "BUGFIX".into(), vec![], vec![], 0.9).unwrap();
        assert_eq!(c.status, ClaimStatus::Active);
        // first touch: both new
        let new = s.record_touched("agent-1", &["a.rs".into(), "b.rs".into()]).unwrap();
        assert_eq!(new, vec!["a.rs".to_string(), "b.rs".to_string()]);
        // re-touch a.rs + new c.rs: only c.rs is new
        let new2 = s.record_touched("agent-1", &["a.rs".into(), "c.rs".into()]).unwrap();
        assert_eq!(new2, vec!["c.rs".to_string()]);
        // no live claim
        assert!(s.record_touched("agent-404", &["x".into()]).is_none());
    }
```

In `packages/coordify-core/src/state.rs` tests module, add (use the module's existing helpers to register an agent + propose a claim; if none, build a `State` directly as other tests do):
```rust
    #[test]
    fn heat_inputs_union_estimated_and_actual_files() {
        let mut st = State::new();
        let id = st.register(serde_json::json!({}), 1000);
        st.claims.propose(&id, "t".into(), "BUGFIX".into(), vec![], vec!["est.rs".into()], 0.9);
        st.promote_active(&id);
        st.claims.record_touched(&id, &["act.rs".into()]);
        let inputs = st.heat_inputs_for(&id).unwrap();
        assert!(inputs.files.contains("est.rs"));
        assert!(inputs.files.contains("act.rs"));
    }
```
(If `State::register`/`promote_active` signatures differ, mirror the exact calls used by the existing `state.rs` tests for setting up an agent with a live claim.)

In `packages/coordify-core/src/knowledge.rs` tests module, add:
```rust
    #[test]
    fn record_cotouch_accrues_only_pairs_touching_new() {
        let mut s = KnowledgeStore::new();
        // existing actual set {a,b}, newly touched {c}: accrue (a,c) and (b,c), NOT (a,b)
        s.record_cotouch(&["a".into(), "b".into(), "c".into()], &["c".into()]);
        assert_eq!(s.coupling_count("a", "c"), 1);
        assert_eq!(s.coupling_count("b", "c"), 1);
        assert_eq!(s.coupling_count("a", "b"), 0);
        // touching two new files together: (a,b) among the new pair accrues too
        let mut s2 = KnowledgeStore::new();
        s2.record_cotouch(&["a".into(), "b".into()], &["a".into(), "b".into()]);
        assert_eq!(s2.coupling_count("a", "b"), 1);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p coordify-core --lib 2>&1 | tail -20`
Expected: FAIL — `record_touched`, `actual_files`, `record_cotouch` not found (compile errors).

- [ ] **Step 3: Implement**

`cap.rs` — add to the `CapEvent` enum (after `ConflictUserDecision`):
```rust
    #[serde(rename_all = "camelCase")]
    FileTouched {
        agent_id: String,
        #[serde(default)]
        files: Vec<String>,
    },
```

`claim.rs` — add the import at the top (alongside the existing `use`):
```rust
use std::collections::BTreeSet;
```
Add the field to `struct Claim` (after `estimated_files`):
```rust
    pub actual_files: BTreeSet<String>,
```
Initialise it in `propose`'s `Claim { .. }` literal (after `estimated_files,`):
```rust
            actual_files: BTreeSet::new(),
```
Add the method to `impl ClaimStore` (after `live_claim_for`):
```rust
    /// Add touched files to the agent's live claim's actual-file set.
    /// Returns the subset that was newly inserted (empty if all already
    /// present), or None if the agent has no live claim.
    pub fn record_touched(&mut self, agent_id: &str, files: &[String]) -> Option<Vec<String>> {
        let id = self
            .claims
            .values()
            .find(|c| {
                c.agent_id == agent_id
                    && matches!(c.status, ClaimStatus::Active | ClaimStatus::Provisional)
            })
            .map(|c| c.claim_id.clone())?;
        let claim = self.claims.get_mut(&id).unwrap();
        let mut newly = Vec::new();
        for f in files {
            if claim.actual_files.insert(f.clone()) {
                newly.push(f.clone());
            }
        }
        Some(newly)
    }
```

`state.rs` — in `heat_inputs_for`, change the `files` field to the union:
```rust
            files: claim
                .estimated_files
                .iter()
                .cloned()
                .chain(claim.actual_files.iter().cloned())
                .collect(),
```

`knowledge.rs` — add to `impl KnowledgeStore` (after `record_claim_files`):
```rust
    /// Accrue +1 coupling for every unordered pair of `all_files` that has at
    /// least one endpoint in `new_files`. Pairs of pre-existing files (neither
    /// endpoint new) are NOT re-accrued — avoids inflation on repeated touches.
    pub fn record_cotouch(&mut self, all_files: &[String], new_files: &[String]) {
        let new: std::collections::BTreeSet<&String> = new_files.iter().collect();
        for i in 0..all_files.len() {
            for j in (i + 1)..all_files.len() {
                if new.contains(&all_files[i]) || new.contains(&all_files[j]) {
                    let k = pair(&all_files[i], &all_files[j]);
                    *self.coupling_counts.entry(k).or_insert(0) += 1;
                }
            }
        }
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p coordify-core --lib 2>&1 | tail -15`
Then: `cargo clippy -p coordify-core --lib -- -D warnings 2>&1 | tail -5`
Expected: PASS (new tests green; the whole lib still compiles — note `server.rs`'s `match` on `CapEvent` is now non-exhaustive; add a temporary stub arm so the crate compiles and this task commits green:

In `packages/coordify-core/src/server.rs` `handle_cap_event`, add a temporary arm before the closing `}` of the `match event` (it is fully replaced in Task 2):
```rust
        CapEvent::FileTouched { .. } => Response::ok_for(&req.id),
```
). Re-run the lib tests; PASS.

- [ ] **Step 5: Commit**

```bash
git add packages/coordify-core/src/cap.rs packages/coordify-core/src/claim.rs packages/coordify-core/src/state.rs packages/coordify-core/src/knowledge.rs packages/coordify-core/src/server.rs
git commit -m "feat(core): FILE_TOUCHED event, claim actual_files, heat union, cotouch coupling"
```

---

### Task 2: Server FILE_TOUCHED handler + tests

**Files:**
- Modify: `packages/coordify-core/src/server.rs`
- Modify: `packages/coordify-core/tests/integration.rs`

**Interfaces:**
- Consumes: `CapEvent::FileTouched`, `ClaimStore::record_touched`, `KnowledgeStore::record_cotouch`, `state::heat_inputs_for` union (Task 1); existing `recompute_current_heat`, `cap_err`.
- Produces: the real `FileTouched` handler replacing the Task-1 stub.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `packages/coordify-core/src/server.rs` (helpers `shared_for_test`, `req`, `cap_req` exist):
```rust
    #[test]
    fn file_touched_raises_overlap_heat_and_accrues_coupling() {
        let s = shared_for_test("good");
        let a = handle_request(&s, &{ let mut r = req("good", "register"); r.meta = json!({"branch":"main"}); r }).agent_id.unwrap();
        let b = handle_request(&s, &{ let mut r = req("good", "register"); r.meta = json!({"branch":"main"}); r }).agent_id.unwrap();
        // Disjoint estimated files -> low overlap.
        let ca = json!({"type":"CLAIM_PROPOSED","agentId":a,"intent":"BUGFIX","domains":["AUTH"],"estimatedFiles":["src/a.rs"],"task":{"summary":"alpha work"},"confidence":0.9});
        let cb = json!({"type":"CLAIM_PROPOSED","agentId":b,"intent":"BUGFIX","domains":["AUTH"],"estimatedFiles":["src/b.rs"],"task":{"summary":"beta work"},"confidence":0.9});
        assert!(handle_request(&s, &cap_req("good", ca)).ok);
        assert!(handle_request(&s, &cap_req("good", cb)).ok);
        // Both touch the SAME file -> overlap heat rises.
        assert!(handle_request(&s, &cap_req("good", json!({"type":"FILE_TOUCHED","agentId":a,"files":["src/shared.rs","src/a.rs"]}))).ok);
        assert!(handle_request(&s, &cap_req("good", json!({"type":"FILE_TOUCHED","agentId":b,"files":["src/shared.rs"]}))).ok);
        let edge = s.heat.lock().unwrap().get(&a, &b).expect("edge exists").heat;
        assert!(edge > 25, "shared touched file should raise heat, got {edge}");
        // a touched two files together -> they couple.
        assert!(s.knowledge.lock().unwrap().coupling_count("src/shared.rs", "src/a.rs") >= 1);
    }

    #[test]
    fn file_touched_unknown_agent_errors() {
        let s = shared_for_test("good");
        let resp = handle_request(&s, &cap_req("good", json!({"type":"FILE_TOUCHED","agentId":"agent-404","files":["x"]})));
        assert!(!resp.ok);
        assert_eq!(resp.error.as_deref(), Some("AGENT_NOT_FOUND"));
    }

    #[test]
    fn file_touched_claimless_agent_errors() {
        let s = shared_for_test("good");
        let a = handle_request(&s, &req("good", "register")).agent_id.unwrap();
        // Registered but no claim -> CLAIM_NOT_FOUND.
        let resp = handle_request(&s, &cap_req("good", json!({"type":"FILE_TOUCHED","agentId":a,"files":["x"]})));
        assert!(!resp.ok);
        assert_eq!(resp.error.as_deref(), Some("CLAIM_NOT_FOUND"));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p coordify-core --lib server::tests::file_touched 2>&1 | tail -20`
Expected: FAIL — the stub arm returns ok for everything (unknown-agent/claimless tests fail; the heat/coupling test fails because the stub does nothing).

- [ ] **Step 3: Implement the handler**

In `packages/coordify-core/src/server.rs`, replace the Task-1 stub arm
```rust
        CapEvent::FileTouched { .. } => Response::ok_for(&req.id),
```
with:
```rust
        CapEvent::FileTouched { agent_id, files } => {
            // Add touched files under a short state lock; capture the new subset.
            let outcome = {
                let mut st = shared.state.lock().unwrap();
                if st.agent_state(&agent_id).is_none() {
                    None // agent unknown
                } else {
                    Some(st.claims.record_touched(&agent_id, &files))
                }
            };
            let new_files = match outcome {
                None => return cap_err(&req.id, CapErrorCode::AgentNotFound),
                Some(None) => return cap_err(&req.id, CapErrorCode::ClaimNotFound),
                Some(Some(v)) => v,
            };
            {
                let _ = shared.log.lock().unwrap().append(&serde_json::json!({
                    "type": "FILE_TOUCHED",
                    "agentId": agent_id,
                    "files": files,
                    "newFiles": new_files,
                    "ts": crate::bootstrap::now_iso(),
                }));
            }
            // Heat reflects the new files.
            recompute_current_heat(shared, &agent_id);
            // Coupling among co-touched files (new x existing), knowledge lock alone.
            if !new_files.is_empty() {
                let all_files: Vec<String> = {
                    let st = shared.state.lock().unwrap();
                    st.claims
                        .live_claim_for(&agent_id)
                        .map(|c| c.actual_files.iter().cloned().collect())
                        .unwrap_or_default()
                };
                if all_files.len() >= 2 {
                    shared.knowledge.lock().unwrap().record_cotouch(&all_files, &new_files);
                }
            }
            Response::ok_for(&req.id)
        }
```

- [ ] **Step 4: Add a Core integration test**

In `packages/coordify-core/tests/integration.rs`, modelled on `high_overlap_claims_open_conflict` (raw JSON strings, two registers on one stream), add:
```rust
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
}
```

- [ ] **Step 5: Run tests + clippy + commit**

Run: `cargo test -p coordify-core 2>&1 | tail -15`
Run: `cargo clippy -p coordify-core --all-targets -- -D warnings 2>&1 | tail -5`
Then confirm the integration test is stable: `for i in 1 2 3; do cargo test -p coordify-core --test integration file_touched_over_socket 2>&1 | grep "test result"; done`
Expected: ALL PASS; 3/3 stable.

```bash
git add packages/coordify-core/src/server.rs packages/coordify-core/tests/integration.rs
git commit -m "feat(core): FILE_TOUCHED handler — actual files into heat + cotouch coupling"
```

---

### Task 3: Adapter flip — forward FILE_TOUCHED

**Files:**
- Modify: `packages/coordify-hook/lib/mapping.js`
- Modify: `packages/coordify-hook/test/mapping.test.js`

**Interfaces:**
- Consumes: nothing new; the sidecar already forwards `{kind:'forward', event}` and injects `agentId`.
- Produces: `PostToolUse(Edit/Write/MultiEdit)` maps to `{kind:'forward', event:{type:'FILE_TOUCHED', files:[file]}}`.

- [ ] **Step 1: Update the failing tests**

In `packages/coordify-hook/test/mapping.test.js`, change the `PostToolUse` Edit/Write assertion. Replace the existing line that asserts `FILE_TOUCHED` as a record with a forward assertion, and keep Read/Bash as records:
```js
test('PostToolUse Edit/Write -> forwarded FILE_TOUCHED', () => {
  const w = mapEvent('PostToolUse', { tool_name: 'Write', tool_input: { file_path: 'src/x.rs' } });
  assert.equal(w.kind, 'forward');
  assert.equal(w.event.type, 'FILE_TOUCHED');
  assert.deepEqual(w.event.files, ['src/x.rs']);
  const e = mapEvent('PostToolUse', { tool_name: 'Edit', tool_input: { file_path: 'src/y.rs' } });
  assert.equal(e.kind, 'forward');
  assert.deepEqual(e.event.files, ['src/y.rs']);
  // Read + Bash stay recorded-only.
  assert.equal(mapEvent('PostToolUse', { tool_name: 'Read', tool_input: { file_path: 'r' } }).kind, 'record');
  assert.equal(mapEvent('PostToolUse', { tool_name: 'Bash', tool_input: { command: 'ls' } }).kind, 'record');
});
```
Remove/replace the old `PostToolUse -> recorded file/read/command` test's `FILE_TOUCHED` record assertion (the `FILE_TOUCHED` line in it is now wrong); keep its `FILE_READ`/`COMMAND_EXECUTED`/`TEST_RUN` record assertions.

- [ ] **Step 2: Run to verify it fails**

Run: `cd packages/coordify-hook && npm test 2>&1 | grep -iE "pass|fail"`
Expected: FAIL — current mapping returns `kind:'record'` for Write/Edit.

- [ ] **Step 3: Implement the flip**

In `packages/coordify-hook/lib/mapping.js`, in the `PostToolUse` branch, change the Edit/Write/MultiEdit case from a record to a forward. Replace:
```js
      if (tool === 'Edit' || tool === 'Write' || tool === 'MultiEdit') {
        return { kind: 'record', record: { type: 'FILE_TOUCHED', tool, file: ti.file_path || ti.path || null } };
      }
```
with:
```js
      if (tool === 'Edit' || tool === 'Write' || tool === 'MultiEdit') {
        const file = ti.file_path || ti.path;
        return file
          ? { kind: 'forward', event: { type: 'FILE_TOUCHED', files: [file] } }
          : { kind: 'record', record: { type: 'FILE_TOUCHED', tool, file: null } };
      }
```
(When the tool input carries no path, fall back to recording — nothing useful to forward.)

- [ ] **Step 4: Run the adapter suite**

Run: `cd packages/coordify-hook && npm test 2>&1 | grep -iE "ℹ (pass|fail|tests)"`
Expected: all pass, 0 fail.

- [ ] **Step 5: Commit**

```bash
git add packages/coordify-hook/lib/mapping.js packages/coordify-hook/test/mapping.test.js
git commit -m "feat(hook): forward FILE_TOUCHED (was recorded-only) for Edit/Write"
```

---

## Notes for the Final Whole-Branch Review

- Lock discipline: the FILE_TOUCHED handler takes state, then (after drop) recompute, then knowledge alone, then log — confirm no two of `{state, heat, conflict, waitgraph, knowledge, log}` are co-held; the second state lock (to read `actual_files` for `all_files`) is its own scope and drops before the knowledge lock.
- Determinism / no inflation: `record_cotouch` accrues only pairs touching a NEW file; re-touching a file (already in the set) yields an empty `new_files` and no accrual. Confirm.
- Heat union: `heat_inputs_for` uses `estimated ∪ actual`; empty-actual behaviour is unchanged from before (regression check).
- Adapter: the flip changes only the mapping result; hooks stay emit-only/crash-safe; `agentId` is injected by the sidecar (the forwarded `FILE_TOUCHED` event must NOT carry `agentId` from mapping — the sidecar adds it).
- Coverage ≥90% (target ≥95%) Core; adapter suite green.
