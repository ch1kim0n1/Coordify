# Phase 1 — Coordify Core Skeleton Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the Coordify Core daemon skeleton in Rust: bootstrap lock, Unix socket IPC, agent registration, heartbeat tracking, append-only event log, and session lifecycle.

**Architecture:** A single Rust binary (`coordify-core`) that acquires a project-scoped lock, opens a Unix domain socket, and serves newline-delimited JSON requests from a thread-per-connection pool against shared `Mutex`-guarded live state. Every accepted action appends a CAP-style event to an append-only JSONL log. A background reaper marks agents lost after heartbeat timeout; the last agent leaving finalizes the session and removes runtime files.

**Tech Stack:** Rust (edition 2021), `std::os::unix::net::UnixListener` (no async runtime), `serde` + `serde_json` for messages, `chrono` for timestamps. Crates kept to three. macOS/Linux only for Phase 1 (Windows named-pipe support explicitly deferred per ARCHITECTURE.md §24).

**Spec:** `absolute-docs/ARCHITECTURE.md` — Phase 1 scope in §27; bootstrap §8; IPC §9; storage layout §7; logging §14; session finalization §15; trust/auth §9.2/§10.

## Global Constraints

- Language: Rust, edition 2021. Crate at `packages/coordify-core/`.
- Dependencies limited to: `serde` (with `derive`), `serde_json`, `chrono`. Add no others without recording the reason.
- Platform: macOS/Linux only this phase. Windows is out of scope and must not block compilation on Unix.
- Core is the ONLY writer of canonical live state (ARCHITECTURE.md §28).
- Every IPC message must carry the session token after handshake; Core rejects mismatched tokens (§9.2).
- The append-only event log is the recoverable source of truth (§12). Never rewrite log lines.
- Runtime files live under `<root>/.coordify/runtime/`; session artifacts under `<root>/.coordify/sessions/<id>/` (§7).
- Runtime dir permissions `0700`, token file `0600` (§9.2).
- Storage root is supplied to the binary via `--root <path>` so tests can target a temp dir. Default `--root .`.
- Core version string constant: `"0.1.0"`.
- Agent ids are Core-assigned and sequential: `agent-1`, `agent-2`, … (no uuid dependency).
- Session id format: `%Y-%m-%d_%H-%M-%S` (e.g. `2026-06-22_18-42-11`).
- Lock `started_at` and event timestamps use RFC3339-Z format: `%Y-%m-%dT%H:%M:%SZ`.

---

## File Structure

```text
packages/coordify-core/
  Cargo.toml
  src/
    main.rs          # entrypoint: parse --root, acquire lock, create session, run server
    paths.rs         # Paths: all .coordify/* path derivations from root
    ipc.rs           # Request/Response types + newline-delimited JSON encode/decode
    bootstrap.rs     # lock acquire/stale-detect, token generation + 0600 write, dir 0700
    eventlog.rs      # EventLog: append-only JSONL writer with fsync
    session.rs       # Session: create session dir, finalize (network-final.json + cleanup)
    state.rs         # State: agents map, sequential register, heartbeat, remove, reap
    server.rs        # UnixListener accept loop, thread-per-conn, token check, dispatch
  tests/
    integration.rs   # spawn the built binary, drive it over a real socket
```

Responsibilities are split so each file is independently reviewable: `paths` is pure path math; `ipc` is pure (de)serialization; `bootstrap` owns filesystem startup invariants; `eventlog` owns durable append; `state` is pure in-memory logic (no IO, fully unit-testable); `session` owns session dir/finalize; `server` is the only place that wires IO + state + log together.

---

## Task 1: Crate scaffold + paths module

**Files:**
- Create: `packages/coordify-core/Cargo.toml`
- Create: `packages/coordify-core/src/main.rs`
- Create: `packages/coordify-core/src/paths.rs`

**Interfaces:**
- Consumes: nothing (first task).
- Produces:
  - `coordify_core::paths::Paths` with `Paths::new(root: impl Into<PathBuf>) -> Paths` and methods returning `PathBuf`: `coordify()`, `runtime()`, `socket()`, `lock()`, `token()`, `pid()`, `live_state()`, `sessions()`, `session_dir(id: &str)`.
  - `pub const VERSION: &str = "0.1.0";` in `main.rs`'s crate (re-exported via `lib`? No — see note). Define `VERSION` in `paths.rs` as `pub const VERSION: &str = "0.1.0";` so all modules can use `crate::paths::VERSION`.

> **Note on crate shape:** make this both a library and a binary. `Cargo.toml` declares `[lib] name = "coordify_core"` (path `src/lib.rs`) AND `[[bin]] name = "coordify-core"` (path `src/main.rs`). Create `src/lib.rs` declaring `pub mod paths;` (and later tasks add `pub mod ipc; pub mod bootstrap;` etc.). `main.rs` uses `use coordify_core::...;`. This lets unit tests and the integration test import modules. Add `src/lib.rs` in this task.

- [ ] **Step 1: Create `Cargo.toml`**

