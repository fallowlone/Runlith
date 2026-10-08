//! TAKEOVER record, authored-field, anchor and packet types.

use std::fmt;

use context_engine::model::Sensitivity;
use continuity::model::NoteBody;
use plan_domain::{HumanDecisionRequestId, PlanBranchId};
use task_domain::TaskId;

/// Maximum bytes of one human-authored takeover field (actions,
/// uncertainty, question). The TASK-0169 amendment bounds each at 4 KiB.
pub const TAKEOVER_AUTHORED_MAX_BYTES: usize = 4096;

/// Default maximum takeover lifetime: policy is silent on takeover
/// durations, so the TASK-0169 amendment sets 24h.
pub const TAKEOVER_MAX_TTL_MS: u64 = 24 * 60 * 60 * 1000;

/// Takeover domain refusal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TakeoverError {
    InvalidInput(String),
}

impl fmt::Display for TakeoverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(detail) => write!(f, "invalid takeover input: {detail}"),
        }
    }
}

impl std::error::Error for TakeoverError {}

/// Opaque takeover-admission record identity (store-minted `takeover-{seq}`).
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TakeoverRecordId(String);

impl TakeoverRecordId {
    pub fn new(value: impl Into<String>) -> Result<Self, TakeoverError> {
        let value = value.into();
        if value.trim().is_empty() || value.chars().any(char::is_control) {
            return Err(TakeoverError::InvalidInput(
                "takeover record id must be non-blank with no control characters".to_owned(),
            ));
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for TakeoverRecordId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Takeover record lifecycle. ACTIVE is the durable marker distinguishing
/// takeover-Blocked from other Blocked causes — the return gate matches on
/// it, never on journal text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TakeoverStatus {
    Active,
    Completed,
    Expired,
}

impl TakeoverStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "ACTIVE",
            Self::Completed => "COMPLETED",
            Self::Expired => "EXPIRED",
        }
    }

    pub fn parse(value: &str) -> Result<Self, TakeoverError> {
        match value {
            "ACTIVE" => Ok(Self::Active),
            "COMPLETED" => Ok(Self::Completed),
            "EXPIRED" => Ok(Self::Expired),
            _ => Err(TakeoverError::InvalidInput(format!(
                "unknown takeover status {value:?}"
            ))),
        }
    }
}

/// One human-authored takeover field. Bodies travel the ADR-0043 note
/// rules by construction: the body is a continuity note body and
/// SECRET values are reference-only.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthoredField {
    pub body: NoteBody,
    pub sensitivity: Sensitivity,
}

impl AuthoredField {
    pub fn new(body: NoteBody, sensitivity: Sensitivity) -> Result<Self, TakeoverError> {
        if sensitivity == Sensitivity::Secret && matches!(body, NoteBody::Inline(_)) {
            return Err(TakeoverError::InvalidInput(
                "SECRET takeover fields must be references, never inline values".to_owned(),
            ));
        }
        Ok(Self { body, sensitivity })
    }

    /// Total storability check: the byte bound plus the ADR-0043
    /// SECRET-reference rule. Constructor-equivalent, so persistence
    /// boundaries that receive struct-literal values stay fenced.
    pub fn validate(&self) -> Result<(), TakeoverError> {
        if self.sensitivity == Sensitivity::Secret && matches!(self.body, NoteBody::Inline(_)) {
            return Err(TakeoverError::InvalidInput(
                "SECRET takeover fields must be references, never inline values".to_owned(),
            ));
        }
        let text = match &self.body {
            NoteBody::Inline(text) | NoteBody::Ref(text) => text,
        };
        if text.len() > TAKEOVER_AUTHORED_MAX_BYTES {
            return Err(TakeoverError::InvalidInput(format!(
                "takeover authored field exceeds {TAKEOVER_AUTHORED_MAX_BYTES} bytes"
            )));
        }
        Ok(())
    }
}

