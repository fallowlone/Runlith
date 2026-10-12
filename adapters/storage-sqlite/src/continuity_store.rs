//! SQLite continuity seam (TASK-0171, ADR-0043).
//!
//! Staging is a checkpoint state, not a separate store: every
//! committed read filters `state = 'COMMITTED'`, so staged rows are
//! invisible by construction (CR-2), including after reopen. Commit
//! verifies (read-back, hashes, parent chain) and flips state plus
//! the epoch index entry plus history refs in one transaction
//! (ADR-0043 §3).

use continuity::history::{HistoryContent, HistoryHit, SearchFilters};
use continuity::model::{
    ActivationRecord, BudgetEvent, Checkpoint, CheckpointId, CheckpointReceipt, ContinuityError,
    Epoch, EpochId, EpochState, HistoryRef, MigrationLink, PurgeReport, RetentionPolicy,
    SessionLink, WorkingNote,
};
use continuity::port::ContinuityStorePort;
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use sha2::{Digest, Sha256};

use crate::context_store::{decode_source, encode_source, i64_to_u64, sqlite, u64_to_i64};
use crate::task_store::SqliteStore;

fn context_err(err: context_engine::model::ContextError) -> ContinuityError {
    use context_engine::model::ContextError as C;
    match err {
        C::InvalidInput(detail) => ContinuityError::InvalidInput(detail),
        C::BackendFailure { detail } => ContinuityError::BackendFailure { detail },
        other => ContinuityError::InvalidInput(format!("context engine: {other}")),
    }
}

fn backend(error: rusqlite::Error) -> ContinuityError {
    context_err(sqlite(error))
}

fn reconstruct<T>(parsed: Result<T, ContinuityError>, what: &str) -> Result<T, ContinuityError> {
    parsed.map_err(|err| ContinuityError::BackendFailure {
        detail: format!("stored {what} is malformed: {err}"),
    })
}

/// Canonical checkpoint bytes for the staged hash: field order is
/// fixed, children in position order. Detects staged-row corruption
/// and partial writes at commit (ADR-0043 §3 verification).
fn canonical_checkpoint(checkpoint: &Checkpoint) -> Result<String, ContinuityError> {
    let mut out = String::new();
    out.push_str(checkpoint.id.as_str());
    out.push('\u{1f}');
    out.push_str(checkpoint.epoch_ref.as_str());
    out.push('\u{1f}');
    out.push_str(checkpoint.task_ref.as_str());
    out.push('\u{1f}');
    out.push_str(checkpoint.execution_ref.as_str());
    out.push('\u{1f}');
    out.push_str(&checkpoint.created_at.to_string());
    out.push('\u{1f}');
    out.push_str(&checkpoint.format_version.as_u32().to_string());
    out.push('\u{1f}');
    out.push_str(
        checkpoint
            .parent_checkpoint
            .as_ref()
            .map_or("", CheckpointId::as_str),
    );
    for note in &checkpoint.working_state {
        out.push('\u{1f}');
        out.push_str(note.as_str());
    }
    out.push('\u{1e}');
    for source in &checkpoint.source_refs {
        let (kind, p1, p2, n1, start, end) = encode_source(source).map_err(context_err)?;
        out.push_str(&format!(
            "{kind}\u{1f}{p1}\u{1f}{p2}\u{1f}{n1}\u{1f}{start:?}\u{1f}{end:?}\u{1e}"
        ));
    }
    for reference in &checkpoint.evidence_refs {
        out.push_str(reference);
        out.push('\u{1e}');
    }
    out.push_str(&checkpoint.continuation);
    out.push('\u{1f}');
    out.push_str(checkpoint.context_plan_ref.as_str());
    out.push('\u{1f}');
    out.push_str(checkpoint.sensitivity_ceiling.as_str());
    Ok(out)
}

fn staged_hash(checkpoint: &Checkpoint) -> Result<String, ContinuityError> {
    Ok(format!(
        "sha256:{:x}",
        Sha256::digest(canonical_checkpoint(checkpoint)?)
    ))
}

