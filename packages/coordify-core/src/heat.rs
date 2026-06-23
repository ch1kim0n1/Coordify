use serde::Serialize;
use std::collections::{BTreeSet, HashMap};

#[derive(Debug, Clone)]
pub struct HeatConfig {
    pub w_task: f64,
    pub w_intent: f64,
    pub w_domain: f64,
    pub w_file: f64,
    pub w_temporal: f64,
    pub w_branch: f64,
    pub w_hotzone: f64,
    pub w_coupling: f64,
    pub temporal_window_ms: u64,
    pub safe_max: u32,
    pub monitor_max: u32,
    pub overlap_max: u32,
}

impl Default for HeatConfig {
    fn default() -> Self {
        Self {
            w_task: 0.10,
            w_intent: 0.15,
            w_domain: 0.15,
            w_file: 0.20,
            w_temporal: 0.10,
            w_branch: 0.10,
            w_hotzone: 0.10,
            w_coupling: 0.10,
            temporal_window_ms: 60_000,
            safe_max: 25,
            monitor_max: 50,
            overlap_max: 75,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HeatBand {
    Safe,
    Monitor,
    Overlap,
    ConflictCandidate,
}

impl HeatBand {
    pub fn as_str(&self) -> &'static str {
        match self {
            HeatBand::Safe => "SAFE",
            HeatBand::Monitor => "MONITOR",
            HeatBand::Overlap => "OVERLAP",
            HeatBand::ConflictCandidate => "CONFLICT_CANDIDATE",
        }
    }
    pub fn recommendation(&self) -> &'static str {
        match self {
            HeatBand::Safe => "PROCEED",
            HeatBand::Monitor => "MONITOR",
            HeatBand::Overlap => "SPLIT_SCOPE_OR_SEQUENCE",
            HeatBand::ConflictCandidate => "NEGOTIATE_BEFORE_CLAIM",
        }
    }
}

/// Persistent knowledge inputs. Empty in Phase 3 (populated in Phase 5), so the
/// hotzone and coupling components score 0 until then.
#[derive(Debug, Clone, Default)]
pub struct Knowledge {
    pub hotzones: HashMap<String, f64>,
    pub coupling: HashMap<(String, String), f64>,
}

impl Knowledge {
    pub fn hotzone_risk(&self, path: &str) -> f64 {
        self.hotzones.get(path).copied().unwrap_or(0.0)
    }
    pub fn coupling_score(&self, a: &str, b: &str) -> f64 {
        self.coupling
            .get(&(a.to_string(), b.to_string()))
            .or_else(|| self.coupling.get(&(b.to_string(), a.to_string())))
            .copied()
            .unwrap_or(0.0)
    }
}

#[derive(Debug, Clone)]
pub struct HeatInputs {
    pub agent_id: String,
    pub intent: String,
    pub domains: BTreeSet<String>,
    pub files: BTreeSet<String>,
    pub task_tokens: BTreeSet<String>,
    pub last_seen_ms: u64,
    pub branch: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeatComponents {
    pub task_similarity: f64,
    pub intent_similarity: f64,
    pub domain_overlap: f64,
    pub file_path_overlap: f64,
    pub temporal_activity: f64,
    pub branch_worktree_proximity: f64,
    pub historical_hotzone_risk: f64,
    pub historical_coupling: f64,
}

#[derive(Debug, Clone)]
pub struct HeatResult {
    pub heat: u32,
    pub band: HeatBand,
    pub components: HeatComponents,
    pub reasons: Vec<String>,
}

/// Lowercased alphanumeric word tokens (deterministic, order-independent).
pub fn tokens(text: &str) -> BTreeSet<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase())
        .collect()
}

