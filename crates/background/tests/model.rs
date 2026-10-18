use background::mint::{mint_routine_id, mint_run_id};
use background::model::{
    BackgroundError, CapabilityScope, RoutineDefinition, RoutineRun, RoutineState, RunStatus,
};

fn definition(seq: u128) -> RoutineDefinition {
    RoutineDefinition::define(
        mint_routine_id(1_000, seq),
        "nightly check",
        "verify workspace state",
        3600,
        CapabilityScope::ReadOnly,
        3,
        "agent-a",
        1_000,
    )
    .unwrap()
}

#[test]
fn proposed_routine_is_never_due() {
    let def = definition(1);
    assert_eq!(def.state, RoutineState::Proposed);
    assert!(!def.is_due(9_999_999));
}

#[test]
fn author_cannot_authorize_own_routine() {
    let mut def = definition(2);
    let err = def.authorize("agent-a", 2_000).unwrap_err();
    assert!(matches!(err, BackgroundError::SelfAuthorization(_)));
    def.authorize("operator", 2_000).unwrap();
    assert_eq!(def.state, RoutineState::Authorized);
    assert!(def.is_due(2_000));
}

#[test]
fn invalid_interval_and_budget_refused() {
    assert!(RoutineDefinition::define(
        mint_routine_id(1_000, 3),
        "n",
        "o",
        10,
        CapabilityScope::ReadOnly,
        3,
        "a",
        1_000
    )
    .is_err());
    assert!(RoutineDefinition::define(
        mint_routine_id(1_000, 3),
        "n",
        "o",
        3600,
        CapabilityScope::ReadOnly,
        0,
        "a",
        1_000
    )
    .is_err());
}

#[test]
fn completion_advances_schedule_and_exhausts_budget() {
    let mut def = definition(4);
    def.authorize("operator", 1_000).unwrap();
    def.record_completion(2_000).unwrap();
    assert_eq!(def.runs_completed, 1);
    assert_eq!(def.next_run_at, 2_000 + 3_600_000);
    def.record_completion(3_000).unwrap();
    def.record_completion(4_000).unwrap();
    assert_eq!(def.state, RoutineState::Exhausted);
    assert!(!def.is_due(9_999_999));
}

#[test]
fn run_lifecycle_and_interrupt() {
    let def = definition(5);
    let mut run = RoutineRun::start(mint_run_id(1_000, 1), def.id.clone(), 1_500).unwrap();
    assert_eq!(run.status, RunStatus::Running);
    run.interrupt(2_000).unwrap();
    assert_eq!(run.status, RunStatus::Interrupted);
    let mut run2 = RoutineRun::start(mint_run_id(1_000, 2), def.id.clone(), 1_500).unwrap();
    run2.complete("evidence:ok", 2_000).unwrap();
    assert_eq!(run2.status, RunStatus::Completed);
    assert_eq!(run2.evidence_ref.as_deref(), Some("evidence:ok"));
}

#[test]
fn disabled_routine_is_not_due() {
    let mut def = definition(6);
    def.authorize("operator", 1_000).unwrap();
    def.disable().unwrap();
    assert!(!def.is_due(9_999_999));
}

#[test]
fn wire_roundtrip() {
    assert_eq!(
        CapabilityScope::parse("WORKSPACE_WRITE").unwrap(),
        CapabilityScope::WorkspaceWrite
    );
    assert!(CapabilityScope::parse("ROOT").is_err());
    assert_eq!(
        RoutineState::parse("AUTHORIZED").unwrap(),
        RoutineState::Authorized
    );
    assert_eq!(
        RunStatus::parse("INTERRUPTED").unwrap(),
        RunStatus::Interrupted
    );
}
