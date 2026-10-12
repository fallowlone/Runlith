//! G9 continuity service tests (TASK-0171).
//!
//! Real files only: every test owns a temporary database file, and the
//! crash-window proofs drop the service and reopen the same file.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use agent_runtime::ExecutionId;
use app_services::{CommitRequest, ContinuityService, NewNote};
use context_engine::model::{Budget, ContextPlanId, CounterMethod, Sensitivity, SourceRef};
use continuity::mint::{mint_checkpoint_id, mint_note_id};
use continuity::model::{
    Checkpoint, ContinuityError, EpochId, FormatVersion, NoteAuthor, NoteBody, WorkingNote,
};
use continuity::port::ContinuityStorePort;
use storage_sqlite::SqliteStore;
use task_domain::TaskId;

static COUNTER: AtomicU64 = AtomicU64::new(0);

const PLAN_ID: &str = "ctxplan_01ARZ3NDEKTSV4RRFFQ69G5FAV";

fn fresh_db() -> PathBuf {
    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!(
        "runlith-continuity-service-{}-{id}.sqlite",
        std::process::id()
    ))
}

fn service_for(db: &PathBuf) -> ContinuityService<SqliteStore> {
    service_for_at(db, 10_000)
}

fn service_for_at(db: &PathBuf, now_millis: u64) -> ContinuityService<SqliteStore> {
    ContinuityService::new(SqliteStore::open(db).unwrap(), now_millis)
}

fn commit_request(epoch: &EpochId) -> CommitRequest {
    CommitRequest {
        epoch: epoch.clone(),
        task: TaskId::new("T-SVC").unwrap(),
        execution: ExecutionId::new("E-SVC").unwrap(),
        format_version: FormatVersion::new(0x0001_0000),
        parent: None,
        notes: vec![NewNote {
            author: NoteAuthor::Agent,
            body: NoteBody::Inline("done: first window".to_owned()),
            sensitivity: Sensitivity::Project,
        }],
        source_refs: vec![SourceRef::repo_path("rev1", "src/lib.rs", None).unwrap()],
        evidence_refs: vec!["ev:svc".to_owned()],
        continuation: "next: second window".to_owned(),
        plan: ContextPlanId::new(PLAN_ID).unwrap(),
        ceiling: Sensitivity::Project,
    }
}

#[test]
fn stage_drop_reopen_stays_invisible() {
    let db = fresh_db();
    let mut service = service_for(&db);
    let genesis = service
        .open_chain(
            TaskId::new("T-SVC").unwrap(),
            ExecutionId::new("E-SVC").unwrap(),
        )
        .unwrap();
    let staged = Checkpoint {
        id: mint_checkpoint_id(10_000, 77),
        epoch_ref: genesis.clone(),
        task_ref: TaskId::new("T-SVC").unwrap(),
        execution_ref: ExecutionId::new("E-SVC").unwrap(),
        created_at: 10_000,
        format_version: FormatVersion::new(0x0001_0000),
        parent_checkpoint: None,
        working_state: vec![],
        source_refs: vec![],
        evidence_refs: vec![],
        continuation: String::new(),
        context_plan_ref: ContextPlanId::new(PLAN_ID).unwrap(),
        sensitivity_ceiling: Sensitivity::Public,
    };
    SqliteStore::open(&db)
        .unwrap()
        .stage_checkpoint(&staged, &[])
        .unwrap();
    assert_eq!(service.load_checkpoint(&staged.id).unwrap(), None);
    drop(service);
    let service = service_for(&db);
    assert_eq!(service.load_checkpoint(&staged.id).unwrap(), None);
    std::fs::remove_file(&db).ok();
}

