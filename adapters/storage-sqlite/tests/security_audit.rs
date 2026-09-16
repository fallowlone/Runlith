use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

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

fn human_decision_audit(
    event: SecurityAuditEvent,
    outcome: SecurityAuditOutcome,
) -> NewSecurityAuditRecord {
    NewSecurityAuditRecord {
        event,
        author: SecurityAuditAuthor::HumanDecisionService,
        outcome: Some(outcome),
        occurred_at_ms: 1_700_000_000_010,
        correlation_id: "corr-human-decision".into(),
        causation_id: Some("planning-run-1".into()),
        operation_id: None,
        task_id: Some("TASK-human".into()),
        execution_id: None,
        attempt_id: None,
        subject: Some(SecurityAuditSubject {
            kind: SecurityAuditSubjectKind::HumanDecision,
            id: "hdr-1".into(),
        }),
        policy_fingerprint: Some("sha256:policy".into()),
        rule_id: None,
        reason_code: Some("risk_escalation".into()),
        approval_request_id: None,
        capabilities: vec![],
    }
}

fn create_schema_v15(path: &std::path::Path) {
    let connection = rusqlite::Connection::open(path).unwrap();
    connection
        .execute_batch("PRAGMA foreign_keys = ON;")
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
        include_str!("../migrations/0012_plan_persistence.sql"),
        include_str!("../migrations/0013_plan_cost_receipts.sql"),
        include_str!("../migrations/0014_plan_guardrails.sql"),
        include_str!("../migrations/0015_human_decisions.sql"),
    ] {
        connection.execute_batch(migration).unwrap();
    }
    connection
        .execute_batch("PRAGMA user_version = 15;")
        .unwrap();
}

#[test]
fn fresh_database_reaches_schema_v10_with_empty_separate_audit() {
    let db = TestDb::new("fresh");
    let store = db.open();
    assert_eq!(SUPPORTED_SCHEMA_VERSION, 16);
    assert_eq!(store.schema_version().unwrap(), SUPPORTED_SCHEMA_VERSION);
    assert!(store.list_security_audit_ordered().unwrap().is_empty());
}

#[test]
fn schema_v15_security_audit_upgrades_to_v16_without_rewriting_chain_or_capabilities() {
    let db = TestDb::new("v15-preservation");
    create_schema_v15(&db.path);
    let genesis = "sha256:0000000000000000000000000000000000000000000000000000000000000000";
    let first_hash = format!("sha256:{}", "a".repeat(64));
    {
        let raw = db.raw();
        raw.execute(
            "INSERT INTO security_audit (
                seq, record_version, event_type, author, outcome, occurred_at_ms,
                correlation_id, causation_id, operation_id, task_id, execution_id,
                attempt_id, subject_kind, subject_id, policy_fingerprint, rule_id,
                reason_code, approval_request_id, previous_hash, record_hash
             ) VALUES (
                1, 1, 'tool.invoked', 'tool-service', 'succeeded', 42,
                'corr-old', NULL, 'op-old', 'TASK-old', NULL, NULL,
                'tool', 'git', NULL, NULL, NULL, NULL, ?1, ?2
             )",
            rusqlite::params![genesis, first_hash],
        )
        .unwrap();
        raw.execute(
            "INSERT INTO security_audit_capabilities
                (audit_seq, position, capability_kind, workspace, scope)
             VALUES (1, 0, 'tool.invoke', NULL, 'git')",
            [],
        )
        .unwrap();
        raw.execute(
            "UPDATE security_audit_head SET seq = 1, record_hash = ?1 WHERE singleton = 1",
            rusqlite::params![first_hash],
        )
        .unwrap();
    }

    let before = {
        let raw = db.raw();
        let audit: (i64, String, String, String, String, String) = raw
            .query_row(
                "SELECT seq, event_type, author, outcome, previous_hash, record_hash
                 FROM security_audit WHERE seq = 1",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .unwrap();
        let capability: (i64, i64, String, Option<String>, String) = raw
            .query_row(
                "SELECT audit_seq, position, capability_kind, workspace, scope
                 FROM security_audit_capabilities WHERE audit_seq = 1",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap();
        let head: (i64, String) = raw
            .query_row(
                "SELECT seq, record_hash FROM security_audit_head WHERE singleton = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        (audit, capability, head)
    };

    let store = db.open();
    assert_eq!(store.schema_version().unwrap(), 16);
    drop(store);

    let raw = db.raw();
    let audit: (i64, String, String, String, String, String) = raw
        .query_row(
            "SELECT seq, event_type, author, outcome, previous_hash, record_hash
             FROM security_audit WHERE seq = 1",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .unwrap();
    let capability: (i64, i64, String, Option<String>, String) = raw
        .query_row(
            "SELECT audit_seq, position, capability_kind, workspace, scope
             FROM security_audit_capabilities WHERE audit_seq = 1",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .unwrap();
    let head: (i64, String) = raw
        .query_row(
            "SELECT seq, record_hash FROM security_audit_head WHERE singleton = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!((audit, capability, head), before);
    let foreign_key_violations: i64 = raw
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(foreign_key_violations, 0);
}

#[test]
fn schema_v16_accepts_human_decision_audit_vocabulary_and_preserves_append_only_guards() {
    let db = TestDb::new("human-decision-vocabulary");
    let mut store = db.open();
    for (event, outcome) in [
        (
            SecurityAuditEvent::HumanDecisionRequested,
            SecurityAuditOutcome::Requested,
        ),
        (
            SecurityAuditEvent::HumanDecisionDecided,
            SecurityAuditOutcome::Resolved,
        ),
        (
            SecurityAuditEvent::HumanDecisionApplied,
            SecurityAuditOutcome::Consumed,
        ),
        (
            SecurityAuditEvent::HumanDecisionSuperseded,
            SecurityAuditOutcome::Superseded,
        ),
    ] {
        store
            .append_security_audit(&human_decision_audit(event, outcome))
            .unwrap();
    }
    let records = store.list_security_audit_ordered().unwrap();
    assert_eq!(records.len(), 4);
    assert_eq!(
        records[3].fact.event,
        SecurityAuditEvent::HumanDecisionSuperseded
    );
    drop(store);

    let raw = db.raw();
    assert!(raw
        .execute(
            "UPDATE security_audit SET correlation_id = 'changed' WHERE seq = 1",
            [],
        )
        .is_err());
    assert!(raw
        .execute("DELETE FROM security_audit WHERE seq = 1", [])
        .is_err());
    assert!(raw
        .execute(
            "INSERT INTO security_audit (
                seq, record_version, event_type, author, outcome, occurred_at_ms,
                correlation_id, previous_hash, record_hash
             ) VALUES (
                99, 1, 'future.unknown', 'runtime', 'succeeded', 1,
                'corr-invalid',
                'sha256:0000000000000000000000000000000000000000000000000000000000000000',
                'sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb'
             )",
            [],
        )
        .is_err());
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
