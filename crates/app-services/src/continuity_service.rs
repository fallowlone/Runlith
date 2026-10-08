//! Trusted continuity service (TASK-0171, ADR-0043).
//!
//! Orchestrates the [`ContinuityStorePort`] seam: chain opening,
//! staged commit, idempotent activation and recovery, budget gating,
//! migration, retention, and working-set rebuild. Clock and id
//! sequences are caller-supplied (fixed at construction), so tests
//! assert exact values.

use std::sync::atomic::{AtomicU64, Ordering};

use agent_runtime::ExecutionId;
use context_engine::model::{Budget, ContextPlanId, Sensitivity, SourceRef};
use continuity::history::{HistoryContent, HistoryHit, SearchFilters};
use continuity::mint::{mint_checkpoint_id, mint_epoch_id, mint_note_id};
use continuity::model::{
    BudgetEvent, Checkpoint, CheckpointId, CheckpointReceipt, ContinuityError, Epoch, EpochId,
    EpochState, FormatVersion, MigrationLink, NoteAuthor, NoteBody, NoteId, PurgeReport,
    RetentionPolicy, WorkingNote,
};
use continuity::port::ContinuityStorePort;
use continuity::rollover::{plan_activation, ActivationAction, BudgetMonitor, BudgetVerdict};
use task_domain::TaskId;

/// One authored note inside a commit request (ids minted by the service).
pub struct NewNote {
    pub author: NoteAuthor,
    pub body: NoteBody,
    pub sensitivity: Sensitivity,
}

/// Trusted checkpoint commit input (ids minted by the service).
pub struct CommitRequest {
    pub epoch: EpochId,
    pub task: TaskId,
    pub execution: ExecutionId,
    pub format_version: FormatVersion,
    pub parent: Option<CheckpointId>,
    pub notes: Vec<NewNote>,
    pub source_refs: Vec<SourceRef>,
    pub evidence_refs: Vec<String>,
    pub continuation: String,
    pub plan: ContextPlanId,
    pub ceiling: Sensitivity,
}

/// In-flight tool correlation pair: ephemeral memory only, never
/// persisted and never resumed (ADR-0043 §5).
pub struct ToolPair {
    pub call_id: String,
    pub result_ref: String,
}

/// Rebuilt working set: the committed plan plus resumed notes.
/// SECRET-labelled notes never resume (ADR-0042 sensitivity gate
/// re-applied); tool pairs always restart empty.
pub struct WorkingSet {
    pub plan: ContextPlanId,
    pub notes: Vec<NoteId>,
    pub tool_pairs: Vec<ToolPair>,
}

/// Trusted continuity service over a [`ContinuityStorePort`] store.
pub struct ContinuityService<S> {
    store: S,
    now_millis: u64,
    seq: AtomicU64,
}

impl<S: ContinuityStorePort> ContinuityService<S> {
    /// Composes the service. `now_millis` stamps minted ids and
    /// receipts; callers pass wall-clock time (advancing across
    /// restarts, since sequences restart at zero), tests fixed values.
    pub fn new(store: S, now_millis: u64) -> Self {
        Self {
            store,
            now_millis,
            seq: AtomicU64::new(0),
        }
    }

    fn next_seq(&self) -> u128 {
        u128::from(self.seq.fetch_add(1, Ordering::SeqCst))
    }

    /// Opens a chain with its genesis ACTIVE epoch.
    pub fn open_chain(
        &mut self,
        task: TaskId,
        execution: ExecutionId,
    ) -> Result<EpochId, ContinuityError> {
        let genesis = Epoch {
            id: mint_epoch_id(self.now_millis, self.next_seq()),
            task_ref: task,
            execution_ref: execution,
            seq: 0,
            state: EpochState::Active,
            created_at: self.now_millis,
        };
        self.store.open_chain(&genesis)?;
        Ok(genesis.id)
    }

