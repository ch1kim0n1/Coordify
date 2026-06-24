use std::fs::{self, File, OpenOptions, Permissions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;

pub struct EventLog {
    file: File,
}

impl EventLog {
    pub fn create(path: PathBuf) -> std::io::Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
            // 0o700: only the owning user can read/enter the session dir.
            let _ = fs::set_permissions(parent, Permissions::from_mode(0o700));
        }
        // 0o600: session logs may contain agent ids, file paths, intents —
        // restrict to the owning user so other local users cannot read them.
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(path)?;
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
}
