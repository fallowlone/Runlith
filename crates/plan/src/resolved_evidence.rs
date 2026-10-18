//! ADR-0029 option B: resolved planner evidence content.
//!
//! The trusted planning service resolves `EvidenceRef`s it minted itself
//! (attempt conclusions, human decision outcomes) to bounded classified
//! text and attaches it to the planner request. This module owns only the
//! attached value type and its bounds: content is owned data with a
//! classification from `docs/security/DATA_CLASSIFICATION.md`, never a
//! store handle, query, or capability. Refs without an attached item stay
//! identity-only; nothing here can express "the planner may read".

use sha2::{Digest, Sha256};

use crate::error::PlanDomainError;
use crate::ids::EvidenceRef;

/// Per-item content cap (`PLANNER_EVIDENCE_RESOLUTION_POLICY.md` §3).
pub const MAX_RESOLVED_EVIDENCE_BYTES_PER_ITEM: usize = 2_048;
/// Items attached to one planner call (§3).
pub const MAX_RESOLVED_EVIDENCE_ITEMS_PER_CALL: usize = 4;
/// Total attached content bytes per planner call (§3).
pub const MAX_RESOLVED_EVIDENCE_BYTES_PER_CALL: usize = 8_192;
/// Framing tokens charged per attached item against the planner budget.
pub const RESOLVED_EVIDENCE_TOKENS_PER_ITEM_OVERHEAD: u32 = 16;

/// Suffix marking an item cut at the per-item byte cap. Truncation is
/// explicit so the planner can see the fact of it instead of reading a
/// silent prefix as the whole record.
const TRUNCATION_MARKER: &str = "\n[truncated]";

/// Which trusted record a resolved item came from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResolvedEvidenceKind {
    /// Durable execution attempt record (`attempt:<id>`).
    Attempt,
    /// Durable human decision record (`human-decision:<id>`).
    HumanDecision,
}

impl ResolvedEvidenceKind {
    /// Stable wire name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Attempt => "attempt",
            Self::HumanDecision => "human-decision",
        }
    }
}

/// Sensitivity level of attached content (`DATA_CLASSIFICATION.md`).
/// Only levels the resolution policy currently admits; attaching content
/// at any other level requires a policy change, not a new variant here.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvidenceClassification {
    /// Ordinary repository/project execution evidence.
    Project,
    /// Approval-chain material; decision outcomes only.
    Restricted,
}

impl EvidenceClassification {
    /// Stable wire name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Project => "PROJECT",
            Self::Restricted => "RESTRICTED",
        }
    }
}

/// One resolved evidence reference: bounded classified content bound to
/// its ref and to the exact bytes attached to the request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedEvidence {
    /// The ref this item resolves (also still present, identity-only, in
    /// the request's `evidence_refs`).
    pub evidence_ref: EvidenceRef,
    /// Trusted record kind the content came from.
    pub kind: ResolvedEvidenceKind,
    /// Sensitivity level the content crosses at.
    pub classification: EvidenceClassification,
    /// Bounded content bytes (INV-008-screened by the trusted resolver;
    /// this constructor cannot see secrets — it bounds and hashes what
    /// the resolver produced).
    pub content: String,
    /// `sha256:<hex>` over the attached content bytes.
    pub content_hash: String,
}

impl ResolvedEvidence {
    /// Builds one item: refuses blank content, truncates over-long content
    /// at the per-item cap with an explicit marker, and binds the hash to
    /// the final attached bytes.
    pub fn new(
        evidence_ref: EvidenceRef,
        kind: ResolvedEvidenceKind,
        classification: EvidenceClassification,
        content: String,
    ) -> Result<Self, PlanDomainError> {
        if content.trim().is_empty() {
            return Err(PlanDomainError::input(
                "resolved evidence content must be non-empty",
            ));
        }
        let content = bound_content(&content);
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        let content_hash = format!("sha256:{:x}", hasher.finalize());
        Ok(Self {
            evidence_ref,
            kind,
            classification,
            content,
            content_hash,
        })
    }

    /// Conservative input-token estimate for this item (bytes/4 rounded
    /// up, plus per-item framing), charged by the trusted service against
    /// the planner budget before the call.
    #[must_use]
    pub fn estimated_input_tokens(&self) -> u32 {
        let bytes = u32::try_from(self.content.len()).unwrap_or(u32::MAX);
        bytes
            .div_ceil(4)
            .saturating_add(RESOLVED_EVIDENCE_TOKENS_PER_ITEM_OVERHEAD)
    }
}

/// Total estimated input-token cost of one call's attached items.
#[must_use]
pub fn resolved_estimated_input_tokens(items: &[ResolvedEvidence]) -> u32 {
    items
        .iter()
        .map(ResolvedEvidence::estimated_input_tokens)
        .fold(0u32, |acc, next| acc.saturating_add(next))
}

/// Whether attaching `items` stays inside the per-call policy bounds.
#[must_use]
pub fn resolved_within_call_bounds(items: &[ResolvedEvidence]) -> bool {
    items.len() <= MAX_RESOLVED_EVIDENCE_ITEMS_PER_CALL
        && items.iter().map(|item| item.content.len()).sum::<usize>()
            <= MAX_RESOLVED_EVIDENCE_BYTES_PER_CALL
}

/// Truncates at the per-item byte cap on a char boundary, marking the cut.
fn bound_content(content: &str) -> String {
    if content.len() <= MAX_RESOLVED_EVIDENCE_BYTES_PER_ITEM {
        return content.to_owned();
    }
    let keep = MAX_RESOLVED_EVIDENCE_BYTES_PER_ITEM - TRUNCATION_MARKER.len();
    let mut cut = keep;
    while cut > 0 && !content.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut bounded = content[..cut].to_owned();
    bounded.push_str(TRUNCATION_MARKER);
    bounded
}
