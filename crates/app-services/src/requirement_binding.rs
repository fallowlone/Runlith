//! Trusted `EvidenceRequirement` parameter binding boundary (ADR-0036 §1a,
//! IMPL-A).
//!
//! This module is the only code path permitted to construct a
//! non-empty-params requirement for candidate-input evidence. It validates
//! every proposed value against the Policy-owned closed catalog, requires
//! B1-grade fresh presence authority for operator-authored binding (D1),
//! and mints a new requirement revision (Option 3a) — never a mutation in
//! place, never a re-binding of an already-bound requirement.
//!
//! Fail-closed by authority (TASK-0160 IMPL-C): the proof binds the
//! exact operation plus the exact input digest, and anything else
//! refuses — including `for_test` proofs, which bind no binding
//! operation. Durable nonce single-use is enforced by the store at
//! persistence time; this function owns the pure checks.

use policy::{CandidateSelectorCatalogRevision, SelectorCatalogError};
use sha2::{Digest, Sha256};
use task_domain::{EvidenceRequirement, RequirementKind, RequirementParam, SubjectBinding};

use crate::presence::{FreshPresenceProof, PRESENCE_FRESHNESS_MS};

/// Provenance binder for ADR-0036 §1a operator-authored binding.
pub const BINDER_OPERATOR: &str = "operator";

/// Operator-proposed binding values. Untrusted input: every field is
/// validated against the catalog before anything is persisted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProposedCandidateBinding {
    pub selector: String,
    pub path: String,
    pub literal: String,
}

/// Durable who/what record for a binding (ADR-0036 Option 3a).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BindingProvenance {
    binder: String,
    catalog_revision: u64,
    catalog_fingerprint: String,
    bound_at_ms: u64,
}

impl BindingProvenance {
    pub fn binder(&self) -> &str {
        &self.binder
    }
    pub fn catalog_revision(&self) -> u64 {
        self.catalog_revision
    }
    pub fn catalog_fingerprint(&self) -> &str {
        &self.catalog_fingerprint
    }
    pub fn bound_at_ms(&self) -> u64 {
        self.bound_at_ms
    }
}

/// A newly minted bound requirement revision with its provenance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundEvidenceRequirement {
    pub requirement: EvidenceRequirement,
    pub provenance: BindingProvenance,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RequirementBindingError {
    SelectorCatalog(SelectorCatalogError),
    RequirementAlreadyBound,
    InvalidRequirementShape(String),
    InvalidPolicyFingerprint,
    ProofBindingMismatch { field: &'static str },
    StaleProof,
}

impl std::fmt::Display for RequirementBindingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SelectorCatalog(error) => write!(f, "catalog rejected binding: {error}"),
            Self::RequirementAlreadyBound => {
                f.write_str("requirement already carries bound params")
            }
            Self::InvalidRequirementShape(detail) => {
                write!(f, "requirement cannot carry candidate binding: {detail}")
            }
            Self::InvalidPolicyFingerprint => {
                f.write_str("policy fingerprint must be sha256:<64 lowercase hex>")
            }
            Self::ProofBindingMismatch { field } => {
                write!(f, "presence proof does not bind this binding: {field}")
            }
            Self::StaleProof => f.write_str("presence proof is stale"),
        }
    }
}

impl std::error::Error for RequirementBindingError {}

/// Exact operation string a binding proof binds: the criterion plus
/// the catalogued selector and confined path (the binding address).
/// Values are hashed because criterion ids may carry spaces, which
/// can never appear in a valid operation token.
pub fn binding_operation(criterion_id: &str, selector: &str, path: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(b"candidate-binding-op/v1\0");
    hash.update(criterion_id.as_bytes());
    hash.update(b"\0");
    hash.update(selector.as_bytes());
    hash.update(b"\0");
    hash.update(path.as_bytes());
    format!("candidate-binding:{:x}", hash.finalize())
}

/// Exact input digest a binding proof binds: every approved value,
/// including the literal and policy fingerprint the operation
/// address omits. A swapped literal or policy refuses here.
pub fn binding_input_digest(
    criterion_id: &str,
    selector: &str,
    path: &str,
    literal: &str,
    policy_fingerprint: &str,
) -> String {
    let mut hash = Sha256::new();
    hash.update(b"candidate-binding-input/v1\0");
    hash.update(criterion_id.as_bytes());
    hash.update(b"\0");
    hash.update(selector.as_bytes());
    hash.update(b"\0");
    hash.update(path.as_bytes());
    hash.update(b"\0");
    hash.update(literal.as_bytes());
    hash.update(b"\0");
    hash.update(policy_fingerprint.as_bytes());
    format!("sha256:{:x}", hash.finalize())
}