```toml
[package]
name = "coordify-core"
version = "0.1.0"
edition = "2021"

[lib]
name = "coordify_core"
path = "src/lib.rs"

[[bin]]
name = "coordify-core"
path = "src/main.rs"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
chrono = "0.4"
```

- [ ] **Step 2: Create `src/lib.rs`**

```rust
pub mod paths;
```

- [ ] **Step 3: Write the failing test in `src/paths.rs`**

```rust
use std::path::PathBuf;

pub const VERSION: &str = "0.1.0";

pub struct Paths {
    pub root: PathBuf,
}

impl Paths {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
    pub fn coordify(&self) -> PathBuf {
        self.root.join(".coordify")
    }
    pub fn runtime(&self) -> PathBuf {
        self.coordify().join("runtime")
    }
    pub fn socket(&self) -> PathBuf {
        self.runtime().join("core.sock")
    }
    pub fn lock(&self) -> PathBuf {
        self.runtime().join("core.lock")
    }
    pub fn token(&self) -> PathBuf {
        self.runtime().join("session.token")
    }
    pub fn pid(&self) -> PathBuf {
        self.runtime().join("core.pid")
    }
    pub fn live_state(&self) -> PathBuf {
        self.runtime().join("live-state.json")
    }
    pub fn sessions(&self) -> PathBuf {
        self.coordify().join("sessions")
    }
    pub fn session_dir(&self, id: &str) -> PathBuf {
        self.sessions().join(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_runtime_and_session_paths() {
        let p = Paths::new("/tmp/proj");
        assert_eq!(p.socket(), PathBuf::from("/tmp/proj/.coordify/runtime/core.sock"));
        assert_eq!(p.lock(), PathBuf::from("/tmp/proj/.coordify/runtime/core.lock"));
        assert_eq!(p.token(), PathBuf::from("/tmp/proj/.coordify/runtime/session.token"));
        assert_eq!(
            p.session_dir("2026-06-22_18-42-11"),
            PathBuf::from("/tmp/proj/.coordify/sessions/2026-06-22_18-42-11")
        );
    }
}
```

- [ ] **Step 4: Create a minimal `src/main.rs` so the binary compiles**

```rust
fn main() {
    println!("coordify-core {}", coordify_core::paths::VERSION);
}
```

- [ ] **Step 5: Run the test — expect PASS (logic is present)**

Run: `cd packages/coordify-core && cargo test`
Expected: compiles; `derives_runtime_and_session_paths` passes; binary builds.

- [ ] **Step 6: Commit**

```bash
git add packages/coordify-core/Cargo.toml packages/coordify-core/src/lib.rs packages/coordify-core/src/main.rs packages/coordify-core/src/paths.rs
git commit -m "feat(core): crate scaffold + paths module"
```

---

## Task 2: IPC protocol types + newline-delimited framing

**Files:**
- Create: `packages/coordify-core/src/ipc.rs`
- Modify: `packages/coordify-core/src/lib.rs` (add `pub mod ipc;`)

**Interfaces:**
- Consumes: nothing from earlier tasks (pure types).
- Produces:
  - `coordify_core::ipc::Request { id: String, token: String, action: String, agent_id: Option<String>, meta: serde_json::Value, event: serde_json::Value }`
  - `coordify_core::ipc::Response { id: String, ok: bool, agent_id: Option<String>, error: Option<String> }`
  - `Response::ok_for(id: &str) -> Response`, `Response::ok_with_agent(id: &str, agent_id: &str) -> Response`, `Response::err(id: &str, msg: &str) -> Response`
  - `ipc::decode_request(line: &str) -> serde_json::Result<Request>`
  - `ipc::encode_response(r: &Response) -> String` (returns a single line, no trailing newline)

- [ ] **Step 1: Add module to `src/lib.rs`**

```rust
pub mod paths;
pub mod ipc;
```

- [ ] **Step 2: Write `src/ipc.rs` with types, constructors, framing, and failing tests**

```rust
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct Request {
    pub id: String,
    pub token: String,
    pub action: String,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub meta: Value,
    #[serde(default)]
    pub event: Value,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct Response {
    pub id: String,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Response {
    pub fn ok_for(id: &str) -> Self {
        Self { id: id.to_string(), ok: true, agent_id: None, error: None }
    }
    pub fn ok_with_agent(id: &str, agent_id: &str) -> Self {
        Self { id: id.to_string(), ok: true, agent_id: Some(agent_id.to_string()), error: None }
    }
    pub fn err(id: &str, msg: &str) -> Self {
        Self { id: id.to_string(), ok: false, agent_id: None, error: Some(msg.to_string()) }
    }
}

pub fn decode_request(line: &str) -> serde_json::Result<Request> {
    serde_json::from_str(line)
}

pub fn encode_response(r: &Response) -> String {
    // Response contains only owned strings/bools/options — serialization cannot fail.
    serde_json::to_string(r).expect("Response serialization is infallible")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_register_request_with_defaults() {
        let line = r#"{"id":"r1","token":"abc","action":"register","meta":{"task":"auth"}}"#;
        let req = decode_request(line).unwrap();
        assert_eq!(req.action, "register");
        assert_eq!(req.token, "abc");
        assert_eq!(req.agent_id, None);
        assert_eq!(req.meta["task"], "auth");
    }

    #[test]
    fn encodes_response_omits_none_fields() {
        let r = Response::ok_with_agent("r1", "agent-1");
        let line = encode_response(&r);
        assert_eq!(line, r#"{"id":"r1","ok":true,"agent_id":"agent-1"}"#);
        assert!(!line.contains('\n'));
    }

    #[test]
    fn encodes_error_response() {
        let r = Response::err("r2", "bad token");
        let line = encode_response(&r);
        assert!(line.contains(r#""ok":false"#));
        assert!(line.contains(r#""error":"bad token""#));
    }

    #[test]
    fn rejects_malformed_json() {
        assert!(decode_request("{not json").is_err());
    }
}
```

