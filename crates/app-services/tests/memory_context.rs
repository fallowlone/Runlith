//! Memory -> Context integration tests (TASK-0185, ADR-0044 §5).
//!
//! Only validated memory becomes a selection candidate; selections
//! stay explainable from durable state after restart.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use app_services::{CaptureRequest, ContextService, MemoryService, SelectionRequest};
use context_engine::model::{DecisionOutcome, Sensitivity, TrustLevel};
use context_engine::selection::CharHeuristicCounter;
use memory::model::{AnchorFingerprint, AnchorKind, MemoryKind, SourceRef, SourceRefKind};
use storage_sqlite::SqliteStore;

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn fresh_db() -> PathBuf {
    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!(
        "runlith-memory-context-{}-{id}.sqlite",
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
        kind: MemoryKind::Procedural,
        content: content.to_owned(),
        source_refs: vec![SourceRef::new(SourceRefKind::Task, "TASK-1", "rev1").unwrap()],
        confidence: 900,
        sensitivity: Sensitivity::Project,
        anchors: vec![anchor("ab")],
        author_ref: "agent-a".to_owned(),
        expires_at: None,
        supersedes: None,
    }
}

fn context_service(db: &PathBuf) -> ContextService<SqliteStore, CharHeuristicCounter> {
    ContextService::new(
        SqliteStore::open(db).unwrap(),
        CharHeuristicCounter,
        5_000,
        TrustLevel::Unverified,
    )
}

#[test]
fn validated_memory_enters_selection_and_explains_after_restart() {
    let db = fresh_db();
    let mut memory = MemoryService::new(SqliteStore::open(&db).unwrap());
    let captured = memory
        .capture(request("run codegen after proto changes"), 1_000)
        .unwrap();
    // Proposed memory produces no candidates at all.
    assert!(memory.context_candidates(2_000).unwrap().is_empty());

    memory
        .validate(&captured.record.id, "reviewer", 2_000)
        .unwrap();
    let candidates = memory.context_candidates(5_000).unwrap();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].1, "run codegen after proto changes");

    let mut context = context_service(&db);
    let plan_id = context
        .admit_and_persist(SelectionRequest {
            revision_seq: 1,
            candidates,
            soft_tokens: 100,
            hard_tokens: 200,
            policy_fingerprint: "policy:fp".to_owned(),
            renderer: "renderer:plain".to_owned(),
        })
        .unwrap();
    let plan = context.load_plan(&plan_id).unwrap().unwrap();
    assert_eq!(plan.selected.len(), 1);
    let item_id = plan.selected[0].clone();

    // Explainable after restart, with the memory binding as provenance.
    drop(context);
    drop(memory);
    let context = context_service(&db);
    let (envelope, decision) = context.explain(&plan_id, &item_id).unwrap().unwrap();
    assert_eq!(decision.outcome, DecisionOutcome::Selected);
    assert!(format!("{:?}", envelope.source).contains("Memory"));
    assert!(envelope.source_rev.starts_with("sha256:"));
    std::fs::remove_file(&db).ok();
}

#[test]
fn stale_superseded_and_retired_memory_never_become_candidates() {
    let db = fresh_db();
    let mut memory = MemoryService::new(SqliteStore::open(&db).unwrap());
    let first = memory.capture(request("fact v1"), 1_000).unwrap().record;
    memory.validate(&first.id, "reviewer", 2_000).unwrap();
    assert_eq!(memory.context_candidates(3_000).unwrap().len(), 1);

    // Anchor drift -> stale -> excluded.
    assert!(memory.check_staleness(&first.id, &[anchor("cd")]).unwrap());
    assert!(memory.context_candidates(3_000).unwrap().is_empty());

    // Refresh, then supersede -> excluded, successor included.
    memory.validate(&first.id, "reviewer", 3_500).unwrap();
    let mut successor_req = request("fact v2");
    successor_req.supersedes = Some(first.id.clone());
    let successor = memory.capture(successor_req, 4_000).unwrap().record;
    memory.validate(&successor.id, "reviewer", 4_500).unwrap();
    memory.supersede(&first.id, &successor.id).unwrap();
    let candidates = memory.context_candidates(5_000).unwrap();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].1, "fact v2");

    // Retire -> excluded.
    memory.retire(&successor.id).unwrap();
    assert!(memory.context_candidates(5_000).unwrap().is_empty());
    std::fs::remove_file(&db).ok();
}
