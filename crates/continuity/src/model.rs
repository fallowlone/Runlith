//! Continuity domain model (TASK-0171, ADR-0043).

use std::fmt;

use agent_runtime::{AttemptId, ExecutionId, ProviderRef};
use context_engine::model::{ContextPlanId, Sensitivity, SourceRef};
use task_domain::TaskId;

/// Inline authored-text bound (ADR-0043 §6: at most 4 KiB; counted in
/// chars to match SQL length()).
pub const MAX_NOTE_CHARS: usize = 4096;

/// Free-text bound for continuation and opaque refs.
pub const MAX_TEXT_CHARS: usize = 4096;

fn bounded_text(field: &str, raw: &str) -> Result<(), ContinuityError> {
    if raw.trim().is_empty() {
        return Err(ContinuityError::InvalidInput(format!(
            "{field} must not be blank"
        )));
    }
    if raw.chars().count() > MAX_TEXT_CHARS {
        return Err(ContinuityError::InvalidInput(format!(
            "{field} exceeds {MAX_TEXT_CHARS} chars"
        )));
    }
    Ok(())
}

/// Domain errors: input taxonomy, never backend detail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContinuityError {
    /// Blank, over-long, or otherwise malformed input value.
    InvalidInput(String),
    /// Checkpoint format major outside local support (CR-10).
    UnknownVersion(String),
    /// Referenced checkpoint is unknown or not committed.
    MissingCheckpoint(String),
    /// Activation would fork a chain or collide (CR-3).
    ActivationConflict(String),
    /// Rollover needs a committed checkpoint first (CR-1).
    NotCommitted(String),
    /// Past the hard budget: cycles block until rollover (ADR-0043 §4).
    BudgetBlocked(String),
    /// Storage backend failure with bounded detail.
    BackendFailure { detail: String },
}

impl fmt::Display for ContinuityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(detail) => write!(f, "invalid input: {detail}"),
            Self::UnknownVersion(detail) => write!(f, "unknown version: {detail}"),
            Self::MissingCheckpoint(detail) => write!(f, "missing checkpoint: {detail}"),
            Self::ActivationConflict(detail) => write!(f, "activation conflict: {detail}"),
            Self::NotCommitted(detail) => write!(f, "not committed: {detail}"),
            Self::BudgetBlocked(detail) => write!(f, "budget blocked: {detail}"),
            Self::BackendFailure { detail } => write!(f, "backend failure: {detail}"),
        }
    }
}

impl std::error::Error for ContinuityError {}

const CROCKFORD: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

fn is_crockford26(s: &str) -> bool {
    s.len() == 26 && s.bytes().all(|b| CROCKFORD.contains(&b))
}