/// Binds catalog-validated candidate-input params onto an unbound
/// TestSuite requirement, minting a new revision.
///
/// Authority first: the proof must bind this exact operation and
/// input digest and be fresh relative to `bound_at_ms` (which doubles
/// as the verification instant) — a missing, foreign, swapped, stale,
/// or future proof refuses before any value is even validated. The
/// input requirement is never mutated. The policy fingerprint must
/// already be in the `sha256:` shape the TASK-0144 evaluator requires —
/// anything else could never verify, so it fails here, not downstream.
pub fn bind_candidate_input_params(
    catalog: &CandidateSelectorCatalogRevision,
    requirement: &EvidenceRequirement,
    proposed: &ProposedCandidateBinding,
    proof: &FreshPresenceProof,
    policy_fingerprint: &str,
    bound_at_ms: u64,
) -> Result<BoundEvidenceRequirement, RequirementBindingError> {
    let operation = binding_operation(
        &requirement.criterion_id,
        &proposed.selector,
        &proposed.path,
    );
    if proof.operation() != operation {
        return Err(RequirementBindingError::ProofBindingMismatch { field: "operation" });
    }
    let digest = binding_input_digest(
        &requirement.criterion_id,
        &proposed.selector,
        &proposed.path,
        &proposed.literal,
        policy_fingerprint,
    );
    if proof.input_digest() != digest {
        return Err(RequirementBindingError::ProofBindingMismatch { field: "input" });
    }
    if proof.proven_at_ms() > bound_at_ms
        || bound_at_ms - proof.proven_at_ms() > PRESENCE_FRESHNESS_MS
    {
        return Err(RequirementBindingError::StaleProof);
    }
    if !requirement.params.is_empty() {
        return Err(RequirementBindingError::RequirementAlreadyBound);
    }
    if requirement.requirement_kind != RequirementKind::TestSuite || !requirement.required {
        return Err(RequirementBindingError::InvalidRequirementShape(
            "candidate binding requires a required TestSuite requirement".into(),
        ));
    }
    if requirement.subject_binding != SubjectBinding::ContentToolchainAndConfig {
        return Err(RequirementBindingError::InvalidRequirementShape(
            "candidate binding requires content+toolchain+config subject binding".into(),
        ));
    }
    if !valid_policy_fingerprint(policy_fingerprint) {
        return Err(RequirementBindingError::InvalidPolicyFingerprint);
    }
    let bound = catalog
        .catalog()
        .validate_binding(&proposed.selector, &proposed.path, &proposed.literal)
        .map_err(RequirementBindingError::SelectorCatalog)?;
    let params = [
        RequirementParam::new("literal", bound.literal().to_owned()),
        RequirementParam::new("path", bound.path().to_owned()),
        RequirementParam::new("selector", bound.selector().to_owned()),
    ];
    let mut collected = Vec::with_capacity(3);
    for param in params {
        collected.push(param.map_err(|error| {
            RequirementBindingError::InvalidRequirementShape(error.to_string())
        })?);
    }
    // `EvidenceRequirement::new` sorts params by key: literal < path <
    // selector — the exact order the evaluator indexes positionally.
    let minted = EvidenceRequirement::new(
        requirement.criterion_id.clone(),
        requirement.requirement_kind,
        requirement.subject_binding,
        collected,
        Some(policy_fingerprint.to_owned()),
        requirement.required,
    )
    .map_err(|error| RequirementBindingError::InvalidRequirementShape(error.to_string()))?;
    Ok(BoundEvidenceRequirement {
        requirement: minted,
        provenance: BindingProvenance {
            binder: BINDER_OPERATOR.to_owned(),
            catalog_revision: catalog.revision(),
            catalog_fingerprint: catalog.fingerprint().to_owned(),
            bound_at_ms,
        },
    })
}

/// The `sha256:<64 lowercase hex>` shape `CandidateSelector::from_requirement`
/// requires. Mirrors the evaluator; the boundary refuses anything else up
/// front so an unbound requirement can never become unverifiable output.
fn valid_policy_fingerprint(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}