/// Durable takeover evidence: the basis plan revision (mandatory), the
/// HDR/execution/task refs, and a mandatory expiry. Not a second HDR —
/// the HDR stays the decision record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TakeoverAdmission {
    pub id: TakeoverRecordId,
    pub hdr_id: HumanDecisionRequestId,
    pub task_id: TaskId,
    pub plan_branch_id: PlanBranchId,
    pub basis_plan_revision: u64,
    pub expires_at_ms: u64,
    pub status: TakeoverStatus,
    pub actions: Option<AuthoredField>,
    pub uncertainty: Option<AuthoredField>,
    pub question: Option<AuthoredField>,
    pub resulting_revision: Option<u64>,
    pub created_at_ms: u64,
    pub resolved_at_ms: Option<u64>,
}

/// Durable workspace-anchor snapshot for one execution of the takeover
/// scope, captured at admission. Return re-reads the live bindings and
/// refuses with `GitBackendFailure` on any drift. Compared as stored
/// text so a corrupt binding still compares instead of failing to decode.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TakeoverLeaseAnchor {
    pub execution_id: String,
    pub lease_id: String,
    pub base_revision: String,
    pub worktree_ref: String,
    pub lifecycle: String,
    pub anchor: Option<String>,
}

/// Compares captured admission anchors against live bindings. The
/// execution set must match exactly — a vanished execution, a new
/// execution, or any moved binding is drift.
pub fn compare_anchors(
    captured: &[TakeoverLeaseAnchor],
    current: &[TakeoverLeaseAnchor],
) -> Result<(), TakeoverError> {
    let mut missing: Vec<&str> = Vec::new();
    for want in captured {
        match current
            .iter()
            .find(|have| have.execution_id == want.execution_id)
        {
            None => missing.push(want.execution_id.as_str()),
            Some(have) => {
                if have != want {
                    return Err(TakeoverError::InvalidInput(format!(
                        "takeover workspace anchor moved for execution {}",
                        want.execution_id
                    )));
                }
            }
        }
    }
    if !missing.is_empty() {
        return Err(TakeoverError::InvalidInput(format!(
            "takeover workspace anchor lost for execution(s): {}",
            missing.join(",")
        )));
    }
    for have in current {
        if !captured
            .iter()
            .any(|want| want.execution_id == have.execution_id)
        {
            return Err(TakeoverError::InvalidInput(format!(
                "takeover scope gained execution {} after admission",
                have.execution_id
            )));
        }
    }
    Ok(())
}

/// Validates a mandatory takeover expiry: it must be a live deadline
/// within the policy maximum (24h default — policy is silent).
pub fn validate_takeover_expiry(now_ms: u64, expires_at_ms: u64) -> Result<(), TakeoverError> {
    if expires_at_ms <= now_ms {
        return Err(TakeoverError::InvalidInput(
            "takeover expiry must be in the future".to_owned(),
        ));
    }
    if expires_at_ms.saturating_sub(now_ms) > TAKEOVER_MAX_TTL_MS {
        return Err(TakeoverError::InvalidInput(format!(
            "takeover expiry exceeds the maximum lifetime of {TAKEOVER_MAX_TTL_MS} ms"
        )));
    }
    Ok(())
}

/// Canonical state refs for the handoff rendering — refs, never copied truth.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TakeoverStateRefs {
    pub task_id: String,
    pub task_event_seq: u64,
    pub plan_branch_id: String,
    pub plan_head: u64,
    /// First captured anchor lease ordered by execution id (`None`
    /// when no execution holds a lease). Deterministic orientation,
    /// not authority: fencing compares the full anchor set.
    pub lease_id: Option<String>,
}

/// Human-facing takeover rendering over the HDR plus the takeover record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HandoffPacket {
    pub goal: String,
    pub state: TakeoverStateRefs,
    pub actions: Option<AuthoredField>,
    pub blockers: Vec<String>,
    pub uncertainty: Option<AuthoredField>,
    pub question: Option<AuthoredField>,
}

#[cfg(test)]
mod tests {
    use context_engine::model::Sensitivity;
    use continuity::model::NoteBody;
    use plan_domain::HumanContinuation;

    use super::*;

    #[test]
    fn takeover_continuation_roundtrips() {
        assert_eq!(HumanContinuation::Takeover.as_str(), "TAKEOVER");
        assert_eq!(
            HumanContinuation::parse("TAKEOVER").unwrap(),
            HumanContinuation::Takeover
        );
        assert_eq!(
            HumanContinuation::parse("RESUME").unwrap(),
            HumanContinuation::Resume
        );
    }

