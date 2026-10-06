//! Completion admission binding tests (TASK-0125, ADR-0030 CV-1..CV-8):
//! a Task must not reach COMPLETED on an agent self-claim alone; every
//! acceptance criterion needs a current VERIFIED `ClaimVerification`, a
//! CV-4 policy exemption, or a CV-8 human-approval discharge. These tests
//! drive the shipped trusted boundary (TaskService + durable store), not
//! the pure vocabulary alone.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use agent_runtime::{AttemptId, Execution, ExecutionId, ProviderRef};
use app_services::{
    bind_candidate_input_params, mint_candidate_verdicts, FixedClock, FreshPresenceProof,
    ProposedCandidateBinding, ServiceError, TaskService,
};
use evaluation::{
    candidate_sha256, requirement_revision, subject_fingerprint, CandidateCheckpointPhase,
    CandidateDeletion, CandidateFreshnessCheckpoint, CandidateIntegrityAssessment,
    CandidateIntegrityOutcome, ClaimVerdict, ClaimedOutcome, ClaimedSubject, CriterionStatus,
    EffectKind, EffectPayload, EffectProvenance, EffectSubjectRef, EvidenceRefs, ObservedEffect,
    Sha256Digest, SubjectFingerprint,
};
use plan_domain::RiskClass;
use policy::{CandidateSelectorCatalog, CandidateSelectorCatalogRevision};
use storage_sqlite::{EventType, NewJournalEvent};
use task_domain::{
    EvidenceRequirement, Project, ProjectId, RequirementKind, SubjectBinding, Task, TaskId,
    TaskState,
};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// 2024-01-01T00:00:00Z in millis; the canonical stamp every fixture uses.
const FIXED_MILLIS: u64 = 1_704_067_200_000;
const FIXED_STAMP: &str = "2024-01-01T00:00:00Z";

struct TestDb {
    path: PathBuf,
}

impl TestDb {
    fn new(suite: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "runlith-admission-test-{}-{suite}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("test dir");
        Self {
            path: dir.join("test.db"),
        }
    }

