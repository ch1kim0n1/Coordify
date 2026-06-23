use crate::cap::ClaimStatus;
use std::collections::HashMap;

pub const ACTIVE_MIN: f64 = 0.75;
pub const PROVISIONAL_MIN: f64 = 0.45;

#[derive(Debug, Clone)]
pub struct Claim {
    pub claim_id: String,
    pub agent_id: String,
    pub task_summary: String,
    pub status: ClaimStatus,
    pub intent: String,
    pub domains: Vec<String>,
    pub estimated_files: Vec<String>,
    pub confidence: f64,
    pub orphaned_at_ms: Option<u64>,
}

/// Map confidence to the initial claim status, or None if it must be rejected.
pub fn status_for_confidence(c: f64) -> Option<ClaimStatus> {
    if c >= ACTIVE_MIN {
        Some(ClaimStatus::Active)
    } else if c >= PROVISIONAL_MIN {
        Some(ClaimStatus::Provisional)
    } else {
        None
    }
}

#[derive(Default)]
pub struct ClaimStore {
    claims: HashMap<String, Claim>,
    next_id: u64,
}

impl ClaimStore {
    pub fn new() -> Self {
        Self { claims: HashMap::new(), next_id: 1 }
    }

    /// Create a claim from a proposal. Returns None if confidence is too low
    /// (the caller emits CLAIM_REJECTED).
    pub fn propose(
        &mut self,
        agent_id: &str,
        task_summary: String,
        intent: String,
        domains: Vec<String>,
        estimated_files: Vec<String>,
        confidence: f64,
    ) -> Option<Claim> {
        let status = status_for_confidence(confidence)?;
        let claim_id = format!("claim-{}", self.next_id);
        self.next_id += 1;
        let claim = Claim {
            claim_id: claim_id.clone(),
            agent_id: agent_id.to_string(),
            task_summary,
            status,
            intent,
            domains,
            estimated_files,
            confidence,
            orphaned_at_ms: None,
        };
        self.claims.insert(claim_id, claim.clone());
        Some(claim)
    }

    pub fn live_claim_for(&self, agent_id: &str) -> Option<&Claim> {
        self.claims.values().find(|c| {
            c.agent_id == agent_id
                && matches!(c.status, ClaimStatus::Active | ClaimStatus::Provisional)
        })
    }

    /// Return the distinct agent IDs that own at least one live claim.
    pub fn live_claim_agent_ids(&self) -> Vec<String> {
        let mut ids: std::collections::HashSet<String> = std::collections::HashSet::new();
        for c in self.claims.values() {
            if matches!(c.status, ClaimStatus::Active | ClaimStatus::Provisional) {
                ids.insert(c.agent_id.clone());
            }
        }
        ids.into_iter().collect()
    }

    /// Mark a single claim RELEASED. Returns false if the claim does not exist
    /// or is not in a live state (Proposed/Provisional/Active).
    pub fn release(&mut self, claim_id: &str) -> bool {
        match self.claims.get_mut(claim_id) {
            Some(c)
                if matches!(
                    c.status,
                    ClaimStatus::Proposed | ClaimStatus::Provisional | ClaimStatus::Active
                ) =>
            {
                c.status = ClaimStatus::Released;
                true
            }
            _ => false,
        }
    }

    /// Release every live (Proposed/Provisional/Active) claim owned by an agent;
    /// returns the released claim ids. Used by /clear.
    pub fn release_for_agent(&mut self, agent_id: &str) -> Vec<String> {
        let ids: Vec<String> = self
            .claims
            .values()
            .filter(|c| {
                c.agent_id == agent_id
                    && matches!(
                        c.status,
                        ClaimStatus::Proposed | ClaimStatus::Provisional | ClaimStatus::Active
                    )
            })
            .map(|c| c.claim_id.clone())
            .collect();
        for id in &ids {
            self.claims.get_mut(id).unwrap().status = ClaimStatus::Released;
        }
        ids
    }

    /// Orphan every live (Provisional/Active) claim owned by an agent that was
    /// lost uncleanly; stamps orphaned_at_ms. Returns the orphaned claim ids.
    pub fn orphan_for_agent(&mut self, agent_id: &str, now_ms: u64) -> Vec<String> {
        let ids: Vec<String> = self
            .claims
            .values()
            .filter(|c| {
                c.agent_id == agent_id
                    && matches!(c.status, ClaimStatus::Provisional | ClaimStatus::Active)
            })
            .map(|c| c.claim_id.clone())
            .collect();
        for id in &ids {
            let c = self.claims.get_mut(id).unwrap();
            c.status = ClaimStatus::Orphaned;
            c.orphaned_at_ms = Some(now_ms);
        }
        ids
    }

