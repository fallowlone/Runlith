//! Side-effect intent durability (TASK-0126 phase b): intents
//! record at INTENT_RECORDED with an audit row, transitions apply the
//! §2 table atomically with their audit rows, illegal transitions
//! refuse (domain and trigger), one active intent exists per
//! (lease, kind), and escalations resolve exactly once.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use agent_runtime::{Execution, ExecutionId};
use storage_sqlite::SqliteStore;
use task_domain::{Project, ProjectId, Task, TaskId};
use workspace::{
    materialize_params_hash, SideEffectIntent, SideEffectIntentFields, SideEffectKind,
    SideEffectState, TransitionEvent, WorkspaceStore,
};

static COUNTER: AtomicU64 = AtomicU64::new(0);

const NOW: i64 = 1_700_000_000_000;

struct TestDb {
    path: PathBuf,
}

impl TestDb {
    fn new(name: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "runlith-side-effect-{name}-{}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self {
            path: dir.join("db.sqlite"),
        }
    }

    fn open(&self) -> SqliteStore {
        SqliteStore::open(&self.path).unwrap()
    }

    fn raw(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(&self.path).unwrap()
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::remove_dir(parent);
        }
    }
}

fn seed_lease(store: &mut SqliteStore) {
    let project = Project::new(ProjectId::new("P-SE").unwrap(), "se").unwrap();
    store.create_project(&project).unwrap();
    let task = Task::draft(
        TaskId::new("T-SE").unwrap(),
        project.id().clone(),
        "side effects",
    )
    .unwrap();
    store.create_task(&task).unwrap();
    let execution = Execution::begin(ExecutionId::new("E-SE").unwrap(), task.id().clone());
    let stamp = "2026-09-23T12:00:00Z".to_owned();
    store
        .create_execution_with_event(
            &execution,
            &storage_sqlite::NewJournalEvent {
                event_type: storage_sqlite::EventType::ExecutionCreated,
                task_id: Some(task.id().clone()),
                project_id: Some(task.project_id().clone()),
                from_state: None,
                to_state: None,
                reason: None,
                correlation_id: "side-effect-test".to_owned(),
                causation_id: "side-effect-test".to_owned(),
                occurred_at: stamp,
                provenance: "storage-sqlite/test".to_owned(),
                execution_id: Some(execution.id().clone()),
                attempt_id: None,
                attempt_outcome: None,
                workspace_id: None,
                from_workspace_state: None,
                to_workspace_state: None,
                plan_revision: None,
            },
        )
        .unwrap();
    let lease = workspace::WorkspaceLease::allocate(
        workspace::WorkspaceId::new("ws-1").unwrap(),
        workspace::RepositoryRef::new("repo-local:test").unwrap(),
        ExecutionId::new("E-SE").unwrap(),
        workspace::Revision::new("a".repeat(40)).unwrap(),
        workspace::BranchRef::new("refs/heads/agent/E-SE").unwrap(),
        "/tmp/runlith-se-test/ws-1".to_owned(),
    )
    .unwrap();
    store.insert_allocated_lease(&lease).unwrap();
}

fn intent(intent_id: &str) -> SideEffectIntent {
    SideEffectIntent::new(SideEffectIntentFields {
        intent_id: intent_id.to_owned(),
        kind: SideEffectKind::WorktreeMaterialize,
        target_ref: "ws-1".to_owned(),
        params_hash: materialize_params_hash(
            "refs/heads/agent/E-SE",
            "/tmp/runlith-se-test/ws-1",
            &"a".repeat(40),
            "ws-1",
            "E-SE",
        ),
        idempotency_key: format!("idem-{intent_id}"),
        task_id: "T-SE".to_owned(),
        execution_id: "E-SE".to_owned(),
        lease_id: "ws-1".to_owned(),
        pre_state_fingerprint: Some("branch-absent+path-absent".to_owned()),
        policy_fingerprint: "sha256:00".to_owned(),
    })
    .unwrap()
}

fn evidence(value: &str) -> Vec<String> {
    vec![value.to_owned()]
}

#[test]
fn se_1_intent_recorded_before_any_effect_with_audit() {
    let db = TestDb::new("record");
    let mut store = db.open();
    seed_lease(&mut store);
    let recorded = store
        .record_side_effect_intent(&intent("se-1"), NOW)
        .unwrap();
    assert_eq!(recorded.state(), SideEffectState::IntentRecorded);
    let restored = store
        .get_side_effect_intent("se-1")
        .unwrap()
        .expect("intent persists");
    assert_eq!(restored.state(), SideEffectState::IntentRecorded);
    assert_eq!(restored.idempotency_key(), "idem-se-1");
    let transitions = store.list_side_effect_transitions("se-1").unwrap();
    assert_eq!(transitions.len(), 1);
    assert_eq!(transitions[0].from_state, SideEffectState::Prepared);
    assert_eq!(transitions[0].to_state, SideEffectState::IntentRecorded);
}

