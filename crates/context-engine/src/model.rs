//! ADR-0042 selection vocabulary: validated newtypes, closed enums,
//! provenance envelopes, plans and traces. Pure data + validation;
//! no I/O, no clock.

use std::fmt;

/// Crockford Base32 without I, L, O, U (matches ULID shape).
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Length of the random part of context ids (ULID shape).
const ID_RANDOM_LEN: usize = 26;
/// Generous bound for reason/selector/scope text (chars).
pub const MAX_TEXT_CHARS: usize = 4096;
/// Maximum confidence value (per mille).
pub const MAX_CONFIDENCE: u16 = 1000;
/// Maximum renderer id length, prefix included (matches SQL).
pub const MAX_RENDERER_CHARS: usize = 1025;

/// Canonical context-engine failure. Small seam-local taxonomy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextError {
    /// Blank, over-long, or otherwise malformed input value.
    InvalidInput(String),
    /// A `ContextKind` outside the closed v1 enum (CX-1).
    UnknownKind(String),
    /// A `SourceRef` without a revision binding (CX-2).
    UnboundSourceRef(String),
    /// A SECRET item refused from model context (CX-3).
    SensitivityRefusal { item: String },
    /// Required items exceed the hard budget (CX-6).
    BudgetExceeded { needed: u64, hard: u64 },
    /// A plan that violates structural rules (CX-4, CX-9).
    MalformedPlan(String),
    /// Replay diverged from the recorded trace (CX-5).
    ReplayMismatch(String),
    /// Storage backend failure with bounded detail.
    BackendFailure { detail: String },
}

impl fmt::Display for ContextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(detail) => write!(f, "invalid input: {detail}"),
            Self::UnknownKind(detail) => write!(f, "unknown kind: {detail}"),
            Self::UnboundSourceRef(detail) => {
                write!(f, "unbound source ref: {detail}")
            }
            Self::SensitivityRefusal { item } => {
                write!(f, "sensitivity refusal: {item}")
            }
            Self::BudgetExceeded { needed, hard } => {
                write!(f, "budget exceeded: needed {needed}, hard {hard}")
            }
            Self::MalformedPlan(detail) => write!(f, "malformed plan: {detail}"),
            Self::ReplayMismatch(detail) => write!(f, "replay mismatch: {detail}"),
            Self::BackendFailure { detail } => {
                write!(f, "backend failure: {detail}")
            }
        }
    }
}

impl std::error::Error for ContextError {}

/// Shared validation for bounded non-blank text.
fn bounded_text(field: &str, raw: &str) -> Result<String, ContextError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ContextError::InvalidInput(format!(
            "{field} must not be blank"
        )));
    }
    // Raw length: SQL length() counts stored (unpadded-by-us) chars,
    // so the bound must hold before trimming to keep one taxonomy.
    if raw.chars().count() > MAX_TEXT_CHARS {
        return Err(ContextError::InvalidInput(format!(
            "{field} exceeds {MAX_TEXT_CHARS} chars"
        )));
    }
    Ok(trimmed.to_owned())
}

fn is_crockford(s: &str) -> bool {
    s.bytes().all(|b| CROCKFORD.contains(&b))
}