    /// Promote ORPHANED claims past their TTL to RECLAIMABLE. Returns the ids.
    pub fn sweep_reclaimable(&mut self, now_ms: u64, ttl_ms: u64) -> Vec<String> {
        let ids: Vec<String> = self
            .claims
            .values()
            .filter(|c| {
                c.status == ClaimStatus::Orphaned
                    && c.orphaned_at_ms
                        .is_some_and(|t| now_ms.saturating_sub(t) >= ttl_ms)
            })
            .map(|c| c.claim_id.clone())
            .collect();
        for id in &ids {
            self.claims.get_mut(id).unwrap().status = ClaimStatus::Reclaimable;
        }
        ids
    }

    pub fn get(&self, claim_id: &str) -> Option<&Claim> {
        self.claims.get(claim_id)
    }

    pub fn len(&self) -> usize {
        self.claims.len()
    }

    pub fn is_empty(&self) -> bool {
        self.claims.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confidence_maps_to_status() {
        assert_eq!(status_for_confidence(0.9), Some(ClaimStatus::Active));
        assert_eq!(status_for_confidence(0.75), Some(ClaimStatus::Active));
        assert_eq!(status_for_confidence(0.749), Some(ClaimStatus::Provisional));
        assert_eq!(status_for_confidence(0.45), Some(ClaimStatus::Provisional));
        assert_eq!(status_for_confidence(0.44), None);
    }

    #[test]
    fn propose_assigns_sequential_ids_and_rejects_low_confidence() {
        let mut s = ClaimStore::new();
        let c1 = s.propose("agent-1", "t".into(), "BUGFIX".into(), vec![], vec![], 0.9).unwrap();
        assert_eq!(c1.claim_id, "claim-1");
        assert_eq!(c1.status, ClaimStatus::Active);
        let c2 = s.propose("agent-1", "t".into(), "QA".into(), vec![], vec![], 0.5).unwrap();
        assert_eq!(c2.claim_id, "claim-2");
        assert_eq!(c2.status, ClaimStatus::Provisional);
        assert!(s.propose("agent-1", "t".into(), "QA".into(), vec![], vec![], 0.1).is_none());
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn release_and_release_for_agent() {
        let mut s = ClaimStore::new();
        let c = s.propose("agent-1", "t".into(), "BUGFIX".into(), vec![], vec![], 0.9).unwrap();
        assert!(s.release(&c.claim_id));
        assert_eq!(s.get(&c.claim_id).unwrap().status, ClaimStatus::Released);
        assert!(!s.release("claim-999"));

        let a = s.propose("agent-2", "t".into(), "QA".into(), vec![], vec![], 0.9).unwrap();
        let _b = s.propose("agent-2", "t".into(), "FEATURE".into(), vec![], vec![], 0.5).unwrap();
        let released = s.release_for_agent("agent-2");
        assert_eq!(released.len(), 2);
        assert_eq!(s.get(&a.claim_id).unwrap().status, ClaimStatus::Released);
    }

    #[test]
    fn release_only_succeeds_on_live_claim() {
        let mut s = ClaimStore::new();
        let c = s.propose("agent-1", "t".into(), "BUGFIX".into(), vec![], vec![], 0.9).unwrap();
        assert!(s.release(&c.claim_id)); // Active -> Released: ok
        assert!(!s.release(&c.claim_id)); // already Released: no-op, false
        // Orphaned claim cannot be released either.
        let d = s.propose("agent-2", "t".into(), "QA".into(), vec![], vec![], 0.9).unwrap();
        s.orphan_for_agent("agent-2", 1_000);
        assert!(!s.release(&d.claim_id));
    }

    #[test]
    fn orphan_then_sweep_reclaimable_respects_ttl() {
        let mut s = ClaimStore::new();
        let c = s.propose("agent-1", "t".into(), "BUGFIX".into(), vec![], vec![], 0.9).unwrap();
        let orphaned = s.orphan_for_agent("agent-1", 1_000);
        assert_eq!(orphaned, vec![c.claim_id.clone()]);
        assert_eq!(s.get(&c.claim_id).unwrap().status, ClaimStatus::Orphaned);

        // Not yet past TTL (idle 500 < 1000): no sweep.
        assert!(s.sweep_reclaimable(1_500, 1_000).is_empty());
        assert_eq!(s.get(&c.claim_id).unwrap().status, ClaimStatus::Orphaned);
        // Past TTL (idle 1000 >= 1000): swept.
        let swept = s.sweep_reclaimable(2_000, 1_000);
        assert_eq!(swept, vec![c.claim_id.clone()]);
        assert_eq!(s.get(&c.claim_id).unwrap().status, ClaimStatus::Reclaimable);
    }
}