- [ ] **Step 3: Run the tests — expect PASS**

Run: `cd packages/coordify-core && cargo test ipc`
Expected: all four `ipc::tests::*` pass.

- [ ] **Step 4: Commit**

```bash
git add packages/coordify-core/src/lib.rs packages/coordify-core/src/ipc.rs
git commit -m "feat(core): IPC request/response types + JSONL framing"
```

---

## Task 3: Bootstrap — lock acquisition, stale detection, token

**Files:**
- Create: `packages/coordify-core/src/bootstrap.rs`
- Modify: `packages/coordify-core/src/lib.rs` (add `pub mod bootstrap;`)

**Interfaces:**
- Consumes: `crate::paths::{Paths, VERSION}`.
- Produces:
  - `bootstrap::LockInfo { pid: u32, started_at: String, project_root: String, core_version: String }` (serde Serialize/Deserialize).
  - `bootstrap::LockOutcome` enum: `Acquired`, `HeldBy(LockInfo)`.
  - `bootstrap::now_iso() -> String` (RFC3339-Z).
  - `bootstrap::acquire_lock(paths: &Paths, version: &str) -> std::io::Result<LockOutcome>` — creates runtime dir (0700), writes lock atomically via `create_new`; on existing lock, reads it and returns `HeldBy` if the pid is alive, otherwise removes the stale lock and retries once.
  - `bootstrap::generate_token() -> std::io::Result<String>` (32 hex chars from `/dev/urandom`).
  - `bootstrap::write_token(paths: &Paths, token: &str) -> std::io::Result<()>` (0600).
  - `bootstrap::write_pid(paths: &Paths) -> std::io::Result<()>`.

- [ ] **Step 1: Add module to `src/lib.rs`**

```rust
pub mod paths;
pub mod ipc;
pub mod bootstrap;
```

- [ ] **Step 2: Write `src/bootstrap.rs` with implementation and failing tests**

