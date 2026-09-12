//! Append-oriented Security Audit persistence (TASK-0066).

use rusqlite::{params, Transaction, TransactionBehavior};
use security_audit::{
    AuditCapability, NewSecurityAuditRecord, SecurityAuditAuthor, SecurityAuditError,
    SecurityAuditEvent, SecurityAuditId, SecurityAuditOutcome, SecurityAuditRecord,
    SecurityAuditSubject, SecurityAuditSubjectKind, SECURITY_AUDIT_RECORD_VERSION,
};
use sha2::{Digest, Sha256};

use crate::{SqliteStore, StorageError, StorageResult};

const GENESIS_HASH: &str =
    "sha256:0000000000000000000000000000000000000000000000000000000000000000";

impl SqliteStore {
    pub fn append_security_audit(
        &mut self,
        record: &NewSecurityAuditRecord,
    ) -> StorageResult<SecurityAuditRecord> {
        let transaction = self
            .connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let persisted = append_security_audit_in_transaction(&transaction, record)?;
        transaction.commit()?;
        Ok(persisted)
    }

    pub fn list_security_audit_ordered(&self) -> StorageResult<Vec<SecurityAuditRecord>> {
        let mut statement = self.connection().prepare(
            "SELECT seq, record_version, event_type, author, outcome, occurred_at_ms,
                    correlation_id, causation_id, operation_id, task_id, execution_id,
                    attempt_id, subject_kind, subject_id, policy_fingerprint, rule_id,
                    reason_code, approval_request_id, previous_hash, record_hash
             FROM security_audit ORDER BY seq",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(RawAuditRow {
                seq: row.get(0)?,
                record_version: row.get(1)?,
                event_type: row.get(2)?,
                author: row.get(3)?,
                outcome: row.get(4)?,
                occurred_at_ms: row.get(5)?,
                correlation_id: row.get(6)?,
                causation_id: row.get(7)?,
                operation_id: row.get(8)?,
                task_id: row.get(9)?,
                execution_id: row.get(10)?,
                attempt_id: row.get(11)?,
                subject_kind: row.get(12)?,
                subject_id: row.get(13)?,
                policy_fingerprint: row.get(14)?,
                rule_id: row.get(15)?,
                reason_code: row.get(16)?,
                approval_request_id: row.get(17)?,
                previous_hash: row.get(18)?,
                record_hash: row.get(19)?,
            })
        })?;
        let raws = rows.collect::<Result<Vec<_>, _>>()?;
        drop(statement);

        let mut records = Vec::with_capacity(raws.len());
        let mut expected_previous = GENESIS_HASH.to_owned();
        for raw in raws {
            if raw.record_version != SECURITY_AUDIT_RECORD_VERSION {
                return Err(StorageError::Corrupt(format!(
                    "unsupported security audit record version {}",
                    raw.record_version
                )));
            }
            if raw.previous_hash != expected_previous {
                return Err(StorageError::Corrupt(format!(
                    "security audit chain mismatch at seq {}",
                    raw.seq
                )));
            }
            let fact = raw.to_fact(self)?;
            let expected_hash = hash_record(raw.seq, &raw.previous_hash, &fact);
            if raw.record_hash != expected_hash {
                return Err(StorageError::Corrupt(format!(
                    "security audit hash mismatch at seq {}",
                    raw.seq
                )));
            }
            expected_previous.clone_from(&raw.record_hash);
            records.push(SecurityAuditRecord {
                id: SecurityAuditId::from_seq(raw.seq).map_err(map_audit_corruption)?,
                seq: raw.seq,
                fact,
                previous_hash: raw.previous_hash,
                record_hash: raw.record_hash,
            });
        }
        let (head_seq, head_hash): (i64, String) = self.connection().query_row(
            "SELECT seq, record_hash FROM security_audit_head WHERE singleton = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let observed_seq = records.last().map_or(0, |record| record.seq);
        let observed_hash = records
            .last()
            .map_or(GENESIS_HASH, |record| record.record_hash.as_str());
        if head_seq != observed_seq || head_hash != observed_hash {
            return Err(StorageError::Corrupt(
                "security audit head does not match persisted chain".to_owned(),
            ));
        }
        Ok(records)
    }

    pub fn list_security_audit_by_correlation(
        &self,
        correlation_id: &str,
    ) -> StorageResult<Vec<SecurityAuditRecord>> {
        if correlation_id.trim().is_empty()
            || correlation_id.len() > 512
            || correlation_id.chars().any(char::is_control)
        {
            return Err(StorageError::ConstraintViolation(
                "invalid security audit correlation_id".to_owned(),
            ));
        }
        Ok(self
            .list_security_audit_ordered()?
            .into_iter()
            .filter(|record| record.fact.correlation_id == correlation_id)
            .collect())
    }
}