macro_rules! prefixed_id {
    ($name:ident, $prefix:literal, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(String);

        impl $name {
            /// Validates prefix + 26-char Crockford suffix (fail-closed,
            /// no trimming: stored ids are byte-exact).
            pub fn new(raw: impl Into<String>) -> Result<Self, ContinuityError> {
                let raw = raw.into();
                let suffix = raw.strip_prefix($prefix).ok_or_else(|| {
                    ContinuityError::InvalidInput(format!("id must start with {}", $prefix))
                })?;
                if !is_crockford26(suffix) {
                    return Err(ContinuityError::InvalidInput(
                        "id suffix must be 26 Crockford chars".to_owned(),
                    ));
                }
                Ok(Self(raw))
            }

            /// Borrows the validated identifier text.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

prefixed_id!(EpochId, "ctxepoch_", "Epoch identity (ADR-0043 §1).");
prefixed_id!(
    CheckpointId,
    "ctxckpt_",
    "Checkpoint identity (ADR-0043 §2)."
);
prefixed_id!(
    HistoryRef,
    "histref_",
    "Opaque epoch-bound history reference (ADR-0043 §7)."
);
prefixed_id!(
    NoteId,
    "cknote_",
    "Working-note identity (format is TASK-0171's; ADR-0043 §6 pins no shape)."
);

/// Checkpoint wire format version: major in the high 16 bits, minor
/// in the low 16 (packing is TASK-0171's; ADR-0043 §12 pins major
/// refusal + minor compatibility only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FormatVersion(u32);

impl FormatVersion {
    /// The only major this build reads (CR-10).
    pub const SUPPORTED_MAJOR: u32 = 1;

    /// Wraps a raw version word.
    #[must_use]
    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    /// Major version (high 16 bits).
    #[must_use]
    pub const fn major(self) -> u32 {
        self.0 >> 16
    }

    /// Minor version (low 16 bits, backward compatible).
    #[must_use]
    pub const fn minor(self) -> u32 {
        self.0 & 0xFFFF
    }

    /// Raw version word for durable storage.
    #[must_use]
    pub const fn as_u32(self) -> u32 {
        self.0
    }

    /// Refuses unknown majors fail-closed (CR-10).
    pub fn require_supported(self) -> Result<Self, ContinuityError> {
        if self.major() != Self::SUPPORTED_MAJOR {
            return Err(ContinuityError::UnknownVersion(format!(
                "checkpoint major {} is not supported (want {})",
                self.major(),
                Self::SUPPORTED_MAJOR
            )));
        }
        Ok(self)
    }
}

/// Epoch lifecycle state (ADR-0043 §1): exactly one ACTIVE epoch per
/// chain; older epochs are INDEXED and read-only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EpochState {
    Active,
    Indexed,
}

impl EpochState {
    /// Canonical wire name (also the SQL CHECK value).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "ACTIVE",
            Self::Indexed => "INDEXED",
        }
    }

    /// Parses a wire name; unknown values fail closed.
    pub fn parse(raw: &str) -> Result<Self, ContinuityError> {
        match raw {
            "ACTIVE" => Ok(Self::Active),
            "INDEXED" => Ok(Self::Indexed),
            _ => Err(ContinuityError::InvalidInput(format!(
                "unknown epoch state: {raw}"
            ))),
        }
    }
}

/// A bounded active context window in a (task, execution) chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Epoch {
    pub id: EpochId,
    pub task_ref: TaskId,
    pub execution_ref: ExecutionId,
    pub seq: u64,
    pub state: EpochState,
    pub created_at: u64,
}

/// Structured continuity state (ADR-0043 §2): references raw evidence,
/// never embeds transcripts, payloads or files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkpoint {
    pub id: CheckpointId,
    pub epoch_ref: EpochId,
    pub task_ref: TaskId,
    pub execution_ref: ExecutionId,
    pub created_at: u64,
    pub format_version: FormatVersion,
    pub parent_checkpoint: Option<CheckpointId>,
    pub working_state: Vec<NoteId>,
    pub source_refs: Vec<SourceRef>,
    pub evidence_refs: Vec<String>,
    pub continuation: String,
    pub context_plan_ref: ContextPlanId,
    pub sensitivity_ceiling: Sensitivity,
}

impl Checkpoint {
    /// Shape validation: supported version (CR-10), bound source refs
    /// (CX-2), non-blank bounded evidence refs and continuation.
    /// `continuation` is authored-observation text: callers must not
    /// paste secret values (unenforceable by scan); the SECRET gate
    /// holds on the labelled path (notes) and on ceiling dominance.
    pub fn validate(&self) -> Result<(), ContinuityError> {
        self.format_version.require_supported()?;
        for source in &self.source_refs {
            source.validate_binding().map_err(|err| {
                ContinuityError::InvalidInput(format!("checkpoint source ref: {err}"))
            })?;
        }
        for reference in &self.evidence_refs {
            bounded_text("evidence ref", reference)?;
        }
        if self.continuation.chars().count() > MAX_TEXT_CHARS {
            return Err(ContinuityError::InvalidInput(format!(
                "continuation exceeds {MAX_TEXT_CHARS} chars"
            )));
        }
        Ok(())
    }
}

/// Working-note author (ADR-0043 §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NoteAuthor {
    Human,
    Agent,
    System,
}

