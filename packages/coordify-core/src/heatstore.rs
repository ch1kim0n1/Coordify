use crate::heat::HeatResult;
use std::collections::HashMap;

fn key(a: &str, b: &str) -> (String, String) {
    if a <= b {
        (a.to_string(), b.to_string())
    } else {
        (b.to_string(), a.to_string())
    }
}

#[derive(Default)]
pub struct HeatStore {
    edges: HashMap<(String, String), HeatResult>,
}

impl HeatStore {
    pub fn new() -> Self {
        Self {
            edges: HashMap::new(),
        }
    }

    pub fn upsert(&mut self, a: &str, b: &str, result: HeatResult) {
        self.edges.insert(key(a, b), result);
    }

    pub fn get(&self, a: &str, b: &str) -> Option<&HeatResult> {
        self.edges.get(&key(a, b))
    }

    /// Drop every edge touching `agent`. Returns the number removed.
    pub fn remove_agent(&mut self, agent: &str) -> usize {
        let before = self.edges.len();
        self.edges.retain(|(x, y), _| x != agent && y != agent);
        before - self.edges.len()
    }

    pub fn len(&self) -> usize {
        self.edges.len()
    }

    pub fn is_empty(&self) -> bool {
        self.edges.is_empty()
    }

    pub fn snapshot(&self) -> Vec<serde_json::Value> {
        self.edges
            .iter()
            .map(|((a, b), r)| {
                serde_json::json!({
                    "pair": [a, b],
                    "heat": r.heat,
                    "band": r.band.as_str()
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heat::{HeatBand, HeatComponents, HeatResult};

    fn dummy(heat: u32) -> HeatResult {
        HeatResult {
            heat,
            band: HeatBand::Safe,
            components: HeatComponents {
                task_similarity: 0.0,
                intent_similarity: 0.0,
                domain_overlap: 0.0,
                file_path_overlap: 0.0,
                temporal_activity: 0.0,
                branch_worktree_proximity: 0.0,
                historical_hotzone_risk: 0.0,
                historical_coupling: 0.0,
            },
            reasons: vec![],
        }
    }

    #[test]
    fn ordered_key_dedups_pair_directions() {
        let mut s = HeatStore::new();
        s.upsert("agent-2", "agent-1", dummy(10));
        // Same edge regardless of order.
        assert_eq!(s.get("agent-1", "agent-2").unwrap().heat, 10);
        s.upsert("agent-1", "agent-2", dummy(20));
        assert_eq!(s.len(), 1);
        assert_eq!(s.get("agent-2", "agent-1").unwrap().heat, 20);
    }

    #[test]
    fn remove_agent_drops_all_its_edges() {
        let mut s = HeatStore::new();
        s.upsert("agent-1", "agent-2", dummy(10));
        s.upsert("agent-1", "agent-3", dummy(20));
        s.upsert("agent-2", "agent-3", dummy(30));
        let removed = s.remove_agent("agent-1");
        assert_eq!(removed, 2);
        assert_eq!(s.len(), 1);
        assert!(s.get("agent-2", "agent-3").is_some());
    }

    #[test]
    fn is_empty_reflects_contents() {
        let mut s = HeatStore::new();
        assert!(s.is_empty());
        s.upsert("agent-1", "agent-2", dummy(10));
        assert!(!s.is_empty());
    }
}