const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Derives a stable opaque history ref from (checkpoint, kind,
/// position): commit-internal refs need no clock or sequence, and
/// re-commit derives the same refs (idempotent).
fn derive_history_ref(checkpoint: &CheckpointId, kind: &str, position: usize) -> HistoryRef {
    let digest = Sha256::digest(format!("histref|{}|{kind}|{position}", checkpoint.as_str()));
    let mut bits: u128 = 0;
    for byte in digest.iter().take(16) {
        bits = (bits << 8) | u128::from(*byte);
    }
    let mut suffix = [0u8; 26];
    for slot in suffix.iter_mut().rev().take(16) {
        *slot = CROCKFORD[(bits & 31) as usize];
        bits >>= 5;
    }
    let mut high: u128 = 0;
    for byte in digest.iter().skip(16).take(12) {
        high = (high << 8) | u128::from(*byte);
    }
    for slot in suffix.iter_mut().rev().skip(16) {
        *slot = CROCKFORD[(high & 31) as usize];
        high >>= 5;
    }
    let mut id = String::with_capacity(8 + 26);
    id.push_str("histref_");
    id.push_str(std::str::from_utf8(&suffix).expect("crockford"));
    HistoryRef::new(id).expect("derived ref")
}

fn truncate_title(text: &str) -> String {
    if text.chars().count() <= 128 {
        text.to_owned()
    } else {
        text.chars().take(128).collect()
    }
}

fn decode_epoch(
    id: String,
    task: String,
    execution: String,
    seq: i64,
    state: String,
    created_at: i64,
) -> Result<Epoch, ContinuityError> {
    use continuity::model::{EpochId, EpochState as State};
    Ok(Epoch {
        id: reconstruct(EpochId::new(id), "epoch id")?,
        task_ref: reconstruct(
            task_domain::TaskId::new(task)
                .map_err(|err| ContinuityError::InvalidInput(format!("task ref: {err}"))),
            "task ref",
        )?,
        execution_ref: reconstruct(
            agent_runtime::ExecutionId::new(execution)
                .map_err(|err| ContinuityError::InvalidInput(format!("execution ref: {err}"))),
            "execution ref",
        )?,
        seq: i64_to_u64("epoch seq", seq).map_err(context_err)?,
        state: reconstruct(State::parse(&state), "epoch state")?,
        created_at: i64_to_u64("epoch created at", created_at).map_err(context_err)?,
    })
}

type CheckpointRow = (
    String,
    String,
    String,
    String,
    i64,
    i64,
    Option<String>,
    String,
    String,
    String,
    String,
    String,
    Option<i64>,
    Option<i64>,
);

/// A decoded checkpoint plus its commit columns (state, committed_at,
/// store_revision).
type DecodedCheckpoint = (Checkpoint, String, Option<i64>, Option<i64>);

fn decode_checkpoint(
    connection: &rusqlite::Connection,
    id: &str,
    committed_only: bool,
) -> Result<Option<DecodedCheckpoint>, ContinuityError> {
    use context_engine::model::{ContextPlanId, Sensitivity, SourceRef};
    use continuity::model::{CheckpointId, FormatVersion as Version, NoteId};
    let filter = if committed_only {
        "AND state = 'COMMITTED'"
    } else {
        ""
    };
    let row: Option<CheckpointRow> = connection
        .query_row(
            &format!(
                "SELECT checkpoint_id, epoch_ref, task_ref, execution_ref, created_at,
                        format_version, parent_checkpoint, continuation, context_plan_ref,
                        sensitivity_ceiling, staged_hash, state, committed_at, store_revision
                 FROM continuity_checkpoints WHERE checkpoint_id = ?1 {filter}"
            ),
            params![id],
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
                    row.get(13)?,
                ))
            },
        )
        .optional()
        .map_err(backend)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let (
        checkpoint_id,
        epoch_ref,
        task_ref,
        execution_ref,
        created_at,
        format_version,
        parent,
        continuation,
        plan_ref,
        ceiling,
        _staged_hash,
        state,
        committed_at,
        store_revision,
    ) = row;
    let mut sources = connection
        .prepare(
            "SELECT source_kind, source_p1, source_p2, source_n1,
                    source_span_start, source_span_end
             FROM continuity_checkpoint_sources
             WHERE checkpoint_id = ?1 ORDER BY position",
        )
        .map_err(backend)?;
    let source_refs: Vec<SourceRef> = sources
        .query_map(params![id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, Option<i64>>(5)?,
            ))
        })
        .map_err(backend)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(backend)?
        .into_iter()
        .map(|(kind, p1, p2, n1, start, end)| {
            decode_source(&kind, &p1, &p2, n1, start, end).map_err(context_err)
        })
        .collect::<Result<_, _>>()?;
    let mut evidence = connection
        .prepare(
            "SELECT evidence_ref FROM continuity_checkpoint_evidence
             WHERE checkpoint_id = ?1 ORDER BY position",
        )
        .map_err(backend)?;
    let evidence_refs: Vec<String> = evidence
        .query_map(params![id], |row| row.get(0))
        .map_err(backend)?
        .collect::<Result<_, _>>()
        .map_err(backend)?;
    let mut notes = connection
        .prepare(
            "SELECT note_id FROM continuity_checkpoint_notes
             WHERE checkpoint_id = ?1 ORDER BY position",
        )
        .map_err(backend)?;
    let working_state: Vec<NoteId> = notes
        .query_map(params![id], |row| row.get::<_, String>(0))
        .map_err(backend)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(backend)?
        .into_iter()
        .map(|raw| reconstruct(NoteId::new(raw), "note id"))
        .collect::<Result<_, _>>()?;
    let checkpoint = Checkpoint {
        id: reconstruct(CheckpointId::new(checkpoint_id), "checkpoint id")?,
        epoch_ref: reconstruct(
            continuity::model::EpochId::new(epoch_ref),
            "checkpoint epoch ref",
        )?,
        task_ref: reconstruct(
            task_domain::TaskId::new(task_ref)
                .map_err(|err| ContinuityError::InvalidInput(format!("task ref: {err}"))),
            "task ref",
        )?,
        execution_ref: reconstruct(
            agent_runtime::ExecutionId::new(execution_ref)
                .map_err(|err| ContinuityError::InvalidInput(format!("execution ref: {err}"))),
            "execution ref",
        )?,
        created_at: i64_to_u64("checkpoint created at", created_at).map_err(context_err)?,
        format_version: Version::new(u32::try_from(format_version).map_err(|_| {
            ContinuityError::BackendFailure {
                detail: "stored format version exceeds u32".to_owned(),
            }
        })?),
        parent_checkpoint: parent
            .map(|raw| reconstruct(CheckpointId::new(raw), "parent checkpoint"))
            .transpose()?,
        working_state,
        source_refs,
        evidence_refs,
        continuation,
        context_plan_ref: reconstruct(
            ContextPlanId::new(plan_ref).map_err(context_err),
            "context plan ref",
        )?,
        sensitivity_ceiling: reconstruct(
            Sensitivity::parse(&ceiling).map_err(context_err),
            "sensitivity ceiling",
        )?,
    };
    // CR-10 on every read path: unknown majors refuse here, so no
    // load/activate/migrate can run on unreadable rows.
    checkpoint.format_version.require_supported()?;
    Ok(Some((checkpoint, state, committed_at, store_revision)))
}