impl NoteAuthor {
    /// Canonical wire name (also the SQL CHECK value).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Human => "HUMAN",
            Self::Agent => "AGENT",
            Self::System => "SYSTEM",
        }
    }

    /// Parses a wire name; unknown values fail closed.
    pub fn parse(raw: &str) -> Result<Self, ContinuityError> {
        match raw {
            "HUMAN" => Ok(Self::Human),
            "AGENT" => Ok(Self::Agent),
            "SYSTEM" => Ok(Self::System),
            _ => Err(ContinuityError::InvalidInput(format!(
                "unknown note author: {raw}"
            ))),
        }
    }
}

/// Note body: authored observation inline (≤4 KiB) or a reference for
/// anything larger; raw evidence always travels by reference (CR-5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoteBody {
    Inline(String),
    Ref(String),
}

/// A timestamped working note: execution evidence, never auto-promoted
/// to Memory/Decision/ADR/Task state (ADR-0043 §6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkingNote {
    pub id: NoteId,
    pub epoch_ref: EpochId,
    pub created_at: u64,
    pub author: NoteAuthor,
    pub body: NoteBody,
    pub sensitivity: Sensitivity,
}

impl WorkingNote {
    /// Shape validation: inline bound, non-blank bodies and refs.
    /// SECRET-labelled notes travel by reference only: SECRET values
    /// are never persisted inline (CR-8).
    pub fn validate(&self) -> Result<(), ContinuityError> {
        match &self.body {
            NoteBody::Inline(text) => {
                if self.sensitivity == Sensitivity::Secret {
                    return Err(ContinuityError::InvalidInput(
                        "SECRET notes travel by reference, never inline".to_owned(),
                    ));
                }
                if text.trim().is_empty() {
                    return Err(ContinuityError::InvalidInput(
                        "note body must not be blank".to_owned(),
                    ));
                }
                if text.chars().count() > MAX_NOTE_CHARS {
                    return Err(ContinuityError::InvalidInput(format!(
                        "note body exceeds {MAX_NOTE_CHARS} chars"
                    )));
                }
            }
            NoteBody::Ref(reference) => bounded_text("note ref", reference)?,
        }
        Ok(())
    }
}

/// Provider session attached as append-only evidence (ADR-0043 §8):
/// never authoritative for continuation truth (CR-6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionLink {
    pub session_ref: String,
    pub epoch_id: EpochId,
    pub attempt_ref: AttemptId,
    pub provider_id: ProviderRef,
    pub linked_at: u64,
}

impl SessionLink {
    /// Shape validation: the session ref is an opaque provider string.
    pub fn validate(&self) -> Result<(), ContinuityError> {
        bounded_text("session ref", &self.session_ref)
    }
}

/// Migration handoff: the source chain's last committed checkpoint to
/// the new chain's genesis epoch (ADR-0043 §11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationLink {
    pub from_checkpoint: CheckpointId,
    pub to_epoch: EpochId,
}

/// Crash-window disambiguation record (ADR-0043 §9): a pending
/// activation completes idempotently, never forks (CR-3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivationRecord {
    pub checkpoint_id: CheckpointId,
    pub successor: Option<EpochId>,
    pub started_at: u64,
    pub completed_at: Option<u64>,
}

/// Atomic commit receipt (ADR-0043 §3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointReceipt {
    pub checkpoint_id: CheckpointId,
    pub epoch_id: EpochId,
    pub committed_at: u64,
    pub store_revision: u64,
}

/// Retention enforcement inputs (ADR-0043 §12): durations are owned by
/// privacy/storage policy; `None` keeps a class forever.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetentionPolicy {
    pub staged_older_than_ms: u64,
    pub indexed_older_than_ms: Option<u64>,
    pub history_older_than_ms: Option<u64>,
}

/// Retention enforcement outcome: rows purged per class.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PurgeReport {
    pub staged: usize,
    pub epochs: usize,
    pub history_refs: usize,
    pub notes: usize,
}

