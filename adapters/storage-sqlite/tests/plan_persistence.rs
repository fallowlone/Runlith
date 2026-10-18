//! TASK-0077 contract tests: durable Plan snapshots + atomic revision journal.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use plan_domain::{
    compute_proposal_hash, decomposition_fingerprint, planning_evidence_key, BudgetDelta,
    CriterionDecl, CriterionId, EdgeAddition, EvidenceRef, GateRef, HumanContinuation,
    HumanDecisionReason, HumanDecisionStatus, NodeAddition, PlanBranchId, PlanChangeSet, PlanGraph,
    PlanNode, PlanNodeId, PlanRevision, PlannerAdapterId, PlannerIdentity, PlanningCostReceipt,
    PlanningCostReceiptRef, PlanningInputSnapshot, PlanningInputSnapshotId, PlanningRunId,
    RevisionProposedBy, RiskClass, RiskEscalationGrant, ScopeBound, ValidatedBy,
    VerificationMethod,
};
use storage_sqlite::{
    ChildTaskMaterialization, CommitPlanRevisionRequest, EventType, NewHumanDecisionRequest,
    NewJournalEvent, PlanCommitJournalEvidence, PlanRevisionLineage, SqliteStore, StorageError,
    CHILD_TASK_MATERIALIZATION_PROVENANCE, SUPPORTED_SCHEMA_VERSION,
};
use task_domain::{
    EvidenceRequirement, Project, ProjectId, RequirementKind, RequirementParam, SubjectBinding,
    Task, TaskId, TaskState, TransitionCommand,
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

    /// Opens the store with the planning cost receipts the fixture revisions
    /// reference (commits require resolvable receipt refs).
    fn open(&self) -> SqliteStore {
        storage_sqlite::test_support::reseal_journal_chain(&self.path).unwrap();
        let mut store = SqliteStore::open(&self.path).unwrap();
        for seq in 1..=3 {
            store
                .record_plan_cost_receipt(
                    &PlanningCostReceiptRef::new(format!("receipt-{seq}")).unwrap(),
                    &fixture_receipt(),
                )
                .unwrap();
        }
        store
    }

    fn raw(&self) -> rusqlite::Connection {
        let connection = rusqlite::Connection::open(&self.path).unwrap();
        #[cfg(unix)]
        std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o600))
            .expect("secure raw sqlite fixture");
        connection
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

#[test]
fn planning_input_policy_match_binds_full_requirement_shape() {
    let db = TestDb::new("planning-input-typed-requirement");
    let mut store = db.open();
    let requirement = EvidenceRequirement::new(
        "criterion-root",
        RequirementKind::ArtifactHash,
        SubjectBinding::Content,
        vec![RequirementParam::new("path", "dist/app").unwrap()],
        Some("policy:requirement-a".to_owned()),
        true,
    )
    .unwrap();
    let criterion = plan_domain::AffectedParentCriterion {
        id: cid("criterion-root"),
        method: VerificationMethod::AutomatedTest,
        requirement: Some(requirement.clone()),
    };
    let snapshot = PlanningInputSnapshot::new(
        PlanBranchId::new("typed-input").unwrap(),
        "task-root".to_owned(),
        0,
        ScopeBound::new(vec!["repo:src".to_owned()]).unwrap(),
        RiskClass::Low,
        vec![criterion.clone()],
        vec![evidence("typed-input")],
        vec![],
        None,
        "policy:outer".to_owned(),
    )
    .unwrap();
    store
        .start_planning_run(
            &snapshot,
            &planning_evidence_key(&snapshot.triggering_evidence),
            1,
        )
        .unwrap();
    assert!(store
        .planning_input_policy_matches(
            &snapshot.id,
            "task-root",
            &snapshot.scope_bound,
            RiskClass::Low,
            &[criterion],
            &[],
            None,
            "policy:outer"
        )
        .unwrap());
    let changed = plan_domain::AffectedParentCriterion {
        id: cid("criterion-root"),
        method: VerificationMethod::AutomatedTest,
        requirement: Some(
            EvidenceRequirement::new(
                "criterion-root",
                RequirementKind::FsState,
                SubjectBinding::Content,
                vec![RequirementParam::new("path", "dist/app").unwrap()],
                Some("policy:requirement-a".to_owned()),
                true,
            )
            .unwrap(),
        ),
    };
    assert!(!store
        .planning_input_policy_matches(
            &snapshot.id,
            "task-root",
            &snapshot.scope_bound,
            RiskClass::Low,
            &[changed],
            &[],
            None,
            "policy:outer"
        )
        .unwrap());
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
        planning_cost_receipt: fixture_receipt(),
    };
    changeset.proposal_hash = compute_proposal_hash(&changeset);
    changeset
}

fn fixture_receipt() -> PlanningCostReceipt {
    PlanningCostReceipt::new(120, 40, 850, "standard".to_owned()).unwrap()
}

/// Placeholder used only to preserve the original direct-storage test call
/// shape. `commit_revision` replaces this with the real snapshot/run binding
/// it creates immediately before publication.
fn lineage() -> PlanRevisionLineage {
    PlanRevisionLineage {
        parent_task_ref: "task-root".to_owned(),
        trigger_evidence_key: planning_evidence_key(&[evidence("attempt-1")]),
        planning_input_snapshot_id: PlanningInputSnapshotId::new("fixture-unbound").unwrap(),
        planning_input_fingerprint: format!("sha256:{}", "0".repeat(64)),
        planning_run_id: PlanningRunId::new("fixture-unbound-run").unwrap(),
        planning_run_generation: 1,
    }
}

fn commit_revision(
    store: &mut SqliteStore,
    changeset: &PlanChangeSet,
    revision: &PlanRevision,
    graph: &PlanGraph,
    _placeholder_lineage: &PlanRevisionLineage,
    evidence: &PlanCommitJournalEvidence,
) -> storage_sqlite::StorageResult<storage_sqlite::JournalEvent> {
    let trigger_evidence = if changeset.source_evidence_refs.is_empty() {
        vec![crate::evidence("attempt-1")]
    } else {
        changeset.source_evidence_refs.clone()
    };
    let trigger_key = planning_evidence_key(&trigger_evidence);
    let snapshot = PlanningInputSnapshot::new(
        revision.plan_branch_id.clone(),
        "task-root".to_owned(),
        revision.base_revision,
        ScopeBound::new(vec!["repo:src".to_owned()]).unwrap(),
        RiskClass::Low,
        vec![],
        trigger_evidence,
        vec![],
        None,
        revision.validated_by.policy_fingerprint.clone(),
    )
    .unwrap();
    let run = store
        .start_planning_run(
            &snapshot,
            &trigger_key,
            revision.created_at.saturating_sub(1),
        )
        .unwrap();
    let lineage = PlanRevisionLineage {
        parent_task_ref: "task-root".to_owned(),
        trigger_evidence_key: trigger_key,
        planning_input_snapshot_id: snapshot.id,
        planning_input_fingerprint: snapshot.input_fingerprint,
        planning_run_id: run.id,
        planning_run_generation: run.generation,
    };
    store.commit_plan_revision(changeset, revision, graph, &lineage, evidence)
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
fn fresh_database_reaches_schema_16() {
    let db = TestDb::new("schema");
    let store = db.open();
    assert_eq!(SUPPORTED_SCHEMA_VERSION, 34);
    assert_eq!(store.schema_version().unwrap(), SUPPORTED_SCHEMA_VERSION);
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
    assert_eq!(store.schema_version().unwrap(), SUPPORTED_SCHEMA_VERSION);
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
    seed_child_parent(&mut store);
    let branch = PlanBranchId::new("branch-main").unwrap();
    let causes = vec![evidence("attempt-17"), evidence("test-suite-4")];
    let changeset = changeset(0, "decompose root into executable worker", &causes);
    let revision = revision(&branch, 1, &changeset);
    let graph = graph_v1();

    let committed = commit_revision(
        &mut store,
        &changeset,
        &revision,
        &graph,
        &lineage(),
        &journal_evidence(1),
    )
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

    assert!(commit_revision(
        &mut store,
        &changeset,
        &revision,
        &graph_v1(),
        &lineage(),
        &journal_evidence(1),
    )
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
    assert!(commit_revision(
        &mut store,
        &changeset,
        &revision,
        &graph_v1(),
        &lineage(),
        &journal_evidence(1),
    )
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
        seed_child_parent(&mut store);
        commit_revision(
            &mut store,
            &changeset,
            &revision,
            &graph,
            &lineage(),
            &journal_evidence(1),
        )
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
        seed_child_parent(&mut store);
        commit_revision(
            &mut store,
            &changeset,
            &revision,
            &graph_v1(),
            &lineage(),
            &journal_evidence(1),
        )
        .unwrap();
    }

    db.raw()
        .execute_batch("DROP TRIGGER IF EXISTS event_journal_no_update; DROP TRIGGER IF EXISTS event_journal_no_delete;")
        .unwrap();
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
            seed_child_parent(&mut store);
            commit_revision(
                &mut store,
                &changeset,
                &revision,
                &graph_v1(),
                &lineage(),
                &journal_evidence(1),
            )
            .unwrap();
        }

        db.raw()
            .execute_batch("DROP TRIGGER IF EXISTS event_journal_no_update; DROP TRIGGER IF EXISTS event_journal_no_delete;")
            .unwrap();
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
        seed_child_parent(&mut store);
        commit_revision(
            &mut store,
            &changeset,
            &revision,
            &graph_v1(),
            &lineage(),
            &journal_evidence(1),
        )
        .unwrap();
    }

    db.raw()
        .execute_batch("DROP TRIGGER IF EXISTS event_plan_causes_no_update; DROP TRIGGER IF EXISTS event_plan_causes_no_delete;")
        .unwrap();
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
fn plan_causes_table_rejects_update_and_delete_as_append_only() {
    let db = TestDb::new("causes-append-only");
    let branch = PlanBranchId::new("branch-main").unwrap();
    let changeset = changeset(0, "first revision", &[evidence("attempt-1")]);
    let revision = revision(&branch, 1, &changeset);
    {
        let mut store = db.open();
        seed_child_parent(&mut store);
        commit_revision(
            &mut store,
            &changeset,
            &revision,
            &graph_v1(),
            &lineage(),
            &journal_evidence(1),
        )
        .unwrap();
    }

    let raw = db.raw();
    assert!(raw
        .execute(
            "UPDATE event_plan_causes SET evidence_ref = 'forged' WHERE position = 0",
            []
        )
        .is_err());
    assert!(raw
        .execute("DELETE FROM event_plan_causes WHERE position = 0", [])
        .is_err());
}

#[test]
fn journal_revision_noncanonical_hash_fails_closed_on_reconstruction() {
    let db = TestDb::new("journal-invalid-hash");
    let branch = PlanBranchId::new("branch-main").unwrap();
    let changeset = changeset(0, "first revision", &[evidence("attempt-1")]);
    let revision = revision(&branch, 1, &changeset);
    {
        let mut store = db.open();
        seed_child_parent(&mut store);
        commit_revision(
            &mut store,
            &changeset,
            &revision,
            &graph_v1(),
            &lineage(),
            &journal_evidence(1),
        )
        .unwrap();
    }

    db.raw()
        .execute_batch("DROP TRIGGER IF EXISTS event_journal_no_update; DROP TRIGGER IF EXISTS event_journal_no_delete;")
        .unwrap();
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
    seed_child_parent(&mut store);
    let branch = PlanBranchId::new("branch-main").unwrap();
    let first_changeset = changeset(0, "first revision", &[evidence("attempt-1")]);
    let first = revision(&branch, 1, &first_changeset);
    commit_revision(
        &mut store,
        &first_changeset,
        &first,
        &graph_v1(),
        &lineage(),
        &journal_evidence(1),
    )
    .unwrap();

    let result = commit_revision(
        &mut store,
        &first_changeset,
        &first,
        &graph_v1(),
        &lineage(),
        &journal_evidence(2),
    );
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
        seed_child_parent(&mut store);
        commit_revision(
            &mut store,
            &first_changeset,
            &first,
            &graph_v1(),
            &lineage(),
            &journal_evidence(1),
        )
        .unwrap();
    }

    let a_changeset = changeset(1, "writer a", &[evidence("attempt-a")]);
    let a_revision = revision(&branch, 2, &a_changeset);
    let b_changeset = changeset(1, "writer b", &[evidence("attempt-b")]);
    let b_revision = revision(&branch, 2, &b_changeset);
    let mut a = db.open();
    let mut b = db.open();
    let a_result = commit_revision(
        &mut a,
        &a_changeset,
        &a_revision,
        &graph_v1(),
        &lineage(),
        &journal_evidence(2),
    );
    let b_result = commit_revision(
        &mut b,
        &b_changeset,
        &b_revision,
        &graph_v1(),
        &lineage(),
        &journal_evidence(3),
    );
    assert!(a_result.is_ok());
    assert!(matches!(b_result, Err(StorageError::PlanStale { .. })));
    let store = db.open();
    assert_eq!(store.get_plan_head(&branch).unwrap(), Some(2));
    assert_eq!(store.list_events_ordered().unwrap().len(), 2);
}

