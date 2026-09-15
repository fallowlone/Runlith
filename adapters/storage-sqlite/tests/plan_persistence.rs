//! TASK-0077 contract tests: durable Plan snapshots + atomic revision journal.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use plan_domain::{
    compute_proposal_hash, BudgetDelta, CriterionDecl, CriterionId, EdgeAddition, EvidenceRef,
    GateRef, NodeAddition, PlanBranchId, PlanChangeSet, PlanGraph, PlanNode, PlanNodeId,
    PlanRevision, PlannerAdapterId, PlannerIdentity, PlanningCostReceipt, PlanningCostReceiptRef,
    RevisionProposedBy, RiskClass, ValidatedBy, VerificationMethod,
};
use storage_sqlite::{
    EventType, PlanCommitJournalEvidence, SqliteStore, StorageError, SUPPORTED_SCHEMA_VERSION,
};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct TestDb {
    path: PathBuf,
}

impl TestDb {
    fn new(suite: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "runlith-plan-storage-test-{}-{suite}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self {
            path: dir.join("plan.db"),
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

fn nid(value: &str) -> PlanNodeId {
    PlanNodeId::new(value).unwrap()
}

fn cid(value: &str) -> CriterionId {
    CriterionId::new(value).unwrap()
}

fn evidence(value: &str) -> EvidenceRef {
    EvidenceRef::new(value).unwrap()
}

fn identity() -> PlannerIdentity {
    PlannerIdentity::new(
        PlannerAdapterId::new("planner-adapter-test").unwrap(),
        "model-test-1".to_owned(),
    )
    .unwrap()
}

fn node(
    id: &str,
    scope: &str,
    criterion: &str,
    method: VerificationMethod,
    risk: RiskClass,
    gate: Option<&str>,
) -> PlanNode {
    PlanNode::new(
        nid(id),
        plan_domain::PlanScope::new(vec![scope.to_owned()]).unwrap(),
        vec![CriterionDecl::new(cid(criterion), method)],
        risk,
        gate.map(|value| GateRef::new(value).unwrap()),
    )
    .unwrap()
}

fn graph_v1() -> PlanGraph {
    PlanGraph::new(
        vec![
            node(
                "root",
                "repo:src",
                "criterion-root",
                VerificationMethod::IndependentReview,
                RiskClass::Medium,
                Some("G7"),
            ),
            node(
                "worker",
                "repo:src/worker",
                "criterion-worker",
                VerificationMethod::AutomatedTest,
                RiskClass::Low,
                None,
            ),
        ],
        vec![(nid("root"), nid("worker"))],
    )
    .unwrap()
}

fn changeset(base: u64, reason: &str, caused_by: &[EvidenceRef]) -> PlanChangeSet {
    let mut changeset = PlanChangeSet {
        base_plan_revision: base,
        reason: reason.to_owned(),
        source_evidence_refs: caused_by.to_vec(),
        node_additions: vec![NodeAddition {
            id: nid("worker"),
            scope: plan_domain::PlanScope::new(vec!["repo:src/worker".to_owned()]).unwrap(),
            criteria: vec![CriterionDecl::new(
                cid("criterion-worker"),
                VerificationMethod::AutomatedTest,
            )],
            risk_class: RiskClass::Low,
            required_gate: None,
        }],
        node_updates: vec![],
        node_removals: vec![],
        edge_additions: vec![EdgeAddition {
            from: nid("root"),
            to: nid("worker"),
        }],
        edge_removals: vec![],
        expected_outcome: "worker becomes executable".to_owned(),
        risk_delta: vec![],
        budget_delta: BudgetDelta::new(20, 30),
        scope_delta: plan_domain::PlanScope::new(vec!["repo:src/worker".to_owned()]).unwrap(),
        coverage_map: BTreeMap::from([(cid("criterion-root"), vec![cid("criterion-worker")])]),
        proposal_hash: String::new(),
        proposed_by: identity(),
        planning_cost_receipt: PlanningCostReceipt::new(120, 40, 850, "standard".to_owned())
            .unwrap(),
    };
    changeset.proposal_hash = compute_proposal_hash(&changeset);
    changeset
}

fn revision(branch: &PlanBranchId, seq: u64, changeset: &PlanChangeSet) -> PlanRevision {
    PlanRevision::new(
        seq,
        branch.clone(),
        changeset.base_plan_revision,
        changeset.proposal_hash.clone(),
        changeset.reason.clone(),
        changeset.source_evidence_refs.clone(),
        RevisionProposedBy::new(
            changeset.proposed_by.clone(),
            PlanningCostReceiptRef::new(format!("receipt-{seq}")).unwrap(),
        ),
        ValidatedBy::new("plan-reconciler".to_owned(), "policy:abc123".to_owned()).unwrap(),
        1_800_000_000_000 + seq,
    )
    .unwrap()
}

fn journal_evidence(seq: u64) -> PlanCommitJournalEvidence {
    PlanCommitJournalEvidence {
        correlation_id: format!("plan-op-{seq}"),
        causation_id: format!("plan-request-{seq}"),
        occurred_at: format!("2026-09-14T20:00:0{seq}Z"),
        provenance: "plan-application-service/commit_revision".to_owned(),
    }
}

#[test]
fn fresh_database_reaches_schema_12() {
    let db = TestDb::new("schema");
    let store = db.open();
    assert_eq!(SUPPORTED_SCHEMA_VERSION, 12);
    assert_eq!(store.schema_version().unwrap(), 12);
}

#[test]
fn v11_database_migrates_preserving_g2_g3_g5_g6_data_and_starts_with_empty_plan() {
    let db = TestDb::new("v11-preservation");
    {
        let connection = db.raw();
        connection
            .execute_batch("PRAGMA foreign_keys = ON")
            .unwrap();
        for migration in [
            include_str!("../migrations/0001_task_core.sql"),
            include_str!("../migrations/0002_event_journal.sql"),
            include_str!("../migrations/0003_event_scope.sql"),
            include_str!("../migrations/0004_runtime_core.sql"),
            include_str!("../migrations/0005_workspace_leases.sql"),
            include_str!("../migrations/0006_attempt_reservations.sql"),
            include_str!("../migrations/0007_managed_identity_reservations.sql"),
            include_str!("../migrations/0008_approvals.sql"),
            include_str!("../migrations/0009_secret_refs.sql"),
            include_str!("../migrations/0010_security_audit.sql"),
            include_str!("../migrations/0011_workspace_trusted_anchor.sql"),
        ] {
            connection.execute_batch(migration).unwrap();
        }
        connection
            .execute_batch(
                "PRAGMA user_version = 11;
                 INSERT INTO projects (id, name) VALUES ('P-old', 'legacy project');
                 INSERT INTO tasks (id, project_id, objective, state)
                     VALUES ('T-old', 'P-old', 'legacy objective', 'RUNNING');
                 INSERT INTO executions (id, task_id) VALUES ('E-old', 'T-old');
                 INSERT INTO execution_attempts
                     (attempt_id, execution_id, position, provider_ref, outcome, detail)
                     VALUES ('A-old', 'E-old', 0, 'provider-old', 'succeeded', 'legacy detail');
                 INSERT INTO workspace_leases
                     (workspace_id, execution_id, repository_ref, base_revision,
                      branch_ref, worktree_ref, lifecycle)
                     VALUES ('W-old', 'E-old', 'repo-local:old',
                             'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                             'refs/heads/agent/e2-old', '/tmp/legacy-worktree', 'READY');
                 INSERT INTO workspace_trusted_anchor
                     (workspace_id, canonical_commondir, captured_at_millis, capture_provenance)
                     VALUES ('W-old', '/tmp/legacy.git', 123,
                             'workspace-application-service/allocate_workspace');
                 INSERT INTO attempt_reservations
                     (attempt_id, execution_id, task_id, reserved_at)
                     VALUES ('A-reserved', 'E-old', 'T-old', '2026-09-01T00:00:00Z');
                 INSERT INTO managed_identity_reservations
                     (branch_ref, execution_id, origin)
                     VALUES ('refs/heads/agent/e2-old', 'E-old', 'admission');
                 INSERT INTO approvals
                     (request_id, binding_fingerprint, operation_id, requester, task_id,
                      execution_id, attempt_id, policy_fingerprint, expires_at_ms, state)
                     VALUES ('approval-old', 'binding-old', 'op-old', 'requester-old', 'T-old',
                             'E-old', NULL, 'policy-old', 9999999999999, 'pending');
                 INSERT INTO approval_capabilities
                     (request_id, authority_set, position, kind, workspace, scope)
                     VALUES ('approval-old', 'requested', 0, 'filesystem.read', NULL, 'repo:src');
                 INSERT INTO secret_refs (ref_id) VALUES ('secret-ref-old');
                 INSERT INTO secret_bindings
                     (binding_id, secret_ref_id, target_kind, target_id)
                     VALUES ('secret-binding-old', 'secret-ref-old', 'execution', 'E-old');
                 INSERT INTO security_audit
                     (seq, record_version, event_type, author, outcome, occurred_at_ms,
                      correlation_id, task_id, execution_id, previous_hash, record_hash)
                     VALUES (
                         1, 1, 'policy.decided', 'policy-engine', 'allowed', 456,
                         'corr-old', 'T-old', 'E-old',
                         'sha256:0000000000000000000000000000000000000000000000000000000000000000',
                         'sha256:1111111111111111111111111111111111111111111111111111111111111111'
                     );
                 UPDATE security_audit_head
                     SET seq = 1,
                         record_hash = 'sha256:1111111111111111111111111111111111111111111111111111111111111111'
                     WHERE singleton = 1;
                 INSERT INTO event_journal
                     (event_type, event_version, task_id, project_id, from_state, to_state,
                      reason, correlation_id, causation_id, occurred_at, provenance,
                      execution_id, attempt_id, attempt_outcome, workspace_id)
                     VALUES
                     ('task.started', 1, 'T-old', 'P-old', 'CLAIMED', 'RUNNING', NULL,
                      'corr-event-1', 'cause-event-1', '2026-09-01T00:00:00Z',
                      'task-application-service/start', NULL, NULL, NULL, NULL),
                     ('attempt.concluded', 1, 'T-old', 'P-old', NULL, NULL, NULL,
                      'corr-event-2', 'cause-event-2', '2026-09-01T00:00:01Z',
                      'runtime-application-service/conclude', 'E-old', 'A-old', 'succeeded', NULL);",
            )
            .unwrap();
    }

    let store = db.open();
    assert_eq!(store.schema_version().unwrap(), 12);
    let history = store.list_events_ordered().unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].seq, 1);
    assert_eq!(history[0].fact.event_type, EventType::Started);
    assert_eq!(history[1].seq, 2);
    assert_eq!(history[1].fact.event_type, EventType::AttemptConcluded);
    drop(store);

    let connection = db.raw();
    let scalar = |sql: &str| -> i64 { connection.query_row(sql, [], |row| row.get(0)).unwrap() };
    assert_eq!(
        scalar("SELECT COUNT(*) FROM projects WHERE id = 'P-old'"),
        1
    );
    assert_eq!(scalar("SELECT COUNT(*) FROM tasks WHERE id = 'T-old'"), 1);
    assert_eq!(
        scalar("SELECT COUNT(*) FROM executions WHERE id = 'E-old'"),
        1
    );
    assert_eq!(
        scalar("SELECT COUNT(*) FROM attempt_reservations WHERE attempt_id = 'A-reserved'"),
        1
    );
    assert_eq!(
        scalar("SELECT COUNT(*) FROM managed_identity_reservations WHERE branch_ref = 'refs/heads/agent/e2-old'"),
        1
    );
    assert_eq!(
        scalar("SELECT COUNT(*) FROM approvals WHERE request_id = 'approval-old'"),
        1
    );
    assert_eq!(
        scalar("SELECT COUNT(*) FROM secret_bindings WHERE binding_id = 'secret-binding-old'"),
        1
    );
    assert_eq!(
        scalar("SELECT COUNT(*) FROM security_audit WHERE correlation_id = 'corr-old'"),
        1
    );
    assert_eq!(
        scalar("SELECT COUNT(*) FROM workspace_trusted_anchor WHERE workspace_id = 'W-old'"),
        1
    );
    assert_eq!(scalar("SELECT COUNT(*) FROM plan_branches"), 0);
    assert_eq!(scalar("SELECT COUNT(*) FROM plan_revisions"), 0);
    assert_eq!(scalar("SELECT COUNT(*) FROM plan_nodes"), 0);
    assert_eq!(scalar("SELECT COUNT(*) FROM plan_edges"), 0);
}

#[test]
fn revision_commit_persists_graph_provenance_and_exactly_one_bounded_event() {
    let db = TestDb::new("commit");
    let mut store = db.open();
    let branch = PlanBranchId::new("branch-main").unwrap();
    let causes = vec![evidence("attempt-17"), evidence("test-suite-4")];
    let changeset = changeset(0, "decompose root into executable worker", &causes);
    let revision = revision(&branch, 1, &changeset);
    let graph = graph_v1();

    let committed = store
        .commit_plan_revision(&changeset, &revision, &graph, &journal_evidence(1))
        .unwrap();

    assert_eq!(store.get_plan_head(&branch).unwrap(), Some(1));
    assert_eq!(
        store.get_plan_revision(&branch, 1).unwrap(),
        Some(revision.clone())
    );
    assert_eq!(
        store.get_plan_graph(&branch, 1).unwrap(),
        Some(graph.clone())
    );
    let history = store.list_events_ordered().unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0], committed);
    assert_eq!(committed.fact.event_type, EventType::PlanRevisionCommitted);
    assert!(committed.fact.project_id.is_none());
    assert!(committed.fact.task_id.is_none());
    let payload = committed.fact.plan_revision.as_ref().unwrap();
    assert_eq!(payload.plan_branch_id, branch);
    assert_eq!(payload.revision_seq, 1);
    assert_eq!(payload.changeset_hash, revision.changeset_hash);
    assert_eq!(payload.caused_by, causes);
    assert_eq!(payload.proposed_by, revision.proposed_by);
    assert_eq!(payload.validated_by, revision.validated_by);
    assert_eq!(
        committed.fact.reason.as_deref(),
        Some(revision.reason.as_str())
    );

    let columns: Vec<String> = db
        .raw()
        .prepare("PRAGMA table_info(event_journal)")
        .unwrap()
        .query_map([], |row| row.get(1))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert!(columns.iter().all(|name| {
        !name.contains("node")
            && !name.contains("edge")
            && !name.contains("scope")
            && !name.contains("criteria")
    }));
}

