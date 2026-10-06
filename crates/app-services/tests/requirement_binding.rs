//! TASK-0157 (ADR-0036 IMPL-A): trusted binding boundary contract tests.
//!
//! The boundary is the only code path permitted to construct a non-empty-
//! params `EvidenceRequirement` for candidate-input evidence (ADR-0036 §1a).
//! Binding requires B1-grade fresh presence authority (D1), validates every
//! value against the closed catalog, and mints a new requirement revision
//! (Option 3a) — never a mutation in place.

use app_services::{
    bind_candidate_input_params, binding_input_digest, binding_operation, FreshPresenceProof,
    PresenceAuthority, ProposedCandidateBinding, RequirementBindingError, TestPresenceAuthority,
};
use policy::{CandidateSelectorCatalog, CandidateSelectorCatalogRevision};
use task_domain::{EvidenceRequirement, RequirementKind, SubjectBinding};

const SELECTOR_V1: &str = "candidate.file-contains.v1";
const POLICY_FINGERPRINT: &str =
    "sha256:9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";

fn catalog_rev() -> CandidateSelectorCatalogRevision {
    CandidateSelectorCatalogRevision::new(1, CandidateSelectorCatalog::v1()).unwrap()
}

fn unbound_requirement() -> EvidenceRequirement {
    EvidenceRequirement::new(
        "criterion-1",
        RequirementKind::TestSuite,
        RequirementKind::TestSuite.default_subject_binding(),
        Vec::new(),
        Some("policy:v2".to_owned()),
        true,
    )
    .unwrap()
}

fn proposal() -> ProposedCandidateBinding {
    ProposedCandidateBinding {
        selector: SELECTOR_V1.to_owned(),
        path: "src/lib.rs".to_owned(),
        literal: "needle".to_owned(),
    }
}

fn proof_for(
    criterion_id: &str,
    proposed: &ProposedCandidateBinding,
    policy_fingerprint: &str,
    at_ms: u64,
) -> FreshPresenceProof {
    TestPresenceAuthority::new(at_ms)
        .prove(
            &binding_operation(criterion_id, &proposed.selector, &proposed.path),
            &binding_input_digest(
                criterion_id,
                &proposed.selector,
                &proposed.path,
                &proposed.literal,
                policy_fingerprint,
            ),
        )
        .unwrap()
}

fn valid_proof() -> FreshPresenceProof {
    proof_for(
        "criterion-1",
        &proposal(),
        POLICY_FINGERPRINT,
        1_700_000_000_000,
    )
}

#[test]
fn bind_mints_new_revision_with_sorted_params_and_provenance() {
    let unbound = unbound_requirement();
    let bound = bind_candidate_input_params(
        &catalog_rev(),
        &unbound,
        &proposal(),
        &valid_proof(),
        POLICY_FINGERPRINT,
        1_700_000_000_000,
    )
    .unwrap();

    // New revision: the input stays exactly as it was (Option 3a).
    assert!(unbound.params.is_empty());
    let requirement = &bound.requirement;
    assert_eq!(requirement.criterion_id, "criterion-1");
    assert_eq!(requirement.requirement_kind, RequirementKind::TestSuite);
    assert_eq!(
        requirement.subject_binding,
        SubjectBinding::ContentToolchainAndConfig
    );
    assert!(requirement.required);
    // Exact key order the TASK-0144 evaluator indexes positionally.
    let keys: Vec<&str> = requirement.params.iter().map(|param| param.key()).collect();
    assert_eq!(keys, vec!["literal", "path", "selector"]);
    assert_eq!(requirement.params[0].value(), "needle");
    assert_eq!(requirement.params[1].value(), "src/lib.rs");
    assert_eq!(requirement.params[2].value(), SELECTOR_V1);
    assert_eq!(
        requirement.policy_fingerprint.as_deref(),
        Some(POLICY_FINGERPRINT)
    );

    let provenance = &bound.provenance;
    assert_eq!(provenance.binder(), "operator");
    assert_eq!(provenance.catalog_revision(), 1);
    assert_eq!(
        provenance.catalog_fingerprint(),
        catalog_rev().fingerprint()
    );
    assert_eq!(provenance.bound_at_ms(), 1_700_000_000_000);
}

#[test]
fn bind_rejects_already_bound_requirement() {
    let bound_once = bind_candidate_input_params(
        &catalog_rev(),
        &unbound_requirement(),
        &proposal(),
        &valid_proof(),
        POLICY_FINGERPRINT,
        1_700_000_000_000,
    )
    .unwrap();
    let error = bind_candidate_input_params(
        &catalog_rev(),
        &bound_once.requirement,
        &proposal(),
        &valid_proof(),
        POLICY_FINGERPRINT,
        1_700_000_000_001,
    )
    .unwrap_err();
    assert_eq!(error, RequirementBindingError::RequirementAlreadyBound);
}

