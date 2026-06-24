use std::fs::{self, File, OpenOptions, Permissions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;

/// Soft cap on a single event log file. When the file exceeds this size the
/// next append rotates the live file to `.1` and starts fresh, so a runaway
/// session cannot fill the disk. 64 MiB is generous for a normal session and
/// bounds the worst case for a pathological one.
pub const MAX_LOG_BYTES: u64 = 64 * 1024 * 1024;

pub struct EventLog {
    file: File,
    path: PathBuf,
    written: u64,
}

impl EventLog {
    pub fn create(path: PathBuf) -> std::io::Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
            // 0o700: only the owning user can read/enter the session dir.
            let _ = fs::set_permissions(parent, Permissions::from_mode(0o700));
        }
        let written = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        // 0o600: session logs may contain agent ids, file paths, intents —
        // restrict to the owning user so other local users cannot read them.
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(&path)?;
        Ok(Self {
            file,
            path,
            written,
        })
    }

    pub fn append(&mut self, event: &serde_json::Value) -> std::io::Result<()> {
        // Size cap: rotate to .1 before writing once we cross the threshold.
        if self.written >= MAX_LOG_BYTES {
            let rotated = self.path.with_extension("log.1");
            let _ = fs::remove_file(&rotated);
            self.file.sync_data()?;
            fs::rename(&self.path, &rotated)?;
            self.file = OpenOptions::new()
                .create(true)
                .append(true)
                .mode(0o600)
                .open(&self.path)?;
            self.written = 0;
        }
        let line = serde_json::to_string(event)?;
        self.file.write_all(line.as_bytes())?;
        self.file.write_all(b"\n")?;
        self.written += line.len() as u64 + 1;
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
        log.append(&json!({"type": "AGENT_JOINED", "agentId": "agent-1"}))
            .unwrap();
        log.append(&json!({"type": "AGENT_LEFT", "agentId": "agent-1"}))
            .unwrap();

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

    // Target C6: create fails when the parent path is a file (create_dir_all errors).
    #[test]
    fn create_fails_when_parent_is_a_file() {
        let mut base = std::env::temp_dir();
        base.push(format!("coordify-elog-parentfile-{}", std::process::id()));
        let _ = fs::remove_file(&base);
        let _ = fs::remove_dir_all(&base);
        // Write a regular file at `base` so that joining a child path underneath
        // it causes create_dir_all to fail (cannot mkdir through a plain file).
        fs::write(&base, b"I am a file").unwrap();
        let log_path = base.join("events.log");
        let result = EventLog::create(log_path);
        assert!(result.is_err(), "expected Err when parent path is a file");
        let _ = fs::remove_file(&base);
    }

    #[test]
    fn rotates_when_size_cap_exceeded() {
        let mut dir = std::env::temp_dir();
        dir.push(format!("coordify-elog-rot-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("events.log");
        // Pre-seed the file to just under the cap by writing a large blob.
        fs::create_dir_all(&dir).unwrap();
        let seed = "x".repeat(MAX_LOG_BYTES as usize);
        fs::write(&path, &seed).unwrap();
        let mut log = EventLog::create(path.clone()).unwrap();
        log.append(&json!({"type": "AFTER_CAP"})).unwrap();
        // The original oversized file should have rotated to .log.1 and the
        // live file should contain only the new event.
        let rotated = path.with_extension("log.1");
        assert!(rotated.exists(), "rotated file exists");
        let live = fs::read_to_string(&path).unwrap();
        assert!(live.contains("AFTER_CAP"));
        assert!(!live.starts_with('x'), "live file is the fresh one");
        let _ = fs::remove_dir_all(&dir);
    }
}
