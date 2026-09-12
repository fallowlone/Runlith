use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use policy::Capability;
use storage_sqlite::{
    AuditCapability, NewSecurityAuditRecord, SecurityAuditAuthor, SecurityAuditEvent,
    SecurityAuditOutcome, SecurityAuditSubject, SecurityAuditSubjectKind, SqliteStore,
    StorageError, SUPPORTED_SCHEMA_VERSION,
};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct TestDb {
    path: PathBuf,
}

impl TestDb {
    fn new(suite: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "runlith-security-audit-test-{}-{suite}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self {
            path: dir.join("test.db"),
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

fn policy_decision(correlation: &str, outcome: SecurityAuditOutcome) -> NewSecurityAuditRecord {
    NewSecurityAuditRecord {
        event: SecurityAuditEvent::PolicyDecision,
        author: SecurityAuditAuthor::PolicyEngine,
        outcome: Some(outcome),
        occurred_at_ms: 1_700_000_000_000,
        correlation_id: correlation.into(),
        causation_id: Some("request-1".into()),
        operation_id: Some("operation-1".into()),
        task_id: Some("TASK-1".into()),
        execution_id: Some("exec-1".into()),
        attempt_id: Some("attempt-1".into()),
        subject: Some(SecurityAuditSubject {
            kind: SecurityAuditSubjectKind::PolicyRule,
            id: "hard-rule-1".into(),
        }),
        policy_fingerprint: Some("sha256:policy".into()),
        rule_id: Some("hard-rule-1".into()),
        reason_code: Some("deny-network".into()),
        approval_request_id: None,
        capabilities: vec![AuditCapability::from_capability(
            &Capability::network("metadata.example").unwrap(),
        )],
    }
}

fn secret_access(correlation: &str) -> NewSecurityAuditRecord {
    NewSecurityAuditRecord {
        event: SecurityAuditEvent::SecretAccess,
        author: SecurityAuditAuthor::SecretsBroker,
        outcome: Some(SecurityAuditOutcome::Succeeded),
        occurred_at_ms: 1_700_000_000_001,
        correlation_id: correlation.into(),
        causation_id: Some("operation-1".into()),
        operation_id: Some("operation-1".into()),
        task_id: Some("TASK-1".into()),
        execution_id: Some("exec-1".into()),
        attempt_id: Some("attempt-1".into()),
        subject: Some(SecurityAuditSubject {
            kind: SecurityAuditSubjectKind::SecretRef,
            id: "secret-ref/database".into(),
        }),
        policy_fingerprint: Some("sha256:policy".into()),
        rule_id: None,
        reason_code: None,
        approval_request_id: None,
        capabilities: vec![AuditCapability::from_capability(
            &Capability::secret("secret-ref/database").unwrap(),
        )],
    }
}

#[test]
fn fresh_database_reaches_schema_v10_with_empty_separate_audit() {
    let db = TestDb::new("fresh");
    let store = db.open();
    assert_eq!(SUPPORTED_SCHEMA_VERSION, 10);
    assert_eq!(store.schema_version().unwrap(), 10);
    assert!(store.list_security_audit_ordered().unwrap().is_empty());
}

#[test]
fn audit_survives_restart_and_preserves_stable_identity_and_chain() {
    let db = TestDb::new("restart");
    let first_hash;
    {
        let mut store = db.open();
        let first = store
            .append_security_audit(&policy_decision("corr-1", SecurityAuditOutcome::Denied))
            .unwrap();
        first_hash = first.record_hash.clone();
        assert_eq!(first.id.to_string(), "aud-1");
        let second = store
            .append_security_audit(&secret_access("corr-1"))
            .unwrap();
        assert_eq!(second.id.to_string(), "aud-2");
        assert_eq!(second.previous_hash, first_hash);
    }
    let store = db.open();
    let records = store.list_security_audit_ordered().unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].record_hash, first_hash);
    assert_eq!(records[1].fact.event, SecurityAuditEvent::SecretAccess);
}

