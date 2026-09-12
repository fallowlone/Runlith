//! Durable Policy/Security approval state for TASK-0062.
//!
//! This is relational current state only. It deliberately does not append to
//! the Event Journal and is not the Security Audit store (TASK-0066).

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

use crate::{SqliteStore, StorageError, StorageResult};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApprovalState {
    Pending,
    Approved,
    Denied,
    Consumed,
}

impl ApprovalState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Denied => "denied",
            Self::Consumed => "consumed",
        }
    }

    fn parse(value: &str) -> StorageResult<Self> {
        match value {
            "pending" => Ok(Self::Pending),
            "approved" => Ok(Self::Approved),
            "denied" => Ok(Self::Denied),
            "consumed" => Ok(Self::Consumed),
            other => Err(StorageError::Corrupt(format!(
                "unknown approval state {other:?}"
            ))),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApprovalCapabilityRecord {
    pub kind: String,
    pub workspace: Option<String>,
    pub scope: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewApprovalRecord {
    pub request_id: String,
    pub binding_fingerprint: String,
    pub operation_id: String,
    pub requester: String,
    pub task_id: String,
    pub execution_id: Option<String>,
    pub attempt_id: Option<String>,
    pub policy_fingerprint: String,
    pub expires_at_ms: u64,
    pub requested: Vec<ApprovalCapabilityRecord>,
    pub effective: Vec<ApprovalCapabilityRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApprovalRecord {
    pub request_id: String,
    pub binding_fingerprint: String,
    pub operation_id: String,
    pub requester: String,
    pub task_id: String,
    pub execution_id: Option<String>,
    pub attempt_id: Option<String>,
    pub policy_fingerprint: String,
    pub expires_at_ms: u64,
    pub state: ApprovalState,
    pub approver: Option<String>,
    pub decided_at_ms: Option<u64>,
    pub consumed_at_ms: Option<u64>,
    pub requested: Vec<ApprovalCapabilityRecord>,
    pub effective: Vec<ApprovalCapabilityRecord>,
}

impl SqliteStore {
    /// Inserts a pending approval. Re-registering the identical binding is
    /// idempotent; reusing an id for a different binding fails closed.
    pub fn create_approval(&mut self, record: &NewApprovalRecord) -> StorageResult<ApprovalRecord> {
        if let Some(existing) = self.get_approval(&record.request_id)? {
            return if existing.binding_fingerprint == record.binding_fingerprint {
                Ok(existing)
            } else {
                Err(StorageError::Duplicate(format!(
                    "approval {} already exists with another binding",
                    record.request_id
                )))
            };
        }

        let expires = to_sql_millis(record.expires_at_ms, "expires_at_ms")?;
        let transaction = self
            .connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "INSERT INTO approvals (
                request_id, binding_fingerprint, operation_id, requester,
                task_id, execution_id, attempt_id, policy_fingerprint,
                expires_at_ms, state
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'pending')",
            params![
                record.request_id,
                record.binding_fingerprint,
                record.operation_id,
                record.requester,
                record.task_id,
                record.execution_id,
                record.attempt_id,
                record.policy_fingerprint,
                expires,
            ],
        )?;
        insert_capabilities(
            &transaction,
            &record.request_id,
            "requested",
            &record.requested,
        )?;
        insert_capabilities(
            &transaction,
            &record.request_id,
            "effective",
            &record.effective,
        )?;
        transaction.commit()?;
        self.get_approval(&record.request_id)?.ok_or_else(|| {
            StorageError::Corrupt(format!(
                "approval {} vanished after insert",
                record.request_id
            ))
        })
    }

    pub fn get_approval(&self, request_id: &str) -> StorageResult<Option<ApprovalRecord>> {
        load_approval(self.connection(), request_id)
    }

    /// Resolves a pending approval exactly once. Expired approvals cannot be
    /// decided, and an already-resolved row cannot be rewritten.
    pub fn decide_approval(
        &mut self,
        request_id: &str,
        decision: ApprovalState,
        approver: &str,
        now_ms: u64,
    ) -> StorageResult<ApprovalRecord> {
        if !matches!(decision, ApprovalState::Approved | ApprovalState::Denied) {
            return Err(StorageError::ConstraintViolation(
                "approval decision must be approved or denied".to_owned(),
            ));
        }
        if approver.trim().is_empty() || approver.chars().any(char::is_control) {
            return Err(StorageError::ConstraintViolation(
                "approver must be non-blank and contain no control characters".to_owned(),
            ));
        }
        let now = to_sql_millis(now_ms, "decided_at_ms")?;
        let transaction = self
            .connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = load_approval(&transaction, request_id)?.ok_or_else(|| {
            StorageError::NotFound(format!("approval {request_id} does not exist"))
        })?;
        if current.state != ApprovalState::Pending {
            return Err(StorageError::ConstraintViolation(format!(
                "approval {request_id} is already {}",
                current.state.as_str()
            )));
        }
        if now_ms > current.expires_at_ms {
            return Err(StorageError::ConstraintViolation(format!(
                "approval {request_id} is expired"
            )));
        }
        let changed = transaction.execute(
            "UPDATE approvals
             SET state = ?1, approver = ?2, decided_at_ms = ?3
             WHERE request_id = ?4 AND state = 'pending'",
            params![decision.as_str(), approver, now, request_id],
        )?;
        if changed != 1 {
            return Err(StorageError::ConstraintViolation(format!(
                "approval {request_id} changed concurrently"
            )));
        }
        transaction.commit()?;
        self.get_approval(request_id)?.ok_or_else(|| {
            StorageError::Corrupt(format!("approval {request_id} vanished after decision"))
        })
    }

    /// Atomically consumes one approved exact binding. This is the replay
    /// barrier: once consumed, the same durable approval cannot authorize a
    /// second operation attempt.
    pub fn consume_approval(
        &mut self,
        request_id: &str,
        binding_fingerprint: &str,
        now_ms: u64,
    ) -> StorageResult<ApprovalRecord> {
        let now = to_sql_millis(now_ms, "consumed_at_ms")?;
        let transaction = self
            .connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut current = load_approval(&transaction, request_id)?.ok_or_else(|| {
            StorageError::NotFound(format!("approval {request_id} does not exist"))
        })?;
        if current.binding_fingerprint != binding_fingerprint {
            return Err(StorageError::ConstraintViolation(format!(
                "approval {request_id} binding mismatch"
            )));
        }
        if current.state != ApprovalState::Approved {
            return Err(StorageError::ConstraintViolation(format!(
                "approval {request_id} is {} and cannot be consumed",
                current.state.as_str()
            )));
        }
        if now_ms > current.expires_at_ms {
            return Err(StorageError::ConstraintViolation(format!(
                "approval {request_id} is expired"
            )));
        }
        let changed = transaction.execute(
            "UPDATE approvals
             SET state = 'consumed', consumed_at_ms = ?1
             WHERE request_id = ?2 AND state = 'approved' AND binding_fingerprint = ?3",
            params![now, request_id, binding_fingerprint],
        )?;
        if changed != 1 {
            return Err(StorageError::ConstraintViolation(format!(
                "approval {request_id} changed concurrently"
            )));
        }
        transaction.commit()?;
        current.state = ApprovalState::Consumed;
        current.consumed_at_ms = Some(now_ms);
        Ok(current)
    }
}

fn insert_capabilities(
    connection: &Connection,
    request_id: &str,
    authority_set: &str,
    capabilities: &[ApprovalCapabilityRecord],
) -> StorageResult<()> {
    for (position, capability) in capabilities.iter().enumerate() {
        let position = i64::try_from(position).map_err(|_| {
            StorageError::ConstraintViolation("too many approval capabilities".to_owned())
        })?;
        connection.execute(
            "INSERT INTO approval_capabilities
             (request_id, authority_set, position, kind, workspace, scope)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                request_id,
                authority_set,
                position,
                capability.kind,
                capability.workspace,
                capability.scope,
            ],
        )?;
    }
    Ok(())
}

fn load_approval(
    connection: &Connection,
    request_id: &str,
) -> StorageResult<Option<ApprovalRecord>> {
    type ApprovalRow = (
        String,
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        String,
        i64,
        String,
        Option<String>,
        Option<i64>,
        Option<i64>,
    );
    let row: Option<ApprovalRow> = connection
        .query_row(
            "SELECT request_id, binding_fingerprint, operation_id, requester,
                    task_id, execution_id, attempt_id, policy_fingerprint,
                    expires_at_ms, state, approver, decided_at_ms, consumed_at_ms
             FROM approvals WHERE request_id = ?1",
            params![request_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                    row.get(9)?,
                    row.get(10)?,
                    row.get(11)?,
                    row.get(12)?,
                ))
            },
        )
        .optional()?;
    let Some((
        request_id,
        binding_fingerprint,
        operation_id,
        requester,
        task_id,
        execution_id,
        attempt_id,
        policy_fingerprint,
        expires_at_ms,
        state,
        approver,
        decided_at_ms,
        consumed_at_ms,
    )) = row
    else {
        return Ok(None);
    };
    Ok(Some(ApprovalRecord {
        requested: load_capabilities(connection, &request_id, "requested")?,
        effective: load_capabilities(connection, &request_id, "effective")?,
        request_id,
        binding_fingerprint,
        operation_id,
        requester,
        task_id,
        execution_id,
        attempt_id,
        policy_fingerprint,
        expires_at_ms: from_sql_millis(expires_at_ms, "expires_at_ms")?,
        state: ApprovalState::parse(&state)?,
        approver,
        decided_at_ms: decided_at_ms
            .map(|value| from_sql_millis(value, "decided_at_ms"))
            .transpose()?,
        consumed_at_ms: consumed_at_ms
            .map(|value| from_sql_millis(value, "consumed_at_ms"))
            .transpose()?,
    }))
}

fn load_capabilities(
    connection: &Connection,
    request_id: &str,
    authority_set: &str,
) -> StorageResult<Vec<ApprovalCapabilityRecord>> {
    let mut statement = connection.prepare(
        "SELECT kind, workspace, scope FROM approval_capabilities
         WHERE request_id = ?1 AND authority_set = ?2 ORDER BY position",
    )?;
    let rows = statement.query_map(params![request_id, authority_set], |row| {
        Ok(ApprovalCapabilityRecord {
            kind: row.get(0)?,
            workspace: row.get(1)?,
            scope: row.get(2)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn to_sql_millis(value: u64, field: &str) -> StorageResult<i64> {
    i64::try_from(value).map_err(|_| {
        StorageError::ConstraintViolation(format!("{field} exceeds SQLite INTEGER range"))
    })
}

fn from_sql_millis(value: i64, field: &str) -> StorageResult<u64> {
    u64::try_from(value)
        .map_err(|_| StorageError::Corrupt(format!("{field} is negative in approval state")))
}