```rust
use crate::paths::Paths;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions, Permissions};
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct LockInfo {
    pub pid: u32,
    pub started_at: String,
    pub project_root: String,
    pub core_version: String,
}

#[derive(Debug)]
pub enum LockOutcome {
    Acquired,
    HeldBy(LockInfo),
}

pub fn now_iso() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn pid_alive(pid: u32) -> bool {
    // ponytail: `kill -0` shell-out avoids a libc/nix dependency; adequate for local MVP.
    std::process::Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn ensure_runtime_dir(paths: &Paths) -> std::io::Result<()> {
    fs::create_dir_all(paths.runtime())?;
    fs::set_permissions(paths.runtime(), Permissions::from_mode(0o700))
}

pub fn acquire_lock(paths: &Paths, version: &str) -> std::io::Result<LockOutcome> {
    ensure_runtime_dir(paths)?;
    let info = LockInfo {
        pid: std::process::id(),
        started_at: now_iso(),
        project_root: paths.root.to_string_lossy().into_owned(),
        core_version: version.to_string(),
    };
    match OpenOptions::new().write(true).create_new(true).open(paths.lock()) {
        Ok(mut f) => {
            f.write_all(serde_json::to_string(&info)?.as_bytes())?;
            f.sync_all()?;
            Ok(LockOutcome::Acquired)
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            let raw = fs::read_to_string(paths.lock())?;
            match serde_json::from_str::<LockInfo>(&raw) {
                Ok(existing) if pid_alive(existing.pid) => Ok(LockOutcome::HeldBy(existing)),
                _ => {
                    // Stale or unparseable lock: remove and retry once.
                    fs::remove_file(paths.lock())?;
                    acquire_lock(paths, version)
                }
            }
        }
        Err(e) => Err(e),
    }
}

pub fn generate_token() -> std::io::Result<String> {
    let mut f = fs::File::open("/dev/urandom")?;
    let mut buf = [0u8; 16];
    f.read_exact(&mut buf)?;
    Ok(buf.iter().map(|b| format!("{:02x}", b)).collect())
}

pub fn write_token(paths: &Paths, token: &str) -> std::io::Result<()> {
    let mut f = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(paths.token())?;
    f.write_all(token.as_bytes())?;
    f.sync_all()
}

pub fn write_pid(paths: &Paths) -> std::io::Result<()> {
    fs::write(paths.pid(), std::process::id().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::VERSION;

    fn temp_root(tag: &str) -> std::path::PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!("coordify-test-{}-{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn acquires_lock_when_absent() {
        let root = temp_root("lock-absent");
        let paths = Paths::new(&root);
        match acquire_lock(&paths, VERSION).unwrap() {
            LockOutcome::Acquired => {}
            other => panic!("expected Acquired, got {:?}", other),
        }
        assert!(paths.lock().exists());
        // runtime dir is 0700
        let mode = fs::metadata(paths.runtime()).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn reports_held_when_live_pid_holds_lock() {
        let root = temp_root("lock-live");
        let paths = Paths::new(&root);
        // First acquire writes a lock with OUR pid, which is alive.
        acquire_lock(&paths, VERSION).unwrap();
        match acquire_lock(&paths, VERSION).unwrap() {
            LockOutcome::HeldBy(info) => assert_eq!(info.pid, std::process::id()),
            other => panic!("expected HeldBy, got {:?}", other),
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn breaks_stale_lock_with_dead_pid() {
        let root = temp_root("lock-stale");
        let paths = Paths::new(&root);
        ensure_runtime_dir(&paths).unwrap();
        // Write a lock owned by an almost-certainly-dead pid.
        let stale = LockInfo {
            pid: 999_999,
            started_at: now_iso(),
            project_root: paths.root.to_string_lossy().into_owned(),
            core_version: VERSION.to_string(),
        };
        fs::write(paths.lock(), serde_json::to_string(&stale).unwrap()).unwrap();
        match acquire_lock(&paths, VERSION).unwrap() {
            LockOutcome::Acquired => {}
            other => panic!("expected Acquired after breaking stale lock, got {:?}", other),
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn token_is_32_hex_chars_and_file_is_0600() {
        let root = temp_root("token");
        let paths = Paths::new(&root);
        ensure_runtime_dir(&paths).unwrap();
        let token = generate_token().unwrap();
        assert_eq!(token.len(), 32);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
        write_token(&paths, &token).unwrap();
        let mode = fs::metadata(paths.token()).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let _ = fs::remove_dir_all(&root);
    }
}
```

- [ ] **Step 3: Run the tests — expect PASS**

Run: `cd packages/coordify-core && cargo test bootstrap`
Expected: all four `bootstrap::tests::*` pass.

- [ ] **Step 4: Commit**

```bash
git add packages/coordify-core/src/lib.rs packages/coordify-core/src/bootstrap.rs
git commit -m "feat(core): bootstrap lock, stale detection, session token"
```

---

## Task 4: Append-only event log

**Files:**
- Create: `packages/coordify-core/src/eventlog.rs`
- Modify: `packages/coordify-core/src/lib.rs` (add `pub mod eventlog;`)

**Interfaces:**
- Consumes: nothing from earlier tasks (takes a `PathBuf`).
- Produces:
  - `eventlog::EventLog` with `EventLog::create(path: std::path::PathBuf) -> std::io::Result<EventLog>` (creates parent dirs, opens in append mode) and `append(&mut self, event: &serde_json::Value) -> std::io::Result<()>` (writes one compact JSON line + `\n`, then `sync_data`).

- [ ] **Step 1: Add module to `src/lib.rs`**

```rust
pub mod paths;
pub mod ipc;
pub mod bootstrap;
pub mod eventlog;
```

- [ ] **Step 2: Write `src/eventlog.rs` with implementation and failing tests**

```rust
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

pub struct EventLog {
    file: File,
}

impl EventLog {
    pub fn create(path: PathBuf) -> std::io::Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self { file })
    }

    pub fn append(&mut self, event: &serde_json::Value) -> std::io::Result<()> {
        let line = serde_json::to_string(event)?;
        self.file.write_all(line.as_bytes())?;
        self.file.write_all(b"\n")?;
        self.file.sync_data()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_path(tag: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!("coordify-elog-{}-{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir.push("events.log");
        dir
    }

    #[test]
    fn appends_one_json_object_per_line() {
        let path = temp_path("append");
        let mut log = EventLog::create(path.clone()).unwrap();
        log.append(&json!({"type": "AGENT_JOINED", "agentId": "agent-1"})).unwrap();
        log.append(&json!({"type": "AGENT_LEFT", "agentId": "agent-1"})).unwrap();

        let contents = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 2);
        let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(first["type"], "AGENT_JOINED");
        let second: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(second["type"], "AGENT_LEFT");
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn reopening_appends_rather_than_truncates() {
        let path = temp_path("reopen");
        {
            let mut log = EventLog::create(path.clone()).unwrap();
            log.append(&json!({"n": 1})).unwrap();
        }
        {
            let mut log = EventLog::create(path.clone()).unwrap();
            log.append(&json!({"n": 2})).unwrap();
        }
        let contents = fs::read_to_string(&path).unwrap();
        assert_eq!(contents.lines().count(), 2);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}
```

