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
    #[allow(clippy::new_without_default)]
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
        assert!(lost.contains(&a), "agent idle > timeout_ms must be reaped");
        assert_eq!(lost.len(), 1);
        assert_eq!(s.agent_count(), 1);
        assert!(s.heartbeat(&b, 11_000));
    }

    #[test]
    fn reap_boundary_is_strictly_greater() {
        let mut s = State::new();
        // Register agent at now=5000 so idle == timeout (10000-5000 == 5000); must survive.
        let c = s.register(json!({}), 5_000);
        let lost = s.reap(10_000, 5_000);
        assert!(!lost.contains(&c), "agent idle exactly timeout_ms must survive (strict >)");
        assert!(s.heartbeat(&c, 11_000), "surviving agent must still be present");
    }

    #[test]
    fn remove_reports_presence() {
        let mut s = State::new();
        let id = s.register(json!({}), 1000);
        assert!(s.remove(&id));
        assert!(!s.remove(&id));
    }
}
