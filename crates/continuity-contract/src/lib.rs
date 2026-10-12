//! Shared `continuity` store contract suite (TASK-0171, G9).
//!
//! The canonical durable continuity seam is
//! [`continuity::port::ContinuityStorePort`] (ADR-0043): atomic
//! checkpoint commit, idempotent activation, exact-ref history.
//! Every store implementing the seam must satisfy
//! [`run_continuity_store_contract`].
//!
//! What the contract asserts (only behavior the seam promises):
//!
//! ```text
//! chain     → genesis opens ACTIVE; second genesis refuses; chains isolated
//! stage     → staged checkpoints invisible (also after reopen)
//! commit    → atomic receipt; byte-identical read-back; idempotent re-commit
//! activate  → successor ACTIVE, old INDEXED; repeat no-ops; fork conflicts
//! cr1       → rollover/activation without a committed checkpoint refuses
//! parent    → unknown/staged parents refuse; committed parents link
//! ```

use agent_runtime::{AttemptId, ExecutionId, ProviderRef};
use context_engine::model::{ContextItemId, ContextPlanId, Sensitivity, SourceRef};
use continuity::history::{HistoryKind, SearchFilters};
use continuity::mint::{mint_checkpoint_id, mint_epoch_id, mint_history_ref, mint_note_id};
use continuity::model::{
    BudgetEvent, Checkpoint, ContinuityError, Epoch, EpochId, EpochState, FormatVersion,
    NoteAuthor, NoteBody, NoteId, RetentionPolicy, SessionLink, WorkingNote,
};
use continuity::port::ContinuityStorePort;
use task_domain::TaskId;

/// Test-only fixtures shared by store implementers.
pub mod fixtures {
    use super::*;

    pub const PLAN_ID: &str = "ctxplan_01ARZ3NDEKTSV4RRFFQ69G5FAV";

    pub fn epoch(seq_base: u128, task: &str, execution: &str, seq: u64) -> Epoch {
        Epoch {
            id: mint_epoch_id(1000, seq_base),
            task_ref: TaskId::new(task).unwrap(),
            execution_ref: ExecutionId::new(execution).unwrap(),
            seq,
            state: EpochState::Active,
            created_at: 1000,
        }
    }

    pub fn note(seq: u128, epoch: &EpochId) -> WorkingNote {
        WorkingNote {
            id: mint_note_id(1000, seq),
            epoch_ref: epoch.clone(),
            created_at: 1000,
            author: NoteAuthor::Agent,
            body: NoteBody::Inline("observed: green".to_owned()),
            sensitivity: Sensitivity::Project,
        }
    }

    pub fn checkpoint(seq: u128, epoch: &Epoch, notes: Vec<NoteId>) -> Checkpoint {
        Checkpoint {
            id: mint_checkpoint_id(1000, seq),
            epoch_ref: epoch.id.clone(),
            task_ref: epoch.task_ref.clone(),
            execution_ref: epoch.execution_ref.clone(),
            created_at: 1000,
            format_version: FormatVersion::new(0x0001_0000),
            parent_checkpoint: None,
            working_state: notes,
            source_refs: vec![SourceRef::repo_path("rev1", "src/lib.rs", None).unwrap()],
            evidence_refs: vec!["ev:1".to_owned()],
            continuation: "next: roll over".to_owned(),
            context_plan_ref: ContextPlanId::new(PLAN_ID).unwrap(),
            sensitivity_ceiling: Sensitivity::Project,
        }
    }
}

use fixtures::{checkpoint, epoch, note};

/// Runs the store contract against one implementation.
///
/// `reopen` must return a fresh handle to the same durable state
/// (close + reopen semantics).
pub fn run_continuity_store_contract<S: ContinuityStorePort>(
    store: &mut S,
    reopen: impl Fn() -> S,
) {
    assert_chain_open(store);
    assert_stage_commit(store);
    assert_activation(store);
    assert_cr1_and_parents(store);

    let fresh = reopen();
    assert_reopen_holds(&fresh);
    drop(fresh);

    assert_history_search(store);
    assert_search_cap(store);
    assert_links_migrate(store);
    assert_purge(store);
}