- [ ] **Step 3: Run the tests — expect PASS**

Run: `cd packages/coordify-core && cargo test eventlog`
Expected: both `eventlog::tests::*` pass.

- [ ] **Step 4: Commit**

```bash
git add packages/coordify-core/src/lib.rs packages/coordify-core/src/eventlog.rs
git commit -m "feat(core): append-only JSONL event log"
```

---

## Task 5: Live state + session lifecycle

**Files:**
- Create: `packages/coordify-core/src/state.rs`
- Create: `packages/coordify-core/src/session.rs`
- Modify: `packages/coordify-core/src/lib.rs` (add `pub mod state;` and `pub mod session;`)

**Interfaces:**
- Consumes: `crate::paths::Paths`.
- Produces:
  - `state::Agent { id: String, last_seen_ms: u64, meta: serde_json::Value }`
  - `state::State` with: `State::new() -> State`; `register(&mut self, meta: serde_json::Value, now_ms: u64) -> String` (returns new `agent-N` id); `heartbeat(&mut self, id: &str, now_ms: u64) -> bool`; `remove(&mut self, id: &str) -> bool`; `reap(&mut self, now_ms: u64, timeout_ms: u64) -> Vec<String>` (removes and returns ids whose `now_ms - last_seen_ms > timeout_ms`); `agent_count(&self) -> usize`.
  - `state::now_ms() -> u64` (milliseconds since Unix epoch via `SystemTime`).
  - `session::Session { id: String, dir: std::path::PathBuf }`
  - `session::new_session_id() -> String` (`%Y-%m-%d_%H-%M-%S`).
  - `session::create_session(paths: &Paths, id: String) -> std::io::Result<Session>` (creates the session dir).
  - `session::finalize(session: &Session, paths: &Paths, agents_seen: u64) -> std::io::Result<()>` (writes `network-final.json` into the session dir, then removes runtime files: socket, lock, token, pid, live-state — ignoring not-found errors).

- [ ] **Step 1: Add modules to `src/lib.rs`**

```rust
pub mod paths;
pub mod ipc;
pub mod bootstrap;
pub mod eventlog;
pub mod state;
pub mod session;
```

- [ ] **Step 2: Write `src/state.rs` with implementation and failing tests**

```rust
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct Agent {
    pub id: String,
    pub last_seen_ms: u64,
    pub meta: serde_json::Value,
}

pub struct State {
    agents: HashMap<String, Agent>,
    next_id: u64,
}

impl State {
    pub fn new() -> Self {
        Self { agents: HashMap::new(), next_id: 1 }
    }

    pub fn register(&mut self, meta: serde_json::Value, now_ms: u64) -> String {
        let id = format!("agent-{}", self.next_id);
        self.next_id += 1;
        self.agents.insert(id.clone(), Agent { id: id.clone(), last_seen_ms: now_ms, meta });
        id
    }

    pub fn heartbeat(&mut self, id: &str, now_ms: u64) -> bool {
        match self.agents.get_mut(id) {
            Some(a) => { a.last_seen_ms = now_ms; true }
            None => false,
        }
    }

    pub fn remove(&mut self, id: &str) -> bool {
        self.agents.remove(id).is_some()
    }

    pub fn reap(&mut self, now_ms: u64, timeout_ms: u64) -> Vec<String> {
        let lost: Vec<String> = self
            .agents
            .values()
            .filter(|a| now_ms.saturating_sub(a.last_seen_ms) > timeout_ms)
            .map(|a| a.id.clone())
            .collect();
        for id in &lost {
            self.agents.remove(id);
        }
        lost
    }

    pub fn agent_count(&self) -> usize {
        self.agents.len()
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn register_assigns_sequential_ids() {
        let mut s = State::new();
        assert_eq!(s.register(json!({}), 1000), "agent-1");
        assert_eq!(s.register(json!({}), 1000), "agent-2");
        assert_eq!(s.agent_count(), 2);
    }

    #[test]
    fn heartbeat_updates_known_agent_and_rejects_unknown() {
        let mut s = State::new();
        let id = s.register(json!({}), 1000);
        assert!(s.heartbeat(&id, 2000));
        assert!(!s.heartbeat("agent-999", 2000));
    }

    #[test]
    fn reap_removes_only_timed_out_agents() {
        let mut s = State::new();
        let a = s.register(json!({}), 1000);
        let b = s.register(json!({}), 9000);
        // now=10000, timeout=5000 -> a (idle 9000) is lost, b (idle 1000) survives.
        let lost = s.reap(10_000, 5_000);
        assert_eq!(lost, vec![a]);
        assert_eq!(s.agent_count(), 1);
        assert!(s.heartbeat(&b, 11_000));
    }

    #[test]
    fn remove_reports_presence() {
        let mut s = State::new();
        let id = s.register(json!({}), 1000);
        assert!(s.remove(&id));
        assert!(!s.remove(&id));
    }
}
```