    fn service(&self) -> TaskService<FixedClock> {
        TaskService::open(&self.path, FixedClock(FIXED_MILLIS)).expect("open service")
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

/// Ensures the shared test project exists and returns its id.
fn ensure_project(service: &mut TaskService<FixedClock>) -> ProjectId {
    let project_id = ProjectId::new("P-1").unwrap();
    if service.store().get_project(&project_id).unwrap().is_none() {
        service
            .create_project(&Project::new(project_id.clone(), "project P-1").unwrap())
            .unwrap();
    }
    project_id
}

/// Seeds a DRAFT task with explicit evidence requirements.
fn seed_task(
    service: &mut TaskService<FixedClock>,
    id: &str,
    requirements: Vec<EvidenceRequirement>,
) -> TaskId {
    let project_id = ensure_project(service);
    let task_id = TaskId::new(id).unwrap();
    let mut task = Task::draft(task_id.clone(), project_id, format!("objective {id}")).unwrap();
    task.define_planning_metadata(
        vec!["src/**".to_owned()],
        requirements
            .iter()
            .map(|requirement| requirement.criterion_id.clone())
            .collect(),
        "low",
    )
    .unwrap();
    task.define_evidence_requirements(requirements).unwrap();
    service.create_task(&task).unwrap();
    task_id
}

/// Drives DRAFT → … → VERIFYING through the trusted lifecycle.
fn drive_to_verifying(service: &mut TaskService<FixedClock>, task_id: &TaskId) {
    service.mark_ready(task_id).unwrap();
    service.claim(task_id).unwrap();
    service.start(task_id).unwrap();
    service.submit_result(task_id, "done, trust me").unwrap();
    service.route_to_review(task_id).unwrap();
    service.begin_verification(task_id).unwrap();
}

/// Seeds a one-criterion Task (explicit `TEST_SUITE` default requirement)
/// and drives it to VERIFYING.
fn seed_verifying_task(service: &mut TaskService<FixedClock>, id: &str, criterion: &str) -> TaskId {
    let task_id = seed_task(
        service,
        id,
        vec![EvidenceRequirement::authored_default(criterion).unwrap()],
    );
    drive_to_verifying(service, &task_id);
    task_id
}

#[test]
fn cv1_completion_on_agent_claim_alone_is_refused() {
    let db = TestDb::new("cv1-claim-alone");
    let mut service = db.service();
    let task_id = seed_verifying_task(&mut service, "T-1", "tests pass");

    // The only thing backing completion here is the agent's own submission
    // narrative ("done, trust me") plus a verdict string. CV-1: that must
    // never admit completion.
    let refused = service.verification_succeeded_under_policy(
        &task_id,
        "VERIFIED via independent evaluation",
        None,
    );
    assert!(
        matches!(refused, Err(ServiceError::Domain(_))),
        "agent self-claim must not complete a task, got {refused:?}"
    );
    assert_eq!(
        service.store().get_task(&task_id).unwrap().unwrap().state(),
        TaskState::Verifying,
        "a refused completion leaves the task VERIFYING"
    );
}

/// One `NewJournalEvent` for the runtime fixtures (mirrors the TASK-0124
/// observed-effect tests).
fn journal(
    kind: EventType,
    task: &Task,
    execution_id: Option<ExecutionId>,
    attempt_id: Option<AttemptId>,
) -> NewJournalEvent {
    NewJournalEvent {
        event_type: kind,
        task_id: Some(task.id().clone()),
        project_id: Some(task.project_id().clone()),
        from_state: None,
        to_state: None,
        reason: None,
        correlation_id: "completion-admission-test".to_owned(),
        causation_id: "completion-admission-test".to_owned(),
        occurred_at: FIXED_STAMP.to_owned(),
        provenance: "app-services/test".to_owned(),
        execution_id,
        attempt_id,
        attempt_outcome: None,
        workspace_id: None,
        from_workspace_state: None,
        to_workspace_state: None,
        plan_revision: None,
    }
}

/// Seeds an execution + attempt so a trusted observed effect can bind to it.
fn seed_execution(
    service: &mut TaskService<FixedClock>,
    task_id: &TaskId,
) -> (ExecutionId, AttemptId) {
    let task = service.store().get_task(task_id).unwrap().unwrap();
    let execution_id = ExecutionId::new(format!("E-{task_id}")).unwrap();
    let mut execution = Execution::begin(execution_id.clone(), task_id.clone());
    service
        .store_mut()
        .create_execution_with_event(
            &execution,
            &journal(
                EventType::ExecutionCreated,
                &task,
                Some(execution_id.clone()),
                None,
            ),
        )
        .unwrap();
    let attempt_id = AttemptId::new(format!("A-{task_id}")).unwrap();
    execution
        .start_attempt(
            attempt_id.clone(),
            ProviderRef::new("test-provider").unwrap(),
        )
        .unwrap();
    let attempt = execution.get_attempt(&attempt_id).unwrap().clone();
    service
        .store_mut()
        .create_attempt_with_event(
            &attempt,
            &journal(
                EventType::AttemptCreated,
                &task,
                Some(execution_id.clone()),
                Some(attempt_id.clone()),
            ),
        )
        .unwrap();
    (execution_id, attempt_id)
}

/// Appends one trusted observed effect whose fingerprint is derived from
/// `digest_byte`; a different byte is a different (moved) subject state.
fn append_effect(
    service: &mut TaskService<FixedClock>,
    execution_id: &ExecutionId,
    attempt_id: &AttemptId,
    subject: &str,
    digest_byte: char,
) -> SubjectFingerprint {
    append_basis_effect(service, execution_id, attempt_id, subject, digest_byte)
        .subject_fingerprint()
        .clone()
}

/// Like [`append_effect`], but returns the stored effect so verdict basis
/// refs can name it (CV-9 rule 4).
fn append_basis_effect(
    service: &mut TaskService<FixedClock>,
    execution_id: &ExecutionId,
    attempt_id: &AttemptId,
    subject: &str,
    digest_byte: char,
) -> ObservedEffect {
    let digest = format!("sha256:{}", digest_byte.to_string().repeat(64));
    let subject_ref = EffectSubjectRef::new(subject).unwrap();
    let fingerprint = subject_fingerprint(
        EffectKind::ArtifactHash,
        subject_ref.as_str(),
        &[("content", digest.as_str())],
    )
    .unwrap();
    service
        .store_mut()
        .append_observed_effect(
            subject_ref,
            fingerprint.clone(),
            EffectPayload::ArtifactHash {
                bytes: 3,
                sha256: Sha256Digest::new(digest).unwrap(),
            },
            FIXED_STAMP,
            EffectProvenance::new(
                execution_id.clone(),
                attempt_id.clone(),
                "runlith.local-observer.v1",
            )
            .unwrap(),
        )
        .unwrap()
}

/// Appends one durable agent claim for a criterion.
fn append_claim(
    service: &mut TaskService<FixedClock>,
    task_id: &TaskId,
    criterion: &str,
    fp: &SubjectFingerprint,
    asserted_by: &str,
) -> evaluation::AgentClaim {
    service
        .store_mut()
        .append_agent_claim(
            task_id,
            criterion,
            ClaimedOutcome::Satisfied,
            ClaimedSubject {
                subject_ref: format!("subject:{criterion}"),
                subject_fingerprint: fp.clone(),
            },
            EvidenceRefs::default(),
            asserted_by,
            FIXED_STAMP,
        )
        .unwrap()
}

/// Mints one durable verdict against a durable claim.
fn append_verdict(
    service: &mut TaskService<FixedClock>,
    claim: &evaluation::AgentClaim,
    kind: RequirementKind,
    fp: &SubjectFingerprint,
    verdict: ClaimVerdict,
    supersedes: Option<&evaluation::VerificationId>,
) -> evaluation::ClaimVerification {
    service
        .store_mut()
        .append_claim_verification(
            &claim.claim_id,
            kind,
            fp.clone(),
            format!("sha256:{}", "a".repeat(64)),
            Some(format!("sha256:{}", "b".repeat(64))),
            verdict,
            EvidenceRefs::default(),
            supersedes.cloned(),
            FIXED_STAMP,
        )
        .unwrap()
}

/// Distinct `sha256:` policy fingerprints for the CV-7 tests.
fn policy_fp(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

/// A plain `TEST_SUITE` requirement stamped under `policy`: empty params
/// (not candidate-shaped) isolate the CV-7 policy dimension from CV-9
/// candidate integrity.
fn requirement_with_policy(criterion: &str, policy: &str) -> EvidenceRequirement {
    EvidenceRequirement::new(
        criterion,
        RequirementKind::TestSuite,
        RequirementKind::TestSuite.default_subject_binding(),
        Vec::new(),
        Some(policy.to_owned()),
        true,
    )
    .unwrap()
}

/// Mints one durable verdict with an explicit requirement revision, policy
/// echo, and basis refs (CV-9 rule 4 inputs).
#[allow(clippy::too_many_arguments)]
fn append_verdict_full(
    service: &mut TaskService<FixedClock>,
    claim: &evaluation::AgentClaim,
    kind: RequirementKind,
    fp: &SubjectFingerprint,
    revision: &str,
    policy: Option<String>,
    effect_refs: Vec<String>,
    verdict: ClaimVerdict,
    supersedes: Option<&evaluation::VerificationId>,
) -> evaluation::ClaimVerification {
    service
        .store_mut()
        .append_claim_verification(
            &claim.claim_id,
            kind,
            fp.clone(),
            revision.to_owned(),
            policy,
            verdict,
            EvidenceRefs::new(vec![], effect_refs, vec![]).unwrap(),
            supersedes.cloned(),
            FIXED_STAMP,
        )
        .unwrap()
}

/// Reads back the seeded requirement for `criterion`: verdict fixtures
/// bind against the durable value, never a reconstruction.
fn seeded_requirement(
    service: &TaskService<FixedClock>,
    task_id: &TaskId,
    criterion: &str,
) -> EvidenceRequirement {
    service
        .store()
        .get_task(task_id)
        .unwrap()
        .unwrap()
        .evidence_requirements()
        .iter()
        .find(|requirement| requirement.criterion_id.as_str() == criterion)
        .unwrap()
        .clone()
}

/// Binds a candidate requirement through the shipped boundary (TASK-0157):
/// catalog revision 1, `candidate.file-contains.v1`.
fn bind_test_requirement(
    criterion: &str,
    path: &str,
    literal: &str,
    policy: &str,
) -> EvidenceRequirement {
    let catalog = CandidateSelectorCatalogRevision::new(1, CandidateSelectorCatalog::v1()).unwrap();
    let unbound = EvidenceRequirement::new(
        criterion,
        RequirementKind::TestSuite,
        SubjectBinding::ContentToolchainAndConfig,
        Vec::new(),
        Some(policy.to_owned()),
        true,
    )
    .unwrap();
    let proposed = ProposedCandidateBinding {
        selector: "candidate.file-contains.v1".to_owned(),
        path: path.to_owned(),
        literal: literal.to_owned(),
    };
    bind_candidate_input_params(
        &catalog,
        &unbound,
        &proposed,
        &FreshPresenceProof::for_test(),
        policy,
        FIXED_MILLIS,
    )
    .unwrap()
    .requirement
}

/// Seeds a READY workspace lease row directly: production reaches READY
/// through `WorkspaceService` (git-backed, orthogonal here); the lease only
/// scaffolds `bind_evaluated_policy_base`. Raw-SQL seeding follows the
/// `entry_recovery.rs` precedent.
fn seed_ready_lease(
    db_path: &std::path::Path,
    execution_id: &ExecutionId,
    workspace_id: &str,
    base_revision: &str,
) {
    let connection = rusqlite::Connection::open(db_path).expect("raw open");
    connection
        .execute(
            "INSERT INTO workspace_leases
             (workspace_id, execution_id, repository_ref, base_revision,
              branch_ref, worktree_ref, lifecycle)
             VALUES (?1, ?2, 'repo-local:runlith', ?3,
                     'refs/heads/test', 'worktree/test', 'READY')",
            rusqlite::params![workspace_id, execution_id.as_str(), base_revision],
        )
        .expect("seed lease");
}

/// Appends a candidate result + `Intact` assessment (mirrors the
/// `observed_effects.rs` TASK-0144 fixture) and returns the result effect.
fn append_candidate_basis(
    service: &mut TaskService<FixedClock>,
    task_id: &TaskId,
    execution_id: &ExecutionId,
    attempt_id: &AttemptId,
    criterion: &str,
    revision: &str,
    base_fp: &str,
) -> ObservedEffect {
    let subject = EffectSubjectRef::new(format!("attempt:{}", attempt_id.as_str())).unwrap();
    let candidate_manifest = format!("sha256:{}", "d".repeat(64));
    let mut assessment = CandidateIntegrityAssessment {
        task_id: task_id.as_str().to_owned(),
        execution_id: execution_id.as_str().to_owned(),
        attempt_id: attempt_id.as_str().to_owned(),
        subject_ref: subject.as_str().to_owned(),
        requirement_revision: revision.to_owned(),
        policy_revision: 1,
        policy_fingerprint: base_fp.to_owned(),
        criterion_id: criterion.to_owned(),
        selector: "candidate.file-contains.v1".to_owned(),
        selector_path: "src/main.rs".to_owned(),
        selector_literal: "accepted".to_owned(),
        invocation_id: "candidate-invocation-1".to_owned(),
        evaluator_ref: "runlith.agentd.candidate-evaluator.v1".to_owned(),
        runtime_ref: "agentd/1".to_owned(),
        enforcement_ref: "runner-operation-1".to_owned(),
        invocation_descriptor_version: 1,
        invocation_descriptor_sha256: String::new(),
        evaluator_executable: "/trusted/agentd".to_owned(),
        toolchain_ref: "git-executable=/usr/bin/git;git version 2.54.0".to_owned(),
        runner_backend: "native".to_owned(),
        runner_plan_sha256: format!("sha256:{}", "6".repeat(64)),
        runner_enforced_axes: 3,
        runner_env_isolated: true,
        base_commit_oid: "1".repeat(40),
        base_tree_oid: "2".repeat(40),
        base_manifest_sha256: format!("sha256:{}", "a".repeat(64)),
        candidate_commit_oid: "3".repeat(40),
        candidate_tree_oid: "4".repeat(40),
        candidate_manifest_sha256: candidate_manifest.clone(),
        deletions: vec![CandidateDeletion {
            path: b"src/removed.rs".to_vec(),
            base_mode: 0o100644,
            base_blob_sha256: format!("sha256:{}", "5".repeat(64)),
        }],
        checkpoints: [
            CandidateCheckpointPhase::Selection,
            CandidateCheckpointPhase::BeforeEvaluation,
            CandidateCheckpointPhase::AfterEvaluation,
            CandidateCheckpointPhase::ResultAdmission,
        ]
        .into_iter()
        .enumerate()
        .map(|(index, phase)| CandidateFreshnessCheckpoint {
            phase,
            observed_at_ms: 11 + index as u64,
            git_metadata_fingerprint: format!("sha256:{}", "e".repeat(64)),
            live_worktree_manifest_sha256: format!("sha256:{}", "f".repeat(64)),
            matched: true,
        })
        .collect(),
        outcome: CandidateIntegrityOutcome::Intact,
        started_at_ms: 10,
        completed_at_ms: 20,
    };
    assessment.refresh_invocation_descriptor_digest();
    let config = candidate_sha256(
        format!(
            "{}\0{}\0{}\0{}\0{}\0{}",
            assessment.requirement_revision,
            assessment.policy_fingerprint,
            assessment.selector,
            assessment.selector_path,
            assessment.selector_literal,
            assessment.invocation_descriptor_sha256,
        )
        .as_bytes(),
    );
    let fingerprint = subject_fingerprint(
        EffectKind::TestSuiteResult,
        subject.as_str(),
        &[
            ("content", &candidate_manifest),
            ("toolchain", &assessment.runtime_ref),
            ("config", &config),
        ],
    )
    .unwrap();
    // TASK-0151 finding 6: production records the selection (operation)
    // row before the result, and the store now enforces that binding —
    // seed the operation first, mirroring the producer order.
    service
        .store_mut()
        .record_candidate_selection(
            task_id.as_str(),
            execution_id.as_str(),
            attempt_id.as_str(),
            &assessment.invocation_id,
            "base-revision-test",
            &assessment.candidate_commit_oid,
            revision,
            1,
            base_fp,
            criterion,
            &assessment.selector,
            &assessment.selector_path,
            &assessment.selector_literal,
            FIXED_MILLIS,
        )
        .unwrap();
    service
        .store_mut()
        .append_candidate_result(
            subject,
            fingerprint,
            EffectPayload::TestSuiteResult {
                passed: 1,
                failed: 0,
                skipped: 0,
                result_sha256: Sha256Digest::new(format!("sha256:{}", "9".repeat(64))).unwrap(),
            },
            FIXED_STAMP,
            EffectProvenance::new(execution_id.clone(), attempt_id.clone(), "agentd/1")
                .unwrap()
                .with_lineage(
                    Some("runlith.agentd.candidate-evaluator.v1".to_owned()),
                    Some("candidate-invocation-1".to_owned()),
                )
                .unwrap(),
            &assessment,
        )
        .unwrap()
}

#[test]
fn cv7_matching_policy_fingerprint_admits() {
    let db = TestDb::new("cv7-match");
    let mut service = db.service();
    let policy = policy_fp('1');
    let requirement = requirement_with_policy("tests pass", &policy);
    let revision = requirement_revision(&requirement);
    let task_id = seed_task(&mut service, "T-1", vec![requirement]);
    drive_to_verifying(&mut service, &task_id);
    let (execution, attempt) = seed_execution(&mut service, &task_id);
    let effect = append_basis_effect(&mut service, &execution, &attempt, "artifact:one", 'a');
    let fp = effect.subject_fingerprint().clone();
    let claim = append_claim(&mut service, &task_id, "tests pass", &fp, "agent:builder-1");
    append_verdict_full(
        &mut service,
        &claim,
        RequirementKind::TestSuite,
        &fp,
        &revision,
        Some(policy.clone()),
        vec![effect.effect_id().as_str().to_owned()],
        ClaimVerdict::Verified,
        None,
    );

    let applied = service
        .verification_succeeded_under_policy(&task_id, "harness verified", Some(&policy))
        .unwrap();
    assert_eq!(applied.task.state(), TaskState::Completed);
}

#[test]
fn cv7_mismatched_policy_fingerprint_is_stale() {
    let db = TestDb::new("cv7-stale");
    let mut service = db.service();
    let policy = policy_fp('1');
    let requirement = requirement_with_policy("tests pass", &policy);
    let revision = requirement_revision(&requirement);
    let task_id = seed_task(&mut service, "T-1", vec![requirement]);
    drive_to_verifying(&mut service, &task_id);
    let (execution, attempt) = seed_execution(&mut service, &task_id);
    let effect = append_basis_effect(&mut service, &execution, &attempt, "artifact:one", 'a');
    let fp = effect.subject_fingerprint().clone();
    let claim = append_claim(&mut service, &task_id, "tests pass", &fp, "agent:builder-1");
    append_verdict_full(
        &mut service,
        &claim,
        RequirementKind::TestSuite,
        &fp,
        &revision,
        Some(policy.clone()),
        vec![effect.effect_id().as_str().to_owned()],
        ClaimVerdict::Verified,
        None,
    );

    let stale = policy_fp('2');
    let refused =
        service.verification_succeeded_under_policy(&task_id, "harness verified", Some(&stale));
    assert_refused(refused, "stale");
    assert_eq!(
        service.store().get_task(&task_id).unwrap().unwrap().state(),
        TaskState::Verifying,
        "a stale-policy completion leaves the task VERIFYING"
    );
}

#[test]
fn cv9_candidate_basis_with_clean_assessment_admits() {
    let db = TestDb::new("cv9-candidate");
    let mut service = db.service();
    let policy = policy_fp('1');
    let requirement = bind_test_requirement("tests pass", "src/main.rs", "accepted", &policy);
    let revision = requirement_revision(&requirement);
    let task_id = seed_task(&mut service, "T-1", vec![requirement]);
    drive_to_verifying(&mut service, &task_id);
    let (execution, attempt) = seed_execution(&mut service, &task_id);
    let base_oid = "1".repeat(40);
    seed_ready_lease(&db.path, &execution, "ws-1", &base_oid);
    let base = service.bind_evaluated_policy_base(&execution).unwrap();
    let base_fp = base.policy.fingerprint().to_owned();
    let effect = append_candidate_basis(
        &mut service,
        &task_id,
        &execution,
        &attempt,
        "tests pass",
        &revision,
        &base_fp,
    );
    let fp = effect.subject_fingerprint().clone();
    let claim = append_claim(&mut service, &task_id, "tests pass", &fp, "agent:builder-1");
    append_verdict_full(
        &mut service,
        &claim,
        RequirementKind::TestSuite,
        &fp,
        &revision,
        Some(policy.clone()),
        vec![effect.effect_id().as_str().to_owned()],
        ClaimVerdict::Verified,
        None,
    );

    let applied = service
        .verification_succeeded_under_policy(&task_id, "harness verified", Some(&policy))
        .unwrap();
    assert_eq!(applied.task.state(), TaskState::Completed);
}

/// Refusal assertion: a domain error whose text names `needle`.
fn assert_refused(
    result: app_services::ServiceResult<app_services::AppliedTransition>,
    needle: &str,
) {
    match result {
        Err(ServiceError::Domain(error)) => {
            let text = format!("{error:?}");
            assert!(
                text.contains(needle),
                "refusal must name {needle:?}, got {text:?}"
            );
        }
        other => panic!("expected a domain refusal naming {needle:?}, got {other:?}"),
    }
}

#[test]
fn cv3_current_verdict_binds_completion_and_records_durable_admission() {
    let db = TestDb::new("cv3-verified");
    let mut service = db.service();
    let task_id = seed_verifying_task(&mut service, "T-1", "tests pass");
    let (execution, attempt) = seed_execution(&mut service, &task_id);
    let effect = append_basis_effect(&mut service, &execution, &attempt, "artifact:one", 'a');
    let fp = effect.subject_fingerprint().clone();
    let claim = append_claim(&mut service, &task_id, "tests pass", &fp, "agent:builder-1");
    let requirement = seeded_requirement(&service, &task_id, "tests pass");
    let verdict = append_verdict_full(
        &mut service,
        &claim,
        RequirementKind::TestSuite,
        &fp,
        &requirement_revision(&requirement),
        None,
        vec![effect.effect_id().as_str().to_owned()],
        ClaimVerdict::Verified,
        None,
    );

    let applied = service
        .verification_succeeded_under_policy(&task_id, "harness verified", None)
        .unwrap();
    assert_eq!(applied.task.state(), TaskState::Completed);

    let admissions = service
        .store()
        .list_completion_admissions_for_task(&task_id)
        .unwrap();
    assert_eq!(admissions.len(), 1, "one admission record per completion");
    assert_eq!(admissions[0].outcomes[0].status, CriterionStatus::Verified);
    assert_eq!(
        admissions[0].outcomes[0].evidence_ref.as_deref(),
        Some(verdict.verification_id.as_str())
    );

    // Durable across restart: the admission record survives reopen.
    drop(service);
    let reopened = db.service();
    assert_eq!(
        reopened
            .store()
            .list_completion_admissions_for_task(&task_id)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        reopened
            .store()
            .get_task(&task_id)
            .unwrap()
            .unwrap()
            .state(),
        TaskState::Completed
    );
}

#[test]
fn cv3_moved_subject_fingerprint_returns_criterion_to_unverified() {
    let db = TestDb::new("cv3-stale");
    let mut service = db.service();
    let task_id = seed_verifying_task(&mut service, "T-1", "tests pass");
    let (execution, attempt) = seed_execution(&mut service, &task_id);
    let effect = append_basis_effect(&mut service, &execution, &attempt, "artifact:one", 'a');
    let fp = effect.subject_fingerprint().clone();
    let claim = append_claim(&mut service, &task_id, "tests pass", &fp, "agent:builder-1");
    let requirement = seeded_requirement(&service, &task_id, "tests pass");
    let _verdict = append_verdict_full(
        &mut service,
        &claim,
        RequirementKind::TestSuite,
        &fp,
        &requirement_revision(&requirement),
        None,
        vec![effect.effect_id().as_str().to_owned()],
        ClaimVerdict::Verified,
        None,
    );
    // The subject moves after the verdict was minted: new trusted reading.
    let _moved = append_effect(&mut service, &execution, &attempt, "artifact:two", 'b');

    assert_refused(
        service.verification_succeeded_under_policy(&task_id, "harness verified", None),
        "UNVERIFIED",
    );
    assert_eq!(
        service.store().get_task(&task_id).unwrap().unwrap().state(),
        TaskState::Verifying
    );
    assert!(service
        .store()
        .list_completion_admissions_for_task(&task_id)
        .unwrap()
        .is_empty());
}

#[test]
fn cv6_refuted_and_blocked_stay_distinct_and_never_pass() {
    let db = TestDb::new("cv6-taxonomy");
    let mut service = db.service();
    for (task, verdict, needle) in [
        ("T-REFUTED", ClaimVerdict::Refuted, "REFUTED"),
        ("T-BLOCKED", ClaimVerdict::Blocked, "BLOCKED"),
    ] {
        let task_id = seed_verifying_task(&mut service, task, "tests pass");
        let (execution, attempt) = seed_execution(&mut service, &task_id);
        let fp = append_effect(&mut service, &execution, &attempt, "artifact:one", 'a');
        let claim = append_claim(&mut service, &task_id, "tests pass", &fp, "agent:builder-1");
        let minted = append_verdict(
            &mut service,
            &claim,
            RequirementKind::TestSuite,
            &fp,
            verdict,
            None,
        );
        let _ = &minted;
        assert_refused(
            service.verification_succeeded_under_policy(&task_id, "evaluated", None),
            needle,
        );
        assert_eq!(
            service.store().get_task(&task_id).unwrap().unwrap().state(),
            TaskState::Verifying,
            "{needle} must not complete the task"
        );
    }
}

#[test]
fn cv5_superseded_verdict_never_counts() {
    let db = TestDb::new("cv5-supersede");
    let mut service = db.service();
    let task_id = seed_verifying_task(&mut service, "T-1", "tests pass");
    let (execution, attempt) = seed_execution(&mut service, &task_id);
    let effect = append_basis_effect(&mut service, &execution, &attempt, "artifact:one", 'a');
    let fp = effect.subject_fingerprint().clone();
    let claim = append_claim(&mut service, &task_id, "tests pass", &fp, "agent:builder-1");
    let requirement = seeded_requirement(&service, &task_id, "tests pass");
    let first = append_verdict_full(
        &mut service,
        &claim,
        RequirementKind::TestSuite,
        &fp,
        &requirement_revision(&requirement),
        None,
        vec![effect.effect_id().as_str().to_owned()],
        ClaimVerdict::Verified,
        None,
    );
    // The later UNVERIFIED supersedes the VERIFIED: history stays visible
    // (CV-5) but only the current record counts — and it is not a pass.
    let second = append_verdict(
        &mut service,
        &claim,
        RequirementKind::TestSuite,
        &fp,
        ClaimVerdict::Unverified,
        Some(&first.verification_id),
    );

    assert_refused(
        service.verification_succeeded_under_policy(&task_id, "harness verified", None),
        "UNVERIFIED",
    );
    let durable = service
        .store()
        .list_claim_verifications_for_task(&task_id)
        .unwrap();
    assert_eq!(
        durable.len(),
        2,
        "nothing is deleted; supersession is a chain"
    );
    assert_eq!(
        durable[0].superseded_by.as_ref(),
        Some(&second.verification_id)
    );
}

#[test]
fn cv2_self_verified_claim_is_ineligible_for_admission() {
    let db = TestDb::new("cv2-self-verified");
    let mut service = db.service();
    let task_id = seed_verifying_task(&mut service, "T-1", "tests pass");
    let (execution, attempt) = seed_execution(&mut service, &task_id);
    let effect = append_basis_effect(&mut service, &execution, &attempt, "artifact:one", 'a');
    let fp = effect.subject_fingerprint().clone();
    // A claim asserted under the trusted verifier identity can never be
    // verified by that same identity (CV-2): such a verdict is ignored.
    let claim = append_claim(
        &mut service,
        &task_id,
        "tests pass",
        &fp,
        evaluation::TRUSTED_VERIFIER_IDENTITY,
    );
    let requirement = seeded_requirement(&service, &task_id, "tests pass");
    let _verdict = append_verdict_full(
        &mut service,
        &claim,
        RequirementKind::TestSuite,
        &fp,
        &requirement_revision(&requirement),
        None,
        vec![effect.effect_id().as_str().to_owned()],
        ClaimVerdict::Verified,
        None,
    );

    assert_refused(
        service.verification_succeeded_under_policy(&task_id, "harness verified", None),
        "UNVERIFIED",
    );
}

#[test]
fn cv4_exemption_discharges_capture_criterion_only_under_current_policy() {
    let db = TestDb::new("cv4-exemption");
    let mut service = db.service();
    let requirement = EvidenceRequirement::new(
        "tests pass",
        RequirementKind::TestSuite,
        SubjectBinding::ContentToolchainAndConfig,
        vec![],
        Some("policy:g8".to_owned()),
        true,
    )
    .unwrap();
    let task_id = seed_task(&mut service, "T-1", vec![requirement]);
    drive_to_verifying(&mut service, &task_id);
    let (execution, attempt) = seed_execution(&mut service, &task_id);
    let fp = append_effect(&mut service, &execution, &attempt, "artifact:one", 'a');
    service
        .store_mut()
        .append_policy_exemption(
            &task_id,
            Some("tests pass"),
            fp.clone(),
            "policy:g8",
            RiskClass::Low,
            FIXED_STAMP,
        )
        .unwrap();

    let applied = service
        .verification_succeeded_under_policy(&task_id, "policy-exempted capture", Some("policy:g8"))
        .unwrap();
    assert_eq!(applied.task.state(), TaskState::Completed);
    let admissions = service
        .store()
        .list_completion_admissions_for_task(&task_id)
        .unwrap();
    assert_eq!(admissions[0].outcomes[0].status, CriterionStatus::Exempt);
    assert_eq!(
        admissions[0].policy_fingerprint.as_deref(),
        Some("policy:g8")
    );

    // CV-4 typed-risk gate: anything above `Low` is refused at the durable
    // boundary, so a medium-risk exemption can never exist to be read.
    let refused = service.store_mut().append_policy_exemption(
        &task_id,
        Some("tests pass"),
        fp,
        "policy:g8",
        RiskClass::Medium,
        FIXED_STAMP,
    );
    assert!(refused.is_err(), "medium-risk exemption must fail closed");
}

#[test]
fn cv4_stale_policy_exemption_never_counts() {
    let db = TestDb::new("cv4-stale-exemption");
    let mut service = db.service();
    let requirement = EvidenceRequirement::new(
        "tests pass",
        RequirementKind::TestSuite,
        SubjectBinding::Content,
        vec![],
        Some("policy:g8".to_owned()),
        true,
    )
    .unwrap();
    let task_id = seed_task(&mut service, "T-1", vec![requirement]);
    drive_to_verifying(&mut service, &task_id);
    let (execution, attempt) = seed_execution(&mut service, &task_id);
    let fp = append_effect(&mut service, &execution, &attempt, "artifact:one", 'a');
    // Minted under a superseded policy fingerprint: stale input, not
    // authority (CV-7), so it can never discharge the requirement.
    service
        .store_mut()
        .append_policy_exemption(
            &task_id,
            Some("tests pass"),
            fp,
            "policy:g7",
            RiskClass::Low,
            FIXED_STAMP,
        )
        .unwrap();

    assert_refused(
        service.verification_succeeded_under_policy(
            &task_id,
            "policy-exempted capture",
            Some("policy:g8"),
        ),
        "UNVERIFIED",
    );
}

#[test]
fn cv8_human_approval_discharge_admits_once_and_is_consumed() {
    let db = TestDb::new("cv8-discharge");
    let mut service = db.service();
    let requirement = EvidenceRequirement::new(
        "owner approves",
        RequirementKind::HumanApproval,
        SubjectBinding::Content,
        vec![],
        None,
        true,
    )
    .unwrap();
    let task_id = seed_task(&mut service, "T-1", vec![requirement]);
    drive_to_verifying(&mut service, &task_id);
    let (execution, attempt) = seed_execution(&mut service, &task_id);
    let fp = append_effect(&mut service, &execution, &attempt, "artifact:one", 'a');
    service
        .store_mut()
        .append_approval_discharge(
            &task_id,
            "owner approves",
            fp,
            RiskClass::Low,
            "approval:op-1",
            "2023-12-31T00:00:00Z",
            "2024-01-02T00:00:00Z",
            FIXED_STAMP,
        )
        .unwrap();

    let applied = service
        .verification_succeeded_under_policy(&task_id, "owner approved", None)
        .unwrap();
    assert_eq!(applied.task.state(), TaskState::Completed);
    let discharges = service
        .store()
        .list_approval_discharges_for_task(&task_id)
        .unwrap();
    assert!(
        discharges[0].consumed_by.is_some(),
        "the completing commit consumes the discharge (single-use, CV-8)"
    );
}

#[test]
fn cv8_automated_verdict_can_never_discharge_human_approval() {
    let db = TestDb::new("cv8-no-substitute");
    let mut service = db.service();
    let requirement = EvidenceRequirement::new(
        "owner approves",
        RequirementKind::HumanApproval,
        SubjectBinding::Content,
        vec![],
        None,
        true,
    )
    .unwrap();
    let task_id = seed_task(&mut service, "T-1", vec![requirement]);
    drive_to_verifying(&mut service, &task_id);
    let (execution, attempt) = seed_execution(&mut service, &task_id);
    let effect = append_basis_effect(&mut service, &execution, &attempt, "artifact:one", 'a');
    let fp = effect.subject_fingerprint().clone();
    let claim = append_claim(
        &mut service,
        &task_id,
        "owner approves",
        &fp,
        "agent:builder-1",
    );
    // Even a current VERIFIED verdict is never consulted for a
    // HUMAN_APPROVAL requirement (INV-086): only the durable discharge is.
    let requirement = seeded_requirement(&service, &task_id, "owner approves");
    let _verdict = append_verdict_full(
        &mut service,
        &claim,
        RequirementKind::HumanApproval,
        &fp,
        &requirement_revision(&requirement),
        None,
        vec![effect.effect_id().as_str().to_owned()],
        ClaimVerdict::Verified,
        None,
    );

    assert_refused(
        service.verification_succeeded_under_policy(&task_id, "evaluated", None),
        "UNVERIFIED",
    );
}

#[test]
fn cv8_discharge_bound_to_moved_subject_is_refused() {
    let db = TestDb::new("cv8-moved");
    let mut service = db.service();
    let requirement = EvidenceRequirement::new(
        "owner approves",
        RequirementKind::HumanApproval,
        SubjectBinding::Content,
        vec![],
        None,
        true,
    )
    .unwrap();
    let task_id = seed_task(&mut service, "T-1", vec![requirement]);
    drive_to_verifying(&mut service, &task_id);
    let (execution, attempt) = seed_execution(&mut service, &task_id);
    let fp = append_effect(&mut service, &execution, &attempt, "artifact:one", 'a');
    service
        .store_mut()
        .append_approval_discharge(
            &task_id,
            "owner approves",
            fp,
            RiskClass::Low,
            "approval:op-1",
            "2023-12-31T00:00:00Z",
            "2024-01-02T00:00:00Z",
            FIXED_STAMP,
        )
        .unwrap();
    // The subject moves: the approval no longer binds it (CV-3/CV-8).
    let _moved = append_effect(&mut service, &execution, &attempt, "artifact:two", 'b');

    assert_refused(
        service.verification_succeeded_under_policy(&task_id, "owner approved", None),
        "UNVERIFIED",
    );
}

// TASK-0125 phase (c): candidate-only verdict minting. After the capture
// pipeline records the attempt's observed effect (the trusted measurement
// the mint never synthesizes), the candidate path mints one durable claim
// plus one VERIFIED verdict bound to the current subject, and admission
// then completes the task.
#[test]
fn cv_candidate_mint_binds_current_effects_and_admits_completion() {
    let db = TestDb::new("cv-mint-admits");
    let mut service = db.service();
    let requirement = EvidenceRequirement::authored_default("tests pass").unwrap();
    let expected_revision = requirement_revision(&requirement);
    let task_id = seed_task(&mut service, "T-MINT", vec![requirement]);
    drive_to_verifying(&mut service, &task_id);
    let (execution, attempt) = seed_execution(&mut service, &task_id);
    let effect = append_basis_effect(&mut service, &execution, &attempt, "artifact:one", 'c');
    let asserter = format!("candidate:{}:{}", execution.as_str(), attempt.as_str());

    let minted = mint_candidate_verdicts(
        service.store_mut(),
        &task_id,
        &execution,
        &attempt,
        &asserter,
        FIXED_STAMP,
    )
    .expect("minting over bound capture evidence must succeed");
    assert_eq!(minted.len(), 1, "one verdict per required criterion");
    let verdict = &minted[0];
    assert_eq!(verdict.verdict, ClaimVerdict::Verified);
    assert_eq!(verdict.verifier, evaluation::TRUSTED_VERIFIER_IDENTITY);
    assert_eq!(verdict.requirement_revision, expected_revision);
    assert_eq!(verdict.policy_fingerprint, None);
    assert_eq!(
        verdict.subject_fingerprint,
        *effect.subject_fingerprint(),
        "the verdict binds the current subject"
    );
    assert_eq!(
        verdict.basis.effect_refs,
        vec![effect.effect_id().as_str().to_owned()],
        "the basis names exactly the bound capture effects"
    );

    service
        .verification_succeeded_under_policy(&task_id, "harness verified", None)
        .expect("minted verdict must admit completion");
    assert_eq!(
        service.store().get_task(&task_id).unwrap().unwrap().state(),
        TaskState::Completed,
    );
}

// Phase (c) fail-closed twin: with no capture evidence there is nothing to
// bind, so minting succeeds vacuously with zero rows and admission still
// refuses. Missing measurement is never an error here — the refusal is the
// loud signal, owned by admission alone.
#[test]
fn cv_candidate_mint_without_effects_mints_nothing_and_admission_refuses() {
    let db = TestDb::new("cv-mint-empty");
    let mut service = db.service();
    let task_id = seed_task(
        &mut service,
        "T-MINT-EMPTY",
        vec![EvidenceRequirement::authored_default("tests pass").unwrap()],
    );
    drive_to_verifying(&mut service, &task_id);
    let (execution, attempt) = seed_execution(&mut service, &task_id);
    let asserter = format!("candidate:{}:{}", execution.as_str(), attempt.as_str());

    let minted = mint_candidate_verdicts(
        service.store_mut(),
        &task_id,
        &execution,
        &attempt,
        &asserter,
        FIXED_STAMP,
    )
    .expect("nothing to bind is vacuous success, not an error");
    assert!(minted.is_empty(), "no evidence means no minted rows");
    assert!(service
        .store()
        .list_agent_claims_for_task(&task_id)
        .unwrap()
        .is_empty());
    assert!(service
        .store()
        .list_claim_verifications_for_task(&task_id)
        .unwrap()
        .is_empty());
    assert_refused(
        service.verification_succeeded_under_policy(&task_id, "harness verified", None),
        "UNVERIFIED",
    );
}