    /// Stages and commits one checkpoint atomically: mints ids,
    /// enforces the ceiling over note labels (CR-8 mechanics), then
    /// delegates to the store's verify-and-commit.
    pub fn commit_checkpoint(
        &mut self,
        request: CommitRequest,
    ) -> Result<CheckpointReceipt, ContinuityError> {
        let mut notes = Vec::with_capacity(request.notes.len());
        let mut note_ids = Vec::with_capacity(request.notes.len());
        for drafted in &request.notes {
            if drafted.sensitivity > request.ceiling {
                return Err(ContinuityError::InvalidInput(
                    "ceiling sits below a referenced note level".to_owned(),
                ));
            }
            let note = WorkingNote {
                id: mint_note_id(self.now_millis, self.next_seq()),
                epoch_ref: request.epoch.clone(),
                created_at: self.now_millis,
                author: drafted.author,
                body: drafted.body.clone(),
                sensitivity: drafted.sensitivity,
            };
            note.validate()?;
            note_ids.push(note.id.clone());
            notes.push(note);
        }
        let checkpoint = Checkpoint {
            id: mint_checkpoint_id(self.now_millis, self.next_seq()),
            epoch_ref: request.epoch,
            task_ref: request.task,
            execution_ref: request.execution,
            created_at: self.now_millis,
            format_version: request.format_version,
            parent_checkpoint: request.parent,
            working_state: note_ids,
            source_refs: request.source_refs,
            evidence_refs: request.evidence_refs,
            continuation: request.continuation,
            context_plan_ref: request.plan,
            sensitivity_ceiling: request.ceiling,
        };
        checkpoint.validate()?;
        self.store.stage_checkpoint(&checkpoint, &notes)?;
        self.store
            .commit_checkpoint(&checkpoint.id, self.now_millis)
    }

    /// Loads a committed checkpoint (staged ids read as unknown: the
    /// service cannot see staging).
    pub fn load_checkpoint(
        &self,
        id: &CheckpointId,
    ) -> Result<Option<Checkpoint>, ContinuityError> {
        self.store.load_checkpoint(id)
    }

    /// Loads one note (working-set rebuild input).
    pub fn load_note(&self, id: &NoteId) -> Result<Option<WorkingNote>, ContinuityError> {
        self.store.load_note(id)
    }

    /// The ACTIVE epoch of a chain, if any.
    pub fn active_epoch(
        &self,
        task: &TaskId,
        execution: &ExecutionId,
    ) -> Result<Option<Epoch>, ContinuityError> {
        self.store.active_epoch(task, execution)
    }

    /// Activates the successor epoch for a committed checkpoint,
    /// minting the successor id. Unknown-or-staged checkpoints read
    /// as missing (the service cannot see staging). Single-shot: a
    /// repeat call mints a fresh id and conflicts rather than
    /// returning the same successor; use [`Self::recover_pending`]
    /// for idempotent completion.
    pub fn activate_successor(
        &mut self,
        checkpoint_id: &CheckpointId,
    ) -> Result<EpochId, ContinuityError> {
        let checkpoint = self.store.load_checkpoint(checkpoint_id)?.ok_or_else(|| {
            ContinuityError::MissingCheckpoint(format!(
                "checkpoint {} is unknown or not committed",
                checkpoint_id.as_str()
            ))
        })?;
        let old = self
            .store
            .load_epoch(&checkpoint.epoch_ref)?
            .ok_or_else(|| ContinuityError::BackendFailure {
                detail: "checkpoint references a missing epoch".to_owned(),
            })?;
        let successor = Epoch {
            id: mint_epoch_id(self.now_millis, self.next_seq()),
            task_ref: checkpoint.task_ref.clone(),
            execution_ref: checkpoint.execution_ref.clone(),
            seq: old.seq + 1,
            state: EpochState::Active,
            created_at: self.now_millis,
        };
        self.store
            .activate_successor(checkpoint_id, &successor, self.now_millis)
    }

    /// Completes every pending activation (restart recovery,
    /// ADR-0043 §9): recorded successors are reused, unrecorded ones
    /// minted, each completion idempotent.
    pub fn recover_pending(&mut self) -> Result<Vec<EpochId>, ContinuityError> {
        let mut completed = Vec::new();
        for record in self.store.pending_activations()? {
            if plan_activation(&record) == ActivationAction::AlreadyComplete {
                continue;
            }
            let checkpoint = self
                .store
                .load_checkpoint(&record.checkpoint_id)?
                .ok_or_else(|| ContinuityError::BackendFailure {
                    detail: "pending activation references a missing checkpoint".to_owned(),
                })?;
            let old = self
                .store
                .load_epoch(&checkpoint.epoch_ref)?
                .ok_or_else(|| ContinuityError::BackendFailure {
                    detail: "checkpoint references a missing epoch".to_owned(),
                })?;
            let successor = Epoch {
                id: record
                    .successor
                    .clone()
                    .unwrap_or_else(|| mint_epoch_id(self.now_millis, self.next_seq())),
                task_ref: checkpoint.task_ref.clone(),
                execution_ref: checkpoint.execution_ref.clone(),
                seq: old.seq + 1,
                state: EpochState::Active,
                created_at: self.now_millis,
            };
            let id = self.store.activate_successor(
                &record.checkpoint_id,
                &successor,
                self.now_millis,
            )?;
            completed.push(id);
        }
        Ok(completed)
    }

