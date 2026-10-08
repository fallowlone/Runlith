//! G9 context-selection service tests (TASK-0170).
//!
//! Real files only: every test owns a temporary database file, and the
//! durability proof drops the service and reopens the same file.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use app_services::{ContextService, SelectionRequest};
use context_engine::mint::mint_item_id;
use context_engine::model::*;
use context_engine::selection::CharHeuristicCounter;
use storage_sqlite::SqliteStore;

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn fresh_db() -> PathBuf {
    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!(
        "runlith-context-service-{}-{}.sqlite",
        std::process::id(),
        id
    ))
}

const HASH: &str = "sha256:9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";

fn item(seq: u128, priority: u16, required: bool, sensitivity: Sensitivity) -> ContextItem {
    ContextItem {
        id: mint_item_id(1000, seq),
        kind: ContextKind::FileExcerpt,
        source_ref: SourceRef::repo_path("rev1", "src/lib.rs", None).unwrap(),
        content_hash: ContentHash::new(HASH).unwrap(),
        priority,
        freshness: Freshness {
            observed_at: 1000,
            source_rev: "rev1".to_owned(),
            stale_after: None,
        },
        trust: Trust {
            level: TrustLevel::Unverified,
            reason: "fixture".to_owned(),
        },
        sensitivity,
        required,
        cache_class: CacheClass::SessionEphemeral,
        provenance: ProvenanceEnvelope::new(
            SourceRef::repo_path("rev1", "src/lib.rs", None).unwrap(),
            "rev1",
            1000,
            "fixture-selector",
            "scope:test",
            800,
        )
        .unwrap(),
    }
}

fn request(candidates: Vec<(ContextItem, String)>) -> SelectionRequest {
    SelectionRequest {
        revision_seq: 1,
        candidates,
        soft_tokens: 100,
        hard_tokens: 200,
        policy_fingerprint: "policy:fp".to_owned(),
        renderer: "renderer:plain".to_owned(),
    }
}

fn service_for(db: &PathBuf) -> ContextService<SqliteStore, CharHeuristicCounter> {
    ContextService::new(
        SqliteStore::open(db).unwrap(),
        CharHeuristicCounter,
        1000,
        TrustLevel::Unverified,
    )
}

#[test]
fn admit_persist_explain_roundtrip() {
    let db = fresh_db();
    let mut service = service_for(&db);
    let req = item(1, 1, true, Sensitivity::Public);
    let hi = item(2, 90, false, Sensitivity::Public);
    let plan_id = service
        .admit_and_persist(request(vec![
            (req.clone(), "a".repeat(40)),
            (hi.clone(), "b".repeat(40)),
        ]))
        .unwrap();

    let plan = service.load_plan(&plan_id).unwrap().unwrap();
    assert_eq!(plan.required, vec![req.id.clone()]);
    assert_eq!(plan.selected, vec![hi.id.clone()]);

    let (envelope, decision) = service.explain(&plan_id, &hi.id).unwrap().unwrap();
    assert_eq!(envelope, hi.provenance);
    assert_eq!(decision.outcome, DecisionOutcome::Selected);
    std::fs::remove_file(&db).ok();
}

#[test]
fn secret_is_refused_and_never_selected() {
    let db = fresh_db();
    let mut service = service_for(&db);
    let secret = item(1, 255, true, Sensitivity::Secret);
    let plan_id = service
        .admit_and_persist(request(vec![(secret.clone(), "s".repeat(10))]))
        .unwrap();
    let plan = service.load_plan(&plan_id).unwrap().unwrap();
    assert!(plan.required.is_empty());
    assert!(plan.selected.is_empty());
    assert_eq!(
        plan.rejected,
        vec![(secret.id.clone(), ReasonCode::SensitivityRefusal)]
    );
    std::fs::remove_file(&db).ok();
}

#[test]
fn required_over_hard_refuses() {
    let db = fresh_db();
    let mut service = service_for(&db);
    let big = item(1, 1, true, Sensitivity::Public);
    let err = service
        .admit_and_persist(request(vec![(big, "x".repeat(4000))]))
        .unwrap_err();
    assert!(matches!(err, ContextError::BudgetExceeded { .. }));
    std::fs::remove_file(&db).ok();
}

#[test]
fn selection_survives_service_restart() {
    let db = fresh_db();
    let plan_id = {
        let mut service = service_for(&db);
        let req = item(1, 1, true, Sensitivity::Public);
        service
            .admit_and_persist(request(vec![(req, "a".repeat(40))]))
            .unwrap()
    };
    let service = service_for(&db);
    let plan = service.load_plan(&plan_id).unwrap();
    assert!(plan.is_some());
    assert_eq!(
        service.load_trace(&plan_id).unwrap().unwrap().plan_id,
        plan_id
    );
    std::fs::remove_file(&db).ok();
}
