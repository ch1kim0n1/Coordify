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

    // Target D7: create_session fails when the sessions directory cannot be created
    // because a regular file sits in the path that would need to become a directory.
    #[test]
    fn create_session_fails_when_parent_is_a_file() {
        let mut base = std::env::temp_dir();
        base.push(format!("coordify-session-parentfile-{}", std::process::id()));
        let _ = fs::remove_file(&base);
        let _ = fs::remove_dir_all(&base);
        // Write a plain file at `base`.  Paths::new will resolve sessions() as
        // base/.coordify/sessions — but create_dir_all must traverse `base` as
        // a directory, so placing a file there makes it fail.
        fs::write(&base, b"I am a file").unwrap();
        // Build a Paths whose root == base (which is a file, not a dir).
        let paths = Paths::new(&base);
        let result = create_session(&paths, "2026-06-22_00-00-00".to_string());
        assert!(result.is_err(), "expected Err when sessions dir cannot be created");
        let _ = fs::remove_file(&base);
    }

    // Target D8: finalize fails when the session dir does not exist.
    #[test]
    fn finalize_fails_when_session_dir_missing() {
        let root = temp_root("fin-nodir");
        let paths = Paths::new(&root);
        // Construct a Session pointing at a directory that was never created.
        let ghost_dir = root.join("ghost").join("deep").join("nonexistent");
        let session = Session {
            id: "ghost-session".to_string(),
            dir: ghost_dir,
        };
        let result = finalize(&session, &paths, 1);
        assert!(result.is_err(), "expected Err when session dir does not exist");
        let _ = fs::remove_dir_all(&root);
    }
}