fn task() -> TaskId {
    TaskId::new("T-1").unwrap()
}

fn execution() -> ExecutionId {
    ExecutionId::new("E-1").unwrap()
}

fn assert_chain_open<S: ContinuityStorePort>(store: &mut S) {
    let genesis = epoch(1, "T-1", "E-1", 0);
    store.open_chain(&genesis).unwrap();
    assert_eq!(
        store.active_epoch(&task(), &execution()).unwrap().as_ref(),
        Some(&genesis)
    );
    assert_eq!(
        store.load_epoch(&genesis.id).unwrap().as_ref(),
        Some(&genesis)
    );

    let dup = epoch(2, "T-1", "E-1", 0);
    let err = store.open_chain(&dup).unwrap_err();
    assert!(
        matches!(err, ContinuityError::InvalidInput(_)),
        "second genesis must refuse, got {err}"
    );

    let other = epoch(3, "T-1", "E-2", 0);
    store.open_chain(&other).unwrap();
    assert_eq!(
        store
            .active_epoch(&task(), &ExecutionId::new("E-2").unwrap())
            .unwrap()
            .as_ref(),
        Some(&other)
    );
    assert_eq!(
        store.active_epoch(&task(), &execution()).unwrap().as_ref(),
        Some(&genesis),
        "chains stay isolated (CR-9)"
    );
}

fn assert_stage_commit<S: ContinuityStorePort>(store: &mut S) {
    let active = store.active_epoch(&task(), &execution()).unwrap().unwrap();
    let note = note(11, &active.id);
    let point = checkpoint(11, &active, vec![note.id.clone()]);
    store.stage_checkpoint(&point, &[note]).unwrap();
    assert_eq!(
        store.load_checkpoint(&point.id).unwrap(),
        None,
        "staged checkpoints stay invisible (CR-2)"
    );

    let receipt = store.commit_checkpoint(&point.id, 2000).unwrap();
    assert_eq!(receipt.checkpoint_id, point.id);
    assert_eq!(receipt.epoch_id, active.id);
    assert_eq!(receipt.committed_at, 2000);
    assert_eq!(receipt.store_revision, 1);
    assert_eq!(
        store.load_checkpoint(&point.id).unwrap().as_ref(),
        Some(&point),
        "committed checkpoints read back byte-identical"
    );
    let again = store.commit_checkpoint(&point.id, 2000).unwrap();
    assert_eq!(again, receipt, "re-commit is idempotent");
    assert_eq!(
        store
            .load_note(&point.working_state[0])
            .unwrap()
            .as_ref()
            .map(|noted| &noted.id),
        Some(&point.working_state[0]),
        "committed notes load by id"
    );
    assert_eq!(
        store
            .active_epoch(&task(), &execution())
            .unwrap()
            .unwrap()
            .id,
        active.id,
        "commit alone never activates"
    );
}

fn assert_activation<S: ContinuityStorePort>(store: &mut S) {
    let active = store.active_epoch(&task(), &execution()).unwrap().unwrap();
    let committed = mint_checkpoint_id(1000, 11);
    let successor = epoch(12, "T-1", "E-1", 1);
    let id = store
        .activate_successor(&committed, &successor, 3000)
        .unwrap();
    assert_eq!(id, successor.id);
    let stored = store.load_epoch(&id).unwrap().unwrap();
    assert_eq!(stored.state, EpochState::Active, "successor is ACTIVE");
    assert_eq!(
        store
            .active_epoch(&task(), &execution())
            .unwrap()
            .unwrap()
            .id,
        id
    );
    let old = store.load_epoch(&active.id).unwrap().unwrap();
    assert_eq!(old.state, EpochState::Indexed, "old epoch is INDEXED");
    assert!(
        store.pending_activations().unwrap().is_empty(),
        "completed activations leave no pending record"
    );

    let same = store
        .activate_successor(&committed, &successor, 3000)
        .unwrap();
    assert_eq!(same, id, "repeat activation no-ops (CR-3)");

    let fork = epoch(13, "T-1", "E-1", 1);
    let err = store
        .activate_successor(&committed, &fork, 3000)
        .unwrap_err();
    assert!(
        matches!(err, ContinuityError::ActivationConflict(_)),
        "second successor must conflict (CR-3), got {err}"
    );
}

