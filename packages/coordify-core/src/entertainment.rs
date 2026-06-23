use crate::stats::SessionStats;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Serialize)]
pub struct LeaderEntry {
    pub agent: String,
    pub value: f64,
}

#[derive(Serialize)]
pub struct Leaderboard {
    pub metric: String,
    pub color: String,
    pub entries: Vec<LeaderEntry>,
}

#[derive(Serialize)]
pub struct Badge {
    pub id: String,
    pub label: String,
    pub color: String,
    pub agent: String,
}

#[derive(Serialize)]
pub struct Superlative {
    pub key: String,
    pub label: String,
    pub color: String,
    pub value: Value,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Streaks {
    pub longest_auto_resolve_streak: u64,
    pub longest_completion_streak: u64,
}

#[derive(Serialize)]
pub struct Entertainment {
    pub leaderboards: Vec<Leaderboard>,
    pub badges: Vec<Badge>,
    pub superlatives: Vec<Superlative>,
    pub streaks: Streaks,
    pub narrative: String,
}

// ---------------------------------------------------------------------------
// Helpers (private)
// ---------------------------------------------------------------------------

fn ev_type(e: &Value) -> &str {
    e.get("type").and_then(|v| v.as_str()).unwrap_or("")
}

fn ev_str(e: &Value, k: &str) -> String {
    e.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string()
}

fn parse_ms(ts: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(ts)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

/// Longest run of consecutive `true` values in a sequence of bools.
fn longest_run(flags: impl Iterator<Item = bool>) -> u64 {
    let mut cur: u64 = 0;
    let mut max: u64 = 0;
    for hit in flags {
        if hit {
            cur += 1;
            if cur > max {
                max = cur;
            }
        } else {
            cur = 0;
        }
    }
    max
}

fn ordered_pair(a: &str, b: &str) -> (String, String) {
    if a <= b {
        (a.to_string(), b.to_string())
    } else {
        (b.to_string(), a.to_string())
    }
}

/// Build a leaderboard from (agent, value) pairs:
/// - filter value > 0 (for ascending boards, filter count > 0 is handled at call-site)
/// - sort by value desc then agent asc (or value asc for `ascending = true`)
fn make_leaderboard(
    metric: &str,
    color: &str,
    mut pairs: Vec<(String, f64)>,
    ascending: bool,
) -> Leaderboard {
    pairs.retain(|(_, v)| *v > 0.0);
    pairs.sort_by(|(a1, v1), (a2, v2)| {
        if ascending {
            v1.partial_cmp(v2)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a1.cmp(a2))
        } else {
            v2.partial_cmp(v1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a1.cmp(a2))
        }
    });
    let entries = pairs
        .into_iter()
        .map(|(agent, value)| LeaderEntry { agent, value })
        .collect();
    Leaderboard {
        metric: metric.to_string(),
        color: color.to_string(),
        entries,
    }
}

/// Return the lowest agent-id that has the maximum value of `f(tally)`, only when max > 0.
fn badge_max_agent(
    agents: &BTreeMap<String, crate::stats::AgentTally>,
    f: impl Fn(&crate::stats::AgentTally) -> u64,
) -> Option<String> {
    let max_val = agents.values().map(&f).max()?;
    if max_val == 0 {
        return None;
    }
    // BTreeMap iterates in ascending key order, so first match is lowest id.
    agents
        .iter()
        .find(|(_, t)| f(t) == max_val)
        .map(|(id, _)| id.clone())
}

/// Return the lowest agent-id satisfying `pred`, or None.
fn badge_first(
    agents: &BTreeMap<String, crate::stats::AgentTally>,
    pred: impl Fn(&crate::stats::AgentTally) -> bool,
) -> Option<String> {
    agents
        .iter()
        .find(|(_, t)| pred(t))
        .map(|(id, _)| id.clone())
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn build_entertainment(events: &[Value], stats: &SessionStats) -> Entertainment {
    // -----------------------------------------------------------------------
    // Pass 1: Collect per-agent file-touch counts, diplomat counts,
    //         heat-spike tracking, conflict timing, bloodiest-minute buckets,
    //         speed-demon (claim durations).
    // -----------------------------------------------------------------------

    // Per-agent touched files (sets for uniqueness)
    let mut agent_files: BTreeMap<String, std::collections::BTreeSet<String>> = BTreeMap::new();

    // File occurrence counts: CONFLICT_OPENED.paths + FILE_TOUCHED.files
    let mut file_counts: BTreeMap<String, u64> = BTreeMap::new();

    // Diplomat: count of CONFLICT_PROPOSAL_RECEIVED with kind YIELD_CLAIM or CO_OWN* by from
    let mut diplomat_count: BTreeMap<String, u64> = BTreeMap::new();

    // Heat spike: last heat per ordered pair
    let mut last_heat: BTreeMap<(String, String), u64> = BTreeMap::new();
    let mut biggest_spike: Option<(String, String, u64, u64, u64)> = None; // pair0,pair1,from,to,delta

    // Conflict timing: conflictId -> open_ms
    let mut conflict_open: BTreeMap<String, i64> = BTreeMap::new();
    let mut longest_neg: Option<(String, i64)> = None; // conflictId, ms

    // Bloodiest minute: bucket -> count
    let mut minute_buckets: BTreeMap<i64, u64> = BTreeMap::new();

    // Speed demon: claimId -> create_ms
    let mut claim_create_ms: BTreeMap<String, i64> = BTreeMap::new();
    // agent -> (total_ms, count)
    let mut claim_durations: BTreeMap<String, (i64, u64)> = BTreeMap::new();

    for e in events {
        // Bloodiest minute: bucket every event
        if let Some(ms) = e.get("ts").and_then(|v| v.as_str()).and_then(parse_ms) {
            let bucket = ms / 60_000;
            *minute_buckets.entry(bucket).or_insert(0) += 1;
        }

        let etype = ev_type(e);

        match etype {
            "FILE_TOUCHED" => {
                let agent = ev_str(e, "agentId");
                if let Some(arr) = e.get("files").and_then(|v| v.as_array()) {
                    for f in arr {
                        if let Some(p) = f.as_str() {
                            agent_files
                                .entry(agent.clone())
                                .or_default()
                                .insert(p.to_string());
                            *file_counts.entry(p.to_string()).or_insert(0) += 1;
                        }
                    }
                }
            }
            "CONFLICT_OPENED" => {
                let cid = ev_str(e, "conflictId");
                if let Some(ms) = e.get("ts").and_then(|v| v.as_str()).and_then(parse_ms) {
                    conflict_open.insert(cid, ms);
                }
                if let Some(arr) = e.get("paths").and_then(|v| v.as_array()) {
                    for p in arr {
                        if let Some(s) = p.as_str() {
                            *file_counts.entry(s.to_string()).or_insert(0) += 1;
                        }
                    }
                }
            }
            "CONFLICT_RESOLVED" | "CONFLICT_ESCALATED" => {
                let cid = ev_str(e, "conflictId");
                if let Some(open_ms) = conflict_open.remove(&cid) {
                    if let Some(close_ms) =
                        e.get("ts").and_then(|v| v.as_str()).and_then(parse_ms)
                    {
                        let span = close_ms - open_ms;
                        let update = match &longest_neg {
                            None => true,
                            Some((_, max_ms)) => span > *max_ms,
                        };
                        if update {
                            longest_neg = Some((cid, span));
                        }
                    }
                }
            }
            "CONFLICT_PROPOSAL_RECEIVED" => {
                let kind = ev_str(e, "kind");
                if kind == "YIELD_CLAIM" || kind == "CO_OWN" || kind == "CO_OWNERSHIP" {
                    let from = ev_str(e, "from");
                    if !from.is_empty() {
                        *diplomat_count.entry(from).or_insert(0) += 1;
                    }
                }
            }
            "HEAT_UPDATED" => {
                let heat = e.get("heat").and_then(|v| v.as_u64()).unwrap_or(0);
                if let Some(arr) = e.get("pair").and_then(|v| v.as_array()) {
                    if arr.len() == 2 {
                        if let (Some(a), Some(b)) = (arr[0].as_str(), arr[1].as_str()) {
                            let key = ordered_pair(a, b);
                            if let Some(&prev) = last_heat.get(&key) {
                                if heat > prev {
                                    let delta = heat - prev;
                                    let update = match &biggest_spike {
                                        None => true,
                                        Some((_, _, _, _, d)) => delta > *d,
                                    };
                                    if update {
                                        biggest_spike = Some((
                                            key.0.clone(),
                                            key.1.clone(),
                                            prev,
                                            heat,
                                            delta,
                                        ));
                                    }
                                }
                            }
                            last_heat.insert(key, heat);
                        }
                    }
                }
            }
            "CLAIM_CREATED" => {
                let cid = ev_str(e, "claimId");
                if let Some(ms) = e.get("ts").and_then(|v| v.as_str()).and_then(parse_ms) {
                    claim_create_ms.insert(cid, ms);
                }
            }
            "CLAIM_RELEASED" => {
                let cid = ev_str(e, "claimId");
                let agent = ev_str(e, "agentId");
                if let (Some(&create_ms), Some(release_ms)) = (
                    claim_create_ms.get(&cid),
                    e.get("ts").and_then(|v| v.as_str()).and_then(parse_ms),
                ) {
                    let dur = release_ms - create_ms;
                    if dur >= 0 && !agent.is_empty() {
                        let entry = claim_durations.entry(agent).or_insert((0, 0));
                        entry.0 += dur;
                        entry.1 += 1;
                    }
                }
            }
            _ => {}
        }
    }

    // -----------------------------------------------------------------------
    // Leaderboards
    // -----------------------------------------------------------------------

    let most_files: Vec<(String, f64)> = agent_files
        .iter()
        .map(|(id, files)| (id.clone(), files.len() as f64))
        .collect();

    let avg_heat: Vec<(String, f64)> = stats
        .agents
        .iter()
        .filter(|(_, t)| t.heat_generated_count > 0)
        .map(|(id, t)| {
            (
                id.clone(),
                t.heat_generated_sum as f64 / t.heat_generated_count as f64,
            )
        })
        .collect();

    let leaderboards = vec![
        make_leaderboard(
            "most_claims",
            "green",
            stats
                .agents
                .iter()
                .map(|(id, t)| (id.clone(), t.claims_made as f64))
                .collect(),
            false,
        ),
        make_leaderboard(
            "most_tasks_completed",
            "green",
            stats
                .agents
                .iter()
                .map(|(id, t)| (id.clone(), t.tasks_completed as f64))
                .collect(),
            false,
        ),
        make_leaderboard("most_files_touched", "gray", most_files, false),
        make_leaderboard(
            "most_heat_generated",
            "red",
            stats
                .agents
                .iter()
                .map(|(id, t)| (id.clone(), t.heat_generated_sum as f64))
                .collect(),
            false,
        ),
        make_leaderboard("lowest_avg_heat", "cyan", avg_heat, true),
        make_leaderboard(
            "most_conflicts_involved",
            "yellow",
            stats
                .agents
                .iter()
                .map(|(id, t)| (id.clone(), t.conflicts_involved as f64))
                .collect(),
            false,
        ),
    ];

    // -----------------------------------------------------------------------
    // Battleground
    // -----------------------------------------------------------------------

    let battleground: Option<(String, u64)> = {
        let mut best: Option<(String, u64)> = None;
        for (file, count) in &file_counts {
            let update = match &best {
                None => true,
                Some((bf, bc)) => count > bc || (count == bc && file < bf),
            };
            if update {
                best = Some((file.clone(), *count));
            }
        }
        best
    };

    // -----------------------------------------------------------------------
    // Badges
    // -----------------------------------------------------------------------

    let mut badges: Vec<Badge> = Vec::new();

    // firestarter
    if let Some(agent) = badge_max_agent(&stats.agents, |t| t.heat_generated_sum) {
        badges.push(Badge {
            id: "firestarter".to_string(),
            label: "Firestarter".to_string(),
            color: "red".to_string(),
            agent,
        });
    }

    // sprinter
    if let Some(agent) = badge_max_agent(&stats.agents, |t| t.tasks_completed) {
        badges.push(Badge {
            id: "sprinter".to_string(),
            label: "Sprinter".to_string(),
            color: "green".to_string(),
            agent,
        });
    }

    // ghost
    if let Some(agent) = badge_max_agent(&stats.agents, |t| t.ghost_claims) {
        badges.push(Badge {
            id: "ghost".to_string(),
            label: "Ghost".to_string(),
            color: "gray".to_string(),
            agent,
        });
    }

    // conflict_magnet
    if let Some(agent) = badge_max_agent(&stats.agents, |t| t.conflicts_involved) {
        badges.push(Badge {
            id: "conflict_magnet".to_string(),
            label: "Conflict Magnet".to_string(),
            color: "yellow".to_string(),
            agent,
        });
    }

    // diplomat
    {
        let max_val = diplomat_count.values().copied().max().unwrap_or(0);
        if max_val > 0 {
            if let Some((agent, _)) = diplomat_count.iter().find(|(_, &v)| v == max_val) {
                badges.push(Badge {
                    id: "diplomat".to_string(),
                    label: "Diplomat".to_string(),
                    color: "blue".to_string(),
                    agent: agent.clone(),
                });
            }
        }
    }

    // hotzone_hero: lowest-id agent who FILE_TOUCHED the battleground file
    if let Some((ref bg_file, _)) = battleground {
        let hero = agent_files
            .iter()
            .find(|(_, files)| files.contains(bg_file))
            .map(|(id, _)| id.clone());
        if let Some(agent) = hero {
            badges.push(Badge {
                id: "hotzone_hero".to_string(),
                label: "Hotzone Hero".to_string(),
                color: "red".to_string(),
                agent,
            });
        }
    }

    // lone_wolf: lowest-id agent with claims_made>0 and conflicts_involved==0
    if let Some(agent) = badge_first(&stats.agents, |t| {
        t.claims_made > 0 && t.conflicts_involved == 0
    }) {
        badges.push(Badge {
            id: "lone_wolf".to_string(),
            label: "Lone Wolf".to_string(),
            color: "gray".to_string(),
            agent,
        });
    }

    // pacifist: lowest-id agent with conflicts_involved>0 and arbitrations_involved==0
    if let Some(agent) = badge_first(&stats.agents, |t| {
        t.conflicts_involved > 0 && t.arbitrations_involved == 0
    }) {
        badges.push(Badge {
            id: "pacifist".to_string(),
            label: "Pacifist".to_string(),
            color: "cyan".to_string(),
            agent,
        });
    }

    // sniper: lowest-id agent with claims_made>0, tasks_completed==claims_made, ghost_claims==0
    if let Some(agent) = badge_first(&stats.agents, |t| {
        t.claims_made > 0 && t.tasks_completed == t.claims_made && t.ghost_claims == 0
    }) {
        badges.push(Badge {
            id: "sniper".to_string(),
            label: "Sniper".to_string(),
            color: "green".to_string(),
            agent,
        });
    }

    // speed_demon: agent with smallest mean (release_ms - create_ms) over matched claims, >= 1 claim
    {
        let best = claim_durations
            .iter()
            .filter(|(_, (_, count))| *count > 0)
            .min_by(|(a1, (sum1, cnt1)), (a2, (sum2, cnt2))| {
                let avg1 = *sum1 as f64 / *cnt1 as f64;
                let avg2 = *sum2 as f64 / *cnt2 as f64;
                avg1.partial_cmp(&avg2)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| a1.cmp(a2))
            });
        if let Some((agent, _)) = best {
            badges.push(Badge {
                id: "speed_demon".to_string(),
                label: "Speed Demon".to_string(),
                color: "green".to_string(),
                agent: agent.clone(),
            });
        }
    }

    // -----------------------------------------------------------------------
    // Superlatives
    // -----------------------------------------------------------------------

    let mut superlatives: Vec<Superlative> = Vec::new();

    // the_battleground
    if let Some((ref file, count)) = battleground {
        superlatives.push(Superlative {
            key: "the_battleground".to_string(),
            label: "The Battleground".to_string(),
            color: "red".to_string(),
            value: serde_json::json!({ "file": file, "count": count }),
        });
    }

    // peak_heat_moment
    if stats.peak_heat.heat > 0 {
        superlatives.push(Superlative {
            key: "peak_heat_moment".to_string(),
            label: "Peak Heat Moment".to_string(),
            color: "red".to_string(),
            value: serde_json::json!({
                "heat": stats.peak_heat.heat,
                "pair": stats.peak_heat.pair,
                "ts": stats.peak_heat.ts,
            }),
        });
    }

    // biggest_spike
    if let Some((p0, p1, from, to, delta)) = biggest_spike {
        superlatives.push(Superlative {
            key: "biggest_spike".to_string(),
            label: "Biggest Spike".to_string(),
            color: "magenta".to_string(),
            value: serde_json::json!({
                "pair": [p0, p1],
                "from": from,
                "to": to,
                "delta": delta,
            }),
        });
    }

    // mexican_standoffs
    superlatives.push(Superlative {
        key: "mexican_standoffs".to_string(),
        label: "Mexican Standoffs".to_string(),
        color: "yellow".to_string(),
        value: serde_json::json!({ "count": stats.deadlocks }),
    });

    // court_cases
    superlatives.push(Superlative {
        key: "court_cases".to_string(),
        label: "Court Cases".to_string(),
        color: "yellow".to_string(),
        value: serde_json::json!({ "count": stats.arbitrations_requested }),
    });

    // longest_negotiation
    if let Some((cid, ms)) = longest_neg {
        superlatives.push(Superlative {
            key: "longest_negotiation".to_string(),
            label: "Longest Negotiation".to_string(),
            color: "magenta".to_string(),
            value: serde_json::json!({ "conflictId": cid, "ms": ms }),
        });
    }

    // bloodiest_minute
    if let Some((&_bucket, &count)) = minute_buckets.iter().max_by_key(|(_, &c)| c) {
        superlatives.push(Superlative {
            key: "bloodiest_minute".to_string(),
            label: "Bloodiest Minute".to_string(),
            color: "magenta".to_string(),
            value: serde_json::json!({ "count": count }),
        });
    }

    // -----------------------------------------------------------------------
    // Streaks — two independent subsequence passes (each over its own class)
    // -----------------------------------------------------------------------

    // Auto-resolve: filter to conflict-outcome events, longest run of CONFLICT_RESOLVED.
    let auto_resolve_max = longest_run(
        events
            .iter()
            .map(ev_type)
            .filter(|t| {
                matches!(
                    *t,
                    "CONFLICT_RESOLVED"
                        | "CONFLICT_ESCALATED"
                        | "CONFLICT_TIMEOUT"
                        | "CONFLICT_ABORTED"
                )
            })
            .map(|t| t == "CONFLICT_RESOLVED"),
    );

    // Completion: filter to CLAIM_RELEASED events, longest run with reason==TASK_COMPLETED.
    let completion_max = longest_run(
        events
            .iter()
            .filter(|e| ev_type(e) == "CLAIM_RELEASED")
            .map(|e| ev_str(e, "reason") == "TASK_COMPLETED"),
    );

    let streaks = Streaks {
        longest_auto_resolve_streak: auto_resolve_max,
        longest_completion_streak: completion_max,
    };

    // -----------------------------------------------------------------------
    // Narrative
    // -----------------------------------------------------------------------

    let resolved_count = stats.auto_resolved_heat_dropped + stats.negotiated_resolved;
    let mut narrative = format!(
        "Session recap: {} agents, {} claims, {} conflicts ({} resolved, {} escalated), {} deadlocks.\n",
        stats.agents_seen,
        stats.claims_created,
        stats.conflicts_opened,
        resolved_count,
        stats.escalated,
        stats.deadlocks,
    );

    if stats.agents_seen == 0 && stats.claims_created == 0 && stats.conflicts_opened == 0 {
        narrative = format!(
            "Quiet session -- no conflicts, no drama.\nSession recap: {} agents, {} claims, {} conflicts ({} resolved, {} escalated), {} deadlocks.\n",
            stats.agents_seen,
            stats.claims_created,
            stats.conflicts_opened,
            resolved_count,
            stats.escalated,
            stats.deadlocks,
        );
    }

    for b in &badges {
        narrative.push_str(&format!("{}: {}.\n", b.label, b.agent));
    }

    if let Some((ref file, _)) = battleground {
        narrative.push_str(&format!("The battleground was {}.\n", file));
    }

    Entertainment {
        leaderboards,
        badges,
        superlatives,
        streaks,
        narrative,
    }
}

// ---------------------------------------------------------------------------
// Tests (verbatim from brief)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::summarize;
    use serde_json::json;