#[test]
fn commit_drop_recover_completes_activation() {
    let db = fresh_db();
    let mut service = service_for(&db);
    let genesis = service
        .open_chain(
            TaskId::new("T-SVC").unwrap(),
            ExecutionId::new("E-SVC").unwrap(),
        )
        .unwrap();
    let receipt = service.commit_checkpoint(commit_request(&genesis)).unwrap();
    assert_eq!(receipt.epoch_id, genesis);
    drop(service);

    let mut service = service_for_at(&db, 11_000);
    let completed = service.recover_pending().unwrap();
    assert_eq!(completed.len(), 1);
    let active = service
        .active_epoch(
            &TaskId::new("T-SVC").unwrap(),
            &ExecutionId::new("E-SVC").unwrap(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(active.id, completed[0]);
    assert!(service.recover_pending().unwrap().is_empty());
    std::fs::remove_file(&db).ok();
}

#[test]
fn secret_notes_excluded_at_resume() {
    let db = fresh_db();
    let mut service = service_for(&db);
    let genesis = service
        .open_chain(
            TaskId::new("T-SVC").unwrap(),
            ExecutionId::new("E-SVC").unwrap(),
        )
        .unwrap();
    let mut inline_attempt = commit_request(&genesis);
    inline_attempt.notes.push(NewNote {
        author: NoteAuthor::Human,
        body: NoteBody::Inline("token: hunter2".to_owned()),
        sensitivity: Sensitivity::Secret,
    });
    inline_attempt.ceiling = Sensitivity::Secret;
    let err = service.commit_checkpoint(inline_attempt).unwrap_err();
    assert!(
        matches!(err, ContinuityError::InvalidInput(_)),
        "SECRET values never persist inline (CR-8), got {err}"
    );

    let mut request = commit_request(&genesis);
    request.notes.push(NewNote {
        author: NoteAuthor::Human,
        body: NoteBody::Ref("vault:token/7".to_owned()),
        sensitivity: Sensitivity::Secret,
    });
    request.ceiling = Sensitivity::Secret;
    let receipt = service.commit_checkpoint(request).unwrap();
    let set = service.rebuild_working_set(&receipt.checkpoint_id).unwrap();
    assert_eq!(set.notes.len(), 1, "SECRET notes never resume");
    let kept = service.load_note(&set.notes[0]).unwrap().unwrap();
    assert_eq!(kept.sensitivity, Sensitivity::Project);
    std::fs::remove_file(&db).ok();
}

#[test]
fn working_set_rebuild_shape() {
    let db = fresh_db();
    let mut service = service_for(&db);
    let genesis = service
        .open_chain(
            TaskId::new("T-SVC").unwrap(),
            ExecutionId::new("E-SVC").unwrap(),
        )
        .unwrap();
    let receipt = service.commit_checkpoint(commit_request(&genesis)).unwrap();
    let set = service.rebuild_working_set(&receipt.checkpoint_id).unwrap();
    assert_eq!(set.plan.as_str(), PLAN_ID);
    assert_eq!(set.notes.len(), 1);
    assert!(set.tool_pairs.is_empty(), "in-flight pairs never resume");
    std::fs::remove_file(&db).ok();
}

#[test]
fn gate_blocks_past_hard_and_records() {
    use continuity::rollover::BudgetVerdict;

    let db = fresh_db();
    let mut service = service_for(&db);
    let budget = Budget {
        soft_tokens: 100,
        hard_tokens: 200,
        counter: CounterMethod::CharHeuristic,
    };
    let plan = ContextPlanId::new(PLAN_ID).unwrap();
    assert_eq!(
        service
            .gate_execution_cycle(&budget, 50, None, &plan)
            .unwrap(),
        BudgetVerdict::Ok
    );
    assert_eq!(
        service
            .gate_execution_cycle(&budget, 150, None, &plan)
            .unwrap(),
        BudgetVerdict::SoftExceeded
    );
    let err = service
        .gate_execution_cycle(&budget, 250, None, &plan)
        .unwrap_err();
    assert!(matches!(err, ContinuityError::BudgetBlocked(_)));
    std::fs::remove_file(&db).ok();
}

#[test]
fn ceiling_below_note_level_refuses() {
    let db = fresh_db();
    let mut service = service_for(&db);
    let genesis = service
        .open_chain(
            TaskId::new("T-SVC").unwrap(),
            ExecutionId::new("E-SVC").unwrap(),
        )
        .unwrap();
    let mut request = commit_request(&genesis);
    request.ceiling = Sensitivity::Public;
    let err = service.commit_checkpoint(request).unwrap_err();
    assert!(matches!(err, ContinuityError::InvalidInput(_)));
    std::fs::remove_file(&db).ok();
}

#[test]
fn note_fixture_shape() {
    let noted = WorkingNote {
        id: mint_note_id(10_000, 3),
        epoch_ref: mint_epoch_id_for_test(),
        created_at: 10_000,
        author: NoteAuthor::System,
        body: NoteBody::Ref("artifact:log/7".to_owned()),
        sensitivity: Sensitivity::Private,
    };
    assert!(noted.validate().is_ok());
}

fn mint_epoch_id_for_test() -> EpochId {
    continuity::mint::mint_epoch_id(10_000, 9)
}