macro_rules! prefixed_id {
    ($name:ident, $prefix:literal, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(String);

        impl $name {
            /// Validates `prefix` + 26 Crockford chars (ULID shape).
            pub fn new(raw: impl Into<String>) -> Result<Self, ContextError> {
                let raw = raw.into();
                let rest = raw.strip_prefix($prefix).ok_or_else(|| {
                    ContextError::InvalidInput(format!("id must start with {}", $prefix))
                })?;
                if rest.len() != ID_RANDOM_LEN || !is_crockford(rest) {
                    return Err(ContextError::InvalidInput(
                        "id random part must be 26 Crockford chars".to_owned(),
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

prefixed_id!(
    ContextItemId,
    "ctxitem_",
    "Opaque context-item identity. Time-sortable by construction."
);
prefixed_id!(
    ContextPlanId,
    "ctxplan_",
    "Opaque context-plan identity. Time-sortable by construction."
);

macro_rules! wire_enum {
    ($name:ident, [$($variant:ident => $wire:literal),+ $(,)?], $unknown:expr) => {
        /// Declaration order doubles as sort order (used for
        /// deterministic output; for `Sensitivity` it is also
        /// escalation order: `Public < Secret`).
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum $name {
            $($variant,)+
        }

        impl $name {
            /// Canonical wire name (stable; also the SQL CHECK value).
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $wire,)+
                }
            }

            /// Parses a wire name; unknown values fail closed.
            pub fn parse(raw: &str) -> Result<Self, ContextError> {
                match raw {
                    $($wire => Ok(Self::$variant),)+
                    _ => Err($unknown(raw)),
                }
            }
        }
    };
}

wire_enum!(
    ContextKind,
    [
        TaskSnapshot => "TASK_SNAPSHOT",
        FileExcerpt => "FILE_EXCERPT",
        SymbolSpan => "SYMBOL_SPAN",
        DiffHunk => "DIFF_HUNK",
        PlanRevision => "PLAN_REVISION",
        AttemptRecord => "ATTEMPT_RECORD",
        HumanDecision => "HUMAN_DECISION",
        EventFact => "EVENT_FACT",
        MemoryNote => "MEMORY_NOTE",
        ReviewPacket => "REVIEW_PACKET",
        ToolObservation => "TOOL_OBSERVATION",
        CompactionReceipt => "COMPACTION_RECEIPT",
    ],
    |raw: &str| ContextError::UnknownKind(raw.to_owned())
);

/// Closed `ContextKind` v1: twelve variants, no more.
pub const CONTEXT_KIND_V1_COUNT: usize = 12;

wire_enum!(
    Sensitivity,
    [
        Public => "PUBLIC",
        Project => "PROJECT",
        Private => "PRIVATE",
        Restricted => "RESTRICTED",
        Secret => "SECRET",
    ],
    |raw: &str| ContextError::InvalidInput(format!("unknown sensitivity: {raw}"))
);

wire_enum!(
    CacheClass,
    [
        StablePrefix => "STABLE_PREFIX",
        SessionEphemeral => "SESSION_EPHEMERAL",
        NoCache => "NO_CACHE",
    ],
    |raw: &str| ContextError::InvalidInput(format!("unknown cache class: {raw}"))
);

// Closed `ReasonCode` v1 with producer status (`admit` emits the
// first five; the last three are reserved: duplicate ids refuse the
// whole plan as `MalformedPlan` instead of `DuplicateOf`, and
// `RendererLimit`/`DeferredByPolicy` await a renderer-capacity model
// and policy input respectively).
wire_enum!(
    ReasonCode,
    [
        BudgetExceeded => "BUDGET_EXCEEDED",
        Superseded => "SUPERSEDED",
        SensitivityRefusal => "SENSITIVITY_REFUSAL",
        StaleSource => "STALE_SOURCE",
        DuplicateOf => "DUPLICATE_OF",
        TrustBelowFloor => "TRUST_BELOW_FLOOR",
        RendererLimit => "RENDER_LIMIT",
        DeferredByPolicy => "DEFERRED_BY_POLICY",
    ],
    |raw: &str| ContextError::InvalidInput(format!("unknown reason: {raw}"))
);

wire_enum!(
    CounterMethod,
    [
        ExactTokenizer => "EXACT_TOKENIZER",
        CharHeuristic => "CHAR_HEURISTIC",
    ],
    |raw: &str| ContextError::InvalidInput(format!("unknown counter: {raw}"))
);

wire_enum!(
    DecisionOutcome,
    [
        Selected => "SELECTED",
        Deferred => "DEFERRED",
        Rejected => "REJECTED",
    ],
    |raw: &str| ContextError::InvalidInput(format!("unknown outcome: {raw}"))
);

wire_enum!(
    TrustLevel,
    [
        // Ascending trustworthiness: floor comparison is `level < floor`.
        Untrusted => "UNTRUSTED",
        Unverified => "UNVERIFIED",
        Verified => "VERIFIED",
    ],
    |raw: &str| ContextError::InvalidInput(format!("unknown trust: {raw}"))
);

/// SHA-256 content hash over canonical source bytes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ContentHash(String);

impl ContentHash {
    /// Validates `sha256:` + 64 lowercase hex.
    pub fn new(raw: impl Into<String>) -> Result<Self, ContextError> {
        let raw = raw.into();
        let hex = raw
            .strip_prefix("sha256:")
            .ok_or_else(|| ContextError::InvalidInput("hash must start with sha256:".to_owned()))?;
        if hex.len() != 64 || !hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
            return Err(ContextError::InvalidInput(
                "hash must carry 64 hex chars".to_owned(),
            ));
        }
        Ok(Self(raw))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Typed, revision-bound source reference (ADR-0042 section 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceRef {
    Task {
        task_id: String,
        event_seq: u64,
    },
    RepoPath {
        rev: String,
        path: String,
        span: Option<(u64, u64)>,
    },
    Plan {
        branch_id: String,
        revision_seq: u64,
    },
    Attempt {
        execution_id: String,
        attempt_seq: u64,
    },
    Decision {
        request_id: String,
    },
    Event {
        seq: u64,
    },
    /// Opaque: Memory truth shapes belong to their owning gate.
    Memory {
        opaque: String,
    },
    /// Opaque handle; bytes live in a future artifact capability.
    Observation {
        handle: String,
    },
}

impl SourceRef {
    pub fn task(task_id: &str, event_seq: u64) -> Result<Self, ContextError> {
        Ok(Self::Task {
            task_id: bounded_text("task id", task_id)?,
            event_seq,
        })
    }

    pub fn repo_path(
        rev: &str,
        path: &str,
        span: Option<(u64, u64)>,
    ) -> Result<Self, ContextError> {
        let rev = bounded_text("revision", rev).map_err(|_| {
            ContextError::UnboundSourceRef("repo path ref needs a revision".to_owned())
        })?;
        if let Some((start, end)) = span {
            if start > end {
                return Err(ContextError::InvalidInput(
                    "span start exceeds span end".to_owned(),
                ));
            }
        }
        Ok(Self::RepoPath {
            rev,
            path: bounded_text("path", path)?,
            span,
        })
    }

    pub fn plan(branch_id: &str, revision_seq: u64) -> Result<Self, ContextError> {
        Ok(Self::Plan {
            branch_id: bounded_text("branch id", branch_id)?,
            revision_seq,
        })
    }

    pub fn attempt(execution_id: &str, attempt_seq: u64) -> Result<Self, ContextError> {
        Ok(Self::Attempt {
            execution_id: bounded_text("execution id", execution_id)?,
            attempt_seq,
        })
    }

    pub fn decision(request_id: &str) -> Result<Self, ContextError> {
        Ok(Self::Decision {
            request_id: bounded_text("request id", request_id)?,
        })
    }

    #[must_use]
    pub fn event(seq: u64) -> Self {
        Self::Event { seq }
    }

    pub fn memory(opaque: &str) -> Result<Self, ContextError> {
        Ok(Self::Memory {
            opaque: bounded_text("memory ref", opaque)?,
        })
    }

    pub fn observation(handle: &str) -> Result<Self, ContextError> {
        Ok(Self::Observation {
            handle: bounded_text("observation handle", handle)?,
        })
    }

    /// Canonical revision binding text (never empty by construction).
    #[must_use]
    pub fn revision_binding(&self) -> String {
        match self {
            Self::Task { task_id, event_seq } => format!("{task_id}@{event_seq}"),
            Self::RepoPath { rev, .. } => rev.clone(),
            Self::Plan {
                branch_id,
                revision_seq,
            } => format!("{branch_id}@{revision_seq}"),
            Self::Attempt {
                execution_id,
                attempt_seq,
            } => format!("{execution_id}@{attempt_seq}"),
            Self::Decision { request_id } => request_id.clone(),
            Self::Event { seq } => format!("evt@{seq}"),
            Self::Memory { opaque } => opaque.clone(),
            Self::Observation { handle } => handle.clone(),
        }
    }

    /// Validates the per-variant binding (CX-2): every variant except
    /// `Event` needs a non-blank bound value, and the rendered binding
    /// stays within the durable bound. Constructors enforce this at
    /// build time; this covers direct struct literals.
    pub fn validate_binding(&self) -> Result<(), ContextError> {
        let blank = match self {
            Self::Task { task_id, .. }
            | Self::Plan {
                branch_id: task_id, ..
            }
            | Self::Attempt {
                execution_id: task_id,
                ..
            } => task_id.trim().is_empty(),
            Self::RepoPath { rev, path, .. } => rev.trim().is_empty() || path.trim().is_empty(),
            Self::Decision { request_id }
            | Self::Memory { opaque: request_id }
            | Self::Observation { handle: request_id } => request_id.trim().is_empty(),
            Self::Event { .. } => false,
        };
        if blank {
            return Err(ContextError::UnboundSourceRef(
                "source ref binding is blank".to_owned(),
            ));
        }
        if self.revision_binding().chars().count() > MAX_TEXT_CHARS {
            return Err(ContextError::InvalidInput(
                "source binding exceeds text bound".to_owned(),
            ));
        }
        Ok(())
    }
}

/// Freshness stays separate from trust and sensitivity (CX-7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Freshness {
    pub observed_at: u64,
    pub source_rev: String,
    pub stale_after: Option<u64>,
}

impl Freshness {
    /// `source_rev` is mandatory bounded text; `stale_after` past
    /// `observed_at` is enforced by callers that schedule re-checks.
    pub fn new(
        observed_at: u64,
        source_rev: &str,
        stale_after: Option<u64>,
    ) -> Result<Self, ContextError> {
        Ok(Self {
            observed_at,
            source_rev: bounded_text("source rev", source_rev)?,
            stale_after,
        })
    }
}

/// Trust stays separate from relevance, freshness, sensitivity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trust {
    pub level: TrustLevel,
    pub reason: String,
}

impl Trust {
    /// Trust reasons are mandatory bounded text (untraced trust is
    /// indistinguishable from untrusted data).
    pub fn new(level: TrustLevel, reason: &str) -> Result<Self, ContextError> {
        Ok(Self {
            level,
            reason: bounded_text("trust reason", reason)?,
        })
    }
}

/// How raw evidence reopens (or explicitly does not).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReopenRef {
    Handle(String),
    NotReopenable { reason: String },
}

/// Optional supersession link (superseded items defer with reason).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Supersession {
    pub supersedes: ContextItemId,
    pub reason: String,
}

/// The roadmap's Context Provenance Envelope (ADR-0042 section 4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvenanceEnvelope {
    pub source: SourceRef,
    pub source_rev: String,
    pub retrieved_at: u64,
    pub selector: String,
    pub transform_chain: Vec<String>,
    pub reopen: ReopenRef,
    pub permission_scope: String,
    pub confidence: u16,
    pub supersession: Option<Supersession>,
}

impl ProvenanceEnvelope {
    /// Confidence is permille `0..=1000`; transforms start empty and
    /// reopen defaults to explicitly not recorded.
    pub fn new(
        source: SourceRef,
        source_rev: &str,
        retrieved_at: u64,
        selector: &str,
        permission_scope: &str,
        confidence: u16,
    ) -> Result<Self, ContextError> {
        if confidence > MAX_CONFIDENCE {
            return Err(ContextError::InvalidInput(format!(
                "confidence exceeds {MAX_CONFIDENCE} permille"
            )));
        }
        Ok(Self {
            source,
            source_rev: bounded_text("source rev", source_rev)?,
            retrieved_at,
            selector: bounded_text("selector", selector)?,
            transform_chain: Vec::new(),
            reopen: ReopenRef::NotReopenable {
                reason: "not recorded".to_owned(),
            },
            permission_scope: bounded_text("permission scope", permission_scope)?,
            confidence,
            supersession: None,
        })
    }

    #[must_use]
    pub fn with_reopen(mut self, reopen: ReopenRef) -> Self {
        self.reopen = reopen;
        self
    }

    #[must_use]
    pub fn with_transform(mut self, step: String) -> Self {
        self.transform_chain.push(step);
        self
    }

    #[must_use]
    pub fn with_supersession(mut self, supersession: Supersession) -> Self {
        self.supersession = Some(supersession);
        self
    }
}

/// A selected-material candidate with full ADR-0042 section 1 fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextItem {
    pub id: ContextItemId,
    pub kind: ContextKind,
    pub source_ref: SourceRef,
    pub content_hash: ContentHash,
    pub priority: u16,
    pub freshness: Freshness,
    pub trust: Trust,
    pub sensitivity: Sensitivity,
    pub required: bool,
    pub cache_class: CacheClass,
    pub provenance: ProvenanceEnvelope,
}