    fn drama() -> Vec<serde_json::Value> {
        vec![
            json!({"type":"AGENT_JOINED","agentId":"agent-1","ts":"2026-06-23T00:00:00Z"}),
            json!({"type":"AGENT_JOINED","agentId":"agent-2","ts":"2026-06-23T00:00:00Z"}),
            json!({"type":"CLAIM_CREATED","claimId":"claim-1","agentId":"agent-1","ts":"2026-06-23T00:00:01Z"}),
            json!({"type":"CLAIM_CREATED","claimId":"claim-2","agentId":"agent-2","ts":"2026-06-23T00:00:01Z"}),
            json!({"type":"FILE_TOUCHED","agentId":"agent-1","files":["src/hot.rs"],"ts":"2026-06-23T00:00:02Z"}),
            json!({"type":"FILE_TOUCHED","agentId":"agent-2","files":["src/hot.rs"],"ts":"2026-06-23T00:00:02Z"}),
            json!({"type":"HEAT_UPDATED","pair":["agent-1","agent-2"],"heat":20,"band":"SAFE","ts":"2026-06-23T00:00:03Z"}),
            json!({"type":"HEAT_UPDATED","pair":["agent-1","agent-2"],"heat":82,"band":"CONFLICT_CANDIDATE","ts":"2026-06-23T00:00:04Z"}),
            json!({"type":"CONFLICT_OPENED","conflictId":"conflict-1","agents":["agent-1","agent-2"],"paths":["src/hot.rs"],"ts":"2026-06-23T00:00:04Z"}),
            json!({"type":"CONFLICT_PROPOSAL_RECEIVED","conflictId":"conflict-1","from":"agent-1","kind":"YIELD_CLAIM","ts":"2026-06-23T00:00:05Z"}),
            json!({"type":"CONFLICT_RESOLVED","conflictId":"conflict-1","resolution":"PARTICIPANT_STEPPED_ASIDE","ts":"2026-06-23T00:00:06Z"}),
            json!({"type":"CLAIM_RELEASED","claimId":"claim-1","agentId":"agent-1","reason":"TASK_COMPLETED","ts":"2026-06-23T00:00:07Z"}),
        ]
    }

