use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AgentState {
    Discovery,
    Idle,
    Active,
    SubagentWaiting,
    Testing,
    Blocked,
    Negotiating,
    WaitingUser,
    Offline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Intent {
    Security,
    Qa,
    Testing,
    Performance,
    Refactor,
    Documentation,
    Feature,
    Bugfix,
    Architecture,
    Devops,
    Research,
    Migration,
    Configuration,
    Observability,
}

impl Intent {
    pub fn as_str(&self) -> &'static str {
        match self {
            Intent::Security => "SECURITY",
            Intent::Qa => "QA",
            Intent::Testing => "TESTING",
            Intent::Performance => "PERFORMANCE",
            Intent::Refactor => "REFACTOR",
            Intent::Documentation => "DOCUMENTATION",
            Intent::Feature => "FEATURE",
            Intent::Bugfix => "BUGFIX",
            Intent::Architecture => "ARCHITECTURE",
            Intent::Devops => "DEVOPS",
            Intent::Research => "RESEARCH",
            Intent::Migration => "MIGRATION",
            Intent::Configuration => "CONFIGURATION",
            Intent::Observability => "OBSERVABILITY",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProposalKind {
    CoOwn,
    SplitScope,
    YieldClaim,
    TransferTask,
    QueueTask,
    AskUser,
    AbortTask,
}

impl ProposalKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProposalKind::CoOwn => "CO_OWN",
            ProposalKind::SplitScope => "SPLIT_SCOPE",
            ProposalKind::YieldClaim => "YIELD_CLAIM",
            ProposalKind::TransferTask => "TRANSFER_TASK",
            ProposalKind::QueueTask => "QUEUE_TASK",
            ProposalKind::AskUser => "ASK_USER",
            ProposalKind::AbortTask => "ABORT_TASK",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimChange {
    pub agent_id: String,
    #[serde(default)]
    pub keep: Option<Vec<String>>,
    #[serde(default)]
    pub take: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Proposal {
    pub kind: ProposalKind,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub claim_changes: Vec<ClaimChange>,
    #[serde(default)]
    pub requires_user_approval: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ClaimStatus {
    Proposed,
    Provisional,
    Active,
    Released,
    Orphaned,
    Reclaimable,
    Rejected,
}

impl ClaimStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ClaimStatus::Proposed => "PROPOSED",
            ClaimStatus::Provisional => "PROVISIONAL",
            ClaimStatus::Active => "ACTIVE",
            ClaimStatus::Released => "RELEASED",
            ClaimStatus::Orphaned => "ORPHANED",
            ClaimStatus::Reclaimable => "RECLAIMABLE",
            ClaimStatus::Rejected => "REJECTED",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReleaseReason {
    TaskCompleted,
    TaskAborted,
    UserChangedTask,
    ClearInvoked,
    HandoffTransfer,
    ManualRelease,
    SessionEnd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapErrorCode {
    SchemaValidationFailed,
    InvalidStateTransition,
    AuthFailed,
    ClaimConflict,
    AgentNotFound,
    ClaimNotFound,
    ConflictNotFound,
    CoreDegraded,
    Timeout,
    UnsupportedCapVersion,
}

impl CapErrorCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            CapErrorCode::SchemaValidationFailed => "SCHEMA_VALIDATION_FAILED",
            CapErrorCode::InvalidStateTransition => "INVALID_STATE_TRANSITION",
            CapErrorCode::AuthFailed => "AUTH_FAILED",
            CapErrorCode::ClaimConflict => "CLAIM_CONFLICT",
            CapErrorCode::AgentNotFound => "AGENT_NOT_FOUND",
            CapErrorCode::ClaimNotFound => "CLAIM_NOT_FOUND",
            CapErrorCode::ConflictNotFound => "CONFLICT_NOT_FOUND",
            CapErrorCode::CoreDegraded => "CORE_DEGRADED",
            CapErrorCode::Timeout => "TIMEOUT",
            CapErrorCode::UnsupportedCapVersion => "UNSUPPORTED_CAP_VERSION",
        }
    }
}

/// CAP events Phase 2 ingests. `type` selects the variant; fields are camelCase.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CapEvent {
    #[serde(rename_all = "camelCase")]
    ClaimProposed {
        agent_id: String,
        #[serde(default)]
        task: Value,
        intent: Intent,
        #[serde(default)]
        domains: Vec<String>,
        #[serde(default)]
        estimated_files: Vec<String>,
        confidence: f64,
    },
    #[serde(rename_all = "camelCase")]
    ClaimReleased {
        claim_id: String,
        agent_id: String,
        reason: ReleaseReason,
    },
    #[serde(rename_all = "camelCase")]
    AgentStateChanged {
        agent_id: String,
        state: AgentState,
    },
    #[serde(rename_all = "camelCase")]
    ClearInvoked {
        agent_id: String,
    },
    #[serde(rename_all = "camelCase")]
    ConflictProposalSubmitted {
        conflict_id: String,
        from: String,
        proposal: Proposal,
    },
    #[serde(rename_all = "camelCase")]
    ConflictUserDecision {
        conflict_id: String,
        choice: String,
    },
}

/// Deserialize-is-validation: any shape/value error → SCHEMA_VALIDATION_FAILED.
pub fn decode_event(event: &Value) -> Result<CapEvent, CapErrorCode> {
    serde_json::from_value(event.clone()).map_err(|_| CapErrorCode::SchemaValidationFailed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn canonical_enum_strings_match_cap_spec() {
        assert_eq!(serde_json::to_value(AgentState::SubagentWaiting).unwrap(), json!("SUBAGENT_WAITING"));
        assert_eq!(serde_json::to_value(AgentState::WaitingUser).unwrap(), json!("WAITING_USER"));
        assert_eq!(serde_json::to_value(AgentState::Discovery).unwrap(), json!("DISCOVERY"));
        assert_eq!(serde_json::to_value(Intent::Qa).unwrap(), json!("QA"));
        assert_eq!(serde_json::to_value(Intent::Devops).unwrap(), json!("DEVOPS"));
        assert_eq!(serde_json::to_value(Intent::Bugfix).unwrap(), json!("BUGFIX"));
        assert_eq!(Intent::Qa.as_str(), "QA");
        assert_eq!(ClaimStatus::Reclaimable.as_str(), "RECLAIMABLE");
        assert_eq!(serde_json::to_value(ReleaseReason::ClearInvoked).unwrap(), json!("CLEAR_INVOKED"));
        assert_eq!(CapErrorCode::SchemaValidationFailed.as_str(), "SCHEMA_VALIDATION_FAILED");
        assert_eq!(CapErrorCode::UnsupportedCapVersion.as_str(), "UNSUPPORTED_CAP_VERSION");
    }

    #[test]
    fn decodes_claim_proposed_with_camel_case_fields() {
        let ev = json!({
            "type": "CLAIM_PROPOSED",
            "agentId": "agent-1",
            "intent": "BUGFIX",
            "domains": ["AUTHENTICATION"],
            "estimatedFiles": ["src/auth/session.ts"],
            "confidence": 0.86
        });
        match decode_event(&ev).unwrap() {
            CapEvent::ClaimProposed { agent_id, intent, domains, estimated_files, confidence, .. } => {
                assert_eq!(agent_id, "agent-1");
                assert_eq!(intent, Intent::Bugfix);
                assert_eq!(domains, vec!["AUTHENTICATION"]);
                assert_eq!(estimated_files, vec!["src/auth/session.ts"]);
                assert!((confidence - 0.86).abs() < 1e-9);
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn decodes_other_phase2_variants() {
        let rel = json!({"type":"CLAIM_RELEASED","claimId":"claim-1","agentId":"agent-1","reason":"TASK_COMPLETED"});
        assert!(matches!(decode_event(&rel).unwrap(), CapEvent::ClaimReleased { .. }));
        let st = json!({"type":"AGENT_STATE_CHANGED","agentId":"agent-1","state":"TESTING"});
        assert!(matches!(decode_event(&st).unwrap(), CapEvent::AgentStateChanged { state: AgentState::Testing, .. }));
        let clr = json!({"type":"CLEAR_INVOKED","agentId":"agent-1"});
        assert!(matches!(decode_event(&clr).unwrap(), CapEvent::ClearInvoked { .. }));
    }

    #[test]
    fn rejects_bad_intent_and_unknown_type_and_missing_field() {
        let bad_intent = json!({"type":"CLAIM_PROPOSED","agentId":"a","intent":"NOPE","confidence":0.9});
        assert_eq!(decode_event(&bad_intent).unwrap_err(), CapErrorCode::SchemaValidationFailed);
        let unknown = json!({"type":"TOOL_PRECHECK","agentId":"a"});
        assert_eq!(decode_event(&unknown).unwrap_err(), CapErrorCode::SchemaValidationFailed);
        let missing_conf = json!({"type":"CLAIM_PROPOSED","agentId":"a","intent":"BUGFIX"});
        assert_eq!(decode_event(&missing_conf).unwrap_err(), CapErrorCode::SchemaValidationFailed);
    }

    #[test]
    fn intent_as_str_all_variants() {
        use Intent::*;
        let cases = [
            (Security, "SECURITY"), (Qa, "QA"), (Testing, "TESTING"),
            (Performance, "PERFORMANCE"), (Refactor, "REFACTOR"),
            (Documentation, "DOCUMENTATION"), (Feature, "FEATURE"), (Bugfix, "BUGFIX"),
            (Architecture, "ARCHITECTURE"), (Devops, "DEVOPS"), (Research, "RESEARCH"),
            (Migration, "MIGRATION"), (Configuration, "CONFIGURATION"),
            (Observability, "OBSERVABILITY"),
        ];
        for (v, s) in cases {
            assert_eq!(v.as_str(), s);
            assert_eq!(serde_json::to_value(v).unwrap(), serde_json::json!(s));
        }
    }

    #[test]
    fn claim_status_as_str_all_variants() {
        use ClaimStatus::*;
        let cases = [
            (Proposed, "PROPOSED"), (Provisional, "PROVISIONAL"), (Active, "ACTIVE"),
            (Released, "RELEASED"), (Orphaned, "ORPHANED"), (Reclaimable, "RECLAIMABLE"),
            (Rejected, "REJECTED"),
        ];
        for (v, s) in cases {
            assert_eq!(v.as_str(), s);
            assert_eq!(serde_json::to_value(v).unwrap(), serde_json::json!(s));
        }
    }

    #[test]
    fn cap_error_code_as_str_all_variants() {
        use CapErrorCode::*;
        let cases = [
            (SchemaValidationFailed, "SCHEMA_VALIDATION_FAILED"),
            (InvalidStateTransition, "INVALID_STATE_TRANSITION"),
            (AuthFailed, "AUTH_FAILED"), (ClaimConflict, "CLAIM_CONFLICT"),
            (AgentNotFound, "AGENT_NOT_FOUND"), (ClaimNotFound, "CLAIM_NOT_FOUND"),
            (CoreDegraded, "CORE_DEGRADED"), (Timeout, "TIMEOUT"),
            (UnsupportedCapVersion, "UNSUPPORTED_CAP_VERSION"),
        ];
        for (v, s) in cases {
            assert_eq!(v.as_str(), s);
        }
    }

    #[test]
    fn decodes_conflict_proposal_all_kinds() {
        use ProposalKind::*;
        let cases = [
            ("CO_OWN", CoOwn), ("SPLIT_SCOPE", SplitScope), ("YIELD_CLAIM", YieldClaim),
            ("TRANSFER_TASK", TransferTask), ("QUEUE_TASK", QueueTask),
            ("ASK_USER", AskUser), ("ABORT_TASK", AbortTask),
        ];
        for (s, k) in cases {
            let ev = json!({
                "type": "CONFLICT_PROPOSAL_SUBMITTED",
                "conflictId": "conflict-1",
                "from": "agent-1",
                "proposal": {
                    "kind": s,
                    "summary": "do the thing",
                    "claimChanges": [{"agentId":"agent-1","keep":["src/a.rs"]}],
                    "requiresUserApproval": false
                }
            });
            match decode_event(&ev).unwrap() {
                CapEvent::ConflictProposalSubmitted { conflict_id, from, proposal } => {
                    assert_eq!(conflict_id, "conflict-1");
                    assert_eq!(from, "agent-1");
                    assert_eq!(proposal.kind, k);
                    assert_eq!(proposal.summary, "do the thing");
                    assert_eq!(proposal.claim_changes[0].agent_id, "agent-1");
                    assert_eq!(proposal.claim_changes[0].keep.as_ref().unwrap(), &vec!["src/a.rs".to_string()]);
                    assert!(!proposal.requires_user_approval);
                }
                other => panic!("wrong variant: {other:?}"),
            }
        }
    }

    #[test]
    fn proposal_defaults_optional_fields() {
        let ev = json!({
            "type":"CONFLICT_PROPOSAL_SUBMITTED","conflictId":"c1","from":"a1",
            "proposal":{"kind":"CO_OWN"}
        });
        match decode_event(&ev).unwrap() {
            CapEvent::ConflictProposalSubmitted { proposal, .. } => {
                assert_eq!(proposal.summary, "");
                assert!(proposal.claim_changes.is_empty());
                assert!(!proposal.requires_user_approval);
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn decodes_conflict_user_decision() {
        let ev = json!({"type":"CONFLICT_USER_DECISION","conflictId":"c1","choice":"option-2"});
        match decode_event(&ev).unwrap() {
            CapEvent::ConflictUserDecision { conflict_id, choice } => {
                assert_eq!(conflict_id, "c1");
                assert_eq!(choice, "option-2");
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn rejects_bad_proposal_kind() {
        let ev = json!({"type":"CONFLICT_PROPOSAL_SUBMITTED","conflictId":"c1","from":"a1","proposal":{"kind":"NOPE"}});
        assert_eq!(decode_event(&ev).unwrap_err(), CapErrorCode::SchemaValidationFailed);
    }

    #[test]
    fn proposal_kind_as_str_and_conflict_not_found() {
        assert_eq!(ProposalKind::SplitScope.as_str(), "SPLIT_SCOPE");
        assert_eq!(ProposalKind::QueueTask.as_str(), "QUEUE_TASK");
        assert_eq!(serde_json::to_value(ProposalKind::CoOwn).unwrap(), json!("CO_OWN"));
        assert_eq!(CapErrorCode::ConflictNotFound.as_str(), "CONFLICT_NOT_FOUND");
    }
}