#[test]
fn bind_rejects_non_testsuite_shape() {
    // The evaluator only resolves TestSuite/content+toolchain+config/required
    // requirements; anything else stays on the manual path, never bound.
    let mut requirement = unbound_requirement();
    requirement.requirement_kind = RequirementKind::ProcessExit;
    let error = bind_candidate_input_params(
        &catalog_rev(),
        &requirement,
        &proposal(),
        &valid_proof(),
        POLICY_FINGERPRINT,
        1_700_000_000_000,
    )
    .unwrap_err();
    assert!(matches!(
        error,
        RequirementBindingError::InvalidRequirementShape(_)
    ));

    let mut requirement = unbound_requirement();
    requirement.required = false;
    let error = bind_candidate_input_params(
        &catalog_rev(),
        &requirement,
        &proposal(),
        &valid_proof(),
        POLICY_FINGERPRINT,
        1_700_000_000_000,
    )
    .unwrap_err();
    assert!(matches!(
        error,
        RequirementBindingError::InvalidRequirementShape(_)
    ));

    let mut requirement = unbound_requirement();
    requirement.subject_binding = SubjectBinding::Content;
    let error = bind_candidate_input_params(
        &catalog_rev(),
        &requirement,
        &proposal(),
        &valid_proof(),
        POLICY_FINGERPRINT,
        1_700_000_000_000,
    )
    .unwrap_err();
    assert!(matches!(
        error,
        RequirementBindingError::InvalidRequirementShape(_)
    ));
}

#[test]
fn bind_rejects_non_sha256_policy_fingerprint() {
    // The evaluator requires `sha256:<64 lowercase hex>`; anything else
    // could never verify, so the boundary refuses it up front.
    for fingerprint in [
        "",
        "policy:v2",
        "sha256:xyz",
        "SHA256:9F86D081884C7D659A2FEAA0C55AD015A3BF4F1B2B0B822CD15D6C15B0F00A08",
    ] {
        let error = bind_candidate_input_params(
            &catalog_rev(),
            &unbound_requirement(),
            &proposal(),
            &proof_for("criterion-1", &proposal(), fingerprint, 1_700_000_000_000),
            fingerprint,
            1_700_000_000_000,
        )
        .unwrap_err();
        assert_eq!(error, RequirementBindingError::InvalidPolicyFingerprint);
    }
}

#[test]
fn bind_relays_catalog_rejections() {
    let hostile = ProposedCandidateBinding {
        selector: SELECTOR_V1.to_owned(),
        path: "../escape.rs".to_owned(),
        literal: "needle".to_owned(),
    };
    let error = bind_candidate_input_params(
        &catalog_rev(),
        &unbound_requirement(),
        &hostile,
        &proof_for(
            "criterion-1",
            &hostile,
            POLICY_FINGERPRINT,
            1_700_000_000_000,
        ),
        POLICY_FINGERPRINT,
        1_700_000_000_000,
    )
    .unwrap_err();
    assert!(matches!(error, RequirementBindingError::SelectorCatalog(_)));

    let uncatalogued = ProposedCandidateBinding {
        selector: "candidate.evil.v1".to_owned(),
        path: "src/lib.rs".to_owned(),
        literal: "needle".to_owned(),
    };
    let error = bind_candidate_input_params(
        &catalog_rev(),
        &unbound_requirement(),
        &uncatalogued,
        &proof_for(
            "criterion-1",
            &uncatalogued,
            POLICY_FINGERPRINT,
            1_700_000_000_000,
        ),
        POLICY_FINGERPRINT,
        1_700_000_000_000,
    )
    .unwrap_err();
    assert!(matches!(error, RequirementBindingError::SelectorCatalog(_)));

    let secret = ProposedCandidateBinding {
        selector: SELECTOR_V1.to_owned(),
        path: "src/lib.rs".to_owned(),
        literal: "password=hunter2".to_owned(),
    };
    let error = bind_candidate_input_params(
        &catalog_rev(),
        &unbound_requirement(),
        &secret,
        &proof_for(
            "criterion-1",
            &secret,
            POLICY_FINGERPRINT,
            1_700_000_000_000,
        ),
        POLICY_FINGERPRINT,
        1_700_000_000_000,
    )
    .unwrap_err();
    assert!(matches!(error, RequirementBindingError::SelectorCatalog(_)));
}