fn jaccard(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f64 {
    let union = a.union(b).count() as f64;
    if union == 0.0 {
        0.0
    } else {
        a.intersection(b).count() as f64 / union
    }
}

pub fn band_for(heat: u32, cfg: &HeatConfig) -> HeatBand {
    if heat <= cfg.safe_max {
        HeatBand::Safe
    } else if heat <= cfg.monitor_max {
        HeatBand::Monitor
    } else if heat <= cfg.overlap_max {
        HeatBand::Overlap
    } else {
        HeatBand::ConflictCandidate
    }
}

pub fn compute_heat(
    a: &HeatInputs,
    b: &HeatInputs,
    knowledge: &Knowledge,
    cfg: &HeatConfig,
) -> HeatResult {
    let task = jaccard(&a.task_tokens, &b.task_tokens);
    let intent = if a.intent == b.intent { 1.0 } else { 0.0 };
    let domain = jaccard(&a.domains, &b.domains);
    let file = jaccard(&a.files, &b.files);
    let diff = a.last_seen_ms.abs_diff(b.last_seen_ms) as f64;
    let temporal = 1.0 - (diff / cfg.temporal_window_ms as f64).min(1.0);
    let branch = match (&a.branch, &b.branch) {
        (Some(x), Some(y)) if x == y => 1.0,
        _ => 0.0,
    };
    let shared_files: Vec<&String> = a.files.intersection(&b.files).collect();
    let hotzone = shared_files
        .iter()
        .map(|f| knowledge.hotzone_risk(f))
        .fold(0.0_f64, f64::max);
    let mut coupling = 0.0_f64;
    for fa in &a.files {
        for fb in &b.files {
            coupling = coupling.max(knowledge.coupling_score(fa, fb));
        }
    }

    let raw = cfg.w_task * task
        + cfg.w_intent * intent
        + cfg.w_domain * domain
        + cfg.w_file * file
        + cfg.w_temporal * temporal
        + cfg.w_branch * branch
        + cfg.w_hotzone * hotzone
        + cfg.w_coupling * coupling;
    let heat = (raw * 100.0).round() as u32;
    let band = band_for(heat, cfg);

    let mut reasons = Vec::new();
    if intent == 1.0 {
        reasons.push(format!("same intent: {}", a.intent));
    }
    if !shared_files.is_empty() {
        let names: Vec<&str> = shared_files.iter().map(|s| s.as_str()).collect();
        reasons.push(format!("shared files: {}", names.join(", ")));
    }
    if domain > 0.0 {
        reasons.push("overlapping domains".to_string());
    }
    if branch == 1.0 {
        if let Some(br) = &a.branch {
            reasons.push(format!("same branch: {br}"));
        }
    }

    HeatResult {
        heat,
        band,
        components: HeatComponents {
            task_similarity: task,
            intent_similarity: intent,
            domain_overlap: domain,
            file_path_overlap: file,
            temporal_activity: temporal,
            branch_worktree_proximity: branch,
            historical_hotzone_risk: hotzone,
            historical_coupling: coupling,
        },
        reasons,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs(agent: &str, intent: &str, files: &[&str], domains: &[&str], task: &str, last_seen: u64, branch: Option<&str>) -> HeatInputs {
        HeatInputs {
            agent_id: agent.to_string(),
            intent: intent.to_string(),
            domains: domains.iter().map(|s| s.to_string()).collect(),
            files: files.iter().map(|s| s.to_string()).collect(),
            task_tokens: tokens(task),
            last_seen_ms: last_seen,
            branch: branch.map(|s| s.to_string()),
        }
    }

    #[test]
    fn tokens_are_lowercased_words() {
        let t = tokens("Fix Auth-token Expiry!");
        let expected: BTreeSet<String> = ["fix", "auth", "token", "expiry"].iter().map(|s| s.to_string()).collect();
        assert_eq!(t, expected);
    }

    #[test]
    fn identical_high_overlap_scores_eighty() {
        // same intent, same single file, same domain, same task, same time, same branch; empty knowledge.
        let cfg = HeatConfig::default();
        let k = Knowledge::default();
        let a = inputs("agent-1", "BUGFIX", &["src/auth/session.ts"], &["AUTH"], "fix session expiry", 1000, Some("main"));
        let b = inputs("agent-2", "BUGFIX", &["src/auth/session.ts"], &["AUTH"], "fix session expiry", 1000, Some("main"));
        let r = compute_heat(&a, &b, &k, &cfg);
        // task .10 + intent .15 + domain .15 + file .20 + temporal .10 + branch .10 = 0.80 -> 80
        assert_eq!(r.heat, 80);
        assert_eq!(r.band, HeatBand::ConflictCandidate);
        assert_eq!(r.components.intent_similarity, 1.0);
        assert_eq!(r.components.historical_hotzone_risk, 0.0);
    }

    #[test]
    fn disjoint_work_scores_zero() {
        let cfg = HeatConfig::default();
        let k = Knowledge::default();
        // different intent, no shared files/domains/task, far apart in time, different branch.
        let a = inputs("agent-1", "BUGFIX", &["src/a.rs"], &["AUTH"], "alpha", 0, Some("feature-a"));
        let b = inputs("agent-2", "DOCUMENTATION", &["docs/b.md"], &["DOCS"], "beta", 10_000_000, Some("feature-b"));
        let r = compute_heat(&a, &b, &k, &cfg);
        assert_eq!(r.heat, 0);
        assert_eq!(r.band, HeatBand::Safe);
    }

    #[test]
    fn band_boundaries() {
        let cfg = HeatConfig::default();
        assert_eq!(band_for(25, &cfg), HeatBand::Safe);
        assert_eq!(band_for(26, &cfg), HeatBand::Monitor);
        assert_eq!(band_for(50, &cfg), HeatBand::Monitor);
        assert_eq!(band_for(51, &cfg), HeatBand::Overlap);
        assert_eq!(band_for(75, &cfg), HeatBand::Overlap);
        assert_eq!(band_for(76, &cfg), HeatBand::ConflictCandidate);
    }

    #[test]
    fn intent_only_is_safe() {
        let cfg = HeatConfig::default();
        let k = Knowledge::default();
        // same intent only (0.15 -> 15); time far apart so temporal ~0; no other overlap.
        // 15 <= safe_max(25) -> SAFE.
        let a = inputs("agent-1", "BUGFIX", &["a"], &["X"], "p", 0, None);
        let b = inputs("agent-2", "BUGFIX", &["b"], &["Y"], "q", 10_000_000, None);
        let r = compute_heat(&a, &b, &k, &cfg);
        assert_eq!(r.heat, 15);
        assert_eq!(r.band, HeatBand::Safe);
        assert_eq!(r.band.recommendation(), "PROCEED");
    }

    #[test]
    fn intent_and_file_is_monitor() {
        let cfg = HeatConfig::default();
        let k = Knowledge::default();
        // same intent (15) + one shared file (jaccard 1.0 -> 0.20 -> 20) = 35 -> MONITOR.
        // time far apart so temporal ~0; no domain/branch overlap.
        let a = inputs("agent-1", "BUGFIX", &["x.rs"], &[], "p", 0, None);
        let b = inputs("agent-2", "BUGFIX", &["x.rs"], &[], "q", 10_000_000, None);
        let r = compute_heat(&a, &b, &k, &cfg);
        assert_eq!(r.heat, 35);
        assert_eq!(r.band, HeatBand::Monitor);
        assert_eq!(r.band.recommendation(), "MONITOR");
    }

    #[test]
    fn knowledge_adds_hotzone_and_coupling() {
        let cfg = HeatConfig::default();
        let mut k = Knowledge::default();
        k.hotzones.insert("src/auth/session.ts".to_string(), 1.0);
        // a and b share the file; hotzone risk 1.0 contributes 0.10 -> +10.
        let a = inputs("agent-1", "BUGFIX", &["src/auth/session.ts"], &[], "", 0, None);
        let b = inputs("agent-2", "BUGFIX", &["src/auth/session.ts"], &[], "", 10_000_000, None);
        let r = compute_heat(&a, &b, &k, &cfg);
        // intent .15 (15) + file 1.0*.20 (20) + hotzone 1.0*.10 (10) = 45 -> MONITOR
        assert_eq!(r.heat, 45);
        assert_eq!(r.components.historical_hotzone_risk, 1.0);
    }
}