    const PALETTE: &[&str] = &["red", "cyan", "blue", "yellow", "green", "gray", "magenta"];

    #[test]
    fn badges_awarded_to_right_agents_no_emoji() {
        let evs = drama();
        let stats = summarize(&evs);
        let e = build_entertainment(&evs, &stats);
        // firestarter = whoever generated most heat; both share the pair so equal -> tie to agent-1
        let fire = e.badges.iter().find(|b| b.id == "firestarter").unwrap();
        assert_eq!(fire.agent, "agent-1");
        assert_eq!(fire.color, "red");
        // diplomat = agent-1 (only YIELD proposer)
        assert_eq!(
            e.badges.iter().find(|b| b.id == "diplomat").unwrap().agent,
            "agent-1"
        );
        // sprinter = agent-1 (only completer)
        assert_eq!(
            e.badges.iter().find(|b| b.id == "sprinter").unwrap().agent,
            "agent-1"
        );
        // every badge color is in the palette; every label is plain ASCII (no emoji)
        for b in &e.badges {
            assert!(
                PALETTE.contains(&b.color.as_str()),
                "bad color {}",
                b.color
            );
            assert!(
                b.label.chars().all(|c| (c as u32) < 0x7F),
                "non-ASCII label {}",
                b.label
            );
        }
    }