#[test]
fn journal_insert_failure_rolls_back_revision_head_and_graph() {
    let db = TestDb::new("journal-failure");
    let mut store = db.open();
    db.raw()
        .execute_batch(
            "CREATE TRIGGER fail_plan_event BEFORE INSERT ON event_journal
             WHEN NEW.event_type = 'plan.revision_committed'
             BEGIN SELECT RAISE(ABORT, 'boom'); END;",
        )
        .unwrap();
    let branch = PlanBranchId::new("branch-main").unwrap();
    let changeset = changeset(0, "first revision", &[evidence("attempt-1")]);
    let revision = revision(&branch, 1, &changeset);

    assert!(store
        .commit_plan_revision(&changeset, &revision, &graph_v1(), &journal_evidence(1),)
        .is_err());
    drop(store);
    let store = db.open();
    assert_eq!(store.get_plan_head(&branch).unwrap(), None);
    assert_eq!(store.get_plan_revision(&branch, 1).unwrap(), None);
    assert_eq!(store.get_plan_graph(&branch, 1).unwrap(), None);
    assert!(store.list_events_ordered().unwrap().is_empty());
}

#[test]
fn state_write_failure_appends_no_event() {
    let db = TestDb::new("state-failure");
    let mut store = db.open();
    db.raw()
        .execute_batch(
            "CREATE TRIGGER fail_plan_node BEFORE INSERT ON plan_nodes
             BEGIN SELECT RAISE(ABORT, 'boom'); END;",
        )
        .unwrap();
    let branch = PlanBranchId::new("branch-main").unwrap();
    let changeset = changeset(0, "first revision", &[evidence("attempt-1")]);
    let revision = revision(&branch, 1, &changeset);
    assert!(store
        .commit_plan_revision(&changeset, &revision, &graph_v1(), &journal_evidence(1),)
        .is_err());
    drop(store);
    db.raw()
        .execute_batch("DROP TRIGGER fail_plan_node")
        .unwrap();
    let store = db.open();
    assert_eq!(store.get_plan_head(&branch).unwrap(), None);
    assert!(store.list_events_ordered().unwrap().is_empty());
}

