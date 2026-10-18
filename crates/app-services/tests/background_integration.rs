//! TASK-0194: authorized work continues with UI closed — the daemon
//! tick runs due routines with no client connected, across a restart,
//! while unauthorized work stays blocked.
use app_services::background_service::{BackgroundService, DefineRequest};
use background::model::{CapabilityScope, RoutineState, RunStatus};
use storage_sqlite::SqliteStore;

fn open(db: &std::path::Path) -> BackgroundService<SqliteStore> {
    BackgroundService::new(SqliteStore::open(db).unwrap())
}

#[test]
fn authorized_routine_runs_without_client_and_survives_restart() {
    let db =
        std::env::temp_dir().join(format!("runlith-bgint-{}-{}.sqlite", std::process::id(), {
            use std::sync::atomic::{AtomicU64, Ordering};
            static C: AtomicU64 = AtomicU64::new(0);
            C.fetch_add(1, Ordering::SeqCst)
        }));
    let id;
    {
        let mut svc = open(&db);
        let def = svc
            .define(
                DefineRequest {
                    name: "nightly".into(),
                    objective: "verify".into(),
                    interval_seconds: 3600,
                    capability_scope: CapabilityScope::ReadOnly,
                    budget_max_runs: 5,
                    author_ref: "agent-a".into(),
                },
                1_000,
            )
            .unwrap();
        id = def.id.clone();
        // No client/UI: ticking is a daemon-internal act.
        assert!(
            svc.tick(2_000, "evidence:tick").unwrap().is_empty(),
            "unauthorized must not run"
        );
        svc.authorize(&id, "operator", 1_000).unwrap();
        let runs = svc.tick(2_000, "evidence:tick").unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].status, RunStatus::Completed);
    }
    // Daemon restart: state durable, next tick waits for the interval.
    {
        let mut svc = open(&db);
        let def = svc.get(&id).unwrap();
        assert_eq!(def.state, RoutineState::Authorized);
        assert_eq!(def.runs_completed, 1);
        assert!(svc.tick(3_000, "evidence:tick").unwrap().is_empty());
        assert_eq!(svc.list_runs(&id).unwrap().len(), 1);
        let runs = svc.tick(2_000 + 3_600_000, "evidence:tick").unwrap();
        assert_eq!(runs.len(), 1);
    }
    std::fs::remove_file(db).ok();
}