    #[test]
    fn battleground_and_peak_superlatives() {
        let evs = drama();
        let stats = summarize(&evs);
        let e = build_entertainment(&evs, &stats);
        let bg = e
            .superlatives
            .iter()
            .find(|s| s.key == "the_battleground")
            .unwrap();
        assert_eq!(bg.value["file"], "src/hot.rs");
        let peak = e
            .superlatives
            .iter()
            .find(|s| s.key == "peak_heat_moment")
            .unwrap();
        assert_eq!(peak.value["heat"], 82);
        let spike = e
            .superlatives
            .iter()
            .find(|s| s.key == "biggest_spike")
            .unwrap();
        assert_eq!(spike.value["delta"], 62); // 82 - 20 on the same pair
    }

    #[test]
    fn leaderboards_sorted_and_tie_broken() {
        let evs = drama();
        let stats = summarize(&evs);
        let e = build_entertainment(&evs, &stats);
        let lb = e
            .leaderboards
            .iter()
            .find(|l| l.metric == "most_tasks_completed")
            .unwrap();
        assert_eq!(lb.entries[0].agent, "agent-1");
    }

    #[test]
    fn streaks_counted() {
        let evs = drama();
        let stats = summarize(&evs);
        let e = build_entertainment(&evs, &stats);
        assert_eq!(e.streaks.longest_auto_resolve_streak, 1);
        assert_eq!(e.streaks.longest_completion_streak, 1);
    }

