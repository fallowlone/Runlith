//! Memory service lifecycle tests (TASK-0183) over real SQLite files.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use app_services::{CaptureRequest, MemoryService};
use context_engine::model::Sensitivity;
use memory::model::{
    AnchorFingerprint, AnchorKind, MemoryError, MemoryKind, SourceRef, SourceRefKind, TrustState,
};
use memory::port::MemoryStorePort;
use storage_sqlite::SqliteStore;

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn fresh_db() -> PathBuf {
    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!(
        "runlith-memory-service-{}-{id}.sqlite",
        std::process::id()
    ))
}

fn fp(byte: &str) -> String {
    format!("sha256:{}", byte.repeat(32))
}

fn anchor(byte: &str) -> AnchorFingerprint {
    AnchorFingerprint::new(AnchorKind::Code, "crates/x/src/lib.rs", fp(byte)).unwrap()
}

fn request(content: &str, author: &str) -> CaptureRequest {
    CaptureRequest {
        kind: MemoryKind::Semantic,
        content: content.to_owned(),
        source_refs: vec![SourceRef::new(SourceRefKind::Task, "TASK-1", "rev1").unwrap()],
        confidence: 900,
        sensitivity: Sensitivity::Project,
        anchors: vec![anchor("ab")],
        author_ref: author.to_owned(),
        expires_at: None,
        supersedes: None,
    }
}

#[test]
fn full_lifecycle_over_sqlite() {
    let db = fresh_db();
    let mut svc = MemoryService::new(SqliteStore::open(&db).unwrap());

    let captured = svc.capture(request("fact one", "agent-a"), 1_000).unwrap();
    assert!(!captured.deduplicated);
    let id = captured.record.id.clone();
    // Proposed memory is never eligible.
    assert!(svc.list_eligible(2_000).unwrap().is_empty());

    // Author self-validation refuses (INV-029); a reviewer succeeds.
    assert!(matches!(
        svc.validate(&id, "agent-a", 2_000),
        Err(MemoryError::UntrustedPromotion(_))
    ));
    let confirmed = svc.validate(&id, "reviewer", 2_000).unwrap();
    assert_eq!(confirmed.trust_state, TrustState::Confirmed);
    assert_eq!(svc.list_eligible(3_000).unwrap().len(), 1);

    // Re-capture of the same fact deduplicates.
    let dup = svc.capture(request("fact one", "agent-a"), 4_000).unwrap();
    assert!(dup.deduplicated);
    assert_eq!(dup.record.id, id);

    // Anchor drift marks stale and removes eligibility.
    assert!(svc.check_staleness(&id, &[anchor("cd")]).unwrap());
    assert!(svc.list_eligible(5_000).unwrap().is_empty());

    // Supersession requires a confirmed successor naming the old id.
    let mut successor_req = request("fact one v2", "agent-b");
    successor_req.supersedes = Some(id.clone());
    let successor = svc.capture(successor_req, 6_000).unwrap().record;
    assert!(svc.supersede(&id, &successor.id).is_err());
    svc.validate(&successor.id, "reviewer", 7_000).unwrap();
    let old = svc.supersede(&id, &successor.id).unwrap();
    assert_eq!(old.trust_state, TrustState::Superseded);
    let eligible = svc.list_eligible(8_000).unwrap();
    assert_eq!(eligible.len(), 1);
    assert_eq!(eligible[0].id, successor.id);
    // History remains loadable by exact id after restart.
    drop(svc);
    let svc = MemoryService::new(SqliteStore::open(&db).unwrap());
    assert_eq!(svc.get(&id).unwrap().trust_state, TrustState::Superseded);
    std::fs::remove_file(&db).ok();
}

#[test]
fn secret_capture_and_unknown_id_refuse() {
    let db = fresh_db();
    let mut svc = MemoryService::new(SqliteStore::open(&db).unwrap());
    let mut secret = request("hidden", "agent-a");
    secret.sensitivity = Sensitivity::Secret;
    assert!(matches!(
        svc.capture(secret, 1_000),
        Err(MemoryError::SensitivityRefusal { .. })
    ));
    let missing = memory::mint::mint_memory_id(1_000, 99);
    assert!(matches!(svc.get(&missing), Err(MemoryError::NotFound(_))));
    // Direct store port remains reachable for composition checks.
    assert!(svc.store().list_eligible(2_000).unwrap().is_empty());
    std::fs::remove_file(&db).ok();
}
