//! TASK-0125 phase (b): durable claim-verification seam (migration 0023).
//!
//! ADR-0032 rule 4: a ClaimVerification binds one exact criterion
//! requirement revision, policy fingerprint and subject fingerprint to
//! typed durable evidence references. VERIFIED requires a nonempty
//! effect basis; other verdicts may carry an empty basis (e.g. BLOCKED
//! when capture could not run).

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use evaluation::{ClaimVerdict, ClaimedOutcome, ClaimedSubject, EvidenceRefs, SubjectFingerprint};
use storage_sqlite::{SqliteStore, StorageError};
use task_domain::{Project, ProjectId, RequirementKind, Task, TaskId};

static COUNTER: AtomicU64 = AtomicU64::new(0);
const STAMP: &str = "2024-01-01T00:00:00Z";

struct TestDb {
    path: PathBuf,
}

impl TestDb {
    fn new(name: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "runlith-claim-verification-{name}-{}-{id}",
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
}

impl Drop for TestDb {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::remove_dir(parent);
        }
    }
}

fn seed_task(store: &mut SqliteStore) -> TaskId {
    let project = Project::new(ProjectId::new("P-CLAIM").unwrap(), "claims").unwrap();
    store.create_project(&project).unwrap();
    let task = Task::draft(
        TaskId::new("T-CLAIM").unwrap(),
        project.id().clone(),
        "verify claims",
    )
    .unwrap();
    store.create_task(&task).unwrap();
    task.id().clone()
}

fn fingerprint(byte: char) -> SubjectFingerprint {
    SubjectFingerprint::new(format!("sha256:{}", byte.to_string().repeat(64))).unwrap()
}

fn requirement_revision() -> String {
    format!("sha256:{}", "a".repeat(64))
}

fn policy_fingerprint() -> String {
    format!("sha256:{}", "b".repeat(64))
}

#[test]
fn verdict_round_trip_binds_requirement_revision_and_policy() {
    let db = TestDb::new("round-trip");
    let mut store = db.open();
    let task_id = seed_task(&mut store);
    let fp = fingerprint('1');
    let claim = store
        .append_agent_claim(
            &task_id,
            "criterion-1",
            ClaimedOutcome::Satisfied,
            ClaimedSubject {
                subject_ref: "subject:criterion-1".to_owned(),
                subject_fingerprint: fp.clone(),
            },
            EvidenceRefs::default(),
            "agent:test",
            STAMP,
        )
        .unwrap();
    let basis = EvidenceRefs::new(vec![], vec!["effect-1".to_owned()], vec![]).unwrap();
    let minted = store
        .append_claim_verification(
            &claim.claim_id,
            RequirementKind::TestSuite,
            fp.clone(),
            requirement_revision(),
            Some(policy_fingerprint()),
            ClaimVerdict::Verified,
            basis,
            None,
            STAMP,
        )
        .unwrap();
    assert_eq!(minted.requirement_revision, requirement_revision());
    assert_eq!(minted.policy_fingerprint, Some(policy_fingerprint()));

    let listed = store.list_claim_verifications_for_task(&task_id).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].requirement_revision, requirement_revision());
    assert_eq!(listed[0].policy_fingerprint, Some(policy_fingerprint()));
    assert_eq!(listed[0].subject_fingerprint, fp);
    assert_eq!(listed[0].verdict, ClaimVerdict::Verified);
}

#[test]
fn verified_with_empty_effect_basis_is_rejected() {
    let db = TestDb::new("verified-empty-basis");
    let mut store = db.open();
    let task_id = seed_task(&mut store);
    let fp = fingerprint('2');
    let claim = store
        .append_agent_claim(
            &task_id,
            "criterion-1",
            ClaimedOutcome::Satisfied,
            ClaimedSubject {
                subject_ref: "subject:criterion-1".to_owned(),
                subject_fingerprint: fp.clone(),
            },
            EvidenceRefs::default(),
            "agent:test",
            STAMP,
        )
        .unwrap();
    let result = store.append_claim_verification(
        &claim.claim_id,
        RequirementKind::TestSuite,
        fp,
        requirement_revision(),
        Some(policy_fingerprint()),
        ClaimVerdict::Verified,
        EvidenceRefs::default(),
        None,
        STAMP,
    );
    assert!(
        matches!(result, Err(StorageError::ConstraintViolation(_))),
        "VERIFIED with an empty effect basis must fail closed, got {result:?}"
    );
}

#[test]
fn non_verified_verdicts_allow_an_empty_basis() {
    let db = TestDb::new("blocked-empty-basis");
    let mut store = db.open();
    let task_id = seed_task(&mut store);
    let fp = fingerprint('3');
    let claim = store
        .append_agent_claim(
            &task_id,
            "criterion-1",
            ClaimedOutcome::Satisfied,
            ClaimedSubject {
                subject_ref: "subject:criterion-1".to_owned(),
                subject_fingerprint: fp.clone(),
            },
            EvidenceRefs::default(),
            "agent:test",
            STAMP,
        )
        .unwrap();
    for verdict in [ClaimVerdict::Blocked, ClaimVerdict::Refuted] {
        store
            .append_claim_verification(
                &claim.claim_id,
                RequirementKind::TestSuite,
                fp.clone(),
                requirement_revision(),
                Some(policy_fingerprint()),
                verdict,
                EvidenceRefs::default(),
                None,
                STAMP,
            )
            .unwrap();
    }
    assert_eq!(
        store
            .list_claim_verifications_for_task(&task_id)
            .unwrap()
            .len(),
        2
    );
}