#[test]
fn commit_requires_resolvable_receipt_matching_the_accepted_proposal() {
    let db = TestDb::new("receipt-binding");
    let mut store = db.open();
    let branch = PlanBranchId::new("branch-main").unwrap();
    let changeset = changeset(0, "first revision", &[evidence("attempt-1")]);

    let mut unresolved = revision(&branch, 1, &changeset);
    unresolved.proposed_by.planning_cost_receipt_ref =
        PlanningCostReceiptRef::new("receipt-missing").unwrap();
    assert!(matches!(
        commit_revision(
            &mut store,
            &changeset,
            &unresolved,
            &graph_v1(),
            &lineage(),
            &journal_evidence(1)
        ),
        Err(StorageError::ConstraintViolation(_))
    ));

    let other = PlanningCostReceiptRef::new("receipt-other-evidence").unwrap();
    store
        .record_plan_cost_receipt(
            &other,
            &PlanningCostReceipt::new(1, 1, 1, "standard".to_owned()).unwrap(),
        )
        .unwrap();
    let mut mismatched = revision(&branch, 1, &changeset);
    mismatched.proposed_by.planning_cost_receipt_ref = other;
    assert!(matches!(
        commit_revision(
            &mut store,
            &changeset,
            &mismatched,
            &graph_v1(),
            &lineage(),
            &journal_evidence(1)
        ),
        Err(StorageError::ConstraintViolation(_))
    ));

    assert_eq!(store.get_plan_head(&branch).unwrap(), None);
    assert!(store.list_events_ordered().unwrap().is_empty());
}

#[test]
fn commit_records_storage_derived_lineage_and_resets_l3_counter_atomically() {
    let db = TestDb::new("lineage");
    let mut store = db.open();
    seed_child_parent(&mut store);
    let branch = PlanBranchId::new("branch-main").unwrap();
    let changeset = changeset(0, "first revision", &[evidence("attempt-1")]);
    let key = lineage().trigger_evidence_key;
    store.record_plan_proposal_rejection(&branch, &key).unwrap();

    db.raw()
        .execute_batch(
            "CREATE TRIGGER fail_plan_lineage BEFORE INSERT ON plan_revision_lineage
             BEGIN SELECT RAISE(ABORT, 'boom'); END;",
        )
        .unwrap();
    assert!(commit_revision(
        &mut store,
        &changeset,
        &revision(&branch, 1, &changeset),
        &graph_v1(),
        &lineage(),
        &journal_evidence(1)
    )
    .is_err());
    assert_eq!(store.get_plan_head(&branch).unwrap(), None);
    assert!(store.list_events_ordered().unwrap().is_empty());
    db.raw()
        .execute_batch("DROP TRIGGER fail_plan_lineage")
        .unwrap();

    commit_revision(
        &mut store,
        &changeset,
        &revision(&branch, 1, &changeset),
        &graph_v1(),
        &lineage(),
        &journal_evidence(1),
    )
    .unwrap();
    let lineage = store.get_plan_lineage(&branch).unwrap();
    assert_eq!(lineage.len(), 1);
    assert_eq!(lineage[0].revision_seq, 1);
    assert_eq!(lineage[0].parent_task_ref, "task-root");
    assert_eq!(
        lineage[0].decomposition_fingerprint,
        decomposition_fingerprint("task-root", &changeset)
    );
    assert_eq!(lineage[0].trigger_evidence_key, key);
    assert!(store
        .revision_has_current_planning_input(&branch, 1)
        .unwrap());
    assert_eq!(
        store
            .get_plan_guardrail_event(&branch, &key)
            .unwrap()
            .unwrap()
            .consecutive_rejections,
        0
    );
}

#[test]
fn superseded_planning_run_generation_cannot_publish_a_revision() {
    let db = TestDb::new("stale-planning-run-publication");
    let mut store = db.open();
    seed_child_parent(&mut store);
    let branch = PlanBranchId::new("branch-main").unwrap();
    let changeset = changeset(0, "first revision", &[evidence("attempt-1")]);
    let revision = revision(&branch, 1, &changeset);
    let trigger_evidence = changeset.source_evidence_refs.clone();
    let trigger_key = planning_evidence_key(&trigger_evidence);
    let snapshot = PlanningInputSnapshot::new(
        branch.clone(),
        "task-root".to_owned(),
        0,
        ScopeBound::new(vec!["repo:src".to_owned()]).unwrap(),
        RiskClass::Low,
        vec![],
        trigger_evidence,
        vec![],
        None,
        revision.validated_by.policy_fingerprint.clone(),
    )
    .unwrap();
    let stale_run = store
        .start_planning_run(&snapshot, &trigger_key, revision.created_at - 2)
        .unwrap();
    let current_run = store
        .start_planning_run(&snapshot, &trigger_key, revision.created_at - 1)
        .unwrap();
    assert_eq!(current_run.generation, stale_run.generation + 1);

    let stale_lineage = PlanRevisionLineage {
        parent_task_ref: "task-root".to_owned(),
        trigger_evidence_key: trigger_key,
        planning_input_snapshot_id: snapshot.id,
        planning_input_fingerprint: snapshot.input_fingerprint,
        planning_run_id: stale_run.id,
        planning_run_generation: stale_run.generation,
    };
    let error = store
        .commit_plan_revision(
            &changeset,
            &revision,
            &graph_v1(),
            &stale_lineage,
            &journal_evidence(1),
        )
        .expect_err("superseded planning generation must be fenced");
    assert!(matches!(
        error,
        StorageError::PlanningRunStale {
            expected_generation: 1,
            found_generation: 2,
            ..
        }
    ));
    assert_eq!(store.get_plan_head(&branch).unwrap(), None);
    assert!(store.list_events_ordered().unwrap().is_empty());
}

/// TASK-0116: atomicity assessment reserves a run so its PlannerPort spend is
/// crash-safe (ADR-0028 §4), but it proposes nothing, so it must not republish
/// the branch's current planning input and de-admit the accepted revision.
#[test]
fn assessment_run_reserves_a_generation_without_replacing_current_input() {
    let db = TestDb::new("assessment-run-current-input");
    let mut store = db.open();
    seed_child_parent(&mut store);
    let branch = PlanBranchId::new("branch-main").unwrap();
    let first = changeset(0, "first revision", &[evidence("attempt-1")]);
    commit_revision(
        &mut store,
        &first,
        &revision(&branch, 1, &first),
        &graph_v1(),
        &lineage(),
        &journal_evidence(1),
    )
    .unwrap();
    assert!(store
        .revision_has_current_planning_input(&branch, 1)
        .unwrap());

    let assessment_evidence = vec![evidence("is-this-atomic")];
    let assessment_snapshot = PlanningInputSnapshot::new(
        branch.clone(),
        "task-root".to_owned(),
        1,
        ScopeBound::new(vec!["repo:src".to_owned()]).unwrap(),
        RiskClass::Low,
        vec![],
        assessment_evidence.clone(),
        vec![],
        None,
        "policy:abc123".to_owned(),
    )
    .unwrap();

    let run = store
        .start_assessment_planning_run(
            &assessment_snapshot,
            &planning_evidence_key(&assessment_evidence),
            1_800_000_000_010,
        )
        .expect("assessment claims a run for spend accounting");

    assert_eq!(
        run.snapshot_id, assessment_snapshot.id,
        "the run binds to the assessment's own snapshot"
    );
    assert!(
        store
            .revision_has_current_planning_input(&branch, 1)
            .unwrap(),
        "an assessment proposes nothing, so it must not replace the accepted \
         revision's current planning input"
    );

    // ADR-0028 §3 still applies to assessment: a second claim advances the
    // branch's planning generation and takes over the run head, so a stale
    // worker from the earlier generation can no longer publish.
    let later_evidence = vec![evidence("is-this-still-atomic")];
    let later_snapshot = PlanningInputSnapshot::new(
        branch.clone(),
        "task-root".to_owned(),
        1,
        ScopeBound::new(vec!["repo:src".to_owned()]).unwrap(),
        RiskClass::Low,
        vec![],
        later_evidence.clone(),
        vec![],
        None,
        "policy:abc123".to_owned(),
    )
    .unwrap();
    let later = store
        .start_assessment_planning_run(
            &later_snapshot,
            &planning_evidence_key(&later_evidence),
            1_800_000_000_020,
        )
        .expect("a second assessment claims the next generation");
    assert_eq!(
        later.generation,
        run.generation + 1,
        "assessment still advances the planning-run generation"
    );
    assert_eq!(
        store
            .current_planning_run(&branch)
            .unwrap()
            .expect("a current planning run")
            .id,
        later.id,
        "the newer assessment run takes over the run head"
    );
    assert!(
        store
            .revision_has_current_planning_input(&branch, 1)
            .unwrap(),
        "neither assessment replaces current planning input"
    );
}

