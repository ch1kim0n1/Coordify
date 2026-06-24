use crate::cap::{Proposal, ProposalKind};
use serde::Serialize;
use std::collections::{BTreeSet, HashMap};

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
    pub proposals: HashMap<String, Proposal>,
    pub opened_at_ms: u64,
}

impl Conflict {
    /// Proposals ordered by agent id — deterministic arbitration framing (§18.5).
    pub fn proposals_sorted(&self) -> Vec<(&String, &Proposal)> {
        let mut v: Vec<(&String, &Proposal)> = self.proposals.iter().collect();
        v.sort_by(|x, y| x.0.cmp(y.0));
        v
    }
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
    #[allow(clippy::too_many_arguments)]
    pub fn open(
        &mut self,
        a: &str,
        b: &str,
        trigger_heat: u32,
        opened_at_ms: u64,
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
            proposals: HashMap::new(),
            opened_at_ms,
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

    /// Record a proposal from `from` for the conflict. Returns false if the
    /// conflict is unknown or `from` is not a participant. Moves the conflict
    /// to NEGOTIATING.
    pub fn record_proposal(&mut self, conflict_id: &str, from: &str, proposal: Proposal) -> bool {
        for c in self.open.values_mut() {
            if c.conflict_id == conflict_id {
                if c.agents.0 != from && c.agents.1 != from {
                    return false;
                }
                c.proposals.insert(from.to_string(), proposal);
                c.state = ConflictState::Negotiating;
                return true;
            }
        }
        false
    }

    pub fn both_proposed(&self, conflict_id: &str) -> bool {
        self.open
            .values()
            .find(|c| c.conflict_id == conflict_id)
            .map(|c| c.proposals.contains_key(&c.agents.0) && c.proposals.contains_key(&c.agents.1))
            .unwrap_or(false)
    }

    pub fn get_by_id(&self, conflict_id: &str) -> Option<&Conflict> {
        self.open.values().find(|c| c.conflict_id == conflict_id)
    }

    pub fn set_state(&mut self, conflict_id: &str, state: ConflictState) {
        for c in self.open.values_mut() {
            if c.conflict_id == conflict_id {
                c.state = state;
                return;
            }
        }
    }

    /// Resolve (remove) the conflict by id. Returns it with state Resolved.
    pub fn resolve_by_id(&mut self, conflict_id: &str) -> Option<Conflict> {
        let k = self
            .open
            .iter()
            .find(|(_, c)| c.conflict_id == conflict_id)
            .map(|(k, _)| k.clone())?;
        let mut c = self.open.remove(&k)?;
        c.state = ConflictState::Resolved;
        Some(c)
    }

    /// Ids of open conflicts that have aged past `timeout_ms` without both
    /// proposals and are still awaiting them (not yet escalated/resolved).
    pub fn timed_out(&self, now_ms: u64, timeout_ms: u64) -> Vec<String> {
        self.open
            .values()
            .filter(|c| {
                now_ms.saturating_sub(c.opened_at_ms) > timeout_ms
                    && !(c.proposals.contains_key(&c.agents.0) && c.proposals.contains_key(&c.agents.1))
                    && matches!(
                        c.state,
                        ConflictState::Detected
                            | ConflictState::Negotiating
                            | ConflictState::AwaitingAgentResponse
                    )
            })
            .map(|c| c.conflict_id.clone())
            .collect()
    }

    pub fn all_open(&self) -> Vec<&Conflict> {
        self.open.values().collect()
    }
}

#[derive(Debug, Clone)]
pub struct ConflictConfig {
    pub protected_paths: Vec<String>,
    pub allow_co_own: bool,
    pub proposal_timeout_ms: u64,
}

impl Default for ConflictConfig {
    fn default() -> Self {
        Self { protected_paths: Vec::new(), allow_co_own: true, proposal_timeout_ms: 60_000 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    AutoResolve { resolution: &'static str },
    Escalate { reason: &'static str },
}

fn proposal_files(p: &Proposal) -> BTreeSet<String> {
    let mut s = BTreeSet::new();
    for c in &p.claim_changes {
        if let Some(k) = &c.keep {
            s.extend(k.iter().cloned());
        }
        if let Some(t) = &c.take {
            s.extend(t.iter().cloned());
        }
    }
    s
}

fn protected(paths: &[String], cfg: &ConflictConfig) -> bool {
    paths.iter().any(|p| {
        cfg.protected_paths
            .iter()
            .any(|pp| !pp.is_empty() && p.starts_with(pp.as_str()))
    })
}

/// Pure deterministic comparison of two proposals (§18.4). Evaluated in order.
/// Note: mutual QUEUE_TASK is handled as a deadlock by the server before this
/// is called, so the single-queue branch here only ever sees one queue.
pub fn compare(a: &Proposal, b: &Proposal, paths: &[String], cfg: &ConflictConfig) -> Decision {
    use ProposalKind::*;
    if a.requires_user_approval || b.requires_user_approval {
        return Decision::Escalate { reason: "USER_APPROVAL_REQUIRED" };
    }
    if a.kind == AskUser || b.kind == AskUser {
        return Decision::Escalate { reason: "AGENT_REQUESTED_USER" };
    }
    if protected(paths, cfg) {
        return Decision::Escalate { reason: "PROTECTED_PATH" };
    }
    let steps_aside = |k: ProposalKind| matches!(k, YieldClaim | AbortTask | TransferTask);
    if steps_aside(a.kind) || steps_aside(b.kind) {
        return Decision::AutoResolve { resolution: "PARTICIPANT_STEPPED_ASIDE" };
    }
    if (a.kind == QueueTask) ^ (b.kind == QueueTask) {
        return Decision::AutoResolve { resolution: "QUEUED" };
    }
    if a.kind == SplitScope && b.kind == SplitScope {
        return if proposal_files(a).is_disjoint(&proposal_files(b)) {
            Decision::AutoResolve { resolution: "SCOPE_SPLIT" }
        } else {
            Decision::Escalate { reason: "OVERLAPPING_SPLIT" }
        };
    }
    if a.kind == CoOwn && b.kind == CoOwn {
        return if cfg.allow_co_own {
            Decision::AutoResolve { resolution: "CO_OWNERSHIP" }
        } else {
            Decision::Escalate { reason: "CO_OWN_DISALLOWED" }
        };
    }
    Decision::Escalate { reason: "INCOMPATIBLE_PROPOSALS" }
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
        let c1 = s.open("agent-2", "agent-1", 80, 0, vec!["f".into()], vec!["AUTH".into()], vec!["BUGFIX".into()]);
        assert!(c1.is_some());
        let c = c1.unwrap();
        assert_eq!(c.conflict_id, "conflict-1");
        assert_eq!(c.agents, ("agent-1".to_string(), "agent-2".to_string())); // ordered
        assert_eq!(c.state, ConflictState::Detected);
        // Same pair, reversed order: already open -> None.
        assert!(s.open("agent-1", "agent-2", 90, 0, vec![], vec![], vec![]).is_none());
        assert_eq!(s.open_count(), 1);
        assert!(s.has_open("agent-1", "agent-2"));
    }

    #[test]
    fn resolve_removes_and_marks_resolved() {
        let mut s = ConflictStore::new();
        s.open("agent-1", "agent-2", 80, 0, vec![], vec![], vec![]);
        let r = s.resolve("agent-2", "agent-1").unwrap();
        assert_eq!(r.state, ConflictState::Resolved);
        assert!(!s.has_open("agent-1", "agent-2"));
        assert!(s.resolve("agent-1", "agent-2").is_none());
    }

    #[test]
    fn abort_for_agent_removes_all_its_conflicts() {
        let mut s = ConflictStore::new();
        s.open("agent-1", "agent-2", 80, 0, vec![], vec![], vec![]);
        s.open("agent-1", "agent-3", 80, 0, vec![], vec![], vec![]);
        s.open("agent-2", "agent-3", 80, 0, vec![], vec![], vec![]);
        let aborted = s.abort_for_agent("agent-1");
        assert_eq!(aborted.len(), 2);
        assert!(aborted.iter().all(|c| c.state == ConflictState::Aborted));
        assert_eq!(s.open_count(), 1);
        assert!(s.has_open("agent-2", "agent-3"));
    }

    #[test]
    fn ids_are_sequential_across_opens() {
        let mut s = ConflictStore::new();
        let a = s.open("agent-1", "agent-2", 80, 0, vec![], vec![], vec![]).unwrap();
        let b = s.open("agent-3", "agent-4", 80, 0, vec![], vec![], vec![]).unwrap();
        assert_eq!(a.conflict_id, "conflict-1");
        assert_eq!(b.conflict_id, "conflict-2");
    }

    use crate::cap::{ClaimChange, Proposal, ProposalKind};

    fn prop(kind: ProposalKind, files: &[&str]) -> Proposal {
        Proposal {
            kind,
            summary: "s".into(),
            claim_changes: vec![ClaimChange {
                agent_id: "x".into(),
                keep: Some(files.iter().map(|s| s.to_string()).collect()),
                take: None,
            }],
            requires_user_approval: false,
        }
    }

    #[test]
    fn open_records_opened_at_and_empty_proposals() {
        let mut s = ConflictStore::new();
        let c = s.open("a", "b", 80, 12345, vec!["f".into()], vec![], vec![]).unwrap();
        assert_eq!(c.opened_at_ms, 12345);
        assert!(c.proposals.is_empty());
        assert_eq!(c.state, ConflictState::Detected);
    }

    #[test]
    fn record_proposal_sets_negotiating_and_validates_participant() {
        let mut s = ConflictStore::new();
        let c = s.open("a", "b", 80, 0, vec![], vec![], vec![]).unwrap();
        let id = c.conflict_id.clone();
        assert!(!s.record_proposal("conflict-404", "a", prop(ProposalKind::CoOwn, &[])));
        assert!(!s.record_proposal(&id, "stranger", prop(ProposalKind::CoOwn, &[])));
        assert!(s.record_proposal(&id, "a", prop(ProposalKind::CoOwn, &[])));
        assert_eq!(s.get_by_id(&id).unwrap().state, ConflictState::Negotiating);
        assert!(!s.both_proposed(&id));
        assert!(s.record_proposal(&id, "b", prop(ProposalKind::CoOwn, &[])));
        assert!(s.both_proposed(&id));
    }

    #[test]
    fn set_state_resolve_by_id_and_timed_out() {
        let mut s = ConflictStore::new();
        let id = s.open("a", "b", 80, 1000, vec![], vec![], vec![]).unwrap().conflict_id;
        // not timed out yet
        assert!(s.timed_out(1500, 1000).is_empty());
        // age 1001 > 1000 -> timed out (no proposals)
        assert_eq!(s.timed_out(2001, 1000), vec![id.clone()]);
        // once awaiting user, it is no longer swept
        s.set_state(&id, ConflictState::AwaitingUserDecision);
        assert!(s.timed_out(9999, 1000).is_empty());
        let r = s.resolve_by_id(&id).unwrap();
        assert_eq!(r.state, ConflictState::Resolved);
        assert!(s.resolve_by_id(&id).is_none());
    }

    #[test]
    fn compare_covers_every_branch() {
        let cfg = ConflictConfig::default();
        let no_paths: Vec<String> = vec![];
        // requiresUserApproval -> escalate
        let mut p = prop(ProposalKind::CoOwn, &[]); p.requires_user_approval = true;
        assert_eq!(compare(&p, &prop(ProposalKind::CoOwn, &[]), &no_paths, &cfg),
                   Decision::Escalate { reason: "USER_APPROVAL_REQUIRED" });
        // ASK_USER -> escalate
        assert_eq!(compare(&prop(ProposalKind::AskUser, &[]), &prop(ProposalKind::CoOwn, &[]), &no_paths, &cfg),
                   Decision::Escalate { reason: "AGENT_REQUESTED_USER" });
        // protected path -> escalate
        let pcfg = ConflictConfig { protected_paths: vec!["src/auth/".into()], ..ConflictConfig::default() };
        assert_eq!(compare(&prop(ProposalKind::CoOwn, &[]), &prop(ProposalKind::CoOwn, &[]),
                           &["src/auth/session.ts".to_string()], &pcfg),
                   Decision::Escalate { reason: "PROTECTED_PATH" });
        // one yields -> auto-resolve
        assert_eq!(compare(&prop(ProposalKind::YieldClaim, &[]), &prop(ProposalKind::CoOwn, &[]), &no_paths, &cfg),
                   Decision::AutoResolve { resolution: "PARTICIPANT_STEPPED_ASIDE" });
        // exactly one queue -> auto-resolve
        assert_eq!(compare(&prop(ProposalKind::QueueTask, &[]), &prop(ProposalKind::CoOwn, &[]), &no_paths, &cfg),
                   Decision::AutoResolve { resolution: "QUEUED" });
        // both split disjoint -> auto-resolve
        assert_eq!(compare(&prop(ProposalKind::SplitScope, &["a.rs"]), &prop(ProposalKind::SplitScope, &["b.rs"]), &no_paths, &cfg),
                   Decision::AutoResolve { resolution: "SCOPE_SPLIT" });
        // both split overlapping -> escalate
        assert_eq!(compare(&prop(ProposalKind::SplitScope, &["a.rs"]), &prop(ProposalKind::SplitScope, &["a.rs"]), &no_paths, &cfg),
                   Decision::Escalate { reason: "OVERLAPPING_SPLIT" });
        // both co-own, allowed -> auto-resolve
        assert_eq!(compare(&prop(ProposalKind::CoOwn, &[]), &prop(ProposalKind::CoOwn, &[]), &no_paths, &cfg),
                   Decision::AutoResolve { resolution: "CO_OWNERSHIP" });
        // both co-own, disallowed -> escalate
        let nocoown = ConflictConfig { allow_co_own: false, ..ConflictConfig::default() };
        assert_eq!(compare(&prop(ProposalKind::CoOwn, &[]), &prop(ProposalKind::CoOwn, &[]), &no_paths, &nocoown),
                   Decision::Escalate { reason: "CO_OWN_DISALLOWED" });
        // mixed incompatible (co-own vs split) -> escalate
        assert_eq!(compare(&prop(ProposalKind::CoOwn, &[]), &prop(ProposalKind::SplitScope, &["a.rs"]), &no_paths, &cfg),
                   Decision::Escalate { reason: "INCOMPATIBLE_PROPOSALS" });
    }
}