#[test]
fn successful_commit_survives_reopen_with_semantic_equivalence() {
    let db = TestDb::new("restart");
    let branch = PlanBranchId::new("branch-main").unwrap();
    let changeset = changeset(0, "first revision", &[evidence("attempt-1")]);
    let revision = revision(&branch, 1, &changeset);
    let graph = graph_v1();
    {
        let mut store = db.open();
        store
            .commit_plan_revision(&changeset, &revision, &graph, &journal_evidence(1))
            .unwrap();
    }
    let store = db.open();
    assert_eq!(store.get_plan_head(&branch).unwrap(), Some(1));
    assert_eq!(store.get_plan_revision(&branch, 1).unwrap(), Some(revision));
    assert_eq!(store.get_plan_graph(&branch, 1).unwrap(), Some(graph));
    let history = store.list_events_ordered().unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].fact.event_type, EventType::PlanRevisionCommitted);
}

#[test]
fn journal_revision_hash_mismatch_fails_closed_on_reconstruction() {
    let db = TestDb::new("journal-hash-mismatch");
    let branch = PlanBranchId::new("branch-main").unwrap();
    let changeset = changeset(0, "first revision", &[evidence("attempt-1")]);
    let revision = revision(&branch, 1, &changeset);
    {
        let mut store = db.open();
        store
            .commit_plan_revision(&changeset, &revision, &graph_v1(), &journal_evidence(1))
            .unwrap();
    }

    db.raw()
        .execute(
            "UPDATE event_journal SET plan_changeset_hash = ?1
             WHERE event_type = 'plan.revision_committed'",
            [format!("sha256:{}", "f".repeat(64))],
        )
        .unwrap();

    let store = db.open();
    assert!(matches!(
        store.list_events_ordered(),
        Err(StorageError::Corrupt(_))
    ));
}

