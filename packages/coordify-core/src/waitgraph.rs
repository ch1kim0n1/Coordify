use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WaitEdge {
    pub from: String,
    pub to: String,
    pub resource: String,
}

/// Directed wait-for graph. An edge `from -> to` means agent `from` is waiting
/// on agent `to` for `resource`. A cycle is a deadlock (CAP_SPEC §20).
#[derive(Default)]
pub struct WaitGraph {
    edges: Vec<WaitEdge>,
}

impl WaitGraph {
    pub fn new() -> Self {
        Self { edges: Vec::new() }
    }

    /// Add `from -> to`; ignores an existing edge in the same direction.
    /// Dedup is on the `(from, to)` pair only: a second call with the same pair
    /// but a different `resource` is silently ignored (first `resource` wins).
    pub fn add_edge(&mut self, from: &str, to: &str, resource: &str) {
        if self
            .edges
            .iter()
            .any(|e| e.from.as_str() == from && e.to.as_str() == to)
        {
            return;
        }
        self.edges.push(WaitEdge {
            from: from.to_string(),
            to: to.to_string(),
            resource: resource.to_string(),
        });
    }

    pub fn remove_agent(&mut self, agent: &str) {
        self.edges
            .retain(|e| e.from.as_str() != agent && e.to.as_str() != agent);
    }

    /// Return the edges of a directed cycle if one exists (DFS, three-colour).
    pub fn find_cycle(&self) -> Option<Vec<WaitEdge>> {
        // Adjacency values are `Vec`s built in `self.edges` insertion order, so
        // traversal of any node's out-edges is deterministic despite the HashMap.
        let mut adj: HashMap<&str, Vec<&WaitEdge>> = HashMap::new();
        for e in &self.edges {
            adj.entry(e.from.as_str()).or_default().push(e);
        }
        // 0 = unvisited, 1 = on current DFS stack, 2 = done
        let mut state: HashMap<&str, u8> = HashMap::new();
        let mut order: Vec<&str> = Vec::new();
        for e in &self.edges {
            for n in [e.from.as_str(), e.to.as_str()] {
                if !state.contains_key(n) {
                    state.insert(n, 0);
                    order.push(n);
                }
            }
        }
        for &start in &order {
            if state[start] == 0 {
                let mut path: Vec<&WaitEdge> = Vec::new();
                if let Some(c) = Self::dfs(start, &adj, &mut state, &mut path) {
                    return Some(c);
                }
            }
        }
        None
    }

    fn dfs<'a>(
        node: &'a str,
        adj: &HashMap<&'a str, Vec<&'a WaitEdge>>,
        state: &mut HashMap<&'a str, u8>,
        path: &mut Vec<&'a WaitEdge>,
    ) -> Option<Vec<WaitEdge>> {
        state.insert(node, 1);
        if let Some(outs) = adj.get(node) {
            for e in outs {
                let to = e.to.as_str();
                match state.get(to).copied().unwrap_or(0) {
                    1 => {
                        // Back-edge: cycle runs from where `to` entered the path.
                        let mut cyc: Vec<WaitEdge> =
                            match path.iter().position(|pe| pe.from.as_str() == to) {
                                Some(i) => path[i..].iter().map(|pe| (*pe).clone()).collect(),
                                None => Vec::new(), // self-loop: `to` == current node, no entry edge on the path
                            };
                        cyc.push((*e).clone());
                        return Some(cyc);
                    }
                    0 => {
                        path.push(e);
                        if let Some(c) = Self::dfs(to, adj, state, path) {
                            return Some(c);
                        }
                        path.pop();
                    }
                    _ => {}
                }
            }
        }
        state.insert(node, 2);
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_two_cycle() {
        let mut g = WaitGraph::new();
        g.add_edge("a", "b", "f1");
        g.add_edge("b", "a", "f2");
        let cyc = g.find_cycle().expect("expected a cycle");
        assert_eq!(cyc.len(), 2);
        // both agents appear as a `from`
        let froms: std::collections::BTreeSet<&str> = cyc.iter().map(|e| e.from.as_str()).collect();
        assert!(froms.contains("a") && froms.contains("b"));
    }

    #[test]
    fn no_cycle_in_dag() {
        let mut g = WaitGraph::new();
        g.add_edge("a", "b", "f1");
        g.add_edge("b", "c", "f2");
        assert!(g.find_cycle().is_none());
    }

    #[test]
    fn remove_agent_breaks_cycle_and_dedupes() {
        let mut g = WaitGraph::new();
        g.add_edge("a", "b", "f1");
        g.add_edge("a", "b", "f1"); // duplicate ignored
        g.add_edge("b", "a", "f2");
        assert!(g.find_cycle().is_some());
        g.remove_agent("a");
        assert!(g.find_cycle().is_none());
    }

    #[test]
    fn direction_matters() {
        let mut g = WaitGraph::new();
        // a waits on b twice in the SAME direction -> no cycle
        g.add_edge("a", "b", "f1");
        assert!(g.find_cycle().is_none());
    }

    #[test]
    fn detects_three_cycle() {
        let mut g = WaitGraph::new();
        g.add_edge("a", "b", "f1");
        g.add_edge("b", "c", "f2");
        g.add_edge("c", "a", "f3");
        assert_eq!(g.find_cycle().expect("cycle").len(), 3);
    }

    #[test]
    fn self_loop_is_a_cycle_without_tail() {
        let mut g = WaitGraph::new();
        g.add_edge("x", "a", "r0"); // tail leading into the loop
        g.add_edge("a", "a", "r1"); // self-loop
        let cyc = g.find_cycle().expect("self-loop is a cycle");
        assert_eq!(
            cyc.len(),
            1,
            "cycle must be just the self edge, not the tail"
        );
        assert_eq!(cyc[0].from, "a");
        assert_eq!(cyc[0].to, "a");
    }
}