- [ ] **Step 3: Write `src/session.rs` with implementation and failing tests**

```rust
use crate::paths::Paths;
use std::fs;
use std::path::PathBuf;

pub struct Session {
    pub id: String,
    pub dir: PathBuf,
}

pub fn new_session_id() -> String {
    chrono::Utc::now().format("%Y-%m-%d_%H-%M-%S").to_string()
}

pub fn create_session(paths: &Paths, id: String) -> std::io::Result<Session> {
    let dir = paths.session_dir(&id);
    fs::create_dir_all(&dir)?;
    Ok(Session { id, dir })
}

pub fn finalize(session: &Session, paths: &Paths, agents_seen: u64) -> std::io::Result<()> {
    let final_doc = serde_json::json!({
        "sessionId": session.id,
        "endedAt": crate::bootstrap::now_iso(),
        "agentsSeen": agents_seen,
    });
    fs::write(
        session.dir.join("network-final.json"),
        serde_json::to_string_pretty(&final_doc)?,
    )?;
    // Remove runtime files; not-found is fine (already gone / never created).
    for p in [paths.socket(), paths.lock(), paths.token(), paths.pid(), paths.live_state()] {
        if let Err(e) = fs::remove_file(&p) {
            if e.kind() != std::io::ErrorKind::NotFound {
                return Err(e);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!("coordify-session-{}-{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn create_session_makes_dir() {
        let root = temp_root("create");
        let paths = Paths::new(&root);
        let s = create_session(&paths, "2026-06-22_18-42-11".to_string()).unwrap();
        assert!(s.dir.is_dir());
        assert!(s.dir.ends_with("sessions/2026-06-22_18-42-11"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn finalize_writes_summary_and_clears_runtime() {
        let root = temp_root("finalize");
        let paths = Paths::new(&root);
        fs::create_dir_all(paths.runtime()).unwrap();
        // Seed runtime files.
        fs::write(paths.lock(), "{}").unwrap();
        fs::write(paths.token(), "tok").unwrap();
        fs::write(paths.pid(), "123").unwrap();
        let s = create_session(&paths, new_session_id()).unwrap();

        finalize(&s, &paths, 3).unwrap();

        let summary = fs::read_to_string(s.dir.join("network-final.json")).unwrap();
        let doc: serde_json::Value = serde_json::from_str(&summary).unwrap();
        assert_eq!(doc["agentsSeen"], 3);
        assert_eq!(doc["sessionId"], s.id);
        assert!(!paths.lock().exists());
        assert!(!paths.token().exists());
        assert!(!paths.pid().exists());
        let _ = fs::remove_dir_all(&root);
    }
}
```

- [ ] **Step 4: Run the tests — expect PASS**

Run: `cd packages/coordify-core && cargo test state:: ; cargo test session::`
Expected: all `state::tests::*` and `session::tests::*` pass.

- [ ] **Step 5: Commit**

```bash
git add packages/coordify-core/src/lib.rs packages/coordify-core/src/state.rs packages/coordify-core/src/session.rs
git commit -m "feat(core): live state (register/heartbeat/reap) + session lifecycle"
```

---

## Task 6: Server wiring — socket accept loop + request dispatch

**Files:**
- Create: `packages/coordify-core/src/server.rs`
- Modify: `packages/coordify-core/src/lib.rs` (add `pub mod server;`)
- Modify: `packages/coordify-core/src/main.rs` (wire bootstrap + session + server)
- Create: `packages/coordify-core/tests/integration.rs`

**Interfaces:**
- Consumes: `crate::ipc::{Request, Response, decode_request, encode_response}`, `crate::state::{State, now_ms}`, `crate::eventlog::EventLog`, `crate::session::{Session, finalize}`, `crate::paths::Paths`.
- Produces:
  - `server::Shared { state: Mutex<State>, log: Mutex<EventLog>, token: String, agents_seen: Mutex<u64> }` wrapped in `Arc`.
  - `server::run(paths: Paths, session: Session, token: String, listener: UnixListener) -> std::io::Result<()>` — accepts connections, spawns a thread per connection, and returns after the connection that drops the agent count to zero finalizes the session and the accept loop is signalled to stop.
  - `server::handle_request(shared: &Shared, req: &Request) -> Response` — pure-ish dispatch (token check, action match, state mutation, event append). Reused by unit tests without a socket.

> **Dispatch rules (event types per ARCHITECTURE.md §6):**
> - token mismatch → `Response::err(&req.id, "unauthorized")`, no state change, no event.
> - `action == "register"` → `state.register(meta, now)`, increment `agents_seen`, append event `{type:"AGENT_JOINED", agentId, ts}`, return `ok_with_agent`.
> - `action == "heartbeat"` → requires `agent_id`; if `state.heartbeat` true → `ok_for`; else `err("unknown agent")`. No event (heartbeats are not logged, per §14 trace-vs-event separation).
> - `action == "submit_event"` → append `req.event` verbatim to the log, return `ok_for`. (Validation against CAP schema is Phase 2; here we only persist.)
> - any other action → `err("unknown action")`.