#[test]
fn journal_revision_provenance_mismatches_fail_closed_on_reconstruction() {
    let cases = [
        ("reason", "reason", "different valid reason"),
        (
            "planner adapter",
            "plan_planner_adapter_id",
            "planner-adapter-other",
        ),
        ("planner model", "plan_model_ref", "model-other"),
        (
            "planning receipt",
            "plan_planning_cost_receipt_ref",
            "receipt-other",
        ),
        (
            "validator service",
            "plan_validated_service_id",
            "plan-reconciler-other",
        ),
        (
            "policy fingerprint",
            "plan_policy_fingerprint",
            "policy:def456",
        ),
    ];

    for (label, column, alternative) in cases {
        let db = TestDb::new(label);
        let branch = PlanBranchId::new("branch-main").unwrap();
        let changeset = changeset(0, "first revision", &[evidence("attempt-1")]);
        let revision = revision(&branch, 1, &changeset);
        {
            let mut store = db.open();
            store
                .commit_plan_revision(&changeset, &revision, &graph_v1(), &journal_evidence(1))
                .unwrap();
        }

        db.raw()
            .execute(
                &format!(
                    "UPDATE event_journal SET {column} = ?1
                     WHERE event_type = 'plan.revision_committed'"
                ),
                [alternative],
            )
            .unwrap();

        let store = db.open();
        assert!(
            matches!(store.list_events_ordered(), Err(StorageError::Corrupt(_))),
            "journal {label} mismatch must fail closed"
        );
    }
}