#[test]
fn bind_rejects_test_proof_without_exact_operation() {
    // TASK-0160 (IMPL-C): the TASK-0157 `let _ = proof` bypass is dead —
    // a proof that binds no exact binding operation refuses.
    let error = bind_candidate_input_params(
        &catalog_rev(),
        &unbound_requirement(),
        &proposal(),
        &FreshPresenceProof::for_test(),
        POLICY_FINGERPRINT,
        1_700_000_000_000,
    )
    .unwrap_err();
    assert!(matches!(
        error,
        RequirementBindingError::ProofBindingMismatch { .. }
    ));
}

#[test]
fn bind_rejects_proof_for_another_operation() {
    // Presence spoof: a proof minted for a different criterion (or a
    // B1 review operation) cannot authorize this binding.
    let proposed = proposal();
    let foreign = TestPresenceAuthority::new(1_700_000_000_000)
        .prove(
            &binding_operation("criterion-OTHER", &proposed.selector, &proposed.path),
            &binding_input_digest(
                "criterion-OTHER",
                &proposed.selector,
                &proposed.path,
                &proposed.literal,
                POLICY_FINGERPRINT,
            ),
        )
        .unwrap();
    let error = bind_candidate_input_params(
        &catalog_rev(),
        &unbound_requirement(),
        &proposal(),
        &foreign,
        POLICY_FINGERPRINT,
        1_700_000_000_000,
    )
    .unwrap_err();
    assert!(matches!(
        error,
        RequirementBindingError::ProofBindingMismatch { .. }
    ));
}

#[test]
fn bind_rejects_proof_with_swapped_input_digest() {
    // Widening: the proof binds different values (another literal or
    // policy) than the proposed binding — the digest refuses.
    let proposed = proposal();
    let swapped = TestPresenceAuthority::new(1_700_000_000_000)
        .prove(
            &binding_operation("criterion-1", &proposed.selector, &proposed.path),
            &binding_input_digest(
                "criterion-1",
                &proposed.selector,
                &proposed.path,
                "another-needle",
                POLICY_FINGERPRINT,
            ),
        )
        .unwrap();
    let error = bind_candidate_input_params(
        &catalog_rev(),
        &unbound_requirement(),
        &proposal(),
        &swapped,
        POLICY_FINGERPRINT,
        1_700_000_000_000,
    )
    .unwrap_err();
    assert!(matches!(
        error,
        RequirementBindingError::ProofBindingMismatch { .. }
    ));
}

#[test]
fn bind_rejects_stale_and_future_proof() {
    let proposed = proposal();
    let operation = binding_operation("criterion-1", &proposed.selector, &proposed.path);
    let digest = binding_input_digest(
        "criterion-1",
        &proposed.selector,
        &proposed.path,
        &proposed.literal,
        POLICY_FINGERPRINT,
    );
    let stale = TestPresenceAuthority::new(1_700_000_000_000)
        .prove(&operation, &digest)
        .unwrap();
    let error = bind_candidate_input_params(
        &catalog_rev(),
        &unbound_requirement(),
        &proposal(),
        &stale,
        POLICY_FINGERPRINT,
        1_700_000_000_000 + 600_000,
    )
    .unwrap_err();
    assert!(matches!(error, RequirementBindingError::StaleProof));
    let future = TestPresenceAuthority::new(1_700_000_000_000 + 600_000)
        .prove(&operation, &digest)
        .unwrap();
    let error = bind_candidate_input_params(
        &catalog_rev(),
        &unbound_requirement(),
        &proposal(),
        &future,
        POLICY_FINGERPRINT,
        1_700_000_000_000,
    )
    .unwrap_err();
    assert!(matches!(error, RequirementBindingError::StaleProof));
}

#[test]
fn bound_requirement_is_consumable_by_the_task_0144_evaluator() {
    // TASK-0159 (ADR-0036 IMPL-D): the boundary output shape is exactly
    // what CandidateSelector::from_requirement accepts — same keys,
    // order, selector id, and requirement kind/binding/fingerprint.
    let bound = bind_candidate_input_params(
        &catalog_rev(),
        &unbound_requirement(),
        &proposal(),
        &valid_proof(),
        POLICY_FINGERPRINT,
        1_700_000_000_000,
    )
    .unwrap();
    let selector = evaluation::CandidateSelector::from_requirement(&bound.requirement)
        .expect("boundary-minted requirement must satisfy the evaluator");
    assert_eq!(selector.criterion_id, "criterion-1");
    assert_eq!(selector.selector, "candidate.file-contains.v1");
    assert_eq!(selector.path, "src/lib.rs");
    assert_eq!(selector.literal, "needle");
    assert!(selector.requirement_revision.starts_with("sha256:"));
}