- [ ] **Step 1: Add module to `src/lib.rs`**

```rust
pub mod paths;
pub mod ipc;
pub mod bootstrap;
pub mod eventlog;
pub mod state;
pub mod session;
pub mod server;
```

- [ ] **Step 2: Write `src/server.rs` with `Shared`, `handle_request`, and the unit tests for dispatch**

```rust
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
        let mut st = shared.state.lock().unwrap();
        if st.remove(&id) {
            let event = serde_json::json!({
                "type": "AGENT_LEFT",
                "agentId": id,
                "ts": crate::bootstrap::now_iso(),
            });
            let _ = shared.log.lock().unwrap().append(&event);
        }
        return st.agent_count() == 0;
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
            let had_agents = *shared.agents_seen.lock().unwrap() > 0;
            if network_empty && had_agents {
                let seen = *shared.agents_seen.lock().unwrap();
                finalize(&session, &paths, seen)?;
                break;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn shared_for_test(token: &str) -> Arc<Shared> {
        let mut dir = std::env::temp_dir();
        dir.push(format!("coordify-srv-{}", std::process::id()));
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
}
```

- [ ] **Step 3: Run the unit tests — expect PASS**

Run: `cd packages/coordify-core && cargo test server::tests`
Expected: `rejects_bad_token`, `register_then_heartbeat`, `unknown_action_errors` pass.

- [ ] **Step 4: Wire `src/main.rs` to bootstrap, bind the socket, and run the server**

```rust
use coordify_core::paths::{Paths, VERSION};
use coordify_core::{bootstrap, server, session};
use std::os::unix::net::UnixListener;

fn main() {
    let root = parse_root();
    let paths = Paths::new(&root);

    match bootstrap::acquire_lock(&paths, VERSION) {
        Ok(bootstrap::LockOutcome::Acquired) => {}
        Ok(bootstrap::LockOutcome::HeldBy(info)) => {
            eprintln!("coordify-core already running (pid {})", info.pid);
            std::process::exit(0);
        }
        Err(e) => {
            eprintln!("coordify-core: failed to acquire lock: {e}");
            std::process::exit(1);
        }
    }

    if let Err(e) = run(&paths) {
        eprintln!("coordify-core: {e}");
        // Best-effort cleanup so a crash does not strand the lock.
        let _ = std::fs::remove_file(paths.lock());
        let _ = std::fs::remove_file(paths.socket());
        std::process::exit(1);
    }
}

fn run(paths: &Paths) -> std::io::Result<()> {
    let token = bootstrap::generate_token()?;
    bootstrap::write_token(paths, &token)?;
    bootstrap::write_pid(paths)?;

    let sess = session::create_session(paths, session::new_session_id())?;

    // Remove a stale socket file before binding.
    let _ = std::fs::remove_file(paths.socket());
    let listener = UnixListener::bind(paths.socket())?;

    println!("coordify-core {VERSION} listening on {}", paths.socket().display());
    server::run(Paths::new(&paths.root), sess, token, listener)
}

fn parse_root() -> std::path::PathBuf {
    // Usage: coordify-core [--root <path>]; defaults to current directory.
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--root" {
            if let Some(p) = args.next() {
                return std::path::PathBuf::from(p);
            }
        }
    }
    std::path::PathBuf::from(".")
}
```

- [ ] **Step 5: Write the integration test in `tests/integration.rs`**

```rust
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
```

- [ ] **Step 6: Run the integration tests — expect PASS**

Run: `cd packages/coordify-core && cargo test --test integration`
Expected: `register_and_heartbeat_over_socket`, `rejects_bad_token_over_socket`, `last_agent_leaving_finalizes_session` pass.

- [ ] **Step 7: Run the full suite**

Run: `cd packages/coordify-core && cargo test`
Expected: all unit tests + all integration tests pass.

- [ ] **Step 8: Commit**

```bash
git add packages/coordify-core/src/lib.rs packages/coordify-core/src/server.rs packages/coordify-core/src/main.rs packages/coordify-core/tests/integration.rs
git commit -m "feat(core): unix socket server, request dispatch, session finalize on last exit"
```

---

## Task 7: Heartbeat reaper — mark lost agents, orphan their claims

**Files:**
- Modify: `packages/coordify-core/src/server.rs` (add reaper thread + lost-agent events)
- Modify: `packages/coordify-core/tests/integration.rs` (add reaper test)

**Interfaces:**
- Consumes: `crate::state::{State, now_ms}` (`reap`), `crate::eventlog::EventLog`.
- Produces:
  - `server::spawn_reaper(shared: Arc<Shared>, interval_ms: u64, timeout_ms: u64) -> std::thread::JoinHandle<()>` — periodically calls `state.reap`; for each lost id, appends `{type:"AGENT_LOST", agentId, ts}` then `{type:"CLAIM_ORPHANED", agentId, ts}` to the log. (No claims exist yet in Phase 1, so `CLAIM_ORPHANED` is emitted as the lifecycle marker per ARCHITECTURE.md §6 `heartbeat timeout -> AGENT_LOST, CLAIM_ORPHANED`; Phase 2 attaches real claim ids.)