/// TASK-0116: the human-gate writer is the one writer of
/// `planning_input_heads` not fused into a run claim, so it carries its own
/// generation fence. A superseded run must not republish its stale snapshot
/// over a newer generation's. TASK-0119 moved that fence behind the gate's
/// fused publish-and-record writer; the fence itself is unchanged.
#[test]
fn publishing_planning_input_from_a_superseded_run_is_refused() {
    let db = TestDb::new("publish-input-superseded-run");
    let mut store = db.open();
    seed_child_parent(&mut store);
    let branch = PlanBranchId::new("branch-main").unwrap();

    let first_evidence = vec![evidence("first-gate")];
    let first_snapshot = PlanningInputSnapshot::new(
        branch.clone(),
        "task-root".to_owned(),
        0,
        ScopeBound::new(vec!["repo:src".to_owned()]).unwrap(),
        RiskClass::Low,
        vec![],
        first_evidence.clone(),
        vec![],
        None,
        "policy:abc123".to_owned(),
    )
    .unwrap();
    let first = store
        .start_assessment_planning_run(
            &first_snapshot,
            &planning_evidence_key(&first_evidence),
            1_800_000_000_010,
        )
        .unwrap();

    let second_evidence = vec![evidence("second-gate")];
    let second_snapshot = PlanningInputSnapshot::new(
        branch.clone(),
        "task-root".to_owned(),
        0,
        ScopeBound::new(vec!["repo:src".to_owned()]).unwrap(),
        RiskClass::Low,
        vec![],
        second_evidence.clone(),
        vec![],
        None,
        "policy:abc123".to_owned(),
    )
    .unwrap();
    store
        .start_assessment_planning_run(
            &second_snapshot,
            &planning_evidence_key(&second_evidence),
            1_800_000_000_020,
        )
        .expect("a newer assessment supersedes the first run");

    let refusal = store
        .publish_planning_input_and_create_human_decision(
            &first,
            &first_snapshot,
            NewHumanDecisionRequest {
                task_id: TaskId::new("task-root").unwrap(),
                plan_branch_id: branch.clone(),
                reason: HumanDecisionReason::PlannerBlocked,
                risk: RiskClass::Low,
                affected_scope: ScopeBound::new(vec!["repo:src".to_owned()]).unwrap(),
                options: vec![HumanContinuation::Replan, HumanContinuation::Abort],
                recommendation: None,
                evidence: first_evidence,
                proposal_hash: None,
                approved_proposal: None,
                plan_head: 0,
                planning_input_snapshot_id: first_snapshot.id.clone(),
                planning_input_fingerprint: first_snapshot.input_fingerprint.clone(),
                policy_fingerprint: "policy:abc123".to_owned(),
                expires_at_ms: Some(1_800_000_060_000),
            },
            1_800_000_000_030,
        )
        .expect_err("a superseded run must not republish its snapshot as current input");
    assert!(
        matches!(refusal, StorageError::PlanningRunStale { .. }),
        "the refusal must come from the generation fence, not from a later \
         check in the same call: got {refusal:?}"
    );
}
/// TASK-0119: a human gate publishes its snapshot as the branch's current
/// planning input and records its `HumanDecision` in one transaction. When the
/// decision cannot be created, the publication must not survive on its own:
/// that state de-admits the accepted revision and restart does not heal it.
#[test]
fn a_failed_gate_decision_leaves_the_accepted_revision_admissible() {
    let db = TestDb::new("gate-atomic-failure");
    let mut store = db.open();
    seed_child_parent(&mut store);
    let branch = PlanBranchId::new("branch-main").unwrap();

    let accepted = changeset(0, "accepted revision", &[evidence("accepted-work")]);
    commit_revision(
        &mut store,
        &accepted,
        &revision(&branch, 1, &accepted),
        &graph_v1(),
        &lineage(),
        &journal_evidence(1),
    )
    .unwrap();
    assert!(store
        .revision_has_current_planning_input(&branch, 1)
        .unwrap());

    let gate_evidence = vec![evidence("gate-trigger")];
    let gate_snapshot = PlanningInputSnapshot::new(
        branch.clone(),
        "task-root".to_owned(),
        1,
        ScopeBound::new(vec!["repo:src".to_owned()]).unwrap(),
        RiskClass::Low,
        vec![],
        gate_evidence.clone(),
        vec![],
        None,
        "policy:abc123".to_owned(),
    )
    .unwrap();
    let run = store
        .start_human_gate_planning_run(
            &gate_snapshot,
            &planning_evidence_key(&gate_evidence),
            1_800_000_000_500,
        )
        .unwrap();
    assert!(
        store
            .revision_has_current_planning_input(&branch, 1)
            .unwrap(),
        "a human gate's run claim publishes nothing on its own"
    );

    // The decision cannot be created: its owner Task is not durable.
    let doomed = NewHumanDecisionRequest {
        task_id: TaskId::new("task-not-durable").unwrap(),
        plan_branch_id: branch.clone(),
        reason: HumanDecisionReason::PlannerBlocked,
        risk: RiskClass::Low,
        affected_scope: ScopeBound::new(vec!["repo:src".to_owned()]).unwrap(),
        options: vec![HumanContinuation::Replan, HumanContinuation::Abort],
        recommendation: None,
        evidence: gate_evidence.clone(),
        proposal_hash: None,
        approved_proposal: None,
        plan_head: 1,
        planning_input_snapshot_id: gate_snapshot.id.clone(),
        planning_input_fingerprint: gate_snapshot.input_fingerprint.clone(),
        policy_fingerprint: "policy:abc123".to_owned(),
        expires_at_ms: Some(1_800_000_060_000),
    };

    store
        .publish_planning_input_and_create_human_decision(
            &run,
            &gate_snapshot,
            doomed,
            1_800_000_000_600,
        )
        .expect_err("the gate's decision cannot be created");

    assert!(
        store
            .revision_has_current_planning_input(&branch, 1)
            .unwrap(),
        "a gate that failed to record its decision must not leave the accepted \
         revision de-admitted"
    );
}

/// TASK-0119: the succeeding half of the same boundary. A gate that records
/// its decision must leave its own snapshot current, or the decision it just
/// created could never be resumed.
#[test]
fn a_recorded_gate_decision_publishes_its_own_snapshot() {
    let db = TestDb::new("gate-atomic-success");
    let mut store = db.open();
    seed_child_parent(&mut store);
    let branch = PlanBranchId::new("branch-main").unwrap();

    let accepted = changeset(0, "accepted revision", &[evidence("accepted-work")]);
    commit_revision(
        &mut store,
        &accepted,
        &revision(&branch, 1, &accepted),
        &graph_v1(),
        &lineage(),
        &journal_evidence(1),
    )
    .unwrap();

    let gate_evidence = vec![evidence("gate-trigger")];
    let gate_snapshot = PlanningInputSnapshot::new(
        branch.clone(),
        "task-root".to_owned(),
        1,
        ScopeBound::new(vec!["repo:src".to_owned()]).unwrap(),
        RiskClass::Low,
        vec![],
        gate_evidence.clone(),
        vec![],
        None,
        "policy:abc123".to_owned(),
    )
    .unwrap();
    let run = store
        .start_human_gate_planning_run(
            &gate_snapshot,
            &planning_evidence_key(&gate_evidence),
            1_800_000_000_500,
        )
        .unwrap();
    assert!(
        store
            .revision_has_current_planning_input(&branch, 1)
            .unwrap(),
        "a human gate's run claim publishes nothing on its own"
    );

    let decision = store
        .publish_planning_input_and_create_human_decision(
            &run,
            &gate_snapshot,
            NewHumanDecisionRequest {
                task_id: TaskId::new("task-root").unwrap(),
                plan_branch_id: branch.clone(),
                reason: HumanDecisionReason::PlannerBlocked,
                risk: RiskClass::Low,
                affected_scope: ScopeBound::new(vec!["repo:src".to_owned()]).unwrap(),
                options: vec![HumanContinuation::Replan, HumanContinuation::Abort],
                recommendation: None,
                evidence: gate_evidence,
                proposal_hash: None,
                approved_proposal: None,
                plan_head: 1,
                planning_input_snapshot_id: gate_snapshot.id.clone(),
                planning_input_fingerprint: gate_snapshot.input_fingerprint.clone(),
                policy_fingerprint: "policy:abc123".to_owned(),
                expires_at_ms: Some(1_800_000_060_000),
            },
            1_800_000_000_600,
        )
        .expect("the gate publishes and records in one transaction");

    assert_eq!(decision.status, HumanDecisionStatus::Pending);
    assert_eq!(
        decision.precondition.planning_input_snapshot_id,
        gate_snapshot.id
    );
    assert!(
        store
            .current_planning_input_matches(
                &branch,
                &gate_snapshot.id,
                &gate_snapshot.input_fingerprint,
            )
            .unwrap(),
        "the recorded decision's bound snapshot is the branch's current input"
    );
}

