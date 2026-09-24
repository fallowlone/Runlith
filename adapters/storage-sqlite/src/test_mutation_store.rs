use policy::{TestMutationPolicy, TestMutationPolicyRevision};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

use crate::{error::StorageError, task_store::SqliteStore, StorageResult};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvaluatedPolicyBase {
    pub execution_id: String,
    pub task_id: String,
    pub workspace_id: String,
    pub policy: TestMutationPolicyRevision,
    pub base_revision: String,
}

pub(crate) fn insert_default_policy(connection: &Connection, task_id: &str) -> StorageResult<()> {
    let policy = TestMutationPolicyRevision::new(1, TestMutationPolicy::Forbidden)
        .expect("revision one Forbidden is valid");
    connection.execute(
        "INSERT INTO test_mutation_policy_revisions(task_id, revision, serialized, fingerprint)
         VALUES (?1, ?2, ?3, ?4)",
        params![
            task_id,
            policy.revision() as i64,
            policy.serialize(),
            policy.fingerprint()
        ],
    )?;
    Ok(())
}

impl SqliteStore {
    pub fn get_test_mutation_policy(
        &self,
        task_id: &str,
    ) -> StorageResult<Option<TestMutationPolicyRevision>> {
        let row: Option<(i64, String, String)> = self
            .connection()
            .query_row(
                "SELECT revision, serialized, fingerprint FROM test_mutation_policy_revisions
             WHERE task_id = ?1 ORDER BY revision DESC LIMIT 1",
                params![task_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        row.map(|(revision, serialized, fingerprint)| {
            let revision = u64::try_from(revision)
                .map_err(|_| StorageError::Corrupt("invalid policy revision".into()))?;
            TestMutationPolicyRevision::restore(revision, &serialized, &fingerprint)
                .map_err(|error| StorageError::Corrupt(format!("task {task_id} policy: {error}")))
        })
        .transpose()
    }

    pub fn bind_evaluated_policy_base(
        &mut self,
        execution_id: &str,
        task_id: &str,
        workspace_id: &str,
        base_revision: &str,
    ) -> StorageResult<EvaluatedPolicyBase> {
        if !((base_revision.len() == 40 || base_revision.len() == 64)
            && base_revision
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()))
        {
            return Err(StorageError::ConstraintViolation(
                "evaluated base must be a lowercase immutable commit OID".into(),
            ));
        }
        // Serialize the lease-state check and immutable binding insert against
        // workspace lifecycle transitions on other connections.
        let transaction = self
            .connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let lease: Option<(String, String, String)> = transaction
            .query_row(
                "SELECT e.task_id, w.lifecycle, w.base_revision
                 FROM executions e
                 JOIN workspace_leases w ON w.execution_id = e.id
                 WHERE e.id = ?1 AND w.workspace_id = ?2",
                params![execution_id, workspace_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        match lease {
            Some((persisted_task_id, lifecycle, persisted_base))
                if persisted_task_id == task_id
                    && lifecycle == "READY"
                    && persisted_base == base_revision => {}
            _ => {
                return Err(StorageError::ConstraintViolation(
                    "evaluated policy base requires the execution's READY workspace lease and persisted base".into(),
                ));
            }
        }
        let policy = load_current_policy(&transaction, task_id)?;
        transaction.execute(
            "INSERT INTO evaluated_policy_bases(execution_id, task_id, workspace_id,
                 policy_revision, policy_fingerprint, base_revision)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(execution_id) DO NOTHING",
            params![
                execution_id,
                task_id,
                workspace_id,
                policy.revision() as i64,
                policy.fingerprint(),
                base_revision
            ],
        )?;
        let binding = load_base(&transaction, execution_id)?
            .ok_or_else(|| StorageError::Corrupt("policy base binding vanished".into()))?;
        if binding.task_id != task_id
            || binding.workspace_id != workspace_id
            || binding.base_revision != base_revision
            || binding.policy != policy
        {
            return Err(StorageError::ConstraintViolation(
                "execution already has a different immutable policy/base binding".into(),
            ));
        }
        transaction.commit()?;
        Ok(binding)
    }

    pub fn get_evaluated_policy_base(
        &self,
        execution_id: &str,
    ) -> StorageResult<Option<EvaluatedPolicyBase>> {
        load_base(self.connection(), execution_id)
    }
}

fn load_current_policy(
    connection: &Connection,
    task_id: &str,
) -> StorageResult<TestMutationPolicyRevision> {
    let (revision, serialized, fingerprint): (i64, String, String) = connection.query_row(
        "SELECT revision, serialized, fingerprint FROM test_mutation_policy_revisions
         WHERE task_id = ?1 ORDER BY revision DESC LIMIT 1",
        params![task_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    let revision = u64::try_from(revision)
        .map_err(|_| StorageError::Corrupt("invalid policy revision".into()))?;
    TestMutationPolicyRevision::restore(revision, &serialized, &fingerprint)
        .map_err(|error| StorageError::Corrupt(format!("task {task_id} policy: {error}")))
}

fn load_base(
    connection: &Connection,
    execution_id: &str,
) -> StorageResult<Option<EvaluatedPolicyBase>> {
    let row: Option<(
        String,
        String,
        i64,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        i64,
    )> = connection
        .query_row(
            "SELECT b.task_id, b.workspace_id, b.policy_revision, b.policy_fingerprint,
                b.base_revision, p.serialized, p.fingerprint, e.task_id,
                w.execution_id, w.base_revision,
                (SELECT MAX(revision) FROM test_mutation_policy_revisions
                 WHERE task_id = b.task_id)
         FROM evaluated_policy_bases b JOIN test_mutation_policy_revisions p
           ON p.task_id = b.task_id AND p.revision = b.policy_revision
         JOIN executions e ON e.id = b.execution_id
         JOIN workspace_leases w ON w.workspace_id = b.workspace_id
         WHERE b.execution_id = ?1",
            params![execution_id],
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
                ))
            },
        )
        .optional()?;
    row.map(
        |(
            task_id,
            workspace_id,
            revision,
            fingerprint,
            base_revision,
            serialized,
            stored_fingerprint,
            execution_task,
            workspace_execution,
            workspace_base,
            latest_revision,
        )| {
            let revision = u64::try_from(revision)
                .map_err(|_| StorageError::Corrupt("invalid bound policy revision".into()))?;
            let policy =
                TestMutationPolicyRevision::restore(revision, &serialized, &stored_fingerprint)
                    .map_err(|error| {
                        StorageError::Corrupt(format!(
                            "execution {execution_id} policy binding: {error}"
                        ))
                    })?;
            if policy.fingerprint() != fingerprint {
                return Err(StorageError::Corrupt(format!(
                    "execution {execution_id} policy fingerprint mismatch"
                )));
            }
            if execution_task != task_id
                || workspace_execution != execution_id
                || workspace_base != base_revision
            {
                return Err(StorageError::Corrupt(format!(
                    "execution {execution_id} policy base disagrees with its persisted owner or workspace lease"
                )));
            }
            if i64::try_from(revision).ok() != Some(latest_revision) {
                return Err(StorageError::Corrupt(format!(
                    "execution {execution_id} policy revision is stale"
                )));
            }
            Ok(EvaluatedPolicyBase {
                execution_id: execution_id.into(),
                task_id,
                workspace_id,
                policy,
                base_revision,
            })
        },
    )
    .transpose()
}