fn load_epoch_row(
    connection: &rusqlite::Connection,
    id: &str,
) -> Result<Option<Epoch>, ContinuityError> {
    let row: Option<(String, String, String, i64, String, i64)> = connection
        .query_row(
            "SELECT epoch_id, task_ref, execution_ref, epoch_seq, state, created_at
             FROM continuity_epochs WHERE epoch_id = ?1",
            params![id],
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
        .optional()
        .map_err(backend)?;
    row.map(|(id, task, execution, seq, state, created_at)| {
        decode_epoch(id, task, execution, seq, state, created_at)
    })
    .transpose()
}

impl ContinuityStorePort for SqliteStore {
    fn open_chain(&mut self, genesis: &Epoch) -> Result<(), ContinuityError> {
        if genesis.seq != 0 {
            return Err(ContinuityError::InvalidInput(
                "genesis epoch must be seq 0".to_owned(),
            ));
        }
        if genesis.state != EpochState::Active {
            return Err(ContinuityError::InvalidInput(
                "genesis epoch must arrive ACTIVE".to_owned(),
            ));
        }
        let count: i64 = self
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM continuity_epochs WHERE task_ref = ?1 AND execution_ref = ?2",
                params![genesis.task_ref.as_str(), genesis.execution_ref.as_str()],
                |row| row.get(0),
            )
            .map_err(backend)?;
        if count > 0 {
            return Err(ContinuityError::InvalidInput(
                "chain already has epochs".to_owned(),
            ));
        }
        self.connection()
            .execute(
                "INSERT INTO continuity_epochs
                 (epoch_id, task_ref, execution_ref, epoch_seq, state, created_at,
                  checkpointed_by)
                 VALUES (?1, ?2, ?3, 0, 'ACTIVE', ?4, NULL)",
                params![
                    genesis.id.as_str(),
                    genesis.task_ref.as_str(),
                    genesis.execution_ref.as_str(),
                    u64_to_i64("genesis created at", genesis.created_at).map_err(context_err)?,
                ],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn stage_checkpoint(
        &mut self,
        checkpoint: &Checkpoint,
        notes: &[WorkingNote],
    ) -> Result<(), ContinuityError> {
        checkpoint.validate()?;
        for note in notes {
            note.validate()?;
            if note.epoch_ref != checkpoint.epoch_ref {
                return Err(ContinuityError::InvalidInput(format!(
                    "note {} belongs to another epoch",
                    note.id.as_str()
                )));
            }
        }
        let epoch =
            load_epoch_row(self.connection(), checkpoint.epoch_ref.as_str())?.ok_or_else(|| {
                ContinuityError::InvalidInput("checkpoint references an unknown epoch".to_owned())
            })?;
        if epoch.state != EpochState::Active {
            return Err(ContinuityError::InvalidInput(
                "checkpoints close the ACTIVE epoch only".to_owned(),
            ));
        }
        if checkpoint.task_ref != epoch.task_ref || checkpoint.execution_ref != epoch.execution_ref
        {
            return Err(ContinuityError::InvalidInput(
                "checkpoint chain differs from its epoch chain".to_owned(),
            ));
        }
        if decode_checkpoint(self.connection(), checkpoint.id.as_str(), false)?.is_some() {
            return Err(ContinuityError::InvalidInput(
                "checkpoint id is already staged or committed".to_owned(),
            ));
        }
        for note in notes {
            let exists: bool = self
                .connection()
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM continuity_notes WHERE note_id = ?1)",
                    params![note.id.as_str()],
                    |row| row.get(0),
                )
                .map_err(backend)?;
            if exists {
                return Err(ContinuityError::InvalidInput(format!(
                    "note {} is already staged",
                    note.id.as_str()
                )));
            }
        }
        for note_id in &checkpoint.working_state {
            let staged_now = notes.iter().any(|note| &note.id == note_id);
            if staged_now {
                continue;
            }
            let exists: bool = self
                .connection()
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM continuity_notes WHERE note_id = ?1)",
                    params![note_id.as_str()],
                    |row| row.get(0),
                )
                .map_err(backend)?;
            if !exists {
                return Err(ContinuityError::InvalidInput(format!(
                    "working state references unknown note {}",
                    note_id.as_str()
                )));
            }
        }
        let hash = staged_hash(checkpoint)?;
        let transaction = self
            .connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(backend)?;
        transaction
            .execute(
                "INSERT INTO continuity_checkpoints
                 (checkpoint_id, epoch_ref, task_ref, execution_ref, created_at,
                  format_version, parent_checkpoint, continuation, context_plan_ref,
                  sensitivity_ceiling, state, staged_hash, committed_at, store_revision)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'STAGED', ?11, NULL, NULL)",
                params![
                    checkpoint.id.as_str(),
                    checkpoint.epoch_ref.as_str(),
                    checkpoint.task_ref.as_str(),
                    checkpoint.execution_ref.as_str(),
                    u64_to_i64("checkpoint created at", checkpoint.created_at)
                        .map_err(context_err)?,
                    i64::from(checkpoint.format_version.as_u32()),
                    checkpoint.parent_checkpoint.as_ref().map(|id| id.as_str()),
                    checkpoint.continuation,
                    checkpoint.context_plan_ref.as_str(),
                    checkpoint.sensitivity_ceiling.as_str(),
                    hash,
                ],
            )
            .map_err(backend)?;
        for (position, source) in checkpoint.source_refs.iter().enumerate() {
            let (kind, p1, p2, n1, start, end) = encode_source(source).map_err(context_err)?;
            transaction
                .execute(
                    "INSERT INTO continuity_checkpoint_sources
                     (checkpoint_id, position, source_kind, source_p1, source_p2,
                      source_n1, source_span_start, source_span_end)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        checkpoint.id.as_str(),
                        position as i64,
                        kind,
                        p1,
                        p2,
                        n1,
                        start,
                        end
                    ],
                )
                .map_err(backend)?;
        }
        for (position, reference) in checkpoint.evidence_refs.iter().enumerate() {
            transaction
                .execute(
                    "INSERT INTO continuity_checkpoint_evidence
                     (checkpoint_id, position, evidence_ref) VALUES (?1, ?2, ?3)",
                    params![checkpoint.id.as_str(), position as i64, reference],
                )
                .map_err(backend)?;
        }
        for note in notes {
            let (kind, body) = match &note.body {
                continuity::model::NoteBody::Inline(text) => ("INLINE", text.as_str()),
                continuity::model::NoteBody::Ref(reference) => ("REF", reference.as_str()),
            };
            transaction
                .execute(
                    "INSERT INTO continuity_notes
                     (note_id, epoch_ref, created_at, author, body_kind, body, sensitivity)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        note.id.as_str(),
                        note.epoch_ref.as_str(),
                        u64_to_i64("note created at", note.created_at).map_err(context_err)?,
                        note.author.as_str(),
                        kind,
                        body,
                        note.sensitivity.as_str(),
                    ],
                )
                .map_err(backend)?;
        }
        for (position, note_id) in checkpoint.working_state.iter().enumerate() {
            transaction
                .execute(
                    "INSERT INTO continuity_checkpoint_notes
                     (checkpoint_id, position, note_id) VALUES (?1, ?2, ?3)",
                    params![checkpoint.id.as_str(), position as i64, note_id.as_str()],
                )
                .map_err(backend)?;
        }
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    fn commit_checkpoint(
        &mut self,
        id: &CheckpointId,
        committed_at: u64,
    ) -> Result<CheckpointReceipt, ContinuityError> {
        let Some((checkpoint, state, stored_at, stored_revision)) =
            decode_checkpoint(self.connection(), id.as_str(), false)?
        else {
            return Err(ContinuityError::MissingCheckpoint(format!(
                "checkpoint {} is unknown",
                id.as_str()
            )));
        };
        if state == "COMMITTED" {
            let committed_at = stored_at
                .map(|v| i64_to_u64("committed at", v).map_err(context_err))
                .transpose()?
                .ok_or_else(|| ContinuityError::BackendFailure {
                    detail: "committed checkpoint lacks a timestamp".to_owned(),
                })?;
            let store_revision = stored_revision
                .map(|v| i64_to_u64("store revision", v).map_err(context_err))
                .transpose()?
                .ok_or_else(|| ContinuityError::BackendFailure {
                    detail: "committed checkpoint lacks a store revision".to_owned(),
                })?;
            return Ok(CheckpointReceipt {
                checkpoint_id: id.clone(),
                epoch_id: checkpoint.epoch_ref,
                committed_at,
                store_revision,
            });
        }
        checkpoint.validate()?;
        if let Some(parent) = &checkpoint.parent_checkpoint {
            let parent_committed: bool = self
                .connection()
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM continuity_checkpoints
                     WHERE checkpoint_id = ?1 AND state = 'COMMITTED')",
                    params![parent.as_str()],
                    |row| row.get(0),
                )
                .map_err(backend)?;
            if !parent_committed {
                return Err(ContinuityError::MissingCheckpoint(format!(
                    "parent checkpoint {} is not committed",
                    parent.as_str()
                )));
            }
        }
        let stored_hash: String = self
            .connection()
            .query_row(
                "SELECT staged_hash FROM continuity_checkpoints WHERE checkpoint_id = ?1",
                params![id.as_str()],
                |row| row.get(0),
            )
            .map_err(backend)?;
        if staged_hash(&checkpoint)? != stored_hash {
            return Err(ContinuityError::BackendFailure {
                detail: "staged checkpoint bytes changed before commit".to_owned(),
            });
        }
        let transaction = self
            .connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(backend)?;
        let revision: i64 = transaction
            .query_row(
                "UPDATE continuity_meta SET value = value + 1 WHERE key = 'checkpoint_seq'
                 RETURNING value",
                [],
                |row| row.get(0),
            )
            .map_err(backend)?;
        transaction
            .execute(
                "UPDATE continuity_checkpoints SET state = 'COMMITTED', committed_at = ?1,
                        store_revision = ?2 WHERE checkpoint_id = ?3",
                params![
                    u64_to_i64("committed at", committed_at).map_err(context_err)?,
                    revision,
                    id.as_str()
                ],
            )
            .map_err(backend)?;
        transaction
            .execute(
                "UPDATE continuity_epochs SET checkpointed_by = ?1 WHERE epoch_id = ?2",
                params![id.as_str(), checkpoint.epoch_ref.as_str()],
            )
            .map_err(backend)?;
        transaction
            .execute(
                "INSERT INTO continuity_activations
                 (checkpoint_id, successor, started_at, completed_at)
                 VALUES (?1, NULL, ?2, NULL)",
                params![
                    id.as_str(),
                    u64_to_i64("activation started at", committed_at).map_err(context_err)?,
                ],
            )
            .map_err(backend)?;
        let epoch_seq: i64 = transaction
            .query_row(
                "SELECT epoch_seq FROM continuity_epochs WHERE epoch_id = ?1",
                params![checkpoint.epoch_ref.as_str()],
                |row| row.get(0),
            )
            .map_err(backend)?;
        let checkpoint_title = if checkpoint.continuation.trim().is_empty() {
            format!("checkpoint {epoch_seq}")
        } else {
            truncate_title(&checkpoint.continuation)
        };
        transaction
            .execute(
                "INSERT INTO continuity_history_refs
                 (history_ref, epoch_id, kind, target_id, created_at, title, body)
                 VALUES (?1, ?2, 'CHECKPOINT', ?3, ?4, ?5, ?6)",
                params![
                    derive_history_ref(id, "CHECKPOINT", 0).as_str(),
                    checkpoint.epoch_ref.as_str(),
                    id.as_str(),
                    u64_to_i64("history created at", committed_at).map_err(context_err)?,
                    checkpoint_title,
                    checkpoint.continuation,
                ],
            )
            .map_err(backend)?;
        for (position, note_id) in checkpoint.working_state.iter().enumerate() {
            let (body, author): (String, String) = transaction
                .query_row(
                    "SELECT body, author FROM continuity_notes WHERE note_id = ?1",
                    params![note_id.as_str()],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(backend)?;
            transaction
                .execute(
                    "INSERT INTO continuity_history_refs
                     (history_ref, epoch_id, kind, target_id, created_at, title, body)
                     VALUES (?1, ?2, 'NOTE', ?3, ?4, ?5, ?6)",
                    params![
                        derive_history_ref(id, "NOTE", position).as_str(),
                        checkpoint.epoch_ref.as_str(),
                        note_id.as_str(),
                        u64_to_i64("history created at", committed_at).map_err(context_err)?,
                        format!("{} note: {}", author.to_lowercase(), truncate_title(&body)),
                        body,
                    ],
                )
                .map_err(backend)?;
        }
        transaction.commit().map_err(backend)?;
        Ok(CheckpointReceipt {
            checkpoint_id: id.clone(),
            epoch_id: checkpoint.epoch_ref,
            committed_at,
            store_revision: i64_to_u64("store revision", revision).map_err(context_err)?,
        })
    }

    fn load_checkpoint(&self, id: &CheckpointId) -> Result<Option<Checkpoint>, ContinuityError> {
        Ok(decode_checkpoint(self.connection(), id.as_str(), true)?.map(|(point, _, _, _)| point))
    }

    fn load_note(
        &self,
        id: &continuity::model::NoteId,
    ) -> Result<Option<WorkingNote>, ContinuityError> {
        use continuity::model::{NoteAuthor, NoteBody, NoteId};
        // CR-2 for notes: visible only through committed linkage,
        // so staged bodies never leak pre-commit.
        let row: Option<(String, String, i64, String, String, String, String)> = self
            .connection()
            .query_row(
                "SELECT n.note_id, n.epoch_ref, n.created_at, n.author,
                        n.body_kind, n.body, n.sensitivity
                 FROM continuity_notes n
                 WHERE n.note_id = ?1 AND EXISTS(
                   SELECT 1 FROM continuity_checkpoint_notes l
                   JOIN continuity_checkpoints c
                     ON c.checkpoint_id = l.checkpoint_id
                   WHERE l.note_id = n.note_id AND c.state = 'COMMITTED')",
                params![id.as_str()],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                    ))
                },
            )
            .optional()
            .map_err(backend)?;
        row.map(
            |(note_id, epoch_ref, created_at, author, body_kind, body, sensitivity)| {
                Ok(WorkingNote {
                    id: reconstruct(NoteId::new(note_id), "note id")?,
                    epoch_ref: reconstruct(
                        continuity::model::EpochId::new(epoch_ref),
                        "note epoch ref",
                    )?,
                    created_at: i64_to_u64("note created at", created_at).map_err(context_err)?,
                    author: reconstruct(NoteAuthor::parse(&author), "note author")?,
                    body: match body_kind.as_str() {
                        "INLINE" => NoteBody::Inline(body),
                        "REF" => NoteBody::Ref(body),
                        _ => {
                            return Err(ContinuityError::BackendFailure {
                                detail: "stored note body kind is malformed".to_owned(),
                            });
                        }
                    },
                    sensitivity: reconstruct(
                        context_engine::model::Sensitivity::parse(&sensitivity)
                            .map_err(context_err),
                        "note sensitivity",
                    )?,
                })
            },
        )
        .transpose()
    }

    fn load_epoch(&self, id: &EpochId) -> Result<Option<Epoch>, ContinuityError> {
        load_epoch_row(self.connection(), id.as_str())
    }

    fn active_epoch(
        &self,
        task: &task_domain::TaskId,
        execution: &agent_runtime::ExecutionId,
    ) -> Result<Option<Epoch>, ContinuityError> {
        let row: Option<(String, String, String, i64, String, i64)> = self
            .connection()
            .query_row(
                "SELECT epoch_id, task_ref, execution_ref, epoch_seq, state, created_at
                 FROM continuity_epochs
                 WHERE task_ref = ?1 AND execution_ref = ?2 AND state = 'ACTIVE'",
                params![task.as_str(), execution.as_str()],
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
            .optional()
            .map_err(backend)?;
        row.map(|(id, task, execution, seq, state, created_at)| {
            decode_epoch(id, task, execution, seq, state, created_at)
        })
        .transpose()
    }

    fn activate_successor(
        &mut self,
        checkpoint_id: &CheckpointId,
        successor: &Epoch,
        activated_at: u64,
    ) -> Result<EpochId, ContinuityError> {
        let Some((checkpoint, state, _, _)) =
            decode_checkpoint(self.connection(), checkpoint_id.as_str(), false)?
        else {
            return Err(ContinuityError::MissingCheckpoint(format!(
                "checkpoint {} is unknown",
                checkpoint_id.as_str()
            )));
        };
        if state != "COMMITTED" {
            return Err(ContinuityError::NotCommitted(format!(
                "checkpoint {} is not committed",
                checkpoint_id.as_str()
            )));
        }
        let record: Option<(Option<String>, Option<i64>)> = self
            .connection()
            .query_row(
                "SELECT successor, completed_at FROM continuity_activations
                 WHERE checkpoint_id = ?1",
                params![checkpoint_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        let Some((recorded_successor, completed_at)) = record else {
            return Err(ContinuityError::BackendFailure {
                detail: "committed checkpoint lacks an activation record".to_owned(),
            });
        };
        if completed_at.is_some() {
            match recorded_successor {
                Some(recorded) if recorded == successor.id.as_str() => {
                    return Ok(successor.id.clone());
                }
                _ => {
                    return Err(ContinuityError::ActivationConflict(
                        "checkpoint already activated another successor".to_owned(),
                    ));
                }
            }
        }
        if let Some(recorded) = &recorded_successor {
            if recorded != successor.id.as_str() {
                return Err(ContinuityError::ActivationConflict(
                    "pending activation names another successor".to_owned(),
                ));
            }
        }
        if successor.task_ref != checkpoint.task_ref
            || successor.execution_ref != checkpoint.execution_ref
        {
            return Err(ContinuityError::InvalidInput(
                "successor chain differs from checkpoint chain".to_owned(),
            ));
        }
        if successor.state != EpochState::Active {
            return Err(ContinuityError::InvalidInput(
                "successor must arrive ACTIVE".to_owned(),
            ));
        }
        let old =
            load_epoch_row(self.connection(), checkpoint.epoch_ref.as_str())?.ok_or_else(|| {
                ContinuityError::BackendFailure {
                    detail: "checkpoint references a missing epoch".to_owned(),
                }
            })?;
        if old.state != EpochState::Active {
            return Err(ContinuityError::ActivationConflict(
                "chain already advanced past this checkpoint".to_owned(),
            ));
        }
        if successor.seq != old.seq + 1 {
            return Err(ContinuityError::InvalidInput(
                "successor seq must follow the old epoch".to_owned(),
            ));
        }
        if let Some(existing) = load_epoch_row(self.connection(), successor.id.as_str())? {
            if existing.task_ref != successor.task_ref
                || existing.execution_ref != successor.execution_ref
                || existing.seq != successor.seq
                || existing.state != EpochState::Active
            {
                return Err(ContinuityError::ActivationConflict(
                    "successor id is taken by another epoch".to_owned(),
                ));
            }
        }
        // Crash window (ADR-0043 §9c): the record lands first, the
        // epoch flip plus completion second. A crash between them
        // leaves a pending record that recovery completes.
        self.connection()
            .execute(
                "UPDATE continuity_activations SET successor = ?1 WHERE checkpoint_id = ?2",
                params![successor.id.as_str(), checkpoint_id.as_str()],
            )
            .map_err(backend)?;
        let transaction = self
            .connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(backend)?;
        // Flip first: the partial unique index allows exactly one
        // ACTIVE epoch per chain, so the successor insert must land
        // after the old epoch leaves ACTIVE. Plain INSERT (no
        // OR IGNORE): a conflict here is a real bug, fail loudly.
        let flipped = transaction
            .execute(
                "UPDATE continuity_epochs SET state = 'INDEXED'
                 WHERE epoch_id = ?1 AND state = 'ACTIVE'",
                params![old.id.as_str()],
            )
            .map_err(backend)?;
        if flipped != 1 {
            return Err(ContinuityError::ActivationConflict(
                "old epoch left ACTIVE under us".to_owned(),
            ));
        }
        transaction
            .execute(
                "INSERT INTO continuity_epochs
                 (epoch_id, task_ref, execution_ref, epoch_seq, state, created_at,
                  checkpointed_by)
                 VALUES (?1, ?2, ?3, ?4, 'ACTIVE', ?5, NULL)",
                params![
                    successor.id.as_str(),
                    successor.task_ref.as_str(),
                    successor.execution_ref.as_str(),
                    u64_to_i64("successor seq", successor.seq).map_err(context_err)?,
                    u64_to_i64("successor created at", successor.created_at)
                        .map_err(context_err)?,
                ],
            )
            .map_err(backend)?;
        transaction
            .execute(
                "UPDATE continuity_activations SET completed_at = ?1 WHERE checkpoint_id = ?2",
                params![
                    u64_to_i64("activation completed at", activated_at).map_err(context_err)?,
                    checkpoint_id.as_str()
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(successor.id.clone())
    }

    fn pending_activations(&self) -> Result<Vec<ActivationRecord>, ContinuityError> {
        let mut statement = self
            .connection()
            .prepare(
                "SELECT checkpoint_id, successor, started_at, completed_at
                 FROM continuity_activations WHERE completed_at IS NULL
                 ORDER BY checkpoint_id",
            )
            .map_err(backend)?;
        let rows: Vec<(String, Option<String>, i64, Option<i64>)> = statement
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .map_err(backend)?
            .collect::<Result<_, _>>()
            .map_err(backend)?;
        rows.into_iter()
            .map(|(checkpoint_id, successor, started_at, completed_at)| {
                Ok(ActivationRecord {
                    checkpoint_id: reconstruct(
                        continuity::model::CheckpointId::new(checkpoint_id),
                        "activation checkpoint id",
                    )?,
                    successor: successor
                        .map(|raw| {
                            reconstruct(
                                continuity::model::EpochId::new(raw),
                                "activation successor",
                            )
                        })
                        .transpose()?,
                    started_at: i64_to_u64("activation started at", started_at)
                        .map_err(context_err)?,
                    completed_at: completed_at
                        .map(|v| i64_to_u64("activation completed at", v).map_err(context_err))
                        .transpose()?,
                })
            })
            .collect()
    }

    fn search_history(
        &self,
        query: &str,
        filters: &SearchFilters,
        now_millis: u64,
    ) -> Result<Vec<HistoryHit>, ContinuityError> {
        use continuity::history::{rank_hits, HistoryCandidate, HistoryKind, HISTORY_SEARCH_LIMIT};
        let narrowed = filters.task.is_some()
            || filters.epoch.is_some()
            || filters.event_type.is_some()
            || !filters.metadata.is_empty();
        if query.trim().is_empty() && !narrowed {
            return Err(ContinuityError::InvalidInput(
                "blank history queries need a narrowing filter".to_owned(),
            ));
        }
        let mut sql = String::from(
            "SELECT r.history_ref, r.epoch_id, r.kind, r.created_at, r.title, r.body
             FROM continuity_history_refs r JOIN continuity_epochs e
               ON r.epoch_id = e.epoch_id WHERE 1 = 1",
        );
        if filters.task.is_some() {
            sql.push_str(" AND e.task_ref = ?");
        }
        if filters.epoch.is_some() {
            sql.push_str(" AND r.epoch_id = ?");
        }
        if filters.event_type.is_some() {
            sql.push_str(" AND r.kind = ?");
        }
        for (key, _) in filters.metadata.iter() {
            match key.as_str() {
                "kind" => sql.push_str(" AND r.kind = ?"),
                "author" => sql.push_str(
                    " AND EXISTS(SELECT 1 FROM continuity_notes n
                     WHERE n.note_id = r.target_id AND n.author = ?)",
                ),
                "sensitivity" => sql.push_str(
                    " AND ((r.kind = 'NOTE' AND EXISTS(SELECT 1 FROM continuity_notes n
                       WHERE n.note_id = r.target_id AND n.sensitivity = ?))
                      OR (r.kind = 'CHECKPOINT' AND EXISTS(
                       SELECT 1 FROM continuity_checkpoints c
                       WHERE c.checkpoint_id = r.target_id
                         AND c.sensitivity_ceiling = ?)))",
                ),
                _ => {
                    return Err(ContinuityError::InvalidInput(format!(
                        "unknown history metadata key: {key}"
                    )));
                }
            }
        }
        let like = if query.trim().is_empty() {
            None
        } else {
            sql.push_str(" AND (r.title LIKE ? ESCAPE '\\' OR r.body LIKE ? ESCAPE '\\')");
            Some(format!(
                "%{}%",
                query
                    .replace('\\', "\\\\")
                    .replace('%', "\\%")
                    .replace('_', "\\_")
            ))
        };
        sql.push_str(" ORDER BY r.created_at DESC LIMIT 1000");
        let mut statement = self.connection().prepare(&sql).map_err(backend)?;
        let task = filters.task.as_ref().map(|id| id.as_str());
        let epoch = filters.epoch.as_ref().map(|id| id.as_str());
        let event = filters.event_type.as_deref();
        let metas: Vec<&str> = filters.metadata.iter().map(|(_, v)| v.as_str()).collect();
        let likes: Vec<&str> = like
            .as_deref()
            .map(|pattern| vec![pattern, pattern])
            .unwrap_or_default();
        let rows = statement
            .query_map(
                rusqlite::params_from_iter(
                    task.into_iter()
                        .chain(epoch)
                        .chain(event)
                        .chain(metas)
                        .chain(likes),
                ),
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                    ))
                },
            )
            .map_err(backend)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(backend)?;
        // Anonymous placeholders bind in push order: task, epoch,
        // event, metadata values, then the LIKE pattern twice (title
        // and body). The iterator above follows that order exactly.
        let mut candidates = Vec::with_capacity(rows.len());
        for (reference, epoch_id, kind, created_at, title, body) in rows {
            candidates.push(HistoryCandidate {
                history_ref: reconstruct(
                    continuity::model::HistoryRef::new(reference),
                    "history ref",
                )?,
                epoch_id: reconstruct(continuity::model::EpochId::new(epoch_id), "history epoch")?,
                kind: reconstruct(HistoryKind::parse(&kind), "history kind")?,
                created_at: i64_to_u64("history created at", created_at).map_err(context_err)?,
                title,
                body,
            });
        }
        let mut ranked = rank_hits(candidates, query, now_millis);
        ranked.truncate(HISTORY_SEARCH_LIMIT);
        Ok(ranked)
    }

    fn read_history(&self, exact: &HistoryRef) -> Result<Option<HistoryContent>, ContinuityError> {
        use continuity::history::HistoryKind;
        let row: Option<(String, String, String, String, String)> = self
            .connection()
            .query_row(
                "SELECT history_ref, epoch_id, kind, title, body
                 FROM continuity_history_refs WHERE history_ref = ?1",
                params![exact.as_str()],
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
            .optional()
            .map_err(backend)?;
        row.map(|(reference, epoch_id, kind, title, excerpt)| {
            Ok(HistoryContent {
                history_ref: reconstruct(
                    continuity::model::HistoryRef::new(reference),
                    "history ref",
                )?,
                epoch_id: reconstruct(continuity::model::EpochId::new(epoch_id), "history epoch")?,
                kind: reconstruct(HistoryKind::parse(&kind), "history kind")?,
                title,
                excerpt,
            })
        })
        .transpose()
    }

    fn link_session(&mut self, link: &SessionLink) -> Result<(), ContinuityError> {
        link.validate()?;
        if load_epoch_row(self.connection(), link.epoch_id.as_str())?.is_none() {
            return Err(ContinuityError::InvalidInput(
                "session link references an unknown epoch".to_owned(),
            ));
        }
        let exists: bool = self
            .connection()
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM continuity_session_links
                 WHERE session_ref = ?1 AND epoch_id = ?2)",
                params![link.session_ref, link.epoch_id.as_str()],
                |row| row.get(0),
            )
            .map_err(backend)?;
        if exists {
            return Err(ContinuityError::InvalidInput(
                "session link is already recorded".to_owned(),
            ));
        }
        self.connection()
            .execute(
                "INSERT INTO continuity_session_links
                 (session_ref, epoch_id, attempt_ref, provider_id, linked_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    link.session_ref,
                    link.epoch_id.as_str(),
                    link.attempt_ref.as_str(),
                    link.provider_id.as_str(),
                    u64_to_i64("session linked at", link.linked_at).map_err(context_err)?,
                ],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn migrate_chain(
        &mut self,
        from: &CheckpointId,
        genesis: &Epoch,
    ) -> Result<MigrationLink, ContinuityError> {
        let Some((checkpoint, state, _, _)) =
            decode_checkpoint(self.connection(), from.as_str(), false)?
        else {
            return Err(ContinuityError::MissingCheckpoint(format!(
                "checkpoint {} is unknown",
                from.as_str()
            )));
        };
        if state != "COMMITTED" {
            return Err(ContinuityError::NotCommitted(format!(
                "checkpoint {} is not committed",
                from.as_str()
            )));
        }
        if genesis.seq != 0 || genesis.state != EpochState::Active {
            return Err(ContinuityError::InvalidInput(
                "migration target must be a seq-0 ACTIVE genesis".to_owned(),
            ));
        }
        if genesis.task_ref != checkpoint.task_ref {
            return Err(ContinuityError::InvalidInput(
                "migration stays within one task".to_owned(),
            ));
        }
        let chained: bool = self
            .connection()
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM continuity_epochs
                 WHERE task_ref = ?1 AND execution_ref = ?2)",
                params![genesis.task_ref.as_str(), genesis.execution_ref.as_str()],
                |row| row.get(0),
            )
            .map_err(backend)?;
        if chained {
            return Err(ContinuityError::InvalidInput(
                "migration target chain is already open".to_owned(),
            ));
        }
        let transaction = self
            .connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(backend)?;
        transaction
            .execute(
                "INSERT INTO continuity_epochs
                 (epoch_id, task_ref, execution_ref, epoch_seq, state, created_at,
                  checkpointed_by)
                 VALUES (?1, ?2, ?3, 0, 'ACTIVE', ?4, NULL)",
                params![
                    genesis.id.as_str(),
                    genesis.task_ref.as_str(),
                    genesis.execution_ref.as_str(),
                    u64_to_i64("genesis created at", genesis.created_at).map_err(context_err)?,
                ],
            )
            .map_err(backend)?;
        transaction
            .execute(
                "INSERT INTO continuity_migration_links (from_checkpoint, to_epoch, migrated_at)
                 VALUES (?1, ?2, ?3)",
                params![
                    from.as_str(),
                    genesis.id.as_str(),
                    u64_to_i64("migrated at", genesis.created_at).map_err(context_err)?,
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(MigrationLink {
            from_checkpoint: from.clone(),
            to_epoch: genesis.id.clone(),
        })
    }

    fn purge_expired(
        &mut self,
        now_millis: u64,
        policy: &RetentionPolicy,
    ) -> Result<PurgeReport, ContinuityError> {
        use continuity::rollover::retention_cutoff;
        let staged_cutoff = u64_to_i64(
            "staged cutoff",
            retention_cutoff(now_millis, policy.staged_older_than_ms),
        )
        .map_err(context_err)?;
        let transaction = self
            .connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(backend)?;
        let staged_ids: Vec<String> = transaction
            .prepare(
                "SELECT checkpoint_id FROM continuity_checkpoints
                 WHERE state = 'STAGED' AND created_at < ?1",
            )
            .map_err(backend)?
            .query_map(params![staged_cutoff], |row| row.get(0))
            .map_err(backend)?
            .collect::<Result<_, _>>()
            .map_err(backend)?;
        for id in &staged_ids {
            for table in [
                "continuity_checkpoint_sources",
                "continuity_checkpoint_evidence",
                "continuity_checkpoint_notes",
            ] {
                transaction
                    .execute(
                        &format!("DELETE FROM {table} WHERE checkpoint_id = ?1"),
                        params![id],
                    )
                    .map_err(backend)?;
            }
            transaction
                .execute(
                    "DELETE FROM continuity_checkpoints WHERE checkpoint_id = ?1",
                    params![id],
                )
                .map_err(backend)?;
        }
        let mut note_total = transaction
            .execute(
                "DELETE FROM continuity_notes WHERE NOT EXISTS(
                   SELECT 1 FROM continuity_checkpoint_notes l
                   WHERE l.note_id = continuity_notes.note_id)",
                [],
            )
            .map_err(backend)?;
        let mut epochs = 0;
        if let Some(older_than) = policy.indexed_older_than_ms {
            let cutoff = u64_to_i64("indexed cutoff", retention_cutoff(now_millis, older_than))
                .map_err(context_err)?;
            let victims: Vec<String> = transaction
                .prepare(
                    "SELECT epoch_id FROM continuity_epochs
                     WHERE state = 'INDEXED' AND created_at < ?1
                       AND checkpointed_by IS NOT NULL",
                )
                .map_err(backend)?
                .query_map(params![cutoff], |row| row.get(0))
                .map_err(backend)?
                .collect::<Result<_, _>>()
                .map_err(backend)?;
            // Lineage guard: victims referenced as parents by
            // surviving checkpoints stay. Fixpoint over skip marks:
            // chains purge bottom-up over successive retentions,
            // never mid-chain (a skipped victim shields its own
            // parents in the next round).
            let victim_set: std::collections::BTreeSet<&str> =
                victims.iter().map(String::as_str).collect();
            let mut skipped: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
            loop {
                let mut changed = false;
                for epoch_id in &victims {
                    if skipped.contains(epoch_id.as_str()) {
                        continue;
                    }
                    let children: Vec<(String, String)> = transaction
                        .prepare(
                            "SELECT c.checkpoint_id, c.epoch_ref
                             FROM continuity_checkpoints c
                             WHERE c.parent_checkpoint IN (
                               SELECT checkpoint_id FROM continuity_checkpoints
                               WHERE epoch_ref = ?1)",
                        )
                        .map_err(backend)?
                        .query_map(params![epoch_id], |row| Ok((row.get(0)?, row.get(1)?)))
                        .map_err(backend)?
                        .collect::<Result<_, _>>()
                        .map_err(backend)?;
                    let blocked = children.iter().any(|(_, child_epoch)| {
                        !victim_set.contains(child_epoch.as_str())
                            || skipped.contains(child_epoch.as_str())
                    });
                    if blocked {
                        skipped.insert(epoch_id.as_str());
                        changed = true;
                    }
                }
                if !changed {
                    break;
                }
            }
            for epoch_id in &victims {
                if skipped.contains(epoch_id.as_str()) {
                    continue;
                }
                transaction
                    .execute(
                        "DELETE FROM continuity_receipt_refs WHERE epoch_id = ?1",
                        params![epoch_id],
                    )
                    .map_err(backend)?;
                transaction
                    .execute(
                        "DELETE FROM continuity_history_refs WHERE epoch_id = ?1",
                        params![epoch_id],
                    )
                    .map_err(backend)?;
                transaction
                    .execute(
                        "DELETE FROM continuity_session_links WHERE epoch_id = ?1",
                        params![epoch_id],
                    )
                    .map_err(backend)?;
                let subs: Vec<String> = transaction
                    .prepare(
                        "SELECT checkpoint_id FROM continuity_checkpoints WHERE epoch_ref = ?1",
                    )
                    .map_err(backend)?
                    .query_map(params![epoch_id], |row| row.get(0))
                    .map_err(backend)?
                    .collect::<Result<_, _>>()
                    .map_err(backend)?;
                for sub in &subs {
                    for table in [
                        "continuity_checkpoint_sources",
                        "continuity_checkpoint_evidence",
                        "continuity_checkpoint_notes",
                    ] {
                        transaction
                            .execute(
                                &format!("DELETE FROM {table} WHERE checkpoint_id = ?1"),
                                params![sub],
                            )
                            .map_err(backend)?;
                    }
                    transaction
                        .execute(
                            "DELETE FROM continuity_activations WHERE checkpoint_id = ?1",
                            params![sub],
                        )
                        .map_err(backend)?;
                    transaction
                        .execute(
                            "DELETE FROM continuity_migration_links WHERE from_checkpoint = ?1",
                            params![sub],
                        )
                        .map_err(backend)?;
                    transaction
                        .execute(
                            "DELETE FROM continuity_checkpoints WHERE checkpoint_id = ?1",
                            params![sub],
                        )
                        .map_err(backend)?;
                }
                let epoch_notes = transaction
                    .execute(
                        "DELETE FROM continuity_notes WHERE epoch_ref = ?1",
                        params![epoch_id],
                    )
                    .map_err(backend)?;
                note_total += epoch_notes;
                transaction
                    .execute(
                        "DELETE FROM continuity_migration_links WHERE to_epoch = ?1",
                        params![epoch_id],
                    )
                    .map_err(backend)?;
                transaction
                    .execute(
                        "DELETE FROM continuity_epochs WHERE epoch_id = ?1",
                        params![epoch_id],
                    )
                    .map_err(backend)?;
                epochs += 1;
            }
        }
        let mut history_refs = 0;
        if let Some(older_than) = policy.history_older_than_ms {
            let cutoff = u64_to_i64("history cutoff", retention_cutoff(now_millis, older_than))
                .map_err(context_err)?;
            // ACTIVE-epoch refs are live chain data: only INDEXED
            // epochs shed old refs on their own.
            let stale: Vec<String> = transaction
                .prepare(
                    "SELECT r.history_ref FROM continuity_history_refs r
                     JOIN continuity_epochs e ON r.epoch_id = e.epoch_id
                     WHERE r.created_at < ?1 AND e.state = 'INDEXED'",
                )
                .map_err(backend)?
                .query_map(params![cutoff], |row| row.get(0))
                .map_err(backend)?
                .collect::<Result<_, _>>()
                .map_err(backend)?;
            for reference in &stale {
                transaction
                    .execute(
                        "DELETE FROM continuity_receipt_refs WHERE history_ref = ?1",
                        params![reference],
                    )
                    .map_err(backend)?;
                transaction
                    .execute(
                        "DELETE FROM continuity_history_refs WHERE history_ref = ?1",
                        params![reference],
                    )
                    .map_err(backend)?;
                history_refs += 1;
            }
        }
        transaction.commit().map_err(backend)?;
        Ok(PurgeReport {
            staged: staged_ids.len(),
            epochs,
            history_refs,
            notes: note_total,
        })
    }

    fn register_receipt(
        &mut self,
        history_ref: &HistoryRef,
        epoch: &EpochId,
        item: &context_engine::model::ContextItemId,
        now_millis: u64,
    ) -> Result<(), ContinuityError> {
        if load_epoch_row(self.connection(), epoch.as_str())?.is_none() {
            return Err(ContinuityError::InvalidInput(
                "receipt references an unknown epoch".to_owned(),
            ));
        }
        let exists: bool = self
            .connection()
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM continuity_history_refs WHERE history_ref = ?1)",
                params![history_ref.as_str()],
                |row| row.get(0),
            )
            .map_err(backend)?;
        if exists {
            return Err(ContinuityError::InvalidInput(
                "history ref is already registered".to_owned(),
            ));
        }
        let transaction = self
            .connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(backend)?;
        transaction
            .execute(
                "INSERT INTO continuity_history_refs
                 (history_ref, epoch_id, kind, target_id, created_at, title, body)
                 VALUES (?1, ?2, 'RECEIPT', ?3, ?4, 'compaction receipt', ?3)",
                params![
                    history_ref.as_str(),
                    epoch.as_str(),
                    item.as_str(),
                    u64_to_i64("receipt created at", now_millis).map_err(context_err)?,
                ],
            )
            .map_err(backend)?;
        transaction
            .execute(
                "INSERT INTO continuity_receipt_refs (history_ref, epoch_id, receipt_item_id)
                 VALUES (?1, ?2, ?3)",
                params![history_ref.as_str(), epoch.as_str(), item.as_str()],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    fn record_budget_event(&mut self, event: &BudgetEvent) -> Result<(), ContinuityError> {
        self.connection()
            .execute(
                "INSERT INTO continuity_budget_events (plan_ref, hard, recorded_at)
                 VALUES (?1, ?2, ?3)",
                params![
                    event.plan_ref.as_str(),
                    i64::from(event.hard),
                    u64_to_i64("budget recorded at", event.recorded_at).map_err(context_err)?,
                ],
            )
            .map_err(backend)?;
        Ok(())
    }
}