#[test]
fn stale_planning_run_cannot_replace_current_input_after_plan_head_moves() {
    let db = TestDb::new("stale-planning-run-start");
    let mut store = db.open();
    seed_child_parent(&mut store);
    let branch = PlanBranchId::new("branch-main").unwrap();
    let first = changeset(0, "first revision", &[evidence("attempt-1")]);
    commit_revision(
        &mut store,
        &first,
        &revision(&branch, 1, &first),
        &graph_v1(),
        &lineage(),
        &journal_evidence(1),
    )
    .unwrap();
    assert!(store
        .revision_has_current_planning_input(&branch, 1)
        .unwrap());

    let stale_evidence = vec![evidence("late-resume")];
    let stale_snapshot = PlanningInputSnapshot::new(
        branch.clone(),
        "task-root".to_owned(),
        1,
        ScopeBound::new(vec!["repo:src".to_owned()]).unwrap(),
        RiskClass::Low,
        vec![],
        stale_evidence.clone(),
        vec![],
        None,
        "policy:abc123".to_owned(),
    )
    .unwrap();
    let error = store
        .start_or_join_identical_planning_run(
            &stale_snapshot,
            &planning_evidence_key(&stale_evidence),
            0,
            1_800_000_000_010,
        )
        .expect_err("an approved replay bound to the old head must not replace current input");
    assert!(matches!(
        error,
        StorageError::PlanStale {
            expected: 0,
            found: 1,
            ..
        }
    ));
    assert!(store
        .revision_has_current_planning_input(&branch, 1)
        .unwrap());
}

#[test]
fn canonical_task_authority_fence_rechecks_task_facts_inside_publication_transaction() {
    let db = TestDb::new("canonical-task-authority-fence");
    let mut store = db.open();
    let project = ProjectId::new("project-authority").unwrap();
    store
        .create_project(&Project::new(project.clone(), "authority".to_owned()).unwrap())
        .unwrap();
    let mut root = Task::draft(TaskId::new("task-root").unwrap(), project, "root").unwrap();
    root.define_planning_metadata(
        vec!["repo:src".to_owned()],
        vec!["criterion-root".to_owned()],
        "low",
    )
    .unwrap();
    let root_requirement = root.snapshot().evidence_requirements[0].clone();
    root.mark_ready().unwrap();
    store.create_task(&root).unwrap();

    let branch = PlanBranchId::new("branch-authority").unwrap();
    let changeset = changeset(0, "first revision", &[evidence("attempt-1")]);
    let revision = revision(&branch, 1, &changeset);
    let trigger_evidence = changeset.source_evidence_refs.clone();
    let trigger_key = planning_evidence_key(&trigger_evidence);
    let attempt = |store: &mut SqliteStore, task_ref: &str, created_at: u64| {
        let snapshot = PlanningInputSnapshot::new_with_candidates(
            branch.clone(),
            task_ref.to_owned(),
            vec![task_ref.to_owned()],
            0,
            ScopeBound::new(vec!["repo:src".to_owned()]).unwrap(),
            RiskClass::Low,
            vec![plan_domain::AffectedParentCriterion {
                id: cid("criterion-root"),
                method: VerificationMethod::AutomatedTest,
                requirement: Some(root_requirement.clone()),
            }],
            trigger_evidence.clone(),
            vec![],
            None,
            revision.validated_by.policy_fingerprint.clone(),
        )
        .unwrap();
        let run = store
            .start_planning_run(&snapshot, &trigger_key, created_at)
            .unwrap();
        let lineage = PlanRevisionLineage {
            parent_task_ref: task_ref.to_owned(),
            trigger_evidence_key: trigger_key.clone(),
            planning_input_snapshot_id: snapshot.id,
            planning_input_fingerprint: snapshot.input_fingerprint,
            planning_run_id: run.id,
            planning_run_generation: run.generation,
        };
        store.commit_plan_revision_with_human_decision(
            &changeset,
            &revision,
            &graph_v1(),
            &lineage,
            &journal_evidence(1),
            None,
            true,
        )
    };
    let assert_unpublished = |store: &SqliteStore| {
        assert_eq!(store.get_plan_head(&branch).unwrap(), None);
        assert!(!store
            .list_events_ordered()
            .unwrap()
            .iter()
            .any(|event| event.fact.event_type == EventType::PlanRevisionCommitted));
    };

    // A nonexistent parent is refused by admission, ahead of the staleness
    // fence and independently of it. The distinction is deliberate:
    // `PlanningInputStale` promises the caller that the input moved and a
    // fresh planning cycle can proceed — `planned_orchestration` turns it into
    // `PlanningInputMoved`, an `Ok` outcome. A parent that does not exist will
    // not appear on replan, so it must not be reported as a concurrent move.
    let missing = attempt(&mut store, "task-missing", revision.created_at - 4)
        .expect_err("a snapshot for a nonexistent canonical Task cannot publish");
    assert!(
        matches!(missing, StorageError::ConstraintViolation(ref detail) if detail.contains("does not exist")),
        "unexpected error {missing:?}"
    );
    assert_unpublished(&store);

    let mut drifted = root.snapshot();
    drifted.risk = Some("medium".to_owned());
    store.save_task(&Task::restore(drifted).unwrap()).unwrap();
    let stale = attempt(&mut store, "task-root", revision.created_at - 3)
        .expect_err("changed canonical risk fences publication");
    assert!(matches!(stale, StorageError::PlanningInputStale { .. }));
    assert_unpublished(&store);

    // Values, not write history, are the authority: restoring the exact
    // facts the snapshot captured makes the same publication current again.
    store.save_task(&root).unwrap();
    attempt(&mut store, "task-root", revision.created_at - 2)
        .expect("current canonical authority publishes");
    assert_eq!(store.get_plan_head(&branch).unwrap(), Some(1));
}

#[test]
fn risk_decision_consumption_rolls_back_when_plan_commit_fails_then_consumes_on_retry() {
    let db = TestDb::new("human-risk-atomic-rollback");
    let mut store = db.open();
    let project_id = ProjectId::new("P-HD-ATOMIC").unwrap();
    store
        .create_project(&Project::new(project_id.clone(), "hd atomic".to_owned()).unwrap())
        .unwrap();
    let task_id = TaskId::new("task-root").unwrap();
    let mut risk_owner = Task::draft(task_id.clone(), project_id, "risk owner".to_owned()).unwrap();
    risk_owner.mark_ready().unwrap();
    store.create_task(&risk_owner).unwrap();

    let branch = PlanBranchId::new("branch-human-risk").unwrap();
    let changeset = changeset(0, "risk guarded revision", &[evidence("risk-attempt")]);
    let created_at = 1_800_000_000_100;
    let policy_fingerprint = "policy:abc123".to_owned();
    let trigger = changeset.source_evidence_refs.clone();
    let trigger_key = planning_evidence_key(&trigger);
    let old_snapshot = PlanningInputSnapshot::new(
        branch.clone(),
        task_id.as_str().to_owned(),
        0,
        ScopeBound::new(vec!["repo:src".to_owned()]).unwrap(),
        RiskClass::Low,
        vec![],
        trigger.clone(),
        vec![],
        None,
        policy_fingerprint.clone(),
    )
    .unwrap();
    store
        .start_planning_run(&old_snapshot, &trigger_key, created_at - 3)
        .unwrap();

    let expires_at = created_at + 60_000;
    let decision = store
        .create_human_decision_request(
            NewHumanDecisionRequest {
                task_id: task_id.clone(),
                plan_branch_id: branch.clone(),
                reason: HumanDecisionReason::RiskEscalation,
                risk: RiskClass::High,
                affected_scope: ScopeBound::new(vec!["repo:src/worker".to_owned()]).unwrap(),
                options: vec![HumanContinuation::Resume],
                recommendation: Some(HumanContinuation::Resume),
                evidence: trigger.clone(),
                proposal_hash: Some(changeset.proposal_hash.clone()),
                approved_proposal: None,
                plan_head: 0,
                planning_input_snapshot_id: old_snapshot.id.clone(),
                planning_input_fingerprint: old_snapshot.input_fingerprint.clone(),
                policy_fingerprint: policy_fingerprint.clone(),
                expires_at_ms: Some(expires_at),
            },
            created_at - 2,
        )
        .unwrap();
    store
        .resolve_human_decision(
            &decision.id,
            HumanContinuation::Resume,
            "operator:test",
            created_at - 1,
        )
        .unwrap();

    let grant = RiskEscalationGrant {
        proposal_hash: changeset.proposal_hash.clone(),
        expires_at_ms: expires_at,
        consumed: false,
    };
    let resumed_snapshot = PlanningInputSnapshot::new(
        branch.clone(),
        task_id.as_str().to_owned(),
        0,
        ScopeBound::new(vec!["repo:src".to_owned()]).unwrap(),
        RiskClass::Low,
        vec![],
        trigger,
        vec![],
        Some(grant),
        policy_fingerprint.clone(),
    )
    .unwrap();
    let run = store
        .start_planning_run(&resumed_snapshot, &trigger_key, created_at - 1)
        .unwrap();
    store
        .validate_human_decision_current(&decision.id, created_at)
        .expect("exact RESUME grant snapshot must not stale its own human decision");
    let lineage = PlanRevisionLineage {
        parent_task_ref: task_id.as_str().to_owned(),
        trigger_evidence_key: trigger_key,
        planning_input_snapshot_id: resumed_snapshot.id,
        planning_input_fingerprint: resumed_snapshot.input_fingerprint,
        planning_run_id: run.id,
        planning_run_generation: run.generation,
    };
    let missing_receipt = PlanningCostReceiptRef::new("receipt-deliberately-missing").unwrap();
    let revision = PlanRevision::new(
        1,
        branch.clone(),
        0,
        changeset.proposal_hash.clone(),
        changeset.reason.clone(),
        changeset.source_evidence_refs.clone(),
        RevisionProposedBy::new(changeset.proposed_by.clone(), missing_receipt.clone()),
        ValidatedBy::new("plan-reconciler".to_owned(), policy_fingerprint).unwrap(),
        created_at,
    )
    .unwrap();

    // The decision update happens before receipt validation inside the same
    // BEGIN IMMEDIATE transaction. This forced late failure proves rollback
    // restores the decision to reusable RESOLVED state and publishes nothing.
    let error = store
        .commit_plan_revision_with_human_decision(
            &changeset,
            &revision,
            &graph_v1(),
            &lineage,
            &journal_evidence(1),
            Some(&decision.id),
            false,
        )
        .expect_err("missing receipt fails after tentative decision consumption");
    assert!(matches!(error, StorageError::ConstraintViolation(_)));
    assert_eq!(store.get_plan_head(&branch).unwrap(), None);
    let after_failure = store
        .get_human_decision(&decision.id)
        .unwrap()
        .expect("decision survives failed transaction");
    assert_eq!(after_failure.status, HumanDecisionStatus::Resolved);
    assert_eq!(after_failure.consumed_at_ms, None);

    store
        .record_plan_cost_receipt(&missing_receipt, &fixture_receipt())
        .unwrap();
    store
        .commit_plan_revision_with_human_decision(
            &changeset,
            &revision,
            &graph_v1(),
            &lineage,
            &journal_evidence(1),
            Some(&decision.id),
            false,
        )
        .expect("successful retry atomically publishes and consumes");
    assert_eq!(store.get_plan_head(&branch).unwrap(), Some(1));
    let consumed = store.get_human_decision(&decision.id).unwrap().unwrap();
    assert_eq!(consumed.status, HumanDecisionStatus::Consumed);
    assert_eq!(consumed.selected, Some(HumanContinuation::Resume));
}

