//! ADR-0044 memory vocabulary: validated records, trust lifecycle,
//! dedup identity, anchor staleness and context eligibility.
//! Pure data + validation; no I/O, no clock.

use std::fmt;

use context_engine::model::Sensitivity;
use sha2::{Digest, Sha256};

pub use context_engine::model::Sensitivity as MemorySensitivity;

/// Maximum memory content length (chars).
pub const MAX_CONTENT_CHARS: usize = 16_384;
/// Maximum bounded text length (chars) for refs and names.
pub const MAX_TEXT_CHARS: usize = 1024;
/// Maximum confidence value (per mille).
pub const MAX_CONFIDENCE: u16 = 1000;

const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const ID_RANDOM_LEN: usize = 26;

fn is_crockford(s: &str) -> bool {
    s.bytes().all(|b| CROCKFORD.contains(&b))
}

fn valid_prefixed(raw: &str, prefix: &str) -> bool {
    raw.strip_prefix(prefix)
        .is_some_and(|rest| rest.len() == ID_RANDOM_LEN && is_crockford(rest))
}

/// Canonical memory failure. Small seam-local taxonomy (ADR-0044 §6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryError {
    InvalidInput(String),
    UnknownKind(String),
    UnboundSourceRef(String),
    SensitivityRefusal { item: String },
    UntrustedPromotion(String),
    StaleAnchor(String),
    AlreadySuperseded(String),
    Conflict(String),
    NotFound(String),
    BackendFailure { detail: String },
}

impl fmt::Display for MemoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(d) => write!(f, "invalid input: {d}"),
            Self::UnknownKind(d) => write!(f, "unknown kind: {d}"),
            Self::UnboundSourceRef(d) => write!(f, "unbound source ref: {d}"),
            Self::SensitivityRefusal { item } => write!(f, "sensitivity refusal: {item}"),
            Self::UntrustedPromotion(d) => write!(f, "untrusted promotion: {d}"),
            Self::StaleAnchor(d) => write!(f, "stale anchor: {d}"),
            Self::AlreadySuperseded(d) => write!(f, "already superseded: {d}"),
            Self::Conflict(d) => write!(f, "conflict: {d}"),
            Self::NotFound(d) => write!(f, "not found: {d}"),
            Self::BackendFailure { detail } => write!(f, "backend failure: {detail}"),
        }
    }
}

impl std::error::Error for MemoryError {}

macro_rules! memory_id {
    ($name:ident, $prefix:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(String);
        impl $name {
            pub fn new(raw: impl Into<String>) -> Result<Self, MemoryError> {
                let raw = raw.into();
                if !valid_prefixed(&raw, $prefix) {
                    return Err(MemoryError::InvalidInput(format!(
                        "id must be {} + 26 Crockford chars",
                        $prefix
                    )));
                }
                Ok(Self(raw))
            }
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

memory_id!(MemoryId, "mem_");
memory_id!(EntityId, "ent_");

macro_rules! wire_enum {
    ($name:ident, [$($variant:ident => $wire:literal),+ $(,)?]) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum $name { $($variant,)+ }
        impl $name {
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $wire,)+ }
            }
            pub fn parse(raw: &str) -> Result<Self, MemoryError> {
                match raw {
                    $($wire => Ok(Self::$variant),)+
                    _ => Err(MemoryError::UnknownKind(raw.to_owned())),
                }
            }
        }
    };
}

wire_enum!(MemoryKind, [
    Semantic => "SEMANTIC",
    Episodic => "EPISODIC",
    Procedural => "PROCEDURAL",
    Decision => "DECISION",
]);

wire_enum!(TrustState, [
    Proposed => "PROPOSED",
    Confirmed => "CONFIRMED",
    Contradicted => "CONTRADICTED",
    Stale => "STALE",
    Superseded => "SUPERSEDED",
    Retired => "RETIRED",
]);

wire_enum!(SourceRefKind, [
    Task => "TASK",
    Attempt => "ATTEMPT",
    Git => "GIT",
    Decision => "DECISION",
    Event => "EVENT",
    File => "FILE",
]);

