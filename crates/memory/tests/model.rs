use context_engine::model::Sensitivity;
use memory::mint::mint_memory_id;
use memory::model::{
    AnchorFingerprint, AnchorKind, MemoryError, MemoryKind, MemoryRecord, SourceRef, SourceRefKind,
    TrustState,
};

fn anchor(fp: &str) -> AnchorFingerprint {
    AnchorFingerprint::new(AnchorKind::Code, "crates/x/src/lib.rs", fp).unwrap()
}

fn fp(byte: &str) -> String {
    format!("sha256:{}", byte.repeat(32))
}

fn record(seq: u128) -> MemoryRecord {
    MemoryRecord::capture(
        mint_memory_id(1_000, seq),
        MemoryKind::Semantic,
        "run codegen after protobuf changes",
        vec![SourceRef::new(SourceRefKind::Task, "TASK-1", "rev1").unwrap()],
        900,
        Sensitivity::Project,
        vec![anchor(&fp("ab"))],
        "agent-a",
        1_000,
        None,
        None,
    )
    .unwrap()
}

#[test]
fn capture_refuses_secret_and_unbound_and_anchorless() {
    let secret = MemoryRecord::capture(
        mint_memory_id(1_000, 1),
        MemoryKind::Semantic,
        "x",
        vec![SourceRef::new(SourceRefKind::Task, "T", "r").unwrap()],
        1,
        Sensitivity::Secret,
        vec![anchor(&fp("ab"))],
        "a",
        1_000,
        None,
        None,
    );
    assert!(matches!(
        secret,
        Err(MemoryError::SensitivityRefusal { .. })
    ));

    let unbound = MemoryRecord::capture(
        mint_memory_id(1_000, 2),
        MemoryKind::Semantic,
        "x",
        vec![],
        1,
        Sensitivity::Project,
        vec![anchor(&fp("ab"))],
        "a",
        1_000,
        None,
        None,
    );
    assert!(matches!(unbound, Err(MemoryError::UnboundSourceRef(_))));

    let anchorless = MemoryRecord::capture(
        mint_memory_id(1_000, 3),
        MemoryKind::Procedural,
        "x",
        vec![SourceRef::new(SourceRefKind::Task, "T", "r").unwrap()],
        1,
        Sensitivity::Project,
        vec![],
        "a",
        1_000,
        None,
        None,
    );
    assert!(matches!(anchorless, Err(MemoryError::InvalidInput(_))));
}

#[test]
fn author_cannot_validate_own_memory() {
    let mut rec = record(10);
    assert!(matches!(
        rec.validate("agent-a", 2_000),
        Err(MemoryError::UntrustedPromotion(_))
    ));
    rec.validate("reviewer", 2_000).unwrap();
    assert_eq!(rec.trust_state, TrustState::Confirmed);
    assert_eq!(rec.last_verified_at, 2_000);
    assert!(rec.is_context_eligible(3_000));
}

#[test]
fn anchor_drift_marks_stale_and_blocks_eligibility() {
    let mut rec = record(11);
    rec.validate("reviewer", 2_000).unwrap();
    let changed = vec![anchor(&fp("cd"))];
    assert!(rec.mark_stale_if_anchor_changed(&changed).unwrap());
    assert_eq!(rec.trust_state, TrustState::Stale);
    assert!(!rec.is_context_eligible(3_000));
    // Refresh: revalidation restores eligibility.
    rec.validate("reviewer", 4_000).unwrap();
    assert!(rec.is_context_eligible(5_000));
}

#[test]
fn supersede_requires_confirmed_successor_naming_predecessor() {
    let mut old = record(12);
    old.validate("reviewer", 2_000).unwrap();
    let mut new = record(13);
    new.supersedes = Some(old.id.clone());
    assert!(old.supersede_by(&new).is_err()); // successor not confirmed
    new.validate("reviewer", 3_000).unwrap();
    old.supersede_by(&new).unwrap();
    assert_eq!(old.trust_state, TrustState::Superseded);
    assert!(!old.is_context_eligible(4_000));
    assert!(matches!(
        old.supersede_by(&new),
        Err(MemoryError::AlreadySuperseded(_))
    ));
}

#[test]
fn expired_and_proposed_are_ineligible_and_dedup_key_is_stable() {
    let proposed = record(14);
    assert!(!proposed.is_context_eligible(2_000));
    let again = record(15);
    assert_eq!(proposed.dedup_key(), again.dedup_key());
    let mut expiring = record(16);
    expiring.expires_at = Some(2_500);
    expiring.validate("reviewer", 2_000).unwrap();
    assert!(expiring.is_context_eligible(2_499));
    assert!(!expiring.is_context_eligible(2_500));
}

#[test]
fn wire_enums_fail_closed_and_retire_is_terminal() {
    assert!(MemoryKind::parse("NOPE").is_err());
    assert_eq!(TrustState::parse("STALE").unwrap(), TrustState::Stale);
    let mut rec = record(17);
    rec.retire().unwrap();
    assert!(rec.retire().is_err());
    assert!(rec.validate("reviewer", 2_000).is_err());
}