fn assert_cr1_and_parents<S: ContinuityStorePort>(store: &mut S) {
    let active = store.active_epoch(&task(), &execution()).unwrap().unwrap();
    let note = note(21, &active.id);
    let staged = checkpoint(21, &active, vec![note.id.clone()]);
    store.stage_checkpoint(&staged, &[note]).unwrap();
    assert_eq!(
        store.load_note(&staged.working_state[0]).unwrap(),
        None,
        "staged-only notes stay invisible (CR-2)"
    );
    let next = epoch(22, "T-1", "E-1", active.seq + 1);
    let err = store
        .activate_successor(&staged.id, &next, 4000)
        .unwrap_err();
    assert!(
        matches!(err, ContinuityError::NotCommitted(_)),
        "activation without a committed checkpoint refuses (CR-1), got {err}"
    );

    let mut orphan = checkpoint(23, &active, vec![]);
    orphan.parent_checkpoint = Some(mint_checkpoint_id(1000, 999));
    store.stage_checkpoint(&orphan, &[]).unwrap();
    let err = store.commit_checkpoint(&orphan.id, 4000).unwrap_err();
    assert!(
        matches!(err, ContinuityError::MissingCheckpoint(_)),
        "unknown parents refuse, got {err}"
    );

    let mut child_of_staged = checkpoint(24, &active, vec![]);
    child_of_staged.parent_checkpoint = Some(staged.id.clone());
    store.stage_checkpoint(&child_of_staged, &[]).unwrap();
    let err = store
        .commit_checkpoint(&child_of_staged.id, 4000)
        .unwrap_err();
    assert!(
        matches!(err, ContinuityError::MissingCheckpoint(_)),
        "staged parents refuse, got {err}"
    );

    let committed_parent = mint_checkpoint_id(1000, 11);
    let mut child = checkpoint(25, &active, vec![]);
    child.parent_checkpoint = Some(committed_parent);
    store.stage_checkpoint(&child, &[]).unwrap();
    store.commit_checkpoint(&child.id, 4000).unwrap();
    assert_eq!(
        store.load_checkpoint(&child.id).unwrap().as_ref(),
        Some(&child),
        "committed parents link"
    );
}

fn assert_reopen_holds<S: ContinuityStorePort>(store: &S) {
    let active = store.active_epoch(&task(), &execution()).unwrap().unwrap();
    assert_eq!(active.seq, 1);
    assert_eq!(active.state, EpochState::Active);
    let committed = mint_checkpoint_id(1000, 11);
    assert!(store.load_checkpoint(&committed).unwrap().is_some());
    let staged = mint_checkpoint_id(1000, 21);
    assert_eq!(
        store.load_checkpoint(&staged).unwrap(),
        None,
        "staged rows stay invisible after reopen (CR-2)"
    );
}