wire_enum!(AnchorKind, [
    Code => "CODE",
    Task => "TASK",
    Decision => "DECISION",
    Event => "EVENT",
]);

wire_enum!(LinkKind, [
    Supersedes => "SUPERSEDES",
    DerivesFrom => "DERIVES_FROM",
    Contradicts => "CONTRADICTS",
    Related => "RELATED",
]);

fn bounded(value: &str, field: &str) -> Result<String, MemoryError> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.chars().count() > MAX_TEXT_CHARS {
        return Err(MemoryError::InvalidInput(format!(
            "{field} must be 1..={MAX_TEXT_CHARS} chars"
        )));
    }
    Ok(trimmed.to_owned())
}

fn is_sha256(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

/// Computes `sha256:<hex>` over canonical content bytes.
#[must_use]
pub fn content_hash(content: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(content.as_bytes()))
}

/// Computes the ADR-0044 §4 dedup key for a record shape.
#[must_use]
pub fn dedup_key(kind: MemoryKind, hash: &str, primary_scope: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(kind.as_str().as_bytes());
    hasher.update(b"\0");
    hasher.update(hash.as_bytes());
    hasher.update(b"\0");
    hasher.update(primary_scope.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

/// Typed, revision-bound source reference (M-2).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SourceRef {
    pub kind: SourceRefKind,
    pub reference: String,
    pub revision: String,
}

impl SourceRef {
    pub fn new(
        kind: SourceRefKind,
        reference: impl Into<String>,
        revision: impl Into<String>,
    ) -> Result<Self, MemoryError> {
        let reference = bounded(&reference.into(), "source reference")?;
        let revision_raw: String = revision.into();
        let revision = bounded(&revision_raw, "source revision").map_err(|_| {
            MemoryError::UnboundSourceRef(format!("{kind:?}:{reference} lacks revision"))
        })?;
        Ok(Self {
            kind,
            reference,
            revision,
        })
    }

    /// Scope string used by the dedup key.
    #[must_use]
    pub fn scope(&self) -> String {
        format!("{}:{}", self.kind.as_str(), self.reference)
    }
}

/// Fingerprint binding memory to a code/task/decision revision.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AnchorFingerprint {
    pub kind: AnchorKind,
    pub reference: String,
    pub fingerprint: String,
}

impl AnchorFingerprint {
    pub fn new(
        kind: AnchorKind,
        reference: impl Into<String>,
        fingerprint: impl Into<String>,
    ) -> Result<Self, MemoryError> {
        let reference = bounded(&reference.into(), "anchor reference")?;
        let fingerprint = fingerprint.into();
        if !is_sha256(&fingerprint) {
            return Err(MemoryError::InvalidInput(
                "anchor fingerprint must be sha256:<64 hex>".to_owned(),
            ));
        }
        Ok(Self {
            kind,
            reference,
            fingerprint,
        })
    }
}

/// One structured memory record (ADR-0044 §1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryRecord {
    pub id: MemoryId,
    pub kind: MemoryKind,
    pub content: String,
    pub content_hash: String,
    pub source_refs: Vec<SourceRef>,
    pub confidence: u16,
    pub sensitivity: Sensitivity,
    pub trust_state: TrustState,
    pub anchors: Vec<AnchorFingerprint>,
    pub author_ref: String,
    pub created_at: u64,
    pub last_verified_at: u64,
    pub expires_at: Option<u64>,
    pub supersedes: Option<MemoryId>,
}

