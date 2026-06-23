use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ConflictState {
    Detected,
    Negotiating,
    AwaitingAgentResponse,
    AwaitingUserDecision,
    Resolved,
    Timeout,
    Aborted,
}

impl ConflictState {
    pub fn as_str(&self) -> &'static str {
        match self {
            ConflictState::Detected => "DETECTED",
            ConflictState::Negotiating => "NEGOTIATING",
            ConflictState::AwaitingAgentResponse => "AWAITING_AGENT_RESPONSE",
            ConflictState::AwaitingUserDecision => "AWAITING_USER_DECISION",
            ConflictState::Resolved => "RESOLVED",
            ConflictState::Timeout => "TIMEOUT",
            ConflictState::Aborted => "ABORTED",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Conflict {
    pub conflict_id: String,
    pub agents: (String, String),
    pub state: ConflictState,
    pub trigger_heat: u32,
    pub paths: Vec<String>,
    pub domains: Vec<String>,
    pub intents: Vec<String>,
}

fn key(a: &str, b: &str) -> (String, String) {
    if a <= b {
        (a.to_string(), b.to_string())
    } else {
        (b.to_string(), a.to_string())
    }
}

/// Holds currently-open conflicts (one per ordered agent pair). Resolved/aborted
/// conflicts are removed; their lifecycle is recorded in the event log.
#[derive(Default)]
pub struct ConflictStore {
    open: HashMap<(String, String), Conflict>,
    next_id: u64,
}

impl ConflictStore {
    pub fn new() -> Self {
        Self { open: HashMap::new(), next_id: 0 }
    }

    pub fn has_open(&self, a: &str, b: &str) -> bool {
        self.open.contains_key(&key(a, b))
    }

    /// Open a conflict for the pair. Returns None if one is already open.
    pub fn open(
        &mut self,
        a: &str,
        b: &str,
        trigger_heat: u32,
        paths: Vec<String>,
        domains: Vec<String>,
        intents: Vec<String>,
    ) -> Option<Conflict> {
        let k = key(a, b);
        if self.open.contains_key(&k) {
            return None;
        }
        self.next_id += 1;
        let conflict = Conflict {
            conflict_id: format!("conflict-{}", self.next_id),
            agents: k.clone(),
            state: ConflictState::Detected,
            trigger_heat,
            paths,
            domains,
            intents,
        };
        self.open.insert(k, conflict.clone());
        Some(conflict)
    }

    /// Resolve (remove) the pair's open conflict. Returns it with state Resolved.
    pub fn resolve(&mut self, a: &str, b: &str) -> Option<Conflict> {
        let mut c = self.open.remove(&key(a, b))?;
        c.state = ConflictState::Resolved;
        Some(c)
    }

    /// Abort (remove) every open conflict involving `agent`. Returns them with
    /// state Aborted.
    pub fn abort_for_agent(&mut self, agent: &str) -> Vec<Conflict> {
        let keys: Vec<(String, String)> = self
            .open
            .keys()
            .filter(|(x, y)| x == agent || y == agent)
            .cloned()
            .collect();
        let mut out = Vec::new();
        for k in keys {
            let mut c = self.open.remove(&k).unwrap();
            c.state = ConflictState::Aborted;
            out.push(c);
        }
        out
    }

    pub fn open_count(&self) -> usize {
        self.open.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_as_str_values() {
        assert_eq!(ConflictState::Detected.as_str(), "DETECTED");
        assert_eq!(ConflictState::AwaitingUserDecision.as_str(), "AWAITING_USER_DECISION");
        assert_eq!(ConflictState::Aborted.as_str(), "ABORTED");
        assert_eq!(serde_json::to_value(ConflictState::Resolved).unwrap(), serde_json::json!("RESOLVED"));
    }

    #[test]
    fn open_is_idempotent_per_pair_and_direction_independent() {
        let mut s = ConflictStore::new();
        let c1 = s.open("agent-2", "agent-1", 80, vec!["f".into()], vec!["AUTH".into()], vec!["BUGFIX".into()]);
        assert!(c1.is_some());
        let c = c1.unwrap();
        assert_eq!(c.conflict_id, "conflict-1");
        assert_eq!(c.agents, ("agent-1".to_string(), "agent-2".to_string())); // ordered
        assert_eq!(c.state, ConflictState::Detected);
        // Same pair, reversed order: already open -> None.
        assert!(s.open("agent-1", "agent-2", 90, vec![], vec![], vec![]).is_none());
        assert_eq!(s.open_count(), 1);
        assert!(s.has_open("agent-1", "agent-2"));
    }

    #[test]
    fn resolve_removes_and_marks_resolved() {
        let mut s = ConflictStore::new();
        s.open("agent-1", "agent-2", 80, vec![], vec![], vec![]);
        let r = s.resolve("agent-2", "agent-1").unwrap();
        assert_eq!(r.state, ConflictState::Resolved);
        assert!(!s.has_open("agent-1", "agent-2"));
        assert!(s.resolve("agent-1", "agent-2").is_none());
    }

    #[test]
    fn abort_for_agent_removes_all_its_conflicts() {
        let mut s = ConflictStore::new();
        s.open("agent-1", "agent-2", 80, vec![], vec![], vec![]);
        s.open("agent-1", "agent-3", 80, vec![], vec![], vec![]);
        s.open("agent-2", "agent-3", 80, vec![], vec![], vec![]);
        let aborted = s.abort_for_agent("agent-1");
        assert_eq!(aborted.len(), 2);
        assert!(aborted.iter().all(|c| c.state == ConflictState::Aborted));
        assert_eq!(s.open_count(), 1);
        assert!(s.has_open("agent-2", "agent-3"));
    }

    #[test]
    fn ids_are_sequential_across_opens() {
        let mut s = ConflictStore::new();
        let a = s.open("agent-1", "agent-2", 80, vec![], vec![], vec![]).unwrap();
        let b = s.open("agent-3", "agent-4", 80, vec![], vec![], vec![]).unwrap();
        assert_eq!(a.conflict_id, "conflict-1");
        assert_eq!(b.conflict_id, "conflict-2");
    }
}