#[test]
fn correlation_query_returns_only_the_requested_security_chain_slice() {
    let db = TestDb::new("correlation");
    let mut store = db.open();
    store
        .append_security_audit(&policy_decision("corr-a", SecurityAuditOutcome::Denied))
        .unwrap();
    store
        .append_security_audit(&secret_access("corr-b"))
        .unwrap();
    let records = store.list_security_audit_by_correlation("corr-b").unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].fact.correlation_id, "corr-b");
}

#[test]
fn direct_update_and_delete_are_blocked_by_append_only_storage() {
    let db = TestDb::new("append-only");
    let mut store = db.open();
    store
        .append_security_audit(&policy_decision("corr-1", SecurityAuditOutcome::Denied))
        .unwrap();
    drop(store);
    assert!(db
        .raw()
        .execute(
            "UPDATE security_audit SET correlation_id = 'changed' WHERE seq = 1",
            []
        )
        .is_err());
    assert!(db
        .raw()
        .execute("DELETE FROM security_audit WHERE seq = 1", [])
        .is_err());
}

#[test]
fn hash_verification_detects_tampering_even_if_sql_guard_is_removed() {
    let db = TestDb::new("tamper");
    let mut store = db.open();
    store
        .append_security_audit(&policy_decision("corr-1", SecurityAuditOutcome::Denied))
        .unwrap();
    drop(store);
    let raw = db.raw();
    raw.execute_batch("DROP TRIGGER security_audit_no_update")
        .unwrap();
    raw.execute(
        "UPDATE security_audit SET rule_id = 'forged' WHERE seq = 1",
        [],
    )
    .unwrap();
    let store = db.open();
    assert!(matches!(
        store.list_security_audit_ordered(),
        Err(StorageError::Corrupt(message)) if message.contains("hash mismatch")
    ));
}

#[test]
fn persisted_head_detects_tail_truncation() {
    let db = TestDb::new("truncate");
    let mut store = db.open();
    store
        .append_security_audit(&policy_decision("corr-1", SecurityAuditOutcome::Denied))
        .unwrap();
    store
        .append_security_audit(&secret_access("corr-1"))
        .unwrap();
    drop(store);
    let raw = db.raw();
    raw.execute_batch(
        "DROP TRIGGER security_audit_capabilities_no_delete;
         DROP TRIGGER security_audit_no_delete;
         DELETE FROM security_audit_capabilities WHERE audit_seq = 2;
         DELETE FROM security_audit WHERE seq = 2;",
    )
    .unwrap();
    let store = db.open();
    assert!(matches!(
        store.list_security_audit_ordered(),
        Err(StorageError::Corrupt(message)) if message.contains("head does not match")
    ));
}