impl MemoryRecord {
    /// CAPTURE: builds a validated `Proposed` record (M-2, M-3, M-4).
    #[allow(clippy::too_many_arguments)]
    pub fn capture(
        id: MemoryId,
        kind: MemoryKind,
        content: impl Into<String>,
        source_refs: Vec<SourceRef>,
        confidence: u16,
        sensitivity: Sensitivity,
        anchors: Vec<AnchorFingerprint>,
        author_ref: impl Into<String>,
        now: u64,
        expires_at: Option<u64>,
        supersedes: Option<MemoryId>,
    ) -> Result<Self, MemoryError> {
        let content = content.into();
        let chars = content.chars().count();
        if chars == 0 || chars > MAX_CONTENT_CHARS {
            return Err(MemoryError::InvalidInput(format!(
                "content must be 1..={MAX_CONTENT_CHARS} chars"
            )));
        }
        if sensitivity == Sensitivity::Secret {
            return Err(MemoryError::SensitivityRefusal {
                item: id.to_string(),
            });
        }
        if source_refs.is_empty() {
            return Err(MemoryError::UnboundSourceRef(
                "at least one source ref required".to_owned(),
            ));
        }
        if kind != MemoryKind::Episodic && anchors.is_empty() {
            return Err(MemoryError::InvalidInput(
                "non-episodic memory requires at least one anchor".to_owned(),
            ));
        }
        if confidence > MAX_CONFIDENCE {
            return Err(MemoryError::InvalidInput(
                "confidence must be per-mille 0..=1000".to_owned(),
            ));
        }
        let author_ref = bounded(&author_ref.into(), "author_ref")?;
        if let Some(exp) = expires_at {
            if exp <= now {
                return Err(MemoryError::InvalidInput(
                    "expires_at must be after created_at".to_owned(),
                ));
            }
        }
        Ok(Self {
            content_hash: content_hash(&content),
            id,
            kind,
            content,
            source_refs,
            confidence,
            sensitivity,
            trust_state: TrustState::Proposed,
            anchors,
            author_ref,
            created_at: now,
            last_verified_at: now,
            expires_at,
            supersedes,
        })
    }

    /// Revalidates a stored shape (used on decode). Fails closed.
    pub fn revalidate(&self) -> Result<(), MemoryError> {
        if self.content_hash != content_hash(&self.content) {
            return Err(MemoryError::Conflict(format!(
                "content hash mismatch for {}",
                self.id
            )));
        }
        if self.sensitivity == Sensitivity::Secret {
            return Err(MemoryError::SensitivityRefusal {
                item: self.id.to_string(),
            });
        }
        if self.source_refs.is_empty() {
            return Err(MemoryError::UnboundSourceRef(
                "at least one source ref required".to_owned(),
            ));
        }
        if self.last_verified_at < self.created_at {
            return Err(MemoryError::InvalidInput(
                "last_verified_at before created_at".to_owned(),
            ));
        }
        Ok(())
    }

    /// ADR-0044 §4 dedup key (kind + content hash + primary scope).
    #[must_use]
    pub fn dedup_key(&self) -> String {
        let scope = self
            .source_refs
            .first()
            .map(|r| r.scope())
            .unwrap_or_default();
        dedup_key(self.kind, &self.content_hash, &scope)
    }

    fn ensure_validator(&self, validator: &str) -> Result<(), MemoryError> {
        if validator.trim().is_empty() {
            return Err(MemoryError::InvalidInput(
                "validator must be non-empty".to_owned(),
            ));
        }
        if validator == self.author_ref {
            return Err(MemoryError::UntrustedPromotion(format!(
                "author {} cannot validate own memory {}",
                self.author_ref, self.id
            )));
        }
        Ok(())
    }

    /// VALIDATE / REFRESH: `Proposed|Stale -> Confirmed`, or re-verify.
    pub fn validate(&mut self, validator: &str, now: u64) -> Result<(), MemoryError> {
        self.ensure_validator(validator)?;
        match self.trust_state {
            TrustState::Proposed | TrustState::Stale | TrustState::Confirmed => {}
            other => {
                return Err(MemoryError::InvalidInput(format!(
                    "cannot validate memory in state {}",
                    other.as_str()
                )))
            }
        }
        if now < self.created_at {
            return Err(MemoryError::InvalidInput(
                "validation time before created_at".to_owned(),
            ));
        }
        self.trust_state = TrustState::Confirmed;
        self.last_verified_at = now;
        Ok(())
    }