    #[test]
    fn takeover_status_roundtrips() {
        assert_eq!(TakeoverStatus::Active.as_str(), "ACTIVE");
        assert_eq!(
            TakeoverStatus::parse("COMPLETED").unwrap(),
            TakeoverStatus::Completed
        );
        assert_eq!(
            TakeoverStatus::parse("EXPIRED").unwrap(),
            TakeoverStatus::Expired
        );
        assert!(TakeoverStatus::parse("STALE").is_err());
    }

    #[test]
    fn takeover_record_id_rejects_blank() {
        assert!(TakeoverRecordId::new("takeover-1").is_ok());
        assert!(TakeoverRecordId::new("  ").is_err());
    }

    #[test]
    fn authored_field_bounds_and_note_rules() {
        let inline = |text: String| {
            AuthoredField::new(NoteBody::Inline(text), Sensitivity::Project).unwrap()
        };
        assert!(inline("do the thing".to_owned()).validate().is_ok());
        assert!(inline("x".repeat(4096)).validate().is_ok());
        assert!(inline("x".repeat(4097)).validate().is_err());
        let secret_inline = AuthoredField::new(
            NoteBody::Inline("token: hunter2".to_owned()),
            Sensitivity::Secret,
        );
        assert!(secret_inline.is_err());
        let secret_ref = AuthoredField::new(
            NoteBody::Ref("vault:token/7".to_owned()),
            Sensitivity::Secret,
        )
        .unwrap();
        assert!(secret_ref.validate().is_ok());
        // Struct-literal construction bypasses `new`: `validate` is the
        // total storability check and must refuse Secret+Inline too.
        let smuggled = AuthoredField {
            body: NoteBody::Inline("token: hunter2".to_owned()),
            sensitivity: Sensitivity::Secret,
        };
        assert!(smuggled.validate().is_err());
    }

    #[test]
    fn takeover_expiry_is_bounded() {
        assert!(validate_takeover_expiry(1_000, 1_001).is_ok());
        assert!(validate_takeover_expiry(1_000, 1_000 + TAKEOVER_MAX_TTL_MS).is_ok());
        assert!(validate_takeover_expiry(1_000, 1_000).is_err());
        assert!(validate_takeover_expiry(1_000, 1_000 + TAKEOVER_MAX_TTL_MS + 1).is_err());
    }

    fn anchor(lease: &str, base: &str) -> TakeoverLeaseAnchor {
        TakeoverLeaseAnchor {
            execution_id: format!("E-{lease}"),
            lease_id: lease.to_owned(),
            base_revision: base.to_owned(),
            worktree_ref: "/tmp/wt-1".to_owned(),
            lifecycle: "Ready".to_owned(),
            anchor: Some("/repo/.git".to_owned()),
        }
    }

    #[test]
    fn anchor_comparison_catches_drift() {
        let captured = vec![anchor("L-1", "abc")];
        assert!(compare_anchors(&captured, &captured).is_ok());
        let moved = vec![anchor("L-1", "def")];
        assert!(compare_anchors(&captured, &moved).is_err());
        let gone: Vec<TakeoverLeaseAnchor> = Vec::new();
        assert!(compare_anchors(&captured, &gone).is_err());
        let mut extra = captured.clone();
        extra.push(anchor("L-2", "abc"));
        assert!(compare_anchors(&captured, &extra).is_err());
    }

    #[test]
    fn handoff_packet_carries_all_fields() {
        let packet = HandoffPacket {
            goal: "ship it".to_owned(),
            state: TakeoverStateRefs {
                task_id: "T-1".to_owned(),
                task_event_seq: 9,
                plan_branch_id: "B-1".to_owned(),
                plan_head: 4,
                lease_id: Some("L-1".to_owned()),
            },
            actions: None,
            blockers: vec!["T-2".to_owned(), "PLANNER_BLOCKED".to_owned()],
            uncertainty: None,
            question: None,
        };
        assert_eq!(packet.blockers.len(), 2);
    }
}