impl ContextItem {
    /// Defense-in-depth validation for direct struct literals:
    /// constructors validate at build time, this re-checks every
    /// bound before admission or persistence so SQL never sees a
    /// value the domain would refuse.
    pub fn validate(&self) -> Result<(), ContextError> {
        self.source_ref.validate_binding()?;
        self.provenance.source.validate_binding()?;
        check_text("freshness source rev", &self.freshness.source_rev)?;
        check_text("trust reason", &self.trust.reason)?;
        check_text("envelope source rev", &self.provenance.source_rev)?;
        check_text("selector", &self.provenance.selector)?;
        check_text("permission scope", &self.provenance.permission_scope)?;
        for step in &self.provenance.transform_chain {
            check_text("transform step", step)?;
        }
        match &self.provenance.reopen {
            ReopenRef::Handle(handle) => check_text("reopen handle", handle)?,
            ReopenRef::NotReopenable { reason } => {
                check_text("reopen reason", reason)?;
            }
        }
        if let Some(link) = &self.provenance.supersession {
            check_text("supersession reason", &link.reason)?;
        }
        if self.provenance.confidence > MAX_CONFIDENCE {
            return Err(ContextError::InvalidInput(format!(
                "confidence exceeds {MAX_CONFIDENCE} permille"
            )));
        }
        Ok(())
    }
}