#[test]
fn schema_13_revision_history_without_lineage_fails_closed_after_upgrade() {
    let db = TestDb::new("v13-lineage");
    let branch = PlanBranchId::new("branch-main").unwrap();
    let changeset = changeset(0, "first revision", &[evidence("attempt-1")]);
    {
        let mut store = db.open();
        seed_child_parent(&mut store);
        commit_revision(
            &mut store,
            &changeset,
            &revision(&branch, 1, &changeset),
            &graph_v1(),
            &lineage(),
            &journal_evidence(1),
        )
        .unwrap();
    }
    db.raw()
        .execute_batch(
            "DROP TRIGGER IF EXISTS event_journal_no_update;
             ALTER TABLE event_journal DROP COLUMN previous_hash;
             ALTER TABLE event_journal DROP COLUMN record_hash;
             DROP TRIGGER IF EXISTS event_journal_no_delete;
             DROP TRIGGER IF EXISTS event_plan_causes_no_update;
             DROP TRIGGER IF EXISTS event_plan_causes_no_delete;
             DROP TABLE task_prerequisites;
             DROP TABLE evaluated_policy_bases;
             DROP TABLE test_mutation_policy_revisions;
             DROP TABLE task_evidence_requirement_params;
             DROP TABLE task_evidence_requirements;
             DROP TABLE candidate_input_assessments;
             DROP TABLE candidate_input_operations;
             DROP TABLE observed_effects;
             DROP TABLE agent_claims;
             DROP TABLE claim_verifications;
             DROP TABLE policy_exemptions;
             DROP TABLE approval_discharges;
             DROP TABLE completion_admissions;
             DROP TABLE planning_input_candidate_tasks;
             DROP TABLE human_decision_evidence;
             DROP TABLE human_decision_options;
             DROP TABLE human_decision_scope_roots;
             DROP TABLE human_decision_requests;
             DROP TABLE planning_run_heads;
             DROP TABLE planning_runs;
             DROP TABLE planning_input_heads;
             DROP TABLE planning_input_lineage;
             DROP TABLE planning_input_evidence;
             DROP TABLE planning_input_criterion_requirement_params;
             DROP TABLE planning_input_criteria;
             DROP TABLE planning_input_scope_roots;
             DROP TABLE planning_input_snapshots;
             DROP TABLE plan_guardrail_events;
             DROP TABLE plan_revision_lineage;
             DROP TABLE review_records;
             DROP TABLE review_path_settings;
             DROP TABLE reviewer_profiles;
             DROP TABLE owner_presence_proofs;
             DROP TABLE binding_proof_consumption;
             DROP TABLE side_effect_escalations;
             DROP TABLE side_effect_transitions;
             DROP TABLE side_effect_intents;
             DROP TABLE context_trace_revs;
             DROP TABLE context_trace_decisions;
             DROP TABLE context_traces;
             DROP TABLE context_cache_boundary_items;
             DROP TABLE context_cache_boundaries;
             DROP TABLE context_plan_hashes;
             DROP TABLE context_item_transforms;
             DROP TABLE context_items;
             DROP TABLE context_plans;
             DROP TABLE continuity_receipt_refs;
             DROP TABLE continuity_budget_events;
             DROP TABLE continuity_migration_links;
             DROP TABLE continuity_session_links;
             DROP TABLE continuity_history_refs;
             DROP TABLE continuity_activations;
             DROP TABLE continuity_checkpoint_notes;
             DROP TABLE continuity_checkpoint_evidence;
             DROP TABLE continuity_checkpoint_sources;
             DROP TABLE continuity_notes;
             DROP TABLE continuity_checkpoints;
             DROP TABLE continuity_epochs;
             DROP TABLE continuity_meta;
             DROP TABLE takeover_admission_anchors;
             DROP TABLE takeover_admissions;
             DROP TABLE entity_links;
             DROP TABLE entities;
             DROP TABLE memory_links;
             DROP TABLE memory_anchors;
             DROP TABLE memory_source_refs;
             DROP TABLE memories;
             PRAGMA user_version = 13;",
        )
        .unwrap();

    let store = db.open();
    assert_eq!(store.schema_version().unwrap(), SUPPORTED_SCHEMA_VERSION);
    assert_eq!(store.get_plan_head(&branch).unwrap(), Some(1));
    assert!(
        matches!(
            store.get_plan_lineage(&branch),
            Err(StorageError::Corrupt(_))
        ),
        "guardrail lineage must not be guessed for pre-schema-14 revisions"
    );
}

