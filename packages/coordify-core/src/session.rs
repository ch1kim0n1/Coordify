use crate::paths::Paths;
use std::fs;
use std::path::PathBuf;

#[derive(Clone)]
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
        // Seed all five runtime files.
        fs::write(paths.socket(), "x").unwrap();
        fs::write(paths.live_state(), "{}").unwrap();
        fs::write(paths.lock(), "{}").unwrap();
        fs::write(paths.token(), "tok").unwrap();
        fs::write(paths.pid(), "123").unwrap();
        let s = create_session(&paths, new_session_id()).unwrap();

        finalize(&s, &paths, 3).unwrap();

        let summary = fs::read_to_string(s.dir.join("network-final.json")).unwrap();
        let doc: serde_json::Value = serde_json::from_str(&summary).unwrap();
        assert_eq!(doc["agentsSeen"], 3);
        assert_eq!(doc["sessionId"], s.id);
        assert!(!paths.socket().exists());
        assert!(!paths.live_state().exists());
        assert!(!paths.lock().exists());
        assert!(!paths.token().exists());
        assert!(!paths.pid().exists());
        let _ = fs::remove_dir_all(&root);
    }
}