    /// Gates one execution cycle on the plan budget (ADR-0043 §4):
    /// soft excess records an event and proceeds (the caller starts
    /// rollover checkpointing), hard excess records an event and
    /// blocks until rollover completes.
    pub fn gate_execution_cycle(
        &mut self,
        budget: &Budget,
        used: u64,
        hint: Option<u64>,
        plan: &ContextPlanId,
    ) -> Result<BudgetVerdict, ContinuityError> {
        match BudgetMonitor::evaluate(budget, used, hint) {
            BudgetVerdict::Ok => Ok(BudgetVerdict::Ok),
            BudgetVerdict::SoftExceeded => {
                self.store.record_budget_event(&BudgetEvent {
                    plan_ref: plan.clone(),
                    hard: false,
                    recorded_at: self.now_millis,
                })?;
                Ok(BudgetVerdict::SoftExceeded)
            }
            BudgetVerdict::HardExceeded => {
                self.store.record_budget_event(&BudgetEvent {
                    plan_ref: plan.clone(),
                    hard: true,
                    recorded_at: self.now_millis,
                })?;
                Err(ContinuityError::BudgetBlocked(
                    "hard budget exceeded: roll over before new cycles".to_owned(),
                ))
            }
        }
    }

    /// Ranked history search (CR-4: hits, never content).
    pub fn search_history(
        &self,
        query: &str,
        filters: &SearchFilters,
        now_millis: u64,
    ) -> Result<Vec<HistoryHit>, ContinuityError> {
        self.store.search_history(query, filters, now_millis)
    }

    /// Bounded content for one exact history ref.
    pub fn read_history(
        &self,
        exact: &continuity::model::HistoryRef,
    ) -> Result<Option<HistoryContent>, ContinuityError> {
        self.store.read_history(exact)
    }

    /// Migrates execution to a new chain from a committed checkpoint,
    /// minting the genesis epoch (refs only, CR-7).
    pub fn migrate_execution(
        &mut self,
        checkpoint_id: &CheckpointId,
        execution: ExecutionId,
    ) -> Result<MigrationLink, ContinuityError> {
        let checkpoint = self.store.load_checkpoint(checkpoint_id)?.ok_or_else(|| {
            ContinuityError::MissingCheckpoint(format!(
                "checkpoint {} is unknown or not committed",
                checkpoint_id.as_str()
            ))
        })?;
        let genesis = Epoch {
            id: mint_epoch_id(self.now_millis, self.next_seq()),
            task_ref: checkpoint.task_ref.clone(),
            execution_ref: execution,
            seq: 0,
            state: EpochState::Active,
            created_at: self.now_millis,
        };
        self.store.migrate_chain(checkpoint_id, &genesis)
    }

    /// Purges expired rows per policy-owned durations.
    pub fn purge_expired(
        &mut self,
        policy: &RetentionPolicy,
    ) -> Result<PurgeReport, ContinuityError> {
        self.store.purge_expired(self.now_millis, policy)
    }

    /// Rebuilds the working set from a committed checkpoint plus its
    /// plan (ADR-0043 §5): SECRET-labelled notes are excluded, tool
    /// pairs restart empty — crashed images are never trusted.
    pub fn rebuild_working_set(
        &self,
        checkpoint_id: &CheckpointId,
    ) -> Result<WorkingSet, ContinuityError> {
        let checkpoint = self.store.load_checkpoint(checkpoint_id)?.ok_or_else(|| {
            ContinuityError::MissingCheckpoint(format!(
                "checkpoint {} is unknown or not committed",
                checkpoint_id.as_str()
            ))
        })?;
        let mut notes = Vec::with_capacity(checkpoint.working_state.len());
        for note_id in &checkpoint.working_state {
            let note =
                self.store
                    .load_note(note_id)?
                    .ok_or_else(|| ContinuityError::BackendFailure {
                        detail: "checkpoint references a missing note".to_owned(),
                    })?;
            if note.sensitivity == Sensitivity::Secret {
                continue;
            }
            notes.push(note.id);
        }
        Ok(WorkingSet {
            plan: checkpoint.context_plan_ref,
            notes,
            tool_pairs: Vec::new(),
        })
    }
}