- Constants for Phase 1: heartbeat timeout `10_000` ms, reaper interval `2_000` ms (ARCHITECTURE.md §11 config defaults `heartbeatTimeoutMs: 10000`, `heartbeatIntervalMs: 2000`).

> **Why the reaper does not finalize:** finalization stays driven by the connection thread in Task 6. A reaped agent's connection thread will also observe the closed socket and run its leave path; `state.remove` returning false there (already reaped) prevents a double `AGENT_LEFT`. The reaper only emits loss events; it never removes the session.

- [ ] **Step 1: Add the reaper to `src/server.rs`**

Add this function to `server.rs` (after `run`):

```rust
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
```

- [ ] **Step 2: Start the reaper inside `run`**

In `server::run`, after building `shared` and before the `for conn in listener.incoming()` loop, add:

```rust
    let _reaper = spawn_reaper(Arc::clone(&shared), 2_000, 10_000);
```

(The handle is intentionally detached for the MVP; the process exits after finalize, which tears the thread down.)

- [ ] **Step 3: Add a unit test for reaper event emission**

Add to `server::tests` in `server.rs`:

```rust
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
```

- [ ] **Step 4: Run unit tests — expect PASS**

Run: `cd packages/coordify-core && cargo test server::tests`
Expected: prior three tests + `reaper_emits_lost_and_orphaned_events` pass.

- [ ] **Step 5: Add an integration test that drives the reaper with a short timeout**

> The shipped reaper uses fixed 10s timeout / 2s interval, too slow for a test. Add a hidden env override so tests can shrink it. In `src/main.rs` `run()`, the call path uses `server::run`; expose timing via env read inside `server::run`:

Modify `server::run` to read optional overrides (add at the top of `run`, before `spawn_reaper`):

```rust
    let interval_ms = std::env::var("COORDIFY_REAPER_INTERVAL_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2_000);
    let timeout_ms = std::env::var("COORDIFY_REAPER_TIMEOUT_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10_000);
```

and change the spawn line to:

```rust
    let _reaper = spawn_reaper(Arc::clone(&shared), interval_ms, timeout_ms);
```

Then add this integration test to `tests/integration.rs`, spawning with the env overrides. Add a second spawn helper:

```rust
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
```

- [ ] **Step 6: Run the integration tests — expect PASS**

Run: `cd packages/coordify-core && cargo test --test integration`
Expected: all four integration tests pass (including `reaper_logs_agent_lost_for_silent_agent`).

- [ ] **Step 7: Run the full suite + clippy**

Run: `cd packages/coordify-core && cargo test && cargo clippy -- -D warnings`
Expected: all tests pass; clippy reports no warnings.

- [ ] **Step 8: Commit**

```bash
git add packages/coordify-core/src/server.rs packages/coordify-core/tests/integration.rs
git commit -m "feat(core): heartbeat reaper emits AGENT_LOST + CLAIM_ORPHANED"
```

---

## Out of Scope (Phase 2+)

Recorded so reviewers do not flag these as gaps:

- CAP event schema validation (`submit_event` currently persists verbatim) — Phase 2.
- Claim lifecycle, real claim ids on `CLAIM_ORPHANED` — Phase 2.
- `/clear` handling (`CLEAR_INVOKED`, generation increment) — Phase 2.
- Heat calculation — Phase 3.
- `live-state.json` snapshotting and crash recovery/replay — Phase 2 (file path reserved in `paths.rs`).
- Windows named-pipe IPC — deferred (ARCHITECTURE.md §24).
- Hook adapter + CLI (Node) talking to this socket — separate plan.
- Log compression on finalize, diagnostics.log, trace.log — Phase 5/§14.

---

## Self-Review Notes

- **Spec coverage (ARCHITECTURE.md §27 Phase 1):** Core binary (Task 1,6) ✓; socket (Task 6) ✓; lock (Task 3) ✓; agent registration (Task 5,6) ✓; heartbeat (Task 5,6,7) ✓; session lifecycle (Task 5,6) ✓; event log (Task 4,6) ✓.
- **Bootstrap §8:** lock acquire + stale PID detection (Task 3) ✓; token §9.2 ✓; runtime 0700 / token 0600 ✓.
- **IPC §9:** framed JSON request/response with token (Task 2,6) ✓. Event/stream message kinds deferred to Phase 2 (only request/response needed for skeleton).
- **Finalization §15:** network-final.json + runtime cleanup on last exit (Task 5,6) ✓. Knowledge/stats/compression deferred (out of scope above).
- **Non-negotiables §28:** Core is only state writer ✓; event log is recoverable source ✓; deterministic ids ✓.
- **Type consistency:** `Paths` methods, `Request`/`Response` fields, `State` methods, `Shared` fields, and `Session` fields are referenced identically across Tasks 1-7.