    /// VALIDATE (negative): `Proposed -> Contradicted` with evidence.
    pub fn contradict(
        &mut self,
        validator: &str,
        evidence: Vec<SourceRef>,
    ) -> Result<(), MemoryError> {
        self.ensure_validator(validator)?;
        if evidence.is_empty() {
            return Err(MemoryError::UnboundSourceRef(
                "contradiction requires evidence refs".to_owned(),
            ));
        }
        if self.trust_state != TrustState::Proposed {
            return Err(MemoryError::InvalidInput(format!(
                "cannot contradict memory in state {}",
                self.trust_state.as_str()
            )));
        }
        self.trust_state = TrustState::Contradicted;
        Ok(())
    }

    /// ANCHOR DRIFT: marks `Proposed|Confirmed` stale on mismatch (M-10).
    /// Returns true when the record became stale.
    pub fn mark_stale_if_anchor_changed(
        &mut self,
        current: &[AnchorFingerprint],
    ) -> Result<bool, MemoryError> {
        if !matches!(
            self.trust_state,
            TrustState::Proposed | TrustState::Confirmed
        ) {
            return Ok(false);
        }
        for anchor in &self.anchors {
            let now = current
                .iter()
                .find(|c| c.kind == anchor.kind && c.reference == anchor.reference);
            match now {
                Some(c) if c.fingerprint == anchor.fingerprint => {}
                _ => {
                    self.trust_state = TrustState::Stale;
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// SUPERSEDE: successor must be `Confirmed` and name this record (M-11).
    pub fn supersede_by(&mut self, successor: &MemoryRecord) -> Result<(), MemoryError> {
        if self.trust_state == TrustState::Superseded {
            return Err(MemoryError::AlreadySuperseded(self.id.to_string()));
        }
        if !matches!(self.trust_state, TrustState::Confirmed | TrustState::Stale) {
            return Err(MemoryError::InvalidInput(format!(
                "cannot supersede memory in state {}",
                self.trust_state.as_str()
            )));
        }
        if successor.trust_state != TrustState::Confirmed {
            return Err(MemoryError::InvalidInput(
                "successor must be Confirmed before supersession".to_owned(),
            ));
        }
        if successor.supersedes.as_ref() != Some(&self.id) {
            return Err(MemoryError::InvalidInput(
                "successor does not name this record in supersedes".to_owned(),
            ));
        }
        self.trust_state = TrustState::Superseded;
        Ok(())
    }

    /// ARCHIVE: any non-terminal record -> `Retired`.
    pub fn retire(&mut self) -> Result<(), MemoryError> {
        if self.trust_state == TrustState::Retired {
            return Err(MemoryError::InvalidInput(
                "memory already retired".to_owned(),
            ));
        }
        self.trust_state = TrustState::Retired;
        Ok(())
    }

    /// Context eligibility (ADR-0044 §5).
    #[must_use]
    pub fn is_context_eligible(&self, now: u64) -> bool {
        self.trust_state == TrustState::Confirmed
            && self.sensitivity != Sensitivity::Secret
            && self.expires_at.is_none_or(|exp| exp > now)
    }
}

/// Named thing memories attach to (ADR-0012 entities).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entity {
    pub id: EntityId,
    pub name: String,
    pub entity_kind: String,
}

impl Entity {
    pub fn new(
        id: EntityId,
        name: impl Into<String>,
        entity_kind: impl Into<String>,
    ) -> Result<Self, MemoryError> {
        Ok(Self {
            id,
            name: bounded(&name.into(), "entity name")?,
            entity_kind: bounded(&entity_kind.into(), "entity kind")?,
        })
    }
}

/// Memory-to-memory link (ADR-0012 memory_links).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MemoryLink {
    pub from: MemoryId,
    pub to: MemoryId,
    pub kind: LinkKind,
}

/// Memory-to-entity link (ADR-0012 entity_links).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EntityLink {
    pub memory: MemoryId,
    pub entity: EntityId,
    pub relation: String,
}

impl EntityLink {
    pub fn new(
        memory: MemoryId,
        entity: EntityId,
        relation: impl Into<String>,
    ) -> Result<Self, MemoryError> {
        Ok(Self {
            memory,
            entity,
            relation: bounded(&relation.into(), "relation")?,
        })
    }
}
