use app_services::background_service::{BackgroundService, DefineRequest};
use background::model::{BackgroundError, CapabilityScope, RoutineState, RunStatus};
use background_contract::fixtures;
use storage_sqlite::SqliteStore;

fn service() -> (BackgroundService<SqliteStore>, std::path::PathBuf) {
    let db =
        std::env::temp_dir().join(format!("runlith-bgsvc-{}-{}.sqlite", std::process::id(), {
            use std::sync::atomic::{AtomicU64, Ordering};
            static C: AtomicU64 = AtomicU64::new(0);
            C.fetch_add(1, Ordering::SeqCst)
        }));
    (BackgroundService::new(SqliteStore::open(&db).unwrap()), db)
}

fn request() -> DefineRequest {
    DefineRequest {
        name: "nightly".into(),
        objective: "verify state".into(),
        interval_seconds: 3600,
        capability_scope: CapabilityScope::ReadOnly,
        budget_max_runs: 2,
        author_ref: "agent-a".into(),
    }
}

#[test]
fn unauthorized_routine_never_ticks() {
    let (mut svc, db) = service();
    let def = svc.define(request(), 1_000).unwrap();
    assert_eq!(def.state, RoutineState::Proposed);
    let runs = svc.tick(9_999, "evidence:tick").unwrap();
    assert!(
        runs.is_empty(),
        "proposed routine must not run with UI closed"
    );
    std::fs::remove_file(db).ok();
}

#[test]
fn self_authorization_refused_then_operator_authorizes_and_ticks() {
    let (mut svc, db) = service();
    let def = svc.define(request(), 1_000).unwrap();
    let err = svc.authorize(&def.id, "agent-a", 1_000).unwrap_err();
    assert!(matches!(err, BackgroundError::SelfAuthorization(_)));
    svc.authorize(&def.id, "operator", 1_000).unwrap();
    let runs = svc.tick(2_000, "evidence:tick").unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].status, RunStatus::Completed);
    // Not due again until interval passes; budget 2 allows one more.
    assert!(svc.tick(3_000, "evidence:tick").unwrap().is_empty());
    let runs = svc.tick(2_000 + 3_600_000, "evidence:tick").unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(svc.get(&def.id).unwrap().state, RoutineState::Exhausted);
    assert!(svc.tick(9_999_999_999, "evidence:tick").unwrap().is_empty());
    std::fs::remove_file(db).ok();
}

#[test]
fn reconcile_interrupts_running_runs_and_draining_blocks_tick() {
    let (mut svc, db) = service();
    let def = svc.define(request(), 1_000).unwrap();
    svc.authorize(&def.id, "operator", 1_000).unwrap();
    // Simulate a crashed daemon: a RUNNING run left behind.
    let crashed = background::model::RoutineRun::start(
        background::mint::mint_run_id(1_500, 42),
        def.id.clone(),
        1_500,
    )
    .unwrap();
    {
        use background::port::RoutineStorePort;
        svc.store_mut().start_run(&crashed).unwrap();
    }
    assert_eq!(crashed.status, RunStatus::Running);
    let interrupted = svc.reconcile(2_000).unwrap();
    assert_eq!(interrupted.len(), 1);
    assert_eq!(interrupted[0].status, RunStatus::Interrupted);
    svc.begin_drain();
    assert!(matches!(
        svc.tick(9_999, "evidence:tick"),
        Err(BackgroundError::Draining(_))
    ));
    std::fs::remove_file(db).ok();
}

#[test]
fn fixtures_are_usable() {
    let _ = fixtures::definition(1);
}
