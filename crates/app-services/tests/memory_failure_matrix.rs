//! G10 failure matrix (TASK-0186): deterministic refusal and
//! degradation scenarios over real SQLite files. Shipped-binary and
//! cross-user IPC refusal evidence lives in ipc-control (v8 rejects
//! memory) and agentctl execution_binaries (memory cycle).

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use app_services::{CaptureRequest, MemoryService};
use context_engine::model::Sensitivity;
use memory::model::{
    AnchorFingerprint, AnchorKind, MemoryError, MemoryKind, MemoryRecord, SourceRef, SourceRefKind,
    TrustState,
};
use storage_sqlite::SqliteStore;

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn fresh_db() -> PathBuf {
    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!(
        "runlith-memory-matrix-{}-{id}.sqlite",
        std::process::id()
    ))
}

fn fp(byte: &str) -> String {
    format!("sha256:{}", byte.repeat(32))
}

fn anchor(byte: &str) -> AnchorFingerprint {
    AnchorFingerprint::new(AnchorKind::Code, "crates/x/src/lib.rs", fp(byte)).unwrap()
}

fn request(content: &str) -> CaptureRequest {
    CaptureRequest {
        kind: MemoryKind::Semantic,
        content: content.to_owned(),
        source_refs: vec![SourceRef::new(SourceRefKind::Task, "TASK-1", "rev1").unwrap()],
        confidence: 800,
        sensitivity: Sensitivity::Project,
        anchors: vec![anchor("ab")],
        author_ref: "agent-a".to_owned(),
        expires_at: None,
        supersedes: None,
    }
}

#[test]
fn duplicate_conflict_and_malformed_provenance_refuse() {
    let db = fresh_db();
    let mut svc = MemoryService::new(SqliteStore::open(&db).unwrap());
    let first = svc.capture(request("same fact"), 1_000).unwrap();
    assert!(!first.deduplicated);
    // Duplicate capture deduplicates instead of appending unboundedly.
    let dup = svc.capture(request("same fact"), 2_000).unwrap();
    assert!(dup.deduplicated);
    assert_eq!(dup.record.id, first.record.id);

    // Same id, different payload conflicts (direct store seam).
    let mut conflicting = first.record.clone();
    conflicting.content = "different fact".to_owned();
    conflicting.content_hash = memory::model::content_hash("different fact");
    assert!(matches!(
        svc.capture_record(&conflicting),
        Err(MemoryError::Conflict(_))
    ));

    // Malformed provenance: unbound source and bad anchor refuse.
    assert!(matches!(
        SourceRef::new(SourceRefKind::Task, "TASK-1", ""),
        Err(MemoryError::UnboundSourceRef(_))
    ));
    assert!(AnchorFingerprint::new(AnchorKind::Code, "x", "not-a-hash").is_err());
    std::fs::remove_file(&db).ok();
}

#[test]
fn expiry_conflicting_supersession_and_unknown_ids_refuse() {
    let db = fresh_db();
    let mut svc = MemoryService::new(SqliteStore::open(&db).unwrap());
    // Expired memory never becomes eligible even when confirmed.
    let mut expiring = request("short lived");
    expiring.expires_at = Some(2_500);
    let expiring = svc.capture(expiring, 1_000).unwrap().record;
    svc.validate(&expiring.id, "reviewer", 2_000).unwrap();
    assert_eq!(svc.list_eligible(2_499).unwrap().len(), 1);
    assert!(svc.list_eligible(2_500).unwrap().is_empty());

    // Conflicting supersession: successor names a different record.
    let old = svc.capture(request("old"), 3_000).unwrap().record;
    svc.validate(&old.id, "reviewer", 3_100).unwrap();
    let successor = svc.capture(request("successor"), 3_200).unwrap().record;
    svc.validate(&successor.id, "reviewer", 3_300).unwrap();
    assert!(svc.supersede(&old.id, &successor.id).is_err());
    assert_eq!(svc.get(&old.id).unwrap().trust_state, TrustState::Confirmed);

    // Unknown ids refuse typed on every mutation path.
    let missing = memory::mint::mint_memory_id(1_000, 77);
    assert!(matches!(svc.get(&missing), Err(MemoryError::NotFound(_))));
    assert!(matches!(
        svc.validate(&missing, "reviewer", 4_000),
        Err(MemoryError::NotFound(_))
    ));
    std::fs::remove_file(&db).ok();
}

#[test]
fn crash_restart_mid_lifecycle_preserves_state() {
    let db = fresh_db();
    let id = {
        let mut svc = MemoryService::new(SqliteStore::open(&db).unwrap());
        let captured = svc
            .capture(request("survives crash"), 1_000)
            .unwrap()
            .record;
        svc.validate(&captured.id, "reviewer", 2_000).unwrap();
        captured.id
        // svc dropped without graceful shutdown: durable state must hold.
    };
    let svc = MemoryService::new(SqliteStore::open(&db).unwrap());
    let loaded: MemoryRecord = svc.get(&id).unwrap();
    assert_eq!(loaded.trust_state, TrustState::Confirmed);
    assert_eq!(svc.list_eligible(3_000).unwrap().len(), 1);
    std::fs::remove_file(&db).ok();
}