/// Budget-gate trace entry (ADR-0043 §4): plan ref + verdict, no
/// payload bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetEvent {
    pub plan_ref: ContextPlanId,
    pub hard: bool,
    pub recorded_at: u64,
}

#[cfg(test)]
mod tests {
    use agent_runtime::{AttemptId, ExecutionId, ProviderRef};
    use context_engine::model::{ContextPlanId, Sensitivity, SourceRef};
    use task_domain::TaskId;

    use crate::mint::{mint_checkpoint_id, mint_epoch_id, mint_note_id};
    use crate::model::{
        ActivationRecord, BudgetEvent, Checkpoint, CheckpointId, ContinuityError, Epoch, EpochId,
        EpochState, FormatVersion, HistoryRef, MigrationLink, NoteAuthor, NoteBody, SessionLink,
        WorkingNote,
    };

    const PLAN_ID: &str = "ctxplan_01ARZ3NDEKTSV4RRFFQ69G5FAV";

    fn checkpoint() -> Checkpoint {
        Checkpoint {
            id: mint_checkpoint_id(1000, 1),
            epoch_ref: mint_epoch_id(1000, 1),
            task_ref: TaskId::new("T-1").unwrap(),
            execution_ref: ExecutionId::new("E-1").unwrap(),
            created_at: 1000,
            format_version: FormatVersion::new(0x0001_0000),
            parent_checkpoint: None,
            working_state: vec![mint_note_id(1000, 1)],
            source_refs: vec![SourceRef::repo_path("rev1", "src/lib.rs", None).unwrap()],
            evidence_refs: vec!["ev:1".to_owned()],
            continuation: "next: ship it".to_owned(),
            context_plan_ref: ContextPlanId::new(PLAN_ID).unwrap(),
            sensitivity_ceiling: Sensitivity::Project,
        }
    }

    #[test]
    fn id_rejects_bad_prefix_and_shape() {
        assert!(EpochId::new("ctxepoch_01ARZ3NDEKTSV4RRFFQ69G5FAV").is_ok());
        assert!(EpochId::new("ctxckpt_01ARZ3NDEKTSV4RRFFQ69G5FAV").is_err());
        assert!(EpochId::new("ctxepoch_too-short").is_err());
        assert!(EpochId::new("ctxepoch_01ARZ3NDEKTSV4RRFFQ69G5FAI").is_err());
        assert!(CheckpointId::new("ctxckpt_01ARZ3NDEKTSV4RRFFQ69G5FAV").is_ok());
        assert!(HistoryRef::new("histref_01ARZ3NDEKTSV4RRFFQ69G5FAV").is_ok());
        assert!(HistoryRef::new("histref_01ARZ3NDEKTSV4RRFFQ69G5FAV ").is_err());
    }

    #[test]
    fn mint_sorts_by_time_then_sequence() {
        let early = mint_epoch_id(100, 99);
        let late = mint_epoch_id(101, 0);
        assert!(early.as_str() < late.as_str());
        let first = mint_checkpoint_id(100, 1);
        let second = mint_checkpoint_id(100, 2);
        assert!(first.as_str() < second.as_str());
        assert!(early.as_str().starts_with("ctxepoch_"));
        assert_eq!(early.as_str().len(), 9 + 26);
    }

    #[test]
    fn version_parses_major_minor() {
        let v = FormatVersion::new(0x0001_0002);
        assert_eq!((v.major(), v.minor()), (1, 2));
        assert_eq!(FormatVersion::SUPPORTED_MAJOR, 1);
    }

    #[test]
    fn version_refuses_unknown_major() {
        let err = FormatVersion::new(0x0002_0000)
            .require_supported()
            .unwrap_err();
        assert!(matches!(err, ContinuityError::UnknownVersion(_)));
        assert!(FormatVersion::new(0x0001_0000).require_supported().is_ok());
    }

