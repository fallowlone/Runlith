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
//! Fail-closed by absence (conventional, not structural): `for_test` is
//! currently uncalled in production, so operator binding is unreachable
//! outside tests. IMPL-C must provide the sole production minter and
//! audit/remove `for_test` call sites.

use policy::{CandidateSelectorCatalogRevision, SelectorCatalogError};
use task_domain::{EvidenceRequirement, RequirementKind, RequirementParam, SubjectBinding};

/// Provenance binder for ADR-0036 §1a operator-authored binding.
pub const BINDER_OPERATOR: &str = "operator";

use crate::presence::FreshPresenceProof;

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
        }
    }
}

impl std::error::Error for RequirementBindingError {}

/// Binds catalog-validated candidate-input params onto an unbound
/// TestSuite requirement, minting a new revision.
///
/// The input requirement is never mutated. The policy fingerprint must
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
    let _ = proof;
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
