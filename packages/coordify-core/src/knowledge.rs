use crate::heat::Knowledge;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Ordered key so (a,b) and (b,a) map to the same coupling entry.
fn pair(a: &str, b: &str) -> (String, String) {
    if a <= b {
        (a.to_string(), b.to_string())
    } else {
        (b.to_string(), a.to_string())
    }
}

/// Append a literal suffix to a path (e.g. ".prev", ".tmp") without losing the
/// existing extension — `with_extension` would replace `.json`.
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

#[derive(Serialize, Deserialize)]
struct CouplingEdge {
    a: String,
    b: String,
    count: u64,
}

/// Persisted integer counts. Scores are derived on demand (see `snapshot`).
#[derive(Default)]
pub struct KnowledgeStore {
    hotzone_counts: HashMap<String, u64>,
    coupling_counts: HashMap<(String, String), u64>,
}

impl KnowledgeStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_conflict(&mut self, paths: &[String]) {
        for p in paths {
            *self.hotzone_counts.entry(p.clone()).or_insert(0) += 1;
        }
        self.accrue_pairs(paths);
    }

    pub fn record_claim_files(&mut self, files: &[String]) {
        self.accrue_pairs(files);
    }

    fn accrue_pairs(&mut self, files: &[String]) {
        for i in 0..files.len() {
            for j in (i + 1)..files.len() {
                let k = pair(&files[i], &files[j]);
                *self.coupling_counts.entry(k).or_insert(0) += 1;
            }
        }
    }

    /// Derive `heat::Knowledge` scores via the saturating curve `n / (n + k)`.
    pub fn snapshot(&self, k: f64) -> Knowledge {
        let score = |n: u64| (n as f64) / (n as f64 + k);
        Knowledge {
            hotzones: self
                .hotzone_counts
                .iter()
                .map(|(p, &n)| (p.clone(), score(n)))
                .collect(),
            coupling: self
                .coupling_counts
                .iter()
                .map(|(k2, &n)| (k2.clone(), score(n)))
                .collect(),
        }
    }

    pub fn hotzone_count(&self, path: &str) -> u64 {
        self.hotzone_counts.get(path).copied().unwrap_or(0)
    }

    pub fn coupling_count(&self, a: &str, b: &str) -> u64 {
        self.coupling_counts.get(&pair(a, b)).copied().unwrap_or(0)
    }

    /// Atomic write of both knowledge files: rotate the existing file to `.prev`,
    /// write a temp file, rename over the canonical name.
    pub fn save_atomic(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;

        let hz_path = dir.join("hotzones.json");
        let hz = serde_json::to_string_pretty(&self.hotzone_counts).unwrap_or_else(|_| "{}".into());
        write_atomic(&hz_path, &hz)?;

        let cp_path = dir.join("coupling-graph.json");
        let edges: Vec<CouplingEdge> = self
            .coupling_counts
            .iter()
            .map(|((a, b), &count)| CouplingEdge { a: a.clone(), b: b.clone(), count })
            .collect();
        let cp = serde_json::to_string_pretty(&edges).unwrap_or_else(|_| "[]".into());
        write_atomic(&cp_path, &cp)?;
        Ok(())
    }

    /// Load counts from `dir`. A file that fails to parse is moved into
    /// `dir/quarantine/` and its map starts empty; the returned Vec lists the
    /// quarantined paths (for the caller to log).
    pub fn load(dir: &Path) -> (Self, Vec<String>) {
        let mut store = Self::default();
        let mut quarantined = Vec::new();

        let hz_path = dir.join("hotzones.json");
        if hz_path.exists() {
            match std::fs::read_to_string(&hz_path)
                .ok()
                .and_then(|s| serde_json::from_str::<HashMap<String, u64>>(&s).ok())
            {
                Some(m) => store.hotzone_counts = m,
                None => quarantine(&hz_path, &mut quarantined),
            }
        }

        let cp_path = dir.join("coupling-graph.json");
        if cp_path.exists() {
            match std::fs::read_to_string(&cp_path)
                .ok()
                .and_then(|s| serde_json::from_str::<Vec<CouplingEdge>>(&s).ok())
            {
                Some(edges) => {
                    for e in edges {
                        store.coupling_counts.insert(pair(&e.a, &e.b), e.count);
                    }
                }
                None => quarantine(&cp_path, &mut quarantined),
            }
        }

        (store, quarantined)
    }
}