    #[test]
    fn checkpoint_validate_catches_unbound_source() {
        let good = checkpoint();
        assert!(good.validate().is_ok());
        let mut bad = checkpoint();
        bad.source_refs = vec![SourceRef::Task {
            task_id: String::new(),
            event_seq: 3,
        }];
        assert!(bad.validate().is_err());
        let mut bad_version = checkpoint();
        bad_version.format_version = FormatVersion::new(0x0002_0000);
        let err = bad_version.validate().unwrap_err();
        assert!(matches!(err, ContinuityError::UnknownVersion(_)));
        let mut bad_evidence = checkpoint();
        bad_evidence.evidence_refs = vec!["  ".to_owned()];
        assert!(bad_evidence.validate().is_err());
    }

    fn note() -> WorkingNote {
        WorkingNote {
            id: mint_note_id(1000, 2),
            epoch_ref: mint_epoch_id(1000, 1),
            created_at: 1000,
            author: NoteAuthor::Agent,
            body: NoteBody::Inline("observed: tests green".to_owned()),
            sensitivity: Sensitivity::Project,
        }
    }

    #[test]
    fn note_body_inline_bound_is_4096_chars() {
        assert!(note().validate().is_ok());
        let mut over = note();
        over.body = NoteBody::Inline("x".repeat(4097));
        assert!(over.validate().is_err());
        let mut blank = note();
        blank.body = NoteBody::Inline("   ".to_owned());
        assert!(blank.validate().is_err());
    }

    #[test]
    fn note_secret_inline_refuses_but_ref_accepts() {
        let mut secret_inline = note();
        secret_inline.sensitivity = Sensitivity::Secret;
        let err = secret_inline.validate().unwrap_err();
        assert!(matches!(err, ContinuityError::InvalidInput(_)));

        let mut secret_ref = note();
        secret_ref.sensitivity = Sensitivity::Secret;
        secret_ref.body = NoteBody::Ref("vault:token/7".to_owned());
        assert!(secret_ref.validate().is_ok());
    }

    #[test]
    fn note_ref_body_unbounded() {
        let mut noted = note();
        noted.body = NoteBody::Ref("artifact:transcript/42".to_owned());
        assert!(noted.validate().is_ok());
        noted.body = NoteBody::Ref("   ".to_owned());
        assert!(noted.validate().is_err());
    }

    #[test]
    fn ceiling_orders_like_data_classification() {
        use Sensitivity::{Private, Project, Public, Restricted, Secret};
        assert!(Public < Project && Project < Private);
        assert!(Private < Restricted && Restricted < Secret);
        let mut noted = note();
        noted.sensitivity = Sensitivity::Secret;
        noted.body = NoteBody::Ref("vault:ceiling/1".to_owned());
        assert!(noted.validate().is_ok());
    }

    #[test]
    fn epoch_links_model_shapes() {
        let epoch = Epoch {
            id: mint_epoch_id(1000, 1),
            task_ref: TaskId::new("T-1").unwrap(),
            execution_ref: ExecutionId::new("E-1").unwrap(),
            seq: 0,
            state: EpochState::Active,
            created_at: 1000,
        };
        assert_eq!(epoch.state, EpochState::Active);
        let link = SessionLink {
            session_ref: "claude:ses_1".to_owned(),
            epoch_id: epoch.id.clone(),
            attempt_ref: AttemptId::new("A-1").unwrap(),
            provider_id: ProviderRef::new("claude").unwrap(),
            linked_at: 1000,
        };
        assert!(link.validate().is_ok());
        let migration = MigrationLink {
            from_checkpoint: mint_checkpoint_id(1000, 1),
            to_epoch: mint_epoch_id(2000, 1),
        };
        let _ = migration;
        let activation = ActivationRecord {
            checkpoint_id: mint_checkpoint_id(1000, 1),
            successor: None,
            started_at: 1000,
            completed_at: None,
        };
        assert!(activation.completed_at.is_none());
        let event = BudgetEvent {
            plan_ref: ContextPlanId::new(PLAN_ID).unwrap(),
            hard: false,
            recorded_at: 1000,
        };
        assert!(!event.hard);
    }
}
