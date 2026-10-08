//! Durable continuity seam (TASK-0171, ADR-0043).
//!
//! CR mapping per method: CR-1 (rollover needs a committed checkpoint),
//! CR-2 (staging invisible), CR-3 (idempotent activation), CR-4
//! (exact-ref reads), CR-9 (chain isolation), CR-10 (version refusal).
//! CR-5/CR-8 hold by type (refs, never embedded raw/SECRET values);
//! CR-6/CR-7 hold by construction (sessions advisory, migration
//! carries refs only).

use agent_runtime::ExecutionId;
use context_engine::model::ContextItemId;
use task_domain::TaskId;

use crate::history::{HistoryContent, HistoryHit, SearchFilters};
use crate::model::{
    ActivationRecord, BudgetEvent, Checkpoint, CheckpointId, CheckpointReceipt, ContinuityError,
    Epoch, EpochId, HistoryRef, MigrationLink, NoteId, PurgeReport, RetentionPolicy, SessionLink,
    WorkingNote,
};

/// Durable store for epochs, checkpoints, notes and history.
pub trait ContinuityStorePort {
    /// Opens a chain with its genesis ACTIVE epoch (seq 0). Refuses
    /// when the chain already has epochs; the genesis is the only
    /// epoch that exists without a checkpoint (CR-1 covers rollover,
    /// not chain birth).
    fn open_chain(&mut self, genesis: &Epoch) -> Result<(), ContinuityError>;

    /// Writes a checkpoint plus its notes to staging (CR-2:
    /// invisible to every committed read until commit). Ceiling
    /// dominance over note labels is enforced by trusted callers
    /// (the service checks it), not re-checked here.
    fn stage_checkpoint(
        &mut self,
        checkpoint: &Checkpoint,
        notes: &[WorkingNote],
    ) -> Result<(), ContinuityError>;

    /// Verifies (read-back, hashes, parent chain) and atomically
    /// commits (checkpoint row, epoch index entry, history refs),
    /// returning the receipt (ADR-0043 §3). Refuses unknown majors
    /// (CR-10) and unverifiable payloads; a refused commit persists
    /// nothing newly committed.
    fn commit_checkpoint(
        &mut self,
        id: &CheckpointId,
        committed_at: u64,
    ) -> Result<CheckpointReceipt, ContinuityError>;

    /// Loads a committed checkpoint; staged or unknown ids are None.
    fn load_checkpoint(&self, id: &CheckpointId) -> Result<Option<Checkpoint>, ContinuityError>;

    /// Loads one note by id; None when unknown. Notes surface only
    /// through committed checkpoints and history; this read exists
    /// for working-set rebuild, never for bulk export.
    fn load_note(&self, id: &NoteId) -> Result<Option<WorkingNote>, ContinuityError>;

    /// Loads an epoch by id; None when unknown (never invented).
    fn load_epoch(&self, id: &EpochId) -> Result<Option<Epoch>, ContinuityError>;

    /// The ACTIVE epoch of a chain; None when the chain has none.
    /// Chains never cross (CR-9): exactly one ACTIVE per chain.
    fn active_epoch(
        &self,
        task: &TaskId,
        execution: &ExecutionId,
    ) -> Result<Option<Epoch>, ContinuityError>;

    /// Completes activation of the successor epoch for a committed
    /// checkpoint (CR-1: uncommitted sources refuse). Idempotent
    /// (CR-3): repeating with the same successor id is a no-op
    /// returning that id; a different successor id conflicts.
    fn activate_successor(
        &mut self,
        checkpoint_id: &CheckpointId,
        successor: &Epoch,
        activated_at: u64,
    ) -> Result<EpochId, ContinuityError>;

    /// Activation records with no completion (restart recovery input).
    fn pending_activations(&self) -> Result<Vec<ActivationRecord>, ContinuityError>;

    /// Two-stage history search over committed refs (CR-4: no bulk
    /// dump): ranked hits with disclosed signals, never content.
    /// Blank queries need a narrowing filter; results cap at
    /// `HISTORY_SEARCH_LIMIT`. Metadata filters match the documented
    /// keys `kind`/`author`/`sensitivity` only; receipts carry no
    /// sensitivity label and never match `sensitivity` filters.
    fn search_history(
        &self,
        query: &str,
        filters: &SearchFilters,
        now_millis: u64,
    ) -> Result<Vec<HistoryHit>, ContinuityError>;

    /// Bounded content for one exact ref; unknown refs are None.
    fn read_history(&self, exact: &HistoryRef) -> Result<Option<HistoryContent>, ContinuityError>;

    /// Appends a session link (evidence only, CR-6); links are never
    /// updated or deleted through this seam.
    fn link_session(&mut self, link: &SessionLink) -> Result<(), ContinuityError>;

    /// Migrates to a new chain: links the source chain's last
    /// committed checkpoint to the new chain's genesis epoch.
    /// Refuses uncommitted sources; carries refs only, never
    /// permission or side effects (CR-7).
    fn migrate_chain(
        &mut self,
        from: &CheckpointId,
        genesis: &Epoch,
    ) -> Result<MigrationLink, ContinuityError>;

    /// Purges expired rows per policy-owned durations: staged
    /// leftovers, old indexed epochs with committed checkpoints, old
    /// history refs. Refuses ACTIVE epochs and uncheckpointed chains.
    /// Retention tiers: old refs in surviving INDEXED epochs shed
    /// first (cold storage keeps direct-id loads); epochs referenced
    /// as parents by surviving checkpoints stay until their children
    /// purge (lineage guard).
    fn purge_expired(
        &mut self,
        now_millis: u64,
        policy: &RetentionPolicy,
    ) -> Result<PurgeReport, ContinuityError>;

    /// Registers an ADR-0042 CompactionReceipt as searchable history
    /// (C5). Receipts are records of selection decisions, never
    /// checkpoints: the id types keep them out of CR-1/CR-3 paths.
    fn register_receipt(
        &mut self,
        history_ref: &HistoryRef,
        epoch: &EpochId,
        item: &ContextItemId,
        now_millis: u64,
    ) -> Result<(), ContinuityError>;

    /// Records a budget-gate verdict (plan ref + soft/hard, no payload
    /// bytes).
    fn record_budget_event(&mut self, event: &BudgetEvent) -> Result<(), ContinuityError>;
}