#[test]
fn journal_revision_causes_mismatch_fails_closed_on_reconstruction() {
    let db = TestDb::new("journal-causes-mismatch");
    let branch = PlanBranchId::new("branch-main").unwrap();
    let changeset = changeset(0, "first revision", &[evidence("attempt-1")]);
    let revision = revision(&branch, 1, &changeset);
    {
        let mut store = db.open();
        store
            .commit_plan_revision(&changeset, &revision, &graph_v1(), &journal_evidence(1))
            .unwrap();
    }

    db.raw()
        .execute(
            "UPDATE event_plan_causes SET evidence_ref = 'attempt-other' WHERE position = 0",
            [],
        )
        .unwrap();

    let store = db.open();
    assert!(matches!(
        store.list_events_ordered(),
        Err(StorageError::Corrupt(_))
    ));
}

#[test]
fn journal_revision_noncanonical_hash_fails_closed_on_reconstruction() {
    let db = TestDb::new("journal-invalid-hash");
    let branch = PlanBranchId::new("branch-main").unwrap();
    let changeset = changeset(0, "first revision", &[evidence("attempt-1")]);
    let revision = revision(&branch, 1, &changeset);
    {
        let mut store = db.open();
        store
            .commit_plan_revision(&changeset, &revision, &graph_v1(), &journal_evidence(1))
            .unwrap();
    }

    db.raw()
        .execute(
            "UPDATE event_journal SET plan_changeset_hash = 'sha256:ABCDEF'
             WHERE event_type = 'plan.revision_committed'",
            [],
        )
        .unwrap();

    let store = db.open();
    assert!(matches!(
        store.list_events_ordered(),
        Err(StorageError::Corrupt(_))
    ));
}

