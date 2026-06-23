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
    pub fn knowledge_dir(&self) -> PathBuf {
        self.coordify().join("knowledge")
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