fn write_atomic(path: &Path, contents: &str) -> std::io::Result<()> {
    if path.exists() {
        // Rotate the last-good file to .prev (best-effort).
        let _ = std::fs::rename(path, with_suffix(path, ".prev"));
    }
    let tmp = with_suffix(path, ".tmp");
    std::fs::write(&tmp, contents)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn quarantine(path: &Path, out: &mut Vec<String>) {
    let dir = path.parent().map(|p| p.join("quarantine"));
    if let Some(qdir) = dir {
        let _ = std::fs::create_dir_all(&qdir);
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "unknown".into());
        let stamp = crate::bootstrap::now_iso().replace(':', "-");
        let dest = qdir.join(format!("{name}.{stamp}"));
        if std::fs::rename(path, &dest).is_ok() {
            out.push(dest.to_string_lossy().to_string());
        } else {
            out.push(path.to_string_lossy().to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn record_conflict_accrues_hotzone_and_coupling() {
        let mut s = KnowledgeStore::new();
        s.record_conflict(&["a.rs".into(), "b.rs".into()]);
        assert_eq!(s.hotzone_count("a.rs"), 1);
        assert_eq!(s.hotzone_count("b.rs"), 1);
        assert_eq!(s.coupling_count("a.rs", "b.rs"), 1);
        // direction-independent
        assert_eq!(s.coupling_count("b.rs", "a.rs"), 1);
        s.record_conflict(&["a.rs".into(), "b.rs".into()]);
        assert_eq!(s.hotzone_count("a.rs"), 2);
        assert_eq!(s.coupling_count("a.rs", "b.rs"), 2);
    }

    #[test]
    fn record_claim_files_accrues_coupling_only_pairs() {
        let mut s = KnowledgeStore::new();
        s.record_claim_files(&["x".into(), "y".into(), "z".into()]);
        // 3 files -> 3 unordered pairs, each count 1; no hotzone accrual
        assert_eq!(s.coupling_count("x", "y"), 1);
        assert_eq!(s.coupling_count("x", "z"), 1);
        assert_eq!(s.coupling_count("y", "z"), 1);
        assert_eq!(s.hotzone_count("x"), 0);
        // single-file claim -> no pairs
        let mut s2 = KnowledgeStore::new();
        s2.record_claim_files(&["solo".into()]);
        assert_eq!(s2.coupling_count("solo", "solo"), 0);
    }

    #[test]
    fn snapshot_uses_saturating_curve() {
        let mut s = KnowledgeStore::new();
        for _ in 0..5 { s.record_conflict(&["f".into()]); } // count 5
        let k = s.snapshot(5.0);
        // 5/(5+5) = 0.5
        assert!((k.hotzone_risk("f") - 0.5).abs() < 1e-9);
        // unseen file -> 0
        assert_eq!(k.hotzone_risk("missing"), 0.0);
        // n=0 path absent
        let empty = KnowledgeStore::new().snapshot(5.0);
        assert!(empty.hotzones.is_empty());
    }

    #[test]
    fn save_then_load_round_trips_counts() {
        let dir = std::env::temp_dir().join(format!("ck-{}-{}", std::process::id(), 1));
        let _ = std::fs::remove_dir_all(&dir);
        let mut s = KnowledgeStore::new();
        s.record_conflict(&["a".into(), "b".into()]);
        s.record_claim_files(&["a".into(), "c".into()]);
        s.save_atomic(&dir).unwrap();
        let (loaded, quarantined) = KnowledgeStore::load(&dir);
        assert!(quarantined.is_empty());
        assert_eq!(loaded.hotzone_count("a"), 1);
        assert_eq!(loaded.coupling_count("a", "b"), 1);
        assert_eq!(loaded.coupling_count("a", "c"), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_rotates_prev() {
        let dir = std::env::temp_dir().join(format!("ck-{}-{}", std::process::id(), 2));
        let _ = std::fs::remove_dir_all(&dir);
        let mut s = KnowledgeStore::new();
        s.record_conflict(&["a".into()]);
        s.record_claim_files(&["a".into(), "b".into()]); // coupling (a,b) count 1
        s.save_atomic(&dir).unwrap();          // first write, no prev
        let mut s2 = KnowledgeStore::new();
        s2.record_conflict(&["a".into()]);
        s2.record_conflict(&["a".into()]);      // count 2
        s2.record_claim_files(&["a".into(), "b".into()]);
        s2.record_claim_files(&["a".into(), "b".into()]); // coupling (a,b) count 2
        s2.save_atomic(&dir).unwrap();          // rotates prior (counts 1) to .prev
        assert!(dir.join("hotzones.json.prev").exists());
        let prev: HashMap<String, u64> =
            serde_json::from_str(&std::fs::read_to_string(dir.join("hotzones.json.prev")).unwrap()).unwrap();
        assert_eq!(prev.get("a").copied(), Some(1));
        let cur: HashMap<String, u64> =
            serde_json::from_str(&std::fs::read_to_string(dir.join("hotzones.json")).unwrap()).unwrap();
        assert_eq!(cur.get("a").copied(), Some(2));
        // coupling-graph rotates too (substring assertions on pretty JSON to
        // avoid needing the private CouplingEdge type)
        assert!(dir.join("coupling-graph.json.prev").exists());
        let cp_prev =
            std::fs::read_to_string(dir.join("coupling-graph.json.prev")).unwrap();
        assert!(cp_prev.contains("\"count\": 1"));
        let cp_cur = std::fs::read_to_string(dir.join("coupling-graph.json")).unwrap();
        assert!(cp_cur.contains("\"count\": 2"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_file_is_quarantined_and_map_starts_empty() {
        let dir = std::env::temp_dir().join(format!("ck-{}-{}", std::process::id(), 3));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("hotzones.json"), b"{ not json").unwrap();
        let (loaded, quarantined) = KnowledgeStore::load(&dir);
        assert_eq!(quarantined.len(), 1);
        assert_eq!(loaded.hotzone_count("anything"), 0);
        // original corrupt file moved out of the way
        assert!(!dir.join("hotzones.json").exists());
        assert!(dir.join("quarantine").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