#[test]
fn stale_base_and_duplicate_revision_fail_closed_without_new_event() {
    let db = TestDb::new("stale");
    let mut store = db.open();
    let branch = PlanBranchId::new("branch-main").unwrap();
    let first_changeset = changeset(0, "first revision", &[evidence("attempt-1")]);
    let first = revision(&branch, 1, &first_changeset);
    store
        .commit_plan_revision(&first_changeset, &first, &graph_v1(), &journal_evidence(1))
        .unwrap();

    let result =
        store.commit_plan_revision(&first_changeset, &first, &graph_v1(), &journal_evidence(2));
    assert!(matches!(result, Err(StorageError::PlanStale { .. })));
    assert_eq!(store.get_plan_head(&branch).unwrap(), Some(1));
    assert_eq!(store.list_events_ordered().unwrap().len(), 1);
}

#[test]
fn two_same_head_writers_have_exactly_one_winner() {
    let db = TestDb::new("concurrent-head");
    let branch = PlanBranchId::new("branch-main").unwrap();
    let first_changeset = changeset(0, "first revision", &[evidence("attempt-1")]);
    let first = revision(&branch, 1, &first_changeset);
    {
        let mut store = db.open();
        store
            .commit_plan_revision(&first_changeset, &first, &graph_v1(), &journal_evidence(1))
            .unwrap();
    }

    let a_changeset = changeset(1, "writer a", &[evidence("attempt-a")]);
    let a_revision = revision(&branch, 2, &a_changeset);
    let b_changeset = changeset(1, "writer b", &[evidence("attempt-b")]);
    let b_revision = revision(&branch, 2, &b_changeset);
    let mut a = db.open();
    let mut b = db.open();
    let a_result =
        a.commit_plan_revision(&a_changeset, &a_revision, &graph_v1(), &journal_evidence(2));
    let b_result =
        b.commit_plan_revision(&b_changeset, &b_revision, &graph_v1(), &journal_evidence(3));
    assert!(a_result.is_ok());
    assert!(matches!(b_result, Err(StorageError::PlanStale { .. })));
    let store = db.open();
    assert_eq!(store.get_plan_head(&branch).unwrap(), Some(2));
    assert_eq!(store.list_events_ordered().unwrap().len(), 2);
}

#[test]
fn corrupt_plan_rows_fail_closed_on_reconstruction() {
    let db = TestDb::new("corrupt");
    let branch = PlanBranchId::new("branch-main").unwrap();
    let changeset = changeset(0, "first revision", &[evidence("attempt-1")]);
    let revision = revision(&branch, 1, &changeset);
    {
        let mut store = db.open();
        store
            .commit_plan_revision(&changeset, &revision, &graph_v1(), &journal_evidence(1))
            .unwrap();
    }
    db.raw()
        .execute_batch(
            "PRAGMA ignore_check_constraints = ON;
             UPDATE plan_nodes SET risk_class = 'unknown-risk'
             WHERE plan_branch_id = 'branch-main' AND revision_seq = 1 AND node_id = 'worker';
             PRAGMA ignore_check_constraints = OFF;",
        )
        .unwrap();
    let store = db.open();
    assert!(matches!(
        store.get_plan_graph(&branch, 1),
        Err(StorageError::Corrupt(_)) | Err(StorageError::DomainReconstruction(_))
    ));
}