    #[test]
    fn streaks_ignore_intervening_events_of_other_classes() {
        use serde_json::json;
        let evs = vec![
            json!({"type":"CLAIM_RELEASED","reason":"TASK_COMPLETED","ts":"2026-06-23T00:00:00Z"}),
            json!({"type":"HEAT_UPDATED","pair":["a","b"],"heat":10,"band":"SAFE","ts":"2026-06-23T00:00:01Z"}),
            json!({"type":"CLAIM_RELEASED","reason":"TASK_COMPLETED","ts":"2026-06-23T00:00:02Z"}),
            json!({"type":"CLAIM_RELEASED","reason":"SESSION_END","ts":"2026-06-23T00:00:03Z"}),
            json!({"type":"CLAIM_RELEASED","reason":"TASK_COMPLETED","ts":"2026-06-23T00:00:04Z"}),
        ];
        let stats = crate::stats::summarize(&evs);
        let e = build_entertainment(&evs, &stats);
        // intervening HEAT_UPDATED is ignored -> first two completed are consecutive -> streak 2; then SESSION_END breaks it
        assert_eq!(e.streaks.longest_completion_streak, 2);
    }

    #[test]
    fn narrative_plain_text_no_emoji() {
        let evs = drama();
        let stats = summarize(&evs);
        let e = build_entertainment(&evs, &stats);
        assert!(!e.narrative.is_empty());
        assert!(
            e.narrative.chars().all(|c| (c as u32) < 0x7F),
            "narrative has non-ASCII/emoji"
        );
        assert!(
            e.narrative.contains("recap") || e.narrative.contains("Session"),
        );
    }