fn assert_history_search<S: ContinuityStorePort>(store: &mut S) {
    let genesis = epoch(100, "T-H", "E-H", 0);
    store.open_chain(&genesis).unwrap();
    let mut noted = note(100, &genesis.id);
    noted.body = NoteBody::Inline("rollover drill completed smoothly".to_owned());
    let mut point = checkpoint(100, &genesis, vec![noted.id.clone()]);
    point.continuation = "migration plan drafted for review".to_owned();
    store.stage_checkpoint(&point, &[noted]).unwrap();
    store.commit_checkpoint(&point.id, 5000).unwrap();

    let hits = store
        .search_history("rollover", &SearchFilters::default(), 6000)
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].kind, HistoryKind::Note);
    assert!(hits[0].rank.text > 0.0);

    let hits = store
        .search_history("migration", &SearchFilters::default(), 6000)
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].kind, HistoryKind::Checkpoint);

    let mut filters = SearchFilters {
        task: Some(TaskId::new("T-NOPE").unwrap()),
        ..Default::default()
    };
    let hits = store.search_history("rollover", &filters, 6000).unwrap();
    assert!(hits.is_empty());
    filters.task = Some(TaskId::new("T-H").unwrap());
    let hits = store.search_history("rollover", &filters, 6000).unwrap();
    assert_eq!(hits.len(), 1);

    let filters = SearchFilters {
        epoch: Some(genesis.id.clone()),
        ..Default::default()
    };
    let hits = store.search_history("rollover", &filters, 6000).unwrap();
    assert_eq!(hits.len(), 1);

    let mut filters = SearchFilters {
        event_type: Some("CHECKPOINT".to_owned()),
        ..Default::default()
    };
    let hits = store.search_history("migration", &filters, 6000).unwrap();
    assert_eq!(hits.len(), 1);
    filters.event_type = Some("RECEIPT".to_owned());
    let hits = store.search_history("migration", &filters, 6000).unwrap();
    assert!(hits.is_empty());

    let mut filters = SearchFilters {
        metadata: vec![("author".to_owned(), "AGENT".to_owned())],
        ..Default::default()
    };
    let hits = store.search_history("rollover", &filters, 6000).unwrap();
    assert_eq!(hits.len(), 1);
    filters.metadata = vec![("author".to_owned(), "HUMAN".to_owned())];
    let hits = store.search_history("rollover", &filters, 6000).unwrap();
    assert!(hits.is_empty());

    let err = store
        .search_history("  ", &SearchFilters::default(), 6000)
        .unwrap_err();
    assert!(
        matches!(err, ContinuityError::InvalidInput(_)),
        "blank queries need a narrowing filter (CR-4), got {err}"
    );
    let filters = SearchFilters {
        epoch: Some(genesis.id.clone()),
        ..Default::default()
    };
    let hits = store.search_history("", &filters, 6000).unwrap();
    assert_eq!(hits.len(), 2, "checkpoint + note refs");

    let content = store.read_history(&hits[0].history_ref).unwrap().unwrap();
    assert_eq!(content.epoch_id, genesis.id);
    assert!(!content.title.is_empty() && content.excerpt.len() <= 4096);
    let unknown = mint_history_ref(1000, 9999);
    assert_eq!(store.read_history(&unknown).unwrap(), None);

    let item = ContextItemId::new("ctxitem_01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
    let receipt_ref = mint_history_ref(1000, 101);
    store
        .register_receipt(&receipt_ref, &genesis.id, &item, 6000)
        .unwrap();
    let hits = store
        .search_history(
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            &SearchFilters::default(),
            6000,
        )
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].kind, HistoryKind::Receipt);
    assert_eq!(hits[0].history_ref, receipt_ref);
}

fn assert_search_cap<S: ContinuityStorePort>(store: &mut S) {
    let genesis = epoch(110, "T-CAP", "E-CAP", 0);
    store.open_chain(&genesis).unwrap();
    let notes: Vec<WorkingNote> = (0..101)
        .map(|i| {
            let mut noted = note(1000 + i, &genesis.id);
            noted.body = NoteBody::Inline(format!("capybara sighting {i}"));
            noted
        })
        .collect();
    let ids: Vec<NoteId> = notes.iter().map(|noted| noted.id.clone()).collect();
    let mut point = checkpoint(110, &genesis, ids);
    point.continuation = "next: bulk".to_owned();
    store.stage_checkpoint(&point, &notes).unwrap();
    store.commit_checkpoint(&point.id, 7000).unwrap();
    let hits = store
        .search_history("capybara", &SearchFilters::default(), 8000)
        .unwrap();
    assert_eq!(hits.len(), 100, "search is bounded");
}

