//! Shared `background` store contract suite (TASK-0190, G11).
//! Every store implementing `RoutineStorePort` must satisfy
//! [`run_routine_store_contract`].
use background::mint::{mint_routine_id, mint_run_id};
use background::model::{CapabilityScope, RoutineDefinition, RoutineRun, RoutineState, RunStatus};
use background::port::RoutineStorePort;

pub mod fixtures {
    use super::*;
    pub fn definition(seq: u128) -> RoutineDefinition {
        RoutineDefinition::define(
            mint_routine_id(1_000, seq),
            "routine",
            "do authorized work",
            3600,
            CapabilityScope::ReadOnly,
            2,
            "agent-a",
            1_000,
        )
        .unwrap()
    }
    pub fn authorized(seq: u128) -> RoutineDefinition {
        let mut def = definition(seq);
        def.authorize("operator", 1_000).unwrap();
        def
    }
}

fn assert_define_authorize<S: RoutineStorePort>(store: &mut S) {
    let def = fixtures::definition(1);
    store.define(&def).unwrap();
    // Identical re-define is idempotent; differing payload conflicts.
    store.define(&def).unwrap();
    let mut other = fixtures::definition(1);
    other.name = "changed".to_owned();
    assert!(store.define(&other).is_err());
    assert_eq!(store.load(&def.id).unwrap(), Some(def.clone()));
    assert!(
        store.list_due(9_999).unwrap().is_empty(),
        "proposed never due"
    );
    let mut auth = def.clone();
    auth.authorize("operator", 1_000).unwrap();
    store.update(&auth).unwrap();
    assert_eq!(
        store.load(&def.id).unwrap().unwrap().state,
        RoutineState::Authorized
    );
    let due = store.list_due(2_000).unwrap();
    assert_eq!(due.len(), 1);
    assert!(store.load(&mint_routine_id(9, 9)).unwrap().is_none());
}

fn assert_run_lifecycle<S: RoutineStorePort>(store: &mut S) {
    let def = fixtures::authorized(2);
    store.define(&def).unwrap();
    let mut run = RoutineRun::start(mint_run_id(1_000, 1), def.id.clone(), 1_500).unwrap();
    store.start_run(&run).unwrap();
    assert_eq!(store.list_running().unwrap().len(), 1);
    run.complete("evidence:ok", 2_000).unwrap();
    store.update_run(&run).unwrap();
    assert!(store.list_running().unwrap().is_empty());
    let runs = store.list_runs(&def.id).unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].status, RunStatus::Completed);
    // Completion on the definition advances and exhausts.
    let mut done = def.clone();
    done.record_completion(2_000).unwrap();
    done.record_completion(3_000).unwrap();
    store.update(&done).unwrap();
    assert_eq!(
        store.load(&def.id).unwrap().unwrap().state,
        RoutineState::Exhausted
    );
    assert!(store
        .list_due(9_999)
        .unwrap()
        .iter()
        .all(|d| d.id != def.id));
}

fn assert_reopen_holds<S: RoutineStorePort>(store: &S) {
    assert!(store.load(&mint_routine_id(1_000, 1)).unwrap().is_some());
    assert!(store.load(&mint_routine_id(1_000, 2)).unwrap().is_some());
    assert_eq!(
        store.list_runs(&mint_routine_id(1_000, 2)).unwrap().len(),
        1,
        "runs must survive reopen"
    );
}

/// Runs the full routine store contract against a fresh store.
/// `reopen` must yield the same durable state (restart durability).
pub fn run_routine_store_contract<S: RoutineStorePort>(store: &mut S, reopen: impl Fn() -> S) {
    assert_define_authorize(store);
    assert_run_lifecycle(store);
    let fresh = reopen();
    assert_reopen_holds(&fresh);
}
