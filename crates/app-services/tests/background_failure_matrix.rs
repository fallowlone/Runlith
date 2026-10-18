//! TASK-0195 failure matrix (ADR-0045): crashed daemon mid-run,
//! unauthorized execution, budget exhaustion, draining, disable.
use app_services::background_service::{BackgroundService, DefineRequest};
use background::mint::mint_run_id;
use background::model::{BackgroundError, CapabilityScope, RoutineRun, RoutineState, RunStatus};
use background::port::RoutineStorePort;
use storage_sqlite::SqliteStore;

fn fresh() -> (BackgroundService<SqliteStore>, std::path::PathBuf) {
    let db = std::env::temp_dir().join(format!(
        "runlith-bgfail-{}-{}.sqlite",
        std::process::id(),
        {
            use std::sync::atomic::{AtomicU64, Ordering};
            static C: AtomicU64 = AtomicU64::new(0);
            C.fetch_add(1, Ordering::SeqCst)
        }
    ));
    (BackgroundService::new(SqliteStore::open(&db).unwrap()), db)
}

fn define(
    svc: &mut BackgroundService<SqliteStore>,
    budget: u32,
) -> background::model::RoutineDefinition {
    svc.define(
        DefineRequest {
            name: "r".into(),
            objective: "work".into(),
            interval_seconds: 60,
            capability_scope: CapabilityScope::Execution,
            budget_max_runs: budget,
            author_ref: "agent-a".into(),
        },
        1_000,
    )
    .unwrap()
}

#[test]
fn crashed_mid_run_is_interrupted_never_reexecuted() {
    let (mut svc, db) = fresh();
    let def = define(&mut svc, 3);
    svc.authorize(&def.id, "operator", 1_000).unwrap();
    let crashed = RoutineRun::start(mint_run_id(1_500, 7), def.id.clone(), 1_500).unwrap();
    svc.store_mut().start_run(&crashed).unwrap();
    drop(svc);
    // New daemon instance over the same file: reconcile, then tick at a
    // due time must produce at most the scheduled run, not a replay of
    // the crashed one (started_at 1500 stays INTERRUPTED).
    let mut svc = BackgroundService::new(SqliteStore::open(&db).unwrap());
    let interrupted = svc.reconcile(2_000).unwrap();
    assert_eq!(interrupted.len(), 1);
    assert_eq!(interrupted[0].status, RunStatus::Interrupted);
    let runs = svc.tick(2_000, "evidence:tick").unwrap();
    assert_eq!(runs.len(), 1);
    assert_ne!(runs[0].id, crashed.id);
    assert_eq!(svc.list_runs(&def.id).unwrap().len(), 2);
    std::fs::remove_file(db).ok();
}

#[test]
fn unauthorized_and_disabled_never_run() {
    let (mut svc, db) = fresh();
    let proposed = define(&mut svc, 3);
    assert!(matches!(
        svc.run_now(&proposed.id, 2_000, "e"),
        Err(BackgroundError::Unauthorized(_))
    ));
    svc.authorize(&proposed.id, "operator", 1_000).unwrap();
    svc.disable(&proposed.id).unwrap();
    assert!(matches!(
        svc.run_now(&proposed.id, 2_000, "e"),
        Err(BackgroundError::Unauthorized(_))
    ));
    assert!(svc.tick(9_999, "e").unwrap().is_empty());
    std::fs::remove_file(db).ok();
}

#[test]
fn budget_exhaustion_is_terminal() {
    let (mut svc, db) = fresh();
    let def = define(&mut svc, 1);
    svc.authorize(&def.id, "operator", 1_000).unwrap();
    svc.run_now(&def.id, 2_000, "e").unwrap();
    assert_eq!(svc.get(&def.id).unwrap().state, RoutineState::Exhausted);
    assert!(matches!(
        svc.run_now(&def.id, 3_000, "e"),
        Err(BackgroundError::BudgetExhausted(_)) | Err(BackgroundError::Unauthorized(_))
    ));
    std::fs::remove_file(db).ok();
}

#[test]
fn draining_refuses_new_runs() {
    let (mut svc, db) = fresh();
    let def = define(&mut svc, 3);
    svc.authorize(&def.id, "operator", 1_000).unwrap();
    svc.begin_drain();
    assert!(matches!(
        svc.tick(2_000, "e"),
        Err(BackgroundError::Draining(_))
    ));
    assert!(matches!(
        svc.run_now(&def.id, 2_000, "e"),
        Err(BackgroundError::Draining(_))
    ));
    std::fs::remove_file(db).ok();
}