/// Non-blank + bounded check that keeps the owned value untouched.
fn check_text(field: &str, raw: &str) -> Result<(), ContextError> {
    bounded_text(field, raw).map(|_| ())
}

/// Soft/hard token budget with its recorded counter method.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Budget {
    pub soft_tokens: u64,
    pub hard_tokens: u64,
    pub counter: CounterMethod,
}

/// One cache-class segment over explicit item ids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheBoundary {
    pub class: CacheClass,
    pub items: Vec<ContextItemId>,
}

/// An admitted context plan (ADR-0042 section 5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextPlan {
    pub id: ContextPlanId,
    pub revision_seq: u64,
    pub required: Vec<ContextItemId>,
    pub selected: Vec<ContextItemId>,
    pub deferred: Vec<(ContextItemId, ReasonCode)>,
    pub rejected: Vec<(ContextItemId, ReasonCode)>,
    pub budget: Budget,
    pub cache_boundaries: Vec<CacheBoundary>,
    pub renderer: String,
    pub source_hashes: Vec<ContentHash>,
    pub policy_fingerprint: String,
}

/// One recorded selection decision (ADR-0042 section 7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceDecision {
    pub item_id: ContextItemId,
    pub outcome: DecisionOutcome,
    pub reason: Option<ReasonCode>,
    pub rule: String,
}