fn assert_links_migrate<S: ContinuityStorePort>(store: &mut S) {
    let genesis = epoch(120, "T-M", "E-M", 0);
    store.open_chain(&genesis).unwrap();
    let link = SessionLink {
        session_ref: "claude:ses_9".to_owned(),
        epoch_id: genesis.id.clone(),
        attempt_ref: AttemptId::new("A-9").unwrap(),
        provider_id: ProviderRef::new("claude").unwrap(),
        linked_at: 9000,
    };
    store.link_session(&link).unwrap();
    let err = store.link_session(&link).unwrap_err();
    assert!(
        matches!(err, ContinuityError::InvalidInput(_)),
        "duplicate session links refuse, got {err}"
    );

    let mut point = checkpoint(120, &genesis, vec![]);
    point.continuation = "ready to migrate".to_owned();
    store.stage_checkpoint(&point, &[]).unwrap();
    store.commit_checkpoint(&point.id, 9000).unwrap();

    let mut target = epoch(121, "T-M", "E-9", 0);
    target.created_at = 9000;
    let moved = store.migrate_chain(&point.id, &target).unwrap();
    assert_eq!(moved.from_checkpoint, point.id);
    assert_eq!(moved.to_epoch, target.id);
    assert_eq!(
        store
            .active_epoch(
                &TaskId::new("T-M").unwrap(),
                &ExecutionId::new("E-9").unwrap()
            )
            .unwrap()
            .unwrap()
            .id,
        target.id
    );
    assert_eq!(
        store
            .active_epoch(
                &TaskId::new("T-M").unwrap(),
                &ExecutionId::new("E-M").unwrap()
            )
            .unwrap()
            .unwrap()
            .id,
        genesis.id,
        "migration leaves the source chain untouched"
    );

    let staged = checkpoint(122, &genesis, vec![]);
    store.stage_checkpoint(&staged, &[]).unwrap();
    let target2 = epoch(123, "T-M", "E-8", 0);
    let err = store.migrate_chain(&staged.id, &target2).unwrap_err();
    assert!(
        matches!(err, ContinuityError::NotCommitted(_)),
        "migration from staged checkpoints refuses, got {err}"
    );

    store
        .record_budget_event(&BudgetEvent {
            plan_ref: ContextPlanId::new(fixtures::PLAN_ID).unwrap(),
            hard: true,
            recorded_at: 9000,
        })
        .unwrap();
}

