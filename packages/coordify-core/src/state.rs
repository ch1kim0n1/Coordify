use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use crate::cap::AgentState;
use crate::claim::ClaimStore;
use crate::heat::HeatInputs;

#[derive(Debug, Clone)]
pub struct Agent {
    pub id: String,
    pub last_seen_ms: u64,
    pub meta: serde_json::Value,
    pub state: AgentState,
    pub generation: u64,
    pub branch: Option<String>,
}

pub struct State {
    agents: HashMap<String, Agent>,
    pub claims: ClaimStore,
    next_id: u64,
}

impl State {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self { agents: HashMap::new(), claims: ClaimStore::new(), next_id: 1 }
    }

    pub fn register(&mut self, meta: serde_json::Value, now_ms: u64) -> String {
        let id = format!("agent-{}", self.next_id);
        self.next_id += 1;
        let branch = meta.get("branch").and_then(|v| v.as_str()).map(String::from);
        self.agents.insert(
            id.clone(),
            Agent {
                id: id.clone(),
                last_seen_ms: now_ms,
                meta,
                state: AgentState::Discovery,
                generation: 1,
                branch,
            },
        );
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

    pub fn agent_state(&self, id: &str) -> Option<AgentState> {
        self.agents.get(id).map(|a| a.state)
    }

    pub fn agent_generation(&self, id: &str) -> Option<u64> {
        self.agents.get(id).map(|a| a.generation)
    }

    pub fn set_state(&mut self, id: &str, to: AgentState) -> Result<(), StateError> {
        let agent = self.agents.get_mut(id).ok_or(StateError::AgentNotFound)?;
        if !can_transition(agent.state, to) {
            return Err(StateError::InvalidTransition);
        }
        agent.state = to;
        Ok(())
    }

    /// /clear: reset to DISCOVERY and bump generation. Returns the new
    /// generation, or None if the agent is unknown.
    pub fn clear(&mut self, id: &str) -> Option<u64> {
        let agent = self.agents.get_mut(id)?;
        agent.state = AgentState::Discovery;
        agent.generation += 1;
        Some(agent.generation)
    }

    /// Promote a DISCOVERY agent to ACTIVE after an accepted claim (CAP_SPEC §7).
    pub fn promote_active(&mut self, id: &str) {
        if let Some(agent) = self.agents.get_mut(id) {
            if agent.state == AgentState::Discovery {
                agent.state = AgentState::Active;
            }
        }
    }

    pub fn agent_ids(&self) -> Vec<String> {
        self.agents.keys().cloned().collect()
    }

    pub fn agents_get_branch_and_seen(&self, id: &str) -> Option<(Option<String>, u64)> {
        self.agents.get(id).map(|a| (a.branch.clone(), a.last_seen_ms))
    }

    pub fn heat_inputs_for(&self, agent_id: &str) -> Option<HeatInputs> {
        let agent = self.agents.get(agent_id)?;
        let claim = self.claims.live_claim_for(agent_id)?;
        Some(HeatInputs {
            agent_id: agent_id.to_string(),
            intent: claim.intent.clone(),
            domains: claim.domains.iter().cloned().collect(),
            files: claim.estimated_files.iter().cloned().collect(),
            task_tokens: crate::heat::tokens(&claim.task_summary),
            last_seen_ms: agent.last_seen_ms,
            branch: agent.branch.clone(),
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum StateError {
    AgentNotFound,
    InvalidTransition,
}

/// Allowed AGENT_STATE_CHANGED transitions (CAP_SPEC §7). Offline is reachable
/// from any live state; a same-state report is a no-op (allowed). Discovery is
/// re-entered only via /clear (State::clear), not via set_state.
pub fn can_transition(from: AgentState, to: AgentState) -> bool {
    use AgentState::*;
    if from == to {
        return true;
    }
    if to == Offline {
        return true;
    }
    matches!(
        (from, to),
        (Discovery, Active)
            | (Discovery, Idle)
            | (Active, Idle)
            | (Active, SubagentWaiting)
            | (Active, Testing)
            | (Active, Negotiating)
            | (Active, Blocked)
            | (Idle, Active)
            | (SubagentWaiting, Active)
            | (SubagentWaiting, Idle)
            | (Testing, Active)
            | (Testing, Idle)
            | (Negotiating, WaitingUser)
            | (Negotiating, Active)
            | (Blocked, Active)
            | (Blocked, WaitingUser)
            | (WaitingUser, Active)
            | (WaitingUser, Idle)
    )
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

    #[test]
    fn register_starts_in_discovery_generation_one() {
        let mut s = State::new();
        let id = s.register(serde_json::json!({}), 1000);
        assert_eq!(s.agent_state(&id), Some(crate::cap::AgentState::Discovery));
        assert_eq!(s.agent_generation(&id), Some(1));
    }

    #[test]
    fn set_state_enforces_transition_rules() {
        use crate::cap::AgentState::*;
        let mut s = State::new();
        let id = s.register(serde_json::json!({}), 1000);
        // Discovery -> Active is allowed; Discovery -> Testing is not.
        assert!(s.set_state(&id, Active).is_ok());
        assert!(s.set_state(&id, Testing).is_ok()); // Active -> Testing ok
        assert_eq!(s.set_state(&id, SubagentWaiting), Err(super::StateError::InvalidTransition)); // Testing -> SubagentWaiting not allowed
        assert_eq!(s.set_state("agent-999", Idle), Err(super::StateError::AgentNotFound));
        // Any -> Offline allowed
        assert!(s.set_state(&id, Offline).is_ok());
    }

    #[test]
    fn clear_resets_to_discovery_and_bumps_generation() {
        use crate::cap::AgentState::*;
        let mut s = State::new();
        let id = s.register(serde_json::json!({}), 1000);
        s.set_state(&id, Active).unwrap();
        let gen = s.clear(&id).unwrap();
        assert_eq!(gen, 2);
        assert_eq!(s.agent_state(&id), Some(Discovery));
        assert_eq!(s.clear("agent-999"), None);
    }

    #[test]
    fn promote_active_only_from_discovery() {
        use crate::cap::AgentState::*;
        let mut s = State::new();
        let id = s.register(serde_json::json!({}), 1000);
        s.promote_active(&id);
        assert_eq!(s.agent_state(&id), Some(Active));
        // From Active, promote is a no-op (stays Active).
        s.promote_active(&id);
        assert_eq!(s.agent_state(&id), Some(Active));
    }

    #[test]
    fn register_reads_branch_from_meta() {
        let mut s = State::new();
        let id = s.register(serde_json::json!({"branch": "main"}), 1000);
        let inp = {
            // no live claim yet -> heat_inputs_for is None
            assert!(s.heat_inputs_for(&id).is_none());
            // give the agent a live claim, then inputs appear with the branch.
            s.claims.propose(&id, "fix bug".into(), "BUGFIX".into(), vec!["AUTH".into()], vec!["a.rs".into()], 0.9);
            s.heat_inputs_for(&id).unwrap()
        };
        assert_eq!(inp.branch.as_deref(), Some("main"));
        assert_eq!(inp.intent, "BUGFIX");
        assert!(inp.task_tokens.contains("fix"));
        assert!(inp.files.contains("a.rs"));
    }

    #[test]
    fn can_transition_same_state_is_noop_allowed() {
        use crate::cap::AgentState::*;
        assert!(super::can_transition(Active, Active));
        assert!(super::can_transition(Discovery, Discovery));
    }
}