pub(crate) fn append_security_audit_in_transaction(
    transaction: &Transaction<'_>,
    record: &NewSecurityAuditRecord,
) -> StorageResult<SecurityAuditRecord> {
    let fact = record
        .validated()
        .map_err(|error| StorageError::ConstraintViolation(error.to_string()))?;
    let occurred_at_ms = to_sql_millis(fact.occurred_at_ms)?;
    let (head_seq, previous_hash): (i64, String) = transaction.query_row(
        "SELECT seq, record_hash FROM security_audit_head WHERE singleton = 1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let seq = head_seq.checked_add(1).ok_or_else(|| {
        StorageError::ConstraintViolation("security audit sequence exhausted".to_owned())
    })?;
    let record_hash = hash_record(seq, &previous_hash, &fact);
    let (subject_kind, subject_id) = fact
        .subject
        .as_ref()
        .map(|subject| (Some(subject.kind.as_str()), Some(subject.id.as_str())))
        .unwrap_or((None, None));
    transaction.execute(
        "INSERT INTO security_audit (
                seq, record_version, event_type, author, outcome, occurred_at_ms,
                correlation_id, causation_id, operation_id, task_id, execution_id,
                attempt_id, subject_kind, subject_id, policy_fingerprint, rule_id,
                reason_code, approval_request_id, previous_hash, record_hash
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                ?15, ?16, ?17, ?18, ?19, ?20
             )",
        params![
            seq,
            SECURITY_AUDIT_RECORD_VERSION,
            fact.event.as_str(),
            fact.author.as_str(),
            fact.outcome.map(SecurityAuditOutcome::as_str),
            occurred_at_ms,
            fact.correlation_id,
            fact.causation_id,
            fact.operation_id,
            fact.task_id,
            fact.execution_id,
            fact.attempt_id,
            subject_kind,
            subject_id,
            fact.policy_fingerprint,
            fact.rule_id,
            fact.reason_code,
            fact.approval_request_id,
            previous_hash,
            record_hash,
        ],
    )?;
    for (position, capability) in fact.capabilities.iter().enumerate() {
        transaction.execute(
            "INSERT INTO security_audit_capabilities
                    (audit_seq, position, capability_kind, workspace, scope)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                seq,
                i64::try_from(position).map_err(|_| StorageError::ConstraintViolation(
                    "too many security audit capabilities".to_owned()
                ))?,
                capability.kind.as_str(),
                capability.workspace,
                capability.scope,
            ],
        )?;
    }
    let changed = transaction.execute(
        "UPDATE security_audit_head SET seq = ?1, record_hash = ?2
             WHERE singleton = 1 AND seq = ?3 AND record_hash = ?4",
        params![seq, record_hash, head_seq, previous_hash],
    )?;
    if changed != 1 {
        return Err(StorageError::ConstraintViolation(
            "security audit head changed concurrently".to_owned(),
        ));
    }
    Ok(SecurityAuditRecord {
        id: SecurityAuditId::from_seq(seq).map_err(map_audit_corruption)?,
        seq,
        fact,
        previous_hash,
        record_hash,
    })
}

struct RawAuditRow {
    seq: i64,
    record_version: u32,
    event_type: String,
    author: String,
    outcome: Option<String>,
    occurred_at_ms: i64,
    correlation_id: String,
    causation_id: Option<String>,
    operation_id: Option<String>,
    task_id: Option<String>,
    execution_id: Option<String>,
    attempt_id: Option<String>,
    subject_kind: Option<String>,
    subject_id: Option<String>,
    policy_fingerprint: Option<String>,
    rule_id: Option<String>,
    reason_code: Option<String>,
    approval_request_id: Option<String>,
    previous_hash: String,
    record_hash: String,
}