fn assert_purge<S: ContinuityStorePort>(store: &mut S) {
    let genesis = epoch(130, "T-PG", "E-PG", 0);
    store.open_chain(&genesis).unwrap();
    let mut old = checkpoint(130, &genesis, vec![]);
    old.created_at = 100;
    store.stage_checkpoint(&old, &[]).unwrap();

    let mut ancient = epoch(131, "T-OLD", "E-OLD", 0);
    ancient.created_at = 100;
    store.open_chain(&ancient).unwrap();
    let mut sealed_note = note(131, &ancient.id);
    sealed_note.created_at = 100;
    let mut sealed = checkpoint(131, &ancient, vec![sealed_note.id.clone()]);
    sealed.created_at = 100;
    store.stage_checkpoint(&sealed, &[sealed_note]).unwrap();
    store.commit_checkpoint(&sealed.id, 200).unwrap();
    let mut next = epoch(132, "T-OLD", "E-OLD", 1);
    next.created_at = 200;
    store.activate_successor(&sealed.id, &next, 300).unwrap();

    let mut fresh = epoch(133, "T-FRESH", "E-FRESH", 0);
    fresh.created_at = 90_000;
    store.open_chain(&fresh).unwrap();

    // Migrated genesis that indexed and aged: purging it must not
    // trip the migration-link FK (the link goes with the epoch).
    let mut mg_old = epoch(150, "T-MG", "E-MG", 0);
    mg_old.created_at = 100;
    store.open_chain(&mg_old).unwrap();
    let mut mg_point = checkpoint(150, &mg_old, vec![]);
    mg_point.created_at = 100;
    store.stage_checkpoint(&mg_point, &[]).unwrap();
    store.commit_checkpoint(&mg_point.id, 200).unwrap();
    let mut mg_genesis = epoch(151, "T-MG", "E-MG2", 0);
    mg_genesis.created_at = 100;
    store.migrate_chain(&mg_point.id, &mg_genesis).unwrap();
    let mut mg_sealed = checkpoint(151, &mg_genesis, vec![]);
    mg_sealed.created_at = 100;
    store.stage_checkpoint(&mg_sealed, &[]).unwrap();
    store.commit_checkpoint(&mg_sealed.id, 200).unwrap();
    let mut mg_next = epoch(152, "T-MG", "E-MG2", 1);
    mg_next.created_at = 200;
    store
        .activate_successor(&mg_sealed.id, &mg_next, 300)
        .unwrap();

    // Lineage guard: an old indexed epoch referenced as a parent by
    // a surviving checkpoint stays.
    let mut lin_old = epoch(140, "T-LIN", "E-LIN", 0);
    lin_old.created_at = 100;
    store.open_chain(&lin_old).unwrap();
    let mut parent_point = checkpoint(140, &lin_old, vec![]);
    parent_point.created_at = 100;
    store.stage_checkpoint(&parent_point, &[]).unwrap();
    store.commit_checkpoint(&parent_point.id, 200).unwrap();
    let mut lin_new = epoch(141, "T-LIN", "E-LIN", 1);
    lin_new.created_at = 90_000;
    store
        .activate_successor(&parent_point.id, &lin_new, 90_000)
        .unwrap();
    let mut child_point = checkpoint(141, &lin_new, vec![]);
    child_point.parent_checkpoint = Some(parent_point.id.clone());
    child_point.created_at = 90_000;
    store.stage_checkpoint(&child_point, &[]).unwrap();
    store.commit_checkpoint(&child_point.id, 91_000).unwrap();

    let report = store
        .purge_expired(
            100_000,
            &RetentionPolicy {
                staged_older_than_ms: 1000,
                indexed_older_than_ms: Some(1000),
                history_older_than_ms: Some(1000),
            },
        )
        .unwrap();
    // Staged victims: 21, 23, 24 (Task 6), 122 (links), 130.
    assert_eq!(report.staged, 5, "staged leftovers purge");
    // Epoch victims: 131 (T-OLD) and 151 (T-MG2); the T-1/E-1 genesis
    // stays via the lineage guard (child 25 survives), as does 140.
    assert_eq!(report.epochs, 2, "old indexed epochs purge");
    // Notes: orphaned 21 via the sweep, sealed 131 via its epoch.
    assert_eq!(report.notes, 2, "orphan + epoch notes purge");
    // Refs in surviving INDEXED epochs shed (cold storage keeps
    // direct-id loads): lin_old's checkpoint ref plus the two
    // Task-6 genesis refs.
    assert_eq!(report.history_refs, 3, "old INDEXED refs shed");
    assert_eq!(store.load_checkpoint(&old.id).unwrap(), None);
    assert_eq!(store.load_epoch(&ancient.id).unwrap(), None);
    assert_eq!(store.load_checkpoint(&sealed.id).unwrap(), None);
    assert_eq!(
        store.load_epoch(&mg_genesis.id).unwrap(),
        None,
        "migrated genesis purges without FK abort"
    );
    assert!(
        store.load_epoch(&lin_old.id).unwrap().is_some(),
        "lineage guard keeps referenced victims"
    );
    assert!(store.load_epoch(&next.id).unwrap().is_some());
    assert_eq!(
        store
            .active_epoch(
                &TaskId::new("T-OLD").unwrap(),
                &ExecutionId::new("E-OLD").unwrap()
            )
            .unwrap()
            .unwrap()
            .id,
        next.id,
        "ACTIVE successors survive"
    );
    assert!(
        store
            .active_epoch(
                &TaskId::new("T-FRESH").unwrap(),
                &ExecutionId::new("E-FRESH").unwrap()
            )
            .unwrap()
            .is_some(),
        "uncheckpointed chains survive"
    );
    let hits = store
        .search_history("rollover", &SearchFilters::default(), 100_000)
        .unwrap();
    assert_eq!(hits.len(), 1, "ACTIVE-epoch refs survive history retention");
}