#[test]
fn corrupt_plan_rows_fail_closed_on_reconstruction() {
    let db = TestDb::new("corrupt");
    let branch = PlanBranchId::new("branch-main").unwrap();
    let changeset = changeset(0, "first revision", &[evidence("attempt-1")]);
    let revision = revision(&branch, 1, &changeset);
    {
        let mut store = db.open();
        seed_child_parent(&mut store);
        commit_revision(
            &mut store,
            &changeset,
            &revision,
            &graph_v1(),
            &lineage(),
            &journal_evidence(1),
        )
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

// ---------------------------------------------------------------------------
// TASK-0087: atomic child Task materialization
// ---------------------------------------------------------------------------

const CHILD_PARENT: &str = "task-root";

fn child_project() -> ProjectId {
    ProjectId::new("P-CHILD-MATERIALIZATION").unwrap()
}

/// Project plus the DRAFT plan parent every fixture revision's lineage names.
/// The plan parent every publication needs: publication now proves the owning
/// Task exists and authorizes decomposition, so READY is the fixture baseline.
fn seed_child_parent(store: &mut SqliteStore) {
    seed_child_parent_in_state(store, TaskState::Ready);
}

fn child_event(
    task: &Task,
    event_type: EventType,
    to_state: TaskState,
    causation: &str,
) -> NewJournalEvent {
    NewJournalEvent {
        event_type,
        task_id: Some(task.id().clone()),
        project_id: Some(task.project_id().clone()),
        from_state: Some(TaskState::Draft),
        to_state: Some(to_state),
        reason: None,
        correlation_id: format!("plan-child:{}", task.id().as_str()),
        causation_id: causation.to_owned(),
        occurred_at: "2026-09-17T10:00:00Z".to_owned(),
        provenance: CHILD_TASK_MATERIALIZATION_PROVENANCE.to_owned(),
        execution_id: None,
        attempt_id: None,
        attempt_outcome: None,
        workspace_id: None,
        from_workspace_state: None,
        to_workspace_state: None,
        plan_revision: None,
    }
}

/// The READY child plus its `task.created`/`task.marked_ready` facts, exactly
/// as the planning application service mints them for `changeset`.
fn materialized_child(id: &str, changeset: &PlanChangeSet) -> ChildTaskMaterialization {
    let mut task = Task::draft(
        TaskId::new(id).unwrap(),
        child_project(),
        format!("planned child task {id}"),
    )
    .unwrap();
    task.set_parent(TaskId::new(CHILD_PARENT).unwrap()).unwrap();
    task.mark_ready().unwrap();
    child_materialization(task, changeset)
}

fn child_materialization(task: Task, changeset: &PlanChangeSet) -> ChildTaskMaterialization {
    ChildTaskMaterialization {
        created_event: child_event(
            &task,
            EventType::TaskCreated,
            TaskState::Draft,
            &changeset.proposal_hash,
        ),
        ready_event: child_event(
            &task,
            EventType::MarkedReady,
            TaskState::Ready,
            &changeset.proposal_hash,
        ),
        task,
    }
}

/// `changeset` with a second node addition `reviewer`, rehashed.
fn two_child_changeset(reason: &str) -> PlanChangeSet {
    let mut two = changeset(0, reason, &[evidence("child-attempt")]);
    let mut reviewer = two.node_additions[0].clone();
    reviewer.id = nid("reviewer");
    two.node_additions.push(reviewer);
    two.proposal_hash = compute_proposal_hash(&two);
    two
}

/// Publishes genesis revision 1 of `branch` with `children`, bound to a fresh
/// planning run for the child parent.
fn commit_with_children(
    store: &mut SqliteStore,
    branch: &PlanBranchId,
    changeset: &PlanChangeSet,
    receipt_ref: &str,
    children: &[ChildTaskMaterialization],
) -> storage_sqlite::StorageResult<storage_sqlite::JournalEvent> {
    let revision = PlanRevision::new(
        1,
        branch.clone(),
        0,
        changeset.proposal_hash.clone(),
        changeset.reason.clone(),
        changeset.source_evidence_refs.clone(),
        RevisionProposedBy::new(
            changeset.proposed_by.clone(),
            PlanningCostReceiptRef::new(receipt_ref).unwrap(),
        ),
        ValidatedBy::new("plan-reconciler".to_owned(), "policy:abc123".to_owned()).unwrap(),
        1_800_000_000_001,
    )
    .unwrap();
    let trigger = changeset.source_evidence_refs.clone();
    let trigger_key = planning_evidence_key(&trigger);
    let snapshot = PlanningInputSnapshot::new(
        branch.clone(),
        CHILD_PARENT.to_owned(),
        0,
        ScopeBound::new(vec!["repo:src".to_owned()]).unwrap(),
        RiskClass::Low,
        vec![],
        trigger,
        vec![],
        None,
        revision.validated_by.policy_fingerprint.clone(),
    )
    .unwrap();
    let run = store
        .start_planning_run(&snapshot, &trigger_key, revision.created_at - 1)
        .unwrap();
    let lineage = PlanRevisionLineage {
        parent_task_ref: CHILD_PARENT.to_owned(),
        trigger_evidence_key: trigger_key,
        planning_input_snapshot_id: snapshot.id,
        planning_input_fingerprint: snapshot.input_fingerprint,
        planning_run_id: run.id,
        planning_run_generation: run.generation,
    };
    store.commit_plan_revision_with_human_decision_and_children(CommitPlanRevisionRequest {
        changeset,
        revision: &revision,
        resulting_graph: &graph_v1(),
        lineage: &lineage,
        evidence: &journal_evidence(1),
        risk_decision: None,
        canonical_task_authority: false,
        child_tasks: children,
    })
}

fn task_event_types(store: &SqliteStore, id: &str) -> Vec<EventType> {
    store
        .list_events_for_task(&TaskId::new(id).unwrap())
        .unwrap()
        .into_iter()
        .map(|event| event.fact.event_type)
        .collect()
}

/// Seeds the child-materialization parent in `state`. States with accepted
/// incoming transitions are reached through the domain; FAILED and CANCELLED
/// have none (task-engine.md transition registry), so they are reconstructed
/// the only way persistence can hold them.
fn seed_child_parent_in_state(store: &mut SqliteStore, state: TaskState) {
    store
        .create_project(&Project::new(child_project(), "child materialization".to_owned()).unwrap())
        .unwrap();
    let mut task = Task::draft(
        TaskId::new(CHILD_PARENT).unwrap(),
        child_project(),
        "plan parent".to_owned(),
    )
    .unwrap();
    let reached = match state {
        TaskState::Draft => true,
        TaskState::Ready => task.mark_ready().is_ok(),
        TaskState::Claimed => task.mark_ready().and_then(|_| task.claim()).is_ok(),
        TaskState::Running => task
            .mark_ready()
            .and_then(|_| task.claim())
            .and_then(|_| task.start())
            .is_ok(),
        TaskState::Blocked => task
            .mark_ready()
            .and_then(|_| task.claim())
            .and_then(|_| task.start())
            .and_then(|_| task.block("waiting on decomposition"))
            .is_ok(),
        TaskState::Submitted => running_then(&mut task, RunningTail::Submitted),
        TaskState::AwaitingReview => running_then(&mut task, RunningTail::AwaitingReview),
        TaskState::Verifying => running_then(&mut task, RunningTail::Verifying),
        TaskState::RetryReady => running_then(&mut task, RunningTail::RetryReady),
        TaskState::Completed => running_then(&mut task, RunningTail::Completed),
        TaskState::Failed | TaskState::Cancelled => {
            let mut snapshot = task.snapshot();
            snapshot.state = state;
            task = Task::restore(snapshot).unwrap();
            true
        }
    };
    assert!(reached, "could not reach {state} through the domain");
    assert_eq!(task.state(), state, "parent fixture state");
    store.create_task(&task).unwrap();
}

enum RunningTail {
    Submitted,
    AwaitingReview,
    Verifying,
    RetryReady,
    Completed,
}

fn running_then(task: &mut Task, tail: RunningTail) -> bool {
    let started = task
        .mark_ready()
        .and_then(|_| task.claim())
        .and_then(|_| task.start())
        .and_then(|_| task.submit_result("attempt output"));
    if started.is_err() {
        return false;
    }
    let outcome = match tail {
        RunningTail::Submitted => Ok(task.state()),
        RunningTail::AwaitingReview => task.route_to_review(),
        RunningTail::Verifying => task
            .route_to_review()
            .and_then(|_| task.begin_verification()),
        RunningTail::RetryReady => task
            .route_to_review()
            .and_then(|_| task.begin_verification())
            .and_then(|_| task.verification_failed("rejected")),
        RunningTail::Completed => task
            .route_to_review()
            .and_then(|_| task.begin_verification())
            .and_then(|_| {
                task.apply(TransitionCommand::VerificationSucceeded {
                    verdict: "verified".to_owned(),
                })
            }),
    };
    outcome.is_ok()
}

/// Everything a failed materializing commit must leave untouched.
fn assert_rolled_back(
    store: &SqliteStore,
    branch: &PlanBranchId,
    events_before: usize,
    tasks_before: usize,
) {
    assert_eq!(store.get_plan_head(branch).unwrap(), None);
    assert!(store.get_plan_lineage(branch).unwrap().is_empty());
    assert_eq!(store.list_events_ordered().unwrap().len(), events_before);
    assert_eq!(
        store
            .list_tasks_for_project(&child_project())
            .unwrap()
            .len(),
        tasks_before
    );
}

/// ADR-0028 publication fencing item 4 and the ADR-0027 TASK-0091 amendment:
/// decomposition publishes only while the owning parent is doing work. DRAFT
/// is excluded because planning metadata is still authorable there, the
/// review/verification states because the parent already claimed its work is
/// finished, and the terminal states because the work is over.
#[test]
fn publication_requires_the_parent_task_in_the_admission_set() {
    for state in TaskState::all() {
        let admitted = matches!(
            state,
            TaskState::Ready
                | TaskState::Claimed
                | TaskState::Running
                | TaskState::Blocked
                | TaskState::RetryReady
        );
        let label = format!("parent {state}");
        let db = TestDb::new(&format!("admission-{state}"));
        let mut store = db.open();
        seed_child_parent_in_state(&mut store, state);

        let changeset = two_child_changeset("materialize under a parent in a given state");
        let child = materialized_child("worker", &changeset);
        let branch = PlanBranchId::new("branch-admission").unwrap();
        let events_before = store.list_events_ordered().unwrap().len();
        let tasks_before = store
            .list_tasks_for_project(&child_project())
            .unwrap()
            .len();

        let result = commit_with_children(&mut store, &branch, &changeset, "receipt-1", &[child]);

        if admitted {
            result.unwrap_or_else(|error| panic!("{label}: admitted state refused: {error:?}"));
            assert_eq!(store.get_plan_head(&branch).unwrap(), Some(1), "{label}");
            assert_eq!(
                store
                    .get_task(&TaskId::new("worker").unwrap())
                    .unwrap()
                    .map(|task| task.state()),
                Some(TaskState::Ready),
                "{label}"
            );
        } else {
            let error = result.expect_err(&format!("{label}: must refuse publication"));
            assert!(
                matches!(error, StorageError::ConstraintViolation(_)),
                "{label}: unexpected error {error:?}"
            );
            assert_rolled_back(&store, &branch, events_before, tasks_before);
            assert!(
                store
                    .get_task(&TaskId::new("worker").unwrap())
                    .unwrap()
                    .is_none(),
                "{label}: child must not survive a refused publication"
            );
        }
    }
}

/// ADR-0027 TASK-0086: "project identity is inherited from the owning parent
/// and cannot be selected by planner output", with "parent/project mismatch:
/// refuse" required at this trusted boundary — not only in the caller that
/// happens to derive the project correctly today.
#[test]
fn child_materialization_requires_the_parent_project() {
    let db = TestDb::new("child-foreign-project");
    let mut store = db.open();
    seed_child_parent_in_state(&mut store, TaskState::Ready);
    let foreign = ProjectId::new("P-FOREIGN").unwrap();
    store
        .create_project(&Project::new(foreign.clone(), "foreign project".to_owned()).unwrap())
        .unwrap();

    let changeset = two_child_changeset("materialize a child into a foreign project");
    let mut task = Task::draft(
        TaskId::new("worker").unwrap(),
        foreign,
        "planned child task worker".to_owned(),
    )
    .unwrap();
    task.set_parent(TaskId::new(CHILD_PARENT).unwrap()).unwrap();
    task.mark_ready().unwrap();
    let child = child_materialization(task, &changeset);

    let branch = PlanBranchId::new("branch-foreign-child").unwrap();
    let events_before = store.list_events_ordered().unwrap().len();
    let tasks_before = store
        .list_tasks_for_project(&child_project())
        .unwrap()
        .len();

    let error = commit_with_children(&mut store, &branch, &changeset, "receipt-1", &[child])
        .expect_err("a child outside the parent project must be refused");
    assert!(
        matches!(error, StorageError::ConstraintViolation(_)),
        "unexpected error {error:?}"
    );
    assert_rolled_back(&store, &branch, events_before, tasks_before);
    assert!(store
        .get_task(&TaskId::new("worker").unwrap())
        .unwrap()
        .is_none());
}

/// The publication boundary must prove the owning parent exists rather than
/// trusting the lineage reference.
#[test]
fn publication_requires_an_existing_parent_task() {
    let db = TestDb::new("missing-parent");
    let mut store = db.open();
    store
        .create_project(&Project::new(child_project(), "child materialization".to_owned()).unwrap())
        .unwrap();

    let changeset = changeset(0, "publish without a parent task", &[evidence("no-parent")]);
    let branch = PlanBranchId::new("branch-missing-parent").unwrap();
    let events_before = store.list_events_ordered().unwrap().len();

    let error = commit_with_children(&mut store, &branch, &changeset, "receipt-1", &[])
        .expect_err("a missing parent task must refuse publication");
    assert!(
        matches!(error, StorageError::ConstraintViolation(_)),
        "unexpected error {error:?}"
    );
    assert_eq!(store.get_plan_head(&branch).unwrap(), None);
    assert_eq!(store.list_events_ordered().unwrap().len(), events_before);
}

#[test]
fn child_materialization_commits_ready_children_and_reuses_them_on_exact_retry() {
    let db = TestDb::new("child-exact-retry");
    let mut store = db.open();
    seed_child_parent(&mut store);
    let changeset = changeset(0, "materialize worker", &[evidence("child-attempt")]);
    let child = materialized_child("worker", &changeset);

    commit_with_children(
        &mut store,
        &PlanBranchId::new("branch-child-a").unwrap(),
        &changeset,
        "receipt-1",
        std::slice::from_ref(&child),
    )
    .expect("genesis revision materializes its child");
    let worker = TaskId::new("worker").unwrap();
    let stored = store.get_task(&worker).unwrap().expect("child row");
    assert_eq!(stored.snapshot(), child.task.snapshot());
    assert_eq!(stored.state(), TaskState::Ready);
    assert_eq!(
        task_event_types(&store, "worker"),
        [EventType::TaskCreated, EventType::MarkedReady]
    );

    // Same id, provenance, causation and snapshot: reused, never duplicated.
    let retry_branch = PlanBranchId::new("branch-child-b").unwrap();
    commit_with_children(
        &mut store,
        &retry_branch,
        &changeset,
        "receipt-1",
        std::slice::from_ref(&child),
    )
    .expect("exact retry reconciles to the existing child");
    assert_eq!(store.get_plan_head(&retry_branch).unwrap(), Some(1));
    assert_eq!(
        store.get_task(&worker).unwrap().unwrap().snapshot(),
        child.task.snapshot()
    );
    assert_eq!(
        task_event_types(&store, "worker"),
        [EventType::TaskCreated, EventType::MarkedReady]
    );
    assert_eq!(
        store
            .list_tasks_for_project(&child_project())
            .unwrap()
            .len(),
        2,
        "parent plus exactly one child row"
    );
}

#[test]
fn malformed_persisted_child_retry_evidence_fails_closed_without_partial_state() {
    type Mutation = fn(&rusqlite::Connection);

    let cases: [(&str, Mutation); 20] = [
        ("missing-created", |connection| {
            connection
                .execute(
                    "DELETE FROM event_journal
                     WHERE task_id = 'worker' AND event_type = 'task.created'",
                    [],
                )
                .unwrap();
        }),
        ("malformed-created-type", |connection| {
            connection
                .execute(
                    "UPDATE event_journal SET event_type = 'task.claimed'
                     WHERE task_id = 'worker' AND event_type = 'task.created'",
                    [],
                )
                .unwrap();
        }),
        ("wrong-created-provenance", |connection| {
            connection
                .execute(
                    "UPDATE event_journal SET provenance = 'task-service/create_task'
                     WHERE task_id = 'worker' AND event_type = 'task.created'",
                    [],
                )
                .unwrap();
        }),
        ("wrong-created-causation", |connection| {
            connection
                .execute(
                    "UPDATE event_journal SET causation_id = 'sha256:foreign'
                     WHERE task_id = 'worker' AND event_type = 'task.created'",
                    [],
                )
                .unwrap();
        }),
        ("wrong-created-task", |connection| {
            connection
                .execute(
                    "UPDATE event_journal SET task_id = 'task-root'
                     WHERE task_id = 'worker' AND event_type = 'task.created'",
                    [],
                )
                .unwrap();
        }),
        ("wrong-created-project", |connection| {
            connection
                .execute(
                    "UPDATE event_journal SET project_id = 'P-FOREIGN'
                     WHERE task_id = 'worker' AND event_type = 'task.created'",
                    [],
                )
                .unwrap();
        }),
        ("wrong-ready-project", |connection| {
            connection
                .execute(
                    "UPDATE event_journal SET project_id = 'P-FOREIGN'
                     WHERE task_id = 'worker' AND event_type = 'task.marked_ready'",
                    [],
                )
                .unwrap();
        }),
        ("malformed-created-envelope", |connection| {
            connection
                .execute_batch("PRAGMA ignore_check_constraints = ON;")
                .unwrap();
            connection
                .execute(
                    "UPDATE event_journal SET correlation_id = ''
                     WHERE task_id = 'worker' AND event_type = 'task.created'",
                    [],
                )
                .unwrap();
            connection
                .execute_batch("PRAGMA ignore_check_constraints = OFF;")
                .unwrap();
        }),
        ("contradictory-ready-count", |connection| {
            connection
                .execute(
                    "INSERT INTO event_journal
                     (event_type, event_version, task_id, project_id, from_state, to_state,
                      reason, correlation_id, causation_id, occurred_at, provenance)
                     SELECT event_type, event_version, task_id, project_id, from_state, to_state,
                            reason, correlation_id, causation_id, occurred_at, provenance
                     FROM event_journal
                     WHERE task_id = 'worker' AND event_type = 'task.marked_ready'",
                    [],
                )
                .unwrap();
        }),
        ("missing-ready", |connection| {
            connection
                .execute(
                    "DELETE FROM event_journal
                     WHERE task_id = 'worker' AND event_type = 'task.marked_ready'",
                    [],
                )
                .unwrap();
        }),
        ("malformed-ready-type", |connection| {
            connection
                .execute(
                    "UPDATE event_journal SET event_type = 'task.claimed'
                     WHERE task_id = 'worker' AND event_type = 'task.marked_ready'",
                    [],
                )
                .unwrap();
        }),
        ("wrong-ready-states", |connection| {
            connection
                .execute_batch("PRAGMA ignore_check_constraints = ON;")
                .unwrap();
            connection
                .execute(
                    "UPDATE event_journal SET from_state = 'READY', to_state = 'CLAIMED'
                     WHERE task_id = 'worker' AND event_type = 'task.marked_ready'",
                    [],
                )
                .unwrap();
            connection
                .execute_batch("PRAGMA ignore_check_constraints = OFF;")
                .unwrap();
        }),
        ("wrong-ready-provenance", |connection| {
            connection
                .execute(
                    "UPDATE event_journal SET provenance = 'task-service/mark_ready'
                     WHERE task_id = 'worker' AND event_type = 'task.marked_ready'",
                    [],
                )
                .unwrap();
        }),
        ("wrong-ready-causation", |connection| {
            connection
                .execute(
                    "UPDATE event_journal SET causation_id = 'sha256:foreign'
                     WHERE task_id = 'worker' AND event_type = 'task.marked_ready'",
                    [],
                )
                .unwrap();
        }),
        ("wrong-ready-task", |connection| {
            connection
                .execute(
                    "UPDATE event_journal SET task_id = 'task-root'
                     WHERE task_id = 'worker' AND event_type = 'task.marked_ready'",
                    [],
                )
                .unwrap();
        }),
        ("malformed-ready-envelope", |connection| {
            connection
                .execute_batch("PRAGMA ignore_check_constraints = ON;")
                .unwrap();
            connection
                .execute(
                    "UPDATE event_journal SET correlation_id = ''
                     WHERE task_id = 'worker' AND event_type = 'task.marked_ready'",
                    [],
                )
                .unwrap();
            connection
                .execute_batch("PRAGMA ignore_check_constraints = OFF;")
                .unwrap();
        }),
        ("malformed-created-states", |connection| {
            connection
                .execute_batch("PRAGMA ignore_check_constraints = ON;")
                .unwrap();
            connection
                .execute(
                    "UPDATE event_journal SET from_state = 'READY', to_state = 'READY'
                     WHERE task_id = 'worker' AND event_type = 'task.created'",
                    [],
                )
                .unwrap();
            connection
                .execute_batch("PRAGMA ignore_check_constraints = OFF;")
                .unwrap();
        }),
        ("contradictory-created-count", |connection| {
            connection
                .execute(
                    "INSERT INTO event_journal
                     (event_type, event_version, task_id, project_id, from_state, to_state,
                      reason, correlation_id, causation_id, occurred_at, provenance)
                     SELECT event_type, event_version, task_id, project_id, 'DRAFT', 'DRAFT',
                            reason, 'plan-child:contradictory', causation_id, occurred_at,
                            provenance
                     FROM event_journal
                     WHERE task_id = 'worker' AND event_type = 'task.created'",
                    [],
                )
                .unwrap();
        }),
        // The required pair is intact, but the child carries a third fact.
        // No shipped path can produce one without also moving the Task row —
        // claim/start/execution/workspace share one transaction and
        // dependency_added changes `depends_on` — so a child that has one is
        // not the clean materialization this retry claims to repeat. Here the
        // journal says BLOCKED while the Task row still says READY.
        ("foreign-lifecycle-fact", |connection| {
            connection
                .execute(
                    "INSERT INTO event_journal
                     (event_type, event_version, task_id, project_id, from_state, to_state,
                      reason, correlation_id, causation_id, occurred_at, provenance)
                     SELECT 'task.blocked', event_version, task_id, project_id, 'READY',
                            'BLOCKED', 'impediment', 'plan-child:foreign', causation_id,
                            occurred_at, provenance
                     FROM event_journal
                     WHERE task_id = 'worker' AND event_type = 'task.marked_ready'",
                    [],
                )
                .unwrap();
        }),
        // The pair is complete and individually valid, but readiness is
        // journaled before creation. `seq` is the journal's order, so this is
        // a contradictory lifecycle, not an exact retry.
        ("ready-before-created", |connection| {
            connection
                .execute(
                    "UPDATE event_journal SET seq = 900001
                     WHERE task_id = 'worker' AND event_type = 'task.created'",
                    [],
                )
                .unwrap();
            connection
                .execute(
                    "UPDATE event_journal SET seq = 900000
                     WHERE task_id = 'worker' AND event_type = 'task.marked_ready'",
                    [],
                )
                .unwrap();
        }),
    ];

    for (label, mutate) in cases {
        let db = TestDb::new(label);
        let mut store = db.open();
        seed_child_parent(&mut store);
        store
            .create_project(
                &Project::new(
                    ProjectId::new("P-FOREIGN").unwrap(),
                    "foreign project".to_owned(),
                )
                .unwrap(),
            )
            .unwrap();
        let changeset = two_child_changeset("materialize worker with retry sibling");
        let child = materialized_child("worker", &changeset);
        commit_with_children(
            &mut store,
            &PlanBranchId::new("branch-child-seed").unwrap(),
            &changeset,
            "receipt-1",
            std::slice::from_ref(&child),
        )
        .expect("seed a complete child materialization");

        let worker = TaskId::new("worker").unwrap();
        let worker_before = store.get_task(&worker).unwrap().unwrap().snapshot();
        let tasks_before = store
            .list_tasks_for_project(&child_project())
            .unwrap()
            .len();

        db.raw()
            .execute_batch("DROP TRIGGER IF EXISTS event_journal_no_update; DROP TRIGGER IF EXISTS event_journal_no_delete; DROP TRIGGER IF EXISTS event_plan_causes_no_update; DROP TRIGGER IF EXISTS event_plan_causes_no_delete;")
            .unwrap();
        mutate(&db.raw());
        storage_sqlite::test_support::reseal_journal_chain(&db.path).unwrap();
        // The mutation deliberately corrupts the persisted evidence. Capture
        // that prepared state now; the retry itself must not add anything on
        // top of it or alter the existing Task/facts.
        let facts_at_retry = store.list_events_for_task(&worker).unwrap();
        let events_at_retry = store.list_events_ordered().unwrap();

        // The retry must be rejected by storage before its branch, sibling,
        // revision, lineage, or plan journal fact becomes durable.
        let retry_branch = PlanBranchId::new("branch-child-retry").unwrap();
        let error = commit_with_children(
            &mut store,
            &retry_branch,
            &changeset,
            "receipt-1",
            &[materialized_child("reviewer", &changeset), child],
        )
        .expect_err("malformed or incomplete persisted evidence must not be an exact retry");
        assert!(
            matches!(
                error,
                StorageError::ConstraintViolation(_)
                    | StorageError::Corrupt(_)
                    | StorageError::DomainReconstruction(_)
            ),
            "{label}: unexpected error {error:?}"
        );
        assert_eq!(store.get_plan_head(&retry_branch).unwrap(), None, "{label}");
        assert_eq!(
            store.get_plan_revision(&retry_branch, 1).unwrap(),
            None,
            "{label}"
        );
        assert!(
            store
                .get_task(&TaskId::new("reviewer").unwrap())
                .unwrap()
                .is_none(),
            "{label}: sibling inserted before the conflict must roll back"
        );
        assert!(
            store.get_plan_lineage(&retry_branch).unwrap().is_empty(),
            "{label}"
        );
        assert_eq!(
            store.list_events_ordered().unwrap(),
            events_at_retry,
            "{label}"
        );
        assert_eq!(
            store
                .list_tasks_for_project(&child_project())
                .unwrap()
                .len(),
            tasks_before,
            "{label}: no sibling child remains"
        );
        assert_eq!(
            store.get_task(&worker).unwrap().unwrap().snapshot(),
            worker_before,
            "{label}: existing Task changed"
        );
        assert_eq!(
            store.list_events_for_task(&worker).unwrap(),
            facts_at_retry,
            "{label}: facts changed"
        );
    }
}

#[test]
fn conflicting_child_materialization_fails_closed_without_partial_state() {
    // Every case stores a `worker` Task that is not an exact retry of the
    // `competing` materialization, then commits `competing` with a brand-new
    // `reviewer` child inserted before `worker` conflicts.
    let competing = two_child_changeset("competing decomposition");
    let other = changeset(0, "materialize worker", &[evidence("child-attempt")]);

    let foreign_provenance = |store: &mut SqliteStore| {
        let task = materialized_child("worker", &competing).task;
        let mut created = child_event(
            &task,
            EventType::TaskCreated,
            TaskState::Ready,
            &competing.proposal_hash,
        );
        created.from_state = Some(TaskState::Ready);
        created.provenance = "task-service/create_task".to_owned();
        store.create_task_with_event(&task, &created).unwrap();
    };
    let other_causation = |store: &mut SqliteStore| {
        commit_with_children(
            store,
            &PlanBranchId::new("branch-child-other").unwrap(),
            &other,
            "receipt-1",
            &[materialized_child("worker", &other)],
        )
        .unwrap();
    };
    let snapshot_mismatch = |store: &mut SqliteStore| {
        let mut drifted = materialized_child("worker", &competing);
        drifted.task = Task::restore(task_domain::TaskSnapshot {
            objective: "a drifted objective".to_owned(),
            ..drifted.task.snapshot()
        })
        .unwrap();
        commit_with_children(
            store,
            &PlanBranchId::new("branch-child-drift").unwrap(),
            &competing,
            "receipt-1",
            &[drifted],
        )
        .unwrap();
    };

    type Setup<'a> = &'a dyn Fn(&mut SqliteStore);
    let cases: [(&str, Setup); 3] = [
        ("foreign-provenance", &foreign_provenance),
        ("other-proposal-causation", &other_causation),
        ("snapshot-mismatch", &snapshot_mismatch),
    ];
    for (label, setup) in cases {
        let db = TestDb::new(label);
        let mut store = db.open();
        seed_child_parent(&mut store);
        setup(&mut store);
        let worker = TaskId::new("worker").unwrap();
        let worker_before = store.get_task(&worker).unwrap().unwrap().snapshot();
        let worker_events = task_event_types(&store, "worker");
        let events_before = store.list_events_ordered().unwrap().len();
        let tasks_before = store
            .list_tasks_for_project(&child_project())
            .unwrap()
            .len();

        let branch = PlanBranchId::new("branch-child-conflict").unwrap();
        let error = commit_with_children(
            &mut store,
            &branch,
            &competing,
            "receipt-1",
            &[
                materialized_child("reviewer", &competing),
                materialized_child("worker", &competing),
            ],
        )
        .expect_err("a conflicting child identity fails the whole commit");
        assert!(
            matches!(&error, StorageError::ConstraintViolation(detail)
                if detail.contains("worker conflicts with an existing task")),
            "{label}: {error:?}"
        );
        assert_rolled_back(&store, &branch, events_before, tasks_before);
        assert!(
            store
                .get_task(&TaskId::new("reviewer").unwrap())
                .unwrap()
                .is_none(),
            "{label}: the sibling inserted before the conflict rolled back"
        );
        assert_eq!(
            store.get_task(&worker).unwrap().unwrap().snapshot(),
            worker_before,
            "{label}"
        );
        assert_eq!(task_event_types(&store, "worker"), worker_events, "{label}");
    }
}

#[test]
fn late_commit_failure_rolls_back_materialized_children_then_retry_publishes() {
    let db = TestDb::new("child-late-failure");
    let mut store = db.open();
    seed_child_parent(&mut store);
    let changeset = two_child_changeset("late failure");
    let children = [
        materialized_child("worker", &changeset),
        materialized_child("reviewer", &changeset),
    ];
    let branch = PlanBranchId::new("branch-child-late").unwrap();
    let events_before = store.list_events_ordered().unwrap().len();

    // Children are inserted before the receipt check inside the same
    // transaction; the forced late failure must discard them.
    let error = commit_with_children(
        &mut store,
        &branch,
        &changeset,
        "receipt-child-missing",
        &children,
    )
    .expect_err("unresolvable receipt fails after child inserts");
    assert!(matches!(error, StorageError::ConstraintViolation(_)));
    assert_rolled_back(&store, &branch, events_before, 1);

    store
        .record_plan_cost_receipt(
            &PlanningCostReceiptRef::new("receipt-child-missing").unwrap(),
            &fixture_receipt(),
        )
        .unwrap();
    commit_with_children(
        &mut store,
        &branch,
        &changeset,
        "receipt-child-missing",
        &children,
    )
    .expect("retry publishes revision and children together");
    assert_eq!(store.get_plan_head(&branch).unwrap(), Some(1));
    for id in ["worker", "reviewer"] {
        assert_eq!(
            store
                .get_task(&TaskId::new(id).unwrap())
                .unwrap()
                .unwrap()
                .state(),
            TaskState::Ready
        );
        assert_eq!(
            task_event_types(&store, id),
            [EventType::TaskCreated, EventType::MarkedReady]
        );
    }
}

#[test]
fn malformed_child_materialization_is_refused_before_any_write() {
    let base = changeset(0, "materialize worker", &[evidence("child-attempt")]);
    type Mutation = fn(&mut ChildTaskMaterialization, &mut PlanChangeSet);
    let cases: [(&str, &str, Mutation); 7] = [
        (
            "parent-as-child",
            "not owned by the plan revision parent",
            |child, changeset| {
                // A proposed node named after the parent; the domain already
                // refuses a self-parent, so the READY task is parentless.
                changeset.node_additions[0].id = nid(CHILD_PARENT);
                changeset.proposal_hash = compute_proposal_hash(changeset);
                let mut parent = Task::draft(
                    TaskId::new(CHILD_PARENT).unwrap(),
                    child_project(),
                    "plan parent".to_owned(),
                )
                .unwrap();
                parent.mark_ready().unwrap();
                *child = child_materialization(parent, changeset);
            },
        ),
        (
            "not-a-proposed-node",
            "not a node addition",
            |child, changeset| {
                *child = materialized_child("unproposed", changeset);
            },
        ),
        (
            "foreign-parent",
            "not owned by the plan revision parent",
            |child, _| {
                child.task = Task::restore(task_domain::TaskSnapshot {
                    parent: Some(TaskId::new("someone-else").unwrap()),
                    ..child.task.snapshot()
                })
                .unwrap();
            },
        ),
        (
            "draft-placeholder",
            "must be materialized READY",
            |child, _| {
                child.task = Task::restore(task_domain::TaskSnapshot {
                    state: TaskState::Draft,
                    ..child.task.snapshot()
                })
                .unwrap();
            },
        ),
        (
            "foreign-provenance",
            "not minted by this plan",
            |child, _| {
                child.created_event.provenance = "task-service/create_task".to_owned();
            },
        ),
        ("other-causation", "not minted by this plan", |child, _| {
            child.ready_event.causation_id = "another-proposal".to_owned();
        }),
        (
            "ready-fact-type",
            "malformed task.marked_ready fact",
            |child, _| {
                child.ready_event.event_type = EventType::Claimed;
            },
        ),
    ];
    for (label, reason, mutate) in cases {
        let db = TestDb::new(label);
        let mut store = db.open();
        seed_child_parent(&mut store);
        let events_before = store.list_events_ordered().unwrap().len();
        let mut changeset = base.clone();
        let mut child = materialized_child("worker", &changeset);
        mutate(&mut child, &mut changeset);

        let branch = PlanBranchId::new("branch-child-malformed").unwrap();
        let error = commit_with_children(&mut store, &branch, &changeset, "receipt-1", &[child])
            .expect_err("malformed child materialization is refused");
        assert!(
            matches!(&error, StorageError::ConstraintViolation(detail) if detail.contains(reason)),
            "{label}: {error:?}"
        );
        assert_rolled_back(&store, &branch, events_before, 1);
        let parent = store
            .get_task(&TaskId::new(CHILD_PARENT).unwrap())
            .unwrap()
            .unwrap();
        assert_eq!((parent.state(), parent.parent()), (TaskState::Ready, None));
    }
}