#[test]
fn canonical_minimum_taxonomy_round_trips_without_generic_payloads() {
    let db = TestDb::new("taxonomy");
    let mut store = db.open();
    let mut records = vec![
        NewSecurityAuditRecord {
            event: SecurityAuditEvent::PolicyRequest,
            author: SecurityAuditAuthor::Runtime,
            outcome: Some(SecurityAuditOutcome::Requested),
            occurred_at_ms: 10,
            correlation_id: "corr-taxonomy".into(),
            causation_id: None,
            operation_id: Some("operation-taxonomy".into()),
            task_id: Some("TASK-taxonomy".into()),
            execution_id: None,
            attempt_id: None,
            subject: Some(SecurityAuditSubject {
                kind: SecurityAuditSubjectKind::Operation,
                id: "operation-taxonomy".into(),
            }),
            policy_fingerprint: None,
            rule_id: None,
            reason_code: None,
            approval_request_id: None,
            capabilities: vec![],
        },
        policy_decision("corr-taxonomy", SecurityAuditOutcome::ApprovalRequired),
    ];
    for (event, author, outcome, subject, approval_request_id) in [
        (
            SecurityAuditEvent::EffectiveAuthority,
            SecurityAuditAuthor::PolicyEngine,
            SecurityAuditOutcome::Allowed,
            None,
            None,
        ),
        (
            SecurityAuditEvent::ApprovalRequested,
            SecurityAuditAuthor::ApprovalService,
            SecurityAuditOutcome::ApprovalRequired,
            Some((SecurityAuditSubjectKind::Approval, "approval-1")),
            Some("approval-1"),
        ),
        (
            SecurityAuditEvent::ApprovalDecided,
            SecurityAuditAuthor::ApprovalService,
            SecurityAuditOutcome::Approved,
            Some((SecurityAuditSubjectKind::Approval, "approval-1")),
            Some("approval-1"),
        ),
        (
            SecurityAuditEvent::PrivilegeElevation,
            SecurityAuditAuthor::ApprovalService,
            SecurityAuditOutcome::Approved,
            Some((SecurityAuditSubjectKind::Operation, "operation-taxonomy")),
            None,
        ),
        (
            SecurityAuditEvent::DeniedOperation,
            SecurityAuditAuthor::PolicyEngine,
            SecurityAuditOutcome::Denied,
            Some((SecurityAuditSubjectKind::Operation, "operation-taxonomy")),
            None,
        ),
        (
            SecurityAuditEvent::IsolationEnforcement,
            SecurityAuditAuthor::Runner,
            SecurityAuditOutcome::Enforced,
            Some((SecurityAuditSubjectKind::RunnerBackend, "native")),
            None,
        ),
        (
            SecurityAuditEvent::IsolationViolation,
            SecurityAuditAuthor::Runner,
            SecurityAuditOutcome::Violated,
            Some((SecurityAuditSubjectKind::RunnerBackend, "native")),
            None,
        ),
        (
            SecurityAuditEvent::ProviderInvocation,
            SecurityAuditAuthor::ProviderEnvironment,
            SecurityAuditOutcome::Succeeded,
            Some((SecurityAuditSubjectKind::Provider, "claude-code")),
            None,
        ),
        (
            SecurityAuditEvent::ToolInvocation,
            SecurityAuditAuthor::ToolService,
            SecurityAuditOutcome::Succeeded,
            Some((SecurityAuditSubjectKind::Tool, "git")),
            None,
        ),
    ] {
        records.push(NewSecurityAuditRecord {
            event,
            author,
            outcome: Some(outcome),
            occurred_at_ms: 11,
            correlation_id: "corr-taxonomy".into(),
            causation_id: Some("operation-taxonomy".into()),
            operation_id: Some("operation-taxonomy".into()),
            task_id: Some("TASK-taxonomy".into()),
            execution_id: None,
            attempt_id: None,
            subject: subject.map(|(kind, id)| SecurityAuditSubject {
                kind,
                id: id.into(),
            }),
            policy_fingerprint: None,
            rule_id: None,
            reason_code: None,
            approval_request_id: approval_request_id.map(str::to_owned),
            capabilities: vec![],
        });
    }
    records.push(secret_access("corr-taxonomy"));

    for record in &records {
        store.append_security_audit(record).unwrap();
    }
    let stored = store
        .list_security_audit_by_correlation("corr-taxonomy")
        .unwrap();
    assert_eq!(stored.len(), 12);
    assert_eq!(stored[0].fact.event, SecurityAuditEvent::PolicyRequest);
    assert_eq!(stored[11].fact.event, SecurityAuditEvent::SecretAccess);
}

#[test]
fn audit_schema_has_no_generic_or_secret_payload_columns_and_event_journal_stays_empty() {
    let db = TestDb::new("separation");
    let mut store = db.open();
    store
        .append_security_audit(&secret_access("corr-1"))
        .unwrap();
    assert!(store.list_events_ordered().unwrap().is_empty());
    drop(store);

    let raw = db.raw();
    let mut statement = raw.prepare("PRAGMA table_info(security_audit)").unwrap();
    let columns: Vec<String> = statement
        .query_map([], |row| row.get(1))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    for forbidden in [
        "payload",
        "secret_value",
        "provider_text",
        "environment",
        "transcript",
        "context",
    ] {
        assert!(!columns.iter().any(|column| column == forbidden));
    }
}

#[test]
fn malformed_product_fact_is_rejected_before_persistence() {
    let db = TestDb::new("invalid");
    let mut store = db.open();
    let mut record = policy_decision("corr-1", SecurityAuditOutcome::Denied);
    record.policy_fingerprint = None;
    assert!(matches!(
        store.append_security_audit(&record),
        Err(StorageError::ConstraintViolation(_))
    ));
    assert!(store.list_security_audit_ordered().unwrap().is_empty());
}