impl RawAuditRow {
    fn to_fact(&self, store: &SqliteStore) -> StorageResult<NewSecurityAuditRecord> {
        if self.occurred_at_ms < 0 {
            return Err(StorageError::Corrupt(
                "negative security audit timestamp".to_owned(),
            ));
        }
        let subject = match (&self.subject_kind, &self.subject_id) {
            (Some(kind), Some(id)) => Some(SecurityAuditSubject {
                kind: SecurityAuditSubjectKind::parse(kind).map_err(map_audit_corruption)?,
                id: id.clone(),
            }),
            (None, None) => None,
            _ => {
                return Err(StorageError::Corrupt(
                    "partial security audit subject".to_owned(),
                ))
            }
        };
        let mut statement = store.connection().prepare(
            "SELECT capability_kind, workspace, scope
             FROM security_audit_capabilities WHERE audit_seq = ?1 ORDER BY position",
        )?;
        let capability_rows = statement.query_map(params![self.seq], |row| {
            Ok((row.get::<_, String>(0)?, row.get(1)?, row.get(2)?))
        })?;
        let mut capabilities = Vec::new();
        for row in capability_rows {
            let (kind, workspace, scope): (String, Option<String>, String) = row?;
            capabilities.push(
                AuditCapability::parse(&kind, workspace, scope).map_err(map_audit_corruption)?,
            );
        }
        NewSecurityAuditRecord {
            event: SecurityAuditEvent::parse(&self.event_type).map_err(map_audit_corruption)?,
            author: SecurityAuditAuthor::parse(&self.author).map_err(map_audit_corruption)?,
            outcome: self
                .outcome
                .as_deref()
                .map(SecurityAuditOutcome::parse)
                .transpose()
                .map_err(map_audit_corruption)?,
            occurred_at_ms: self.occurred_at_ms as u64,
            correlation_id: self.correlation_id.clone(),
            causation_id: self.causation_id.clone(),
            operation_id: self.operation_id.clone(),
            task_id: self.task_id.clone(),
            execution_id: self.execution_id.clone(),
            attempt_id: self.attempt_id.clone(),
            subject,
            policy_fingerprint: self.policy_fingerprint.clone(),
            rule_id: self.rule_id.clone(),
            reason_code: self.reason_code.clone(),
            approval_request_id: self.approval_request_id.clone(),
            capabilities,
        }
        .validated()
        .map_err(map_audit_corruption)
    }
}

fn hash_record(seq: i64, previous_hash: &str, fact: &NewSecurityAuditRecord) -> String {
    let mut hasher = Sha256::new();
    hash_field(&mut hasher, "contract", Some("security-audit-v1"));
    hash_field(&mut hasher, "seq", Some(&seq.to_string()));
    hash_field(&mut hasher, "previous_hash", Some(previous_hash));
    hash_field(&mut hasher, "event", Some(fact.event.as_str()));
    hash_field(&mut hasher, "author", Some(fact.author.as_str()));
    hash_field(
        &mut hasher,
        "outcome",
        fact.outcome.map(SecurityAuditOutcome::as_str),
    );
    hash_field(
        &mut hasher,
        "occurred_at_ms",
        Some(&fact.occurred_at_ms.to_string()),
    );
    hash_field(&mut hasher, "correlation_id", Some(&fact.correlation_id));
    hash_field(&mut hasher, "causation_id", fact.causation_id.as_deref());
    hash_field(&mut hasher, "operation_id", fact.operation_id.as_deref());
    hash_field(&mut hasher, "task_id", fact.task_id.as_deref());
    hash_field(&mut hasher, "execution_id", fact.execution_id.as_deref());
    hash_field(&mut hasher, "attempt_id", fact.attempt_id.as_deref());
    hash_field(
        &mut hasher,
        "subject_kind",
        fact.subject.as_ref().map(|subject| subject.kind.as_str()),
    );
    hash_field(
        &mut hasher,
        "subject_id",
        fact.subject.as_ref().map(|subject| subject.id.as_str()),
    );
    hash_field(
        &mut hasher,
        "policy_fingerprint",
        fact.policy_fingerprint.as_deref(),
    );
    hash_field(&mut hasher, "rule_id", fact.rule_id.as_deref());
    hash_field(&mut hasher, "reason_code", fact.reason_code.as_deref());
    hash_field(
        &mut hasher,
        "approval_request_id",
        fact.approval_request_id.as_deref(),
    );
    for capability in &fact.capabilities {
        hash_field(
            &mut hasher,
            "capability_kind",
            Some(capability.kind.as_str()),
        );
        hash_field(
            &mut hasher,
            "capability_workspace",
            capability.workspace.as_deref(),
        );
        hash_field(&mut hasher, "capability_scope", Some(&capability.scope));
    }
    format!("sha256:{:x}", hasher.finalize())
}

fn hash_field(hasher: &mut Sha256, label: &str, value: Option<&str>) {
    let label = label.as_bytes();
    hasher.update((label.len() as u64).to_be_bytes());
    hasher.update(label);
    match value {
        Some(value) => {
            hasher.update([1]);
            let value = value.as_bytes();
            hasher.update((value.len() as u64).to_be_bytes());
            hasher.update(value);
        }
        None => hasher.update([0]),
    }
}

fn to_sql_millis(value: u64) -> StorageResult<i64> {
    i64::try_from(value).map_err(|_| {
        StorageError::ConstraintViolation(
            "security audit timestamp exceeds SQLite range".to_owned(),
        )
    })
}

fn map_audit_corruption(error: SecurityAuditError) -> StorageError {
    StorageError::Corrupt(error.to_string())
}