/// Durable per-execution selection evidence: hashes and refs only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetrievalTrace {
    pub plan_id: ContextPlanId,
    pub decisions: Vec<TraceDecision>,
    pub budget: Budget,
    pub source_revs: Vec<String>,
    pub renderer: String,
    pub policy_fingerprint: String,
    pub cache_boundaries: Vec<CacheBoundary>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const ITEM_ID: &str = "ctxitem_01ARZ3NDEKTSV4RRFFQ69G5FAV";
    const PLAN_ID: &str = "ctxplan_01ARZ3NDEKTSV4RRFFQ69G5FAV";
    const HASH: &str = "sha256:9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";

    #[test]
    fn item_id_accepts_prefixed_crockford() {
        assert_eq!(ContextItemId::new(ITEM_ID).unwrap().as_str(), ITEM_ID);
    }

    #[test]
    fn item_id_rejects_wrong_prefix_and_shape() {
        assert!(ContextItemId::new("ctxplan_01ARZ3NDEKTSV4RRFFQ69G5FAV").is_err());
        assert!(ContextItemId::new("ctxitem_too-short").is_err());
        assert!(ContextItemId::new("ctxitem_01ARZ3NDEKTSV4RRFFQ69G5FAU").is_err());
        assert!(ContextItemId::new("  ").is_err());
    }

    #[test]
    fn plan_id_accepts_own_prefix_only() {
        assert_eq!(ContextPlanId::new(PLAN_ID).unwrap().as_str(), PLAN_ID);
        assert!(ContextPlanId::new(ITEM_ID).is_err());
    }

    #[test]
    fn kind_roundtrips_all_twelve_v1_variants() {
        let kinds = [
            ContextKind::TaskSnapshot,
            ContextKind::FileExcerpt,
            ContextKind::SymbolSpan,
            ContextKind::DiffHunk,
            ContextKind::PlanRevision,
            ContextKind::AttemptRecord,
            ContextKind::HumanDecision,
            ContextKind::EventFact,
            ContextKind::MemoryNote,
            ContextKind::ReviewPacket,
            ContextKind::ToolObservation,
            ContextKind::CompactionReceipt,
        ];
        assert_eq!(kinds.len(), 12);
        for kind in kinds {
            assert_eq!(ContextKind::parse(kind.as_str()).unwrap(), kind);
        }
    }

    #[test]
    fn kind_parse_refuses_unknown() {
        assert!(matches!(
            ContextKind::parse("VECTOR_CHUNK"),
            Err(ContextError::UnknownKind(_))
        ));
    }

    #[test]
    fn content_hash_requires_sha256_prefix_and_64_hex() {
        assert!(ContentHash::new(HASH).is_ok());
        assert!(ContentHash::new(
            "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"
        )
        .is_err());
        assert!(ContentHash::new("sha256:xyz").is_err());
        assert!(ContentHash::new(
            "sha256:9F86D081884C7D659A2FEAA0C55AD015A3BF4F1B2B0B822CD15D6C15B0F00A08"
        )
        .is_err());
        assert!(ContentHash::new(
            "blake3:9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"
        )
        .is_err());
    }

    #[test]
    fn sensitivity_orders_public_below_secret() {
        assert!(Sensitivity::Public < Sensitivity::Secret);
        assert_eq!(Sensitivity::parse("SECRET").unwrap(), Sensitivity::Secret);
        assert!(Sensitivity::parse("TOP_SECRET").is_err());
    }

    #[test]
    fn reason_codes_cover_all_eight_v1_outcomes() {
        let codes = [
            ReasonCode::BudgetExceeded,
            ReasonCode::Superseded,
            ReasonCode::SensitivityRefusal,
            ReasonCode::StaleSource,
            ReasonCode::DuplicateOf,
            ReasonCode::TrustBelowFloor,
            ReasonCode::RendererLimit,
            ReasonCode::DeferredByPolicy,
        ];
        for code in codes {
            assert_eq!(ReasonCode::parse(code.as_str()).unwrap(), code);
        }
    }

    #[test]
    fn source_ref_carries_revision_binding_per_variant() {
        let bound = SourceRef::repo_path("abc123", "src/lib.rs", None).unwrap();
        assert_eq!(bound.revision_binding(), "abc123");
        assert!(SourceRef::repo_path("", "src/lib.rs", None).is_err());
        let task = SourceRef::task("TASK-1", 42).unwrap();
        assert_eq!(task.revision_binding(), "TASK-1@42");
    }

    #[test]
    fn envelope_requires_confidence_within_per_mille() {
        let source = SourceRef::task("TASK-1", 42).unwrap();
        assert!(ProvenanceEnvelope::new(source.clone(), "r1", 7, "sel", "scope", 1000).is_ok());
        assert!(ProvenanceEnvelope::new(source, "r1", 7, "sel", "scope", 1001).is_err());
    }
}