    #[test]
    fn quiet_session_graceful() {
        let stats = summarize(&[]);
        let e = build_entertainment(&[], &stats);
        assert!(!e.narrative.is_empty());
        assert!(e.badges.is_empty());
        assert_eq!(e.streaks.longest_auto_resolve_streak, 0);
    }

    #[test]
    fn multi_spike_and_multi_conflict_hits_some_arms() {
        // 3 HEAT_UPDATED on the same pair → two spikes; second spike (50) beats first (20)
        // → exercises the Some(d) => delta > *d arm of biggest_spike
        // 2 CONFLICT_OPENED/RESOLVED → second conflict spans longer (3000ms vs 1000ms)
        // → exercises the Some((_, max_ms)) => span > *max_ms arm of longest_neg
        let evs = vec![
            json!({"type":"HEAT_UPDATED","pair":["a","b"],"heat":10,"band":"SAFE","ts":"2026-06-23T00:00:00Z"}),
            json!({"type":"HEAT_UPDATED","pair":["a","b"],"heat":30,"band":"MONITOR","ts":"2026-06-23T00:00:01Z"}),   // spike delta=20
            json!({"type":"HEAT_UPDATED","pair":["a","b"],"heat":80,"band":"CONFLICT_CANDIDATE","ts":"2026-06-23T00:00:02Z"}), // spike delta=50 → new max
            json!({"type":"CONFLICT_OPENED","conflictId":"c-1","agents":["a","b"],"paths":[],"ts":"2026-06-23T00:00:03Z"}),
            json!({"type":"CONFLICT_RESOLVED","conflictId":"c-1","resolution":"PARTICIPANT_STEPPED_ASIDE","ts":"2026-06-23T00:00:04Z"}), // span=1000ms
            json!({"type":"CONFLICT_OPENED","conflictId":"c-2","agents":["a","b"],"paths":[],"ts":"2026-06-23T00:00:05Z"}),
            json!({"type":"CONFLICT_RESOLVED","conflictId":"c-2","resolution":"PARTICIPANT_STEPPED_ASIDE","ts":"2026-06-23T00:00:08Z"}), // span=3000ms → new max
        ];
        let stats = crate::stats::summarize(&evs);
        let e = build_entertainment(&evs, &stats);
        let spike = e.superlatives.iter().find(|s| s.key == "biggest_spike").unwrap();
        assert_eq!(spike.value["delta"], 50);
        let neg = e.superlatives.iter().find(|s| s.key == "longest_negotiation").unwrap();
        assert_eq!(neg.value["ms"], 3000);
    }
}