#[test]
fn transition_applies_machine_atomically_with_audit() {
    let db = TestDb::new("transition");
    let mut store = db.open();
    seed_lease(&mut store);
    store
        .record_side_effect_intent(&intent("se-1"), NOW)
        .unwrap();
    let moved = store
        .transition_side_effect_intent("se-1", TransitionEvent::Dispatch, &[], NOW + 1)
        .unwrap();
    assert_eq!(moved.state(), SideEffectState::Executing);
    let moved = store
        .transition_side_effect_intent(
            "se-1",
            TransitionEvent::MarkAmbiguous,
            &evidence("dispatch-timeout"),
            NOW + 2,
        )
        .unwrap();
    assert_eq!(moved.state(), SideEffectState::Ambiguous);
    assert_eq!(moved.outcome_evidence_refs(), &evidence("dispatch-timeout"));
    let transitions = store.list_side_effect_transitions("se-1").unwrap();
    assert_eq!(transitions.len(), 3);
}

#[test]
fn illegal_transition_refuses_in_domain_and_trigger() {
    let db = TestDb::new("illegal");
    let mut store = db.open();
    seed_lease(&mut store);
    store
        .record_side_effect_intent(&intent("se-1"), NOW)
        .unwrap();
    assert!(store
        .transition_side_effect_intent("se-1", TransitionEvent::Confirm, &evidence("x"), NOW + 1)
        .is_err());
    let raw = db.raw();
    assert!(raw
        .execute(
            "UPDATE side_effect_intents SET state = 'CONFIRMED' WHERE intent_id = 'se-1'",
            []
        )
        .is_err());
    assert!(raw
        .execute(
            "UPDATE side_effect_intents SET kind = 'workspace-cleanup' WHERE intent_id = 'se-1'",
            []
        )
        .is_err());
    let restored = store.get_side_effect_intent("se-1").unwrap().unwrap();
    assert_eq!(restored.state(), SideEffectState::IntentRecorded);
}

#[test]
fn one_active_intent_per_lease_and_kind() {
    let db = TestDb::new("active-slot");
    let mut store = db.open();
    seed_lease(&mut store);
    store
        .record_side_effect_intent(&intent("se-1"), NOW)
        .unwrap();
    assert!(store
        .record_side_effect_intent(&intent("se-2"), NOW)
        .is_err());
    // A terminal intent frees the slot for an explicit successor.
    store
        .transition_side_effect_intent("se-1", TransitionEvent::Dispatch, &[], NOW + 1)
        .unwrap();
    store
        .transition_side_effect_intent(
            "se-1",
            TransitionEvent::Confirm,
            &evidence("adapter-postconditions"),
            NOW + 2,
        )
        .unwrap();
    store
        .record_side_effect_intent(&intent("se-2"), NOW + 3)
        .unwrap();
    let active = store
        .get_side_effect_intent_for_lease("ws-1", SideEffectKind::WorktreeMaterialize)
        .unwrap()
        .expect("successor is the active intent");
    assert_eq!(active.intent_id(), "se-2");
}

#[test]
fn escalation_requires_escalate_state_and_resolves_once() {
    let db = TestDb::new("escalate");
    let mut store = db.open();
    seed_lease(&mut store);
    store
        .record_side_effect_intent(&intent("se-1"), NOW)
        .unwrap();
    assert!(store
        .record_side_effect_escalation("se-1", "ambiguous-target", &evidence("e"), NOW + 1)
        .is_err());
    store
        .transition_side_effect_intent("se-1", TransitionEvent::Dispatch, &[], NOW + 1)
        .unwrap();
    store
        .transition_side_effect_intent(
            "se-1",
            TransitionEvent::MarkAmbiguous,
            &evidence("timeout"),
            NOW + 2,
        )
        .unwrap();
    store
        .transition_side_effect_intent("se-1", TransitionEvent::BeginReconcile, &[], NOW + 3)
        .unwrap();
    store
        .transition_side_effect_intent(
            "se-1",
            TransitionEvent::Escalate,
            &evidence("unreadable-target"),
            NOW + 4,
        )
        .unwrap();
    store
        .record_side_effect_escalation("se-1", "ambiguous-target", &evidence("e"), NOW + 5)
        .unwrap();
    store
        .resolve_side_effect_escalation("se-1", "discarded", None, NOW + 6)
        .unwrap();
    assert!(store
        .resolve_side_effect_escalation("se-1", "discarded", None, NOW + 7)
        .is_err());
}

#[test]
fn intent_state_survives_reopen() {
    let db = TestDb::new("reopen");
    {
        let mut store = db.open();
        seed_lease(&mut store);
        store
            .record_side_effect_intent(&intent("se-1"), NOW)
            .unwrap();
        store
            .transition_side_effect_intent("se-1", TransitionEvent::Dispatch, &[], NOW + 1)
            .unwrap();
    }
    let store = db.open();
    let restored = store
        .get_side_effect_intent("se-1")
        .unwrap()
        .expect("intent survives reopen");
    assert_eq!(restored.state(), SideEffectState::Executing);
    assert_eq!(
        restored.pre_state_fingerprint(),
        Some("branch-absent+path-absent")
    );
    assert_eq!(store.list_side_effect_transitions("se-1").unwrap().len(), 2);
}
