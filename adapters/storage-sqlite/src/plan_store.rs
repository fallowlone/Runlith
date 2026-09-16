//! Durable G7 Plan revision snapshots and atomic revision-journal commits.
//!
//! The canonical current Plan is the immutable graph snapshot selected by
//! `plan_branches.head_revision`. Revision metadata, graph structure and the
//! required `plan.revision_committed` Event Journal fact commit in one
//! `BEGIN IMMEDIATE` transaction. The journal carries only bounded identity
//! and provenance; full node/edge state lives here exactly once.

use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use plan_domain::{
    decomposition_fingerprint, CriterionDecl, CriterionId, EvidenceRef, GateRef, PlanBranchId,
    PlanChangeSet, PlanGraph, PlanNode, PlanNodeId, PlanRevision, PlannerAdapterId,
    PlannerIdentity, PlanningCostReceiptRef, RevisionProposedBy, RiskClass, ValidatedBy,
    VerificationMethod,
};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

use crate::error::{map_constraint, StorageError, StorageResult};
use crate::event::{EventType, JournalEvent, NewJournalEvent, PlanRevisionJournalPayload};
use crate::journal_store::{insert_journal_event, require_canonical_sha256};
use crate::plan_cost_store::load_plan_cost_receipt;
use crate::plan_guardrail_store::{require_evidence_key, reset_rejections};
use crate::planning_control_store::{enforce_publication_fence, mark_publication_succeeded};

/// Trusted application-service envelope for the Event Journal metadata that
/// surrounds one committed Plan revision. It contains no planner transcript,
/// prompt, provider credential, or authorization token.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanCommitJournalEvidence {
    pub correlation_id: String,
    pub causation_id: String,
    pub occurred_at: String,
    pub provenance: String,
}

/// Trusted guardrail lineage committed with one revision: the parent task
/// ref that feeds the V7 fingerprint and the key of the triggering evidence
/// event that produced the revision. Storage derives the fingerprint itself
/// from the accepted changeset, so it cannot disagree with the proposal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanRevisionLineage {
    pub parent_task_ref: String,
    pub trigger_evidence_key: String,
    pub planning_input_snapshot_id: plan_domain::PlanningInputSnapshotId,
    pub planning_input_fingerprint: String,
    pub planning_run_id: plan_domain::PlanningRunId,
    pub planning_run_generation: u64,
}

/// Durable lineage of one committed revision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanRevisionLineageRecord {
    pub revision_seq: u64,
    pub parent_task_ref: String,
    pub decomposition_fingerprint: String,
    pub trigger_evidence_key: String,
    pub planning_input_snapshot_id: plan_domain::PlanningInputSnapshotId,
    pub planning_input_fingerprint: String,
    pub planning_run_id: plan_domain::PlanningRunId,
    pub planning_run_generation: u64,
}

impl crate::task_store::SqliteStore {
    /// Reads and validates one branch's durable head. A head/revision lineage
    /// disagreement is corruption rather than an implicit repair.
    pub fn get_plan_head(&self, branch: &PlanBranchId) -> StorageResult<Option<u64>> {
        load_validated_head(self.connection(), branch)
    }

    /// Reads one immutable revision and reconstructs all provenance through
    /// the TASK-0076 validated types.
    pub fn get_plan_revision(
        &self,
        branch: &PlanBranchId,
        revision_seq: u64,
    ) -> StorageResult<Option<PlanRevision>> {
        let Some(head) = load_validated_head(self.connection(), branch)? else {
            return Ok(None);
        };
        if revision_seq == 0 || revision_seq > head {
            return Ok(None);
        }
        load_revision(self.connection(), branch, revision_seq)
    }

    /// Reconstructs the canonical normalized graph snapshot of one revision.
    /// Corrupt identifiers, enum values, ordering, references, or cycles fail
    /// explicitly; no defaults or auto-healing are applied.
    pub fn get_plan_graph(
        &self,
        branch: &PlanBranchId,
        revision_seq: u64,
    ) -> StorageResult<Option<PlanGraph>> {
        let Some(head) = load_validated_head(self.connection(), branch)? else {
            return Ok(None);
        };
        if revision_seq == 0 || revision_seq > head {
            return Ok(None);
        }
        if load_revision(self.connection(), branch, revision_seq)?.is_none() {
            return Err(StorageError::Corrupt(format!(
                "plan branch {} head lineage contains missing revision {revision_seq}",
                branch.as_str()
            )));
        }
        load_graph(self.connection(), branch, revision_seq).map(Some)
    }

    /// Reads the durable guardrail lineage of every committed revision on the
    /// branch, in revision order. A committed revision without lineage (for
    /// example history written before schema 14) fails closed: V7 and the
    /// per-evidence revision bound cannot be reconstructed from guesses.
    pub fn get_plan_lineage(
        &self,
        branch: &PlanBranchId,
    ) -> StorageResult<Vec<PlanRevisionLineageRecord>> {
        let connection = self.connection();
        let Some(head) = load_validated_head(connection, branch)? else {
            return Ok(Vec::new());
        };
        let mut statement = connection.prepare(
            "SELECT revision_seq, parent_task_ref, decomposition_fingerprint, trigger_evidence_key,
                    planning_input_snapshot_id, planning_input_fingerprint,
                    planning_run_id, planning_run_generation
             FROM plan_revision_lineage WHERE plan_branch_id = ?1 ORDER BY revision_seq",
        )?;
        let rows = statement.query_map(params![branch.as_str()], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, i64>(7)?,
            ))
        })?;
        let mut lineage = Vec::new();
        for (expected, row) in (1_u64..).zip(rows) {
            let (
                raw_seq,
                parent_task_ref,
                fingerprint,
                trigger_evidence_key,
                snapshot_id,
                input_fingerprint,
                planning_run_id,
                raw_generation,
            ) = row?;
            let revision_seq = positive_u64(raw_seq, "plan lineage revision")?;
            if revision_seq != expected {
                return Err(missing_lineage(branch, expected));
            }
            let canonical = require_canonical_sha256(&fingerprint).is_ok()
                && require_evidence_key(&trigger_evidence_key).is_ok()
                && require_evidence_key(&input_fingerprint).is_ok()
                && !parent_task_ref.trim().is_empty();
            if !canonical {
                return Err(StorageError::Corrupt(format!(
                    "plan branch {} revision {revision_seq} stores malformed lineage",
                    branch.as_str()
                )));
            }
            lineage.push(PlanRevisionLineageRecord {
                revision_seq,
                parent_task_ref,
                decomposition_fingerprint: fingerprint,
                trigger_evidence_key,
                planning_input_snapshot_id: plan_domain::PlanningInputSnapshotId::new(snapshot_id)
                    .map_err(|error| StorageError::DomainReconstruction(error.to_string()))?,
                planning_input_fingerprint: input_fingerprint,
                planning_run_id: plan_domain::PlanningRunId::new(planning_run_id)
                    .map_err(|error| StorageError::DomainReconstruction(error.to_string()))?,
                planning_run_generation: positive_u64(raw_generation, "planning run generation")?,
            });
        }
        let recorded = lineage.len() as u64;
        if recorded != head {
            return Err(missing_lineage(branch, recorded.saturating_add(1)));
        }
        Ok(lineage)
    }

    /// Commits one already-validated Plan mutation as an immutable revision
    /// snapshot and exactly one bounded journal fact.
    ///
    /// This boundary deliberately does not repeat TASK-0076 V1-V9. It does
    /// independently enforce persistence invariants: exact accepted-hash /
    /// provenance binding, genesis/contiguous sequencing, durable stale-head
    /// detection, uniqueness, foreign keys and atomic state+journal outcome.
    /// In the same transaction it requires the revision's planning cost
    /// receipt ref to resolve to exactly the proposal's receipt, records the
    /// revision's guardrail lineage, and resets the triggering event's L3
    /// rejection counter.
    pub fn commit_plan_revision(
        &mut self,
        changeset: &PlanChangeSet,
        revision: &PlanRevision,
        resulting_graph: &PlanGraph,
        lineage: &PlanRevisionLineage,
        evidence: &PlanCommitJournalEvidence,
    ) -> StorageResult<JournalEvent> {
        validate_commit_binding(changeset, revision, evidence)?;
        validate_graph_snapshot(resulting_graph)?;
        require_evidence_key(&lineage.trigger_evidence_key)?;
        require_evidence_key(&lineage.planning_input_fingerprint)?;
        if lineage.parent_task_ref.trim().is_empty() {
            return Err(StorageError::ConstraintViolation(
                "plan revision lineage requires a non-blank parent_task_ref".to_owned(),
            ));
        }
        let fingerprint = decomposition_fingerprint(&lineage.parent_task_ref, changeset);

        let revision_i64 = sqlite_u64(revision.revision_seq, "revision_seq")?;
        let base_i64 = sqlite_u64(revision.base_revision, "base_revision")?;
        let created_at_i64 = sqlite_u64(revision.created_at, "created_at")?;
        let branch = revision.plan_branch_id.as_str();

        let transaction = self
            .connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;

        // ADR-0028 stale-writer fence. The provider call happened outside
        // this transaction; publication is authoritative only while the
        // exact input snapshot and PlanningRun generation still own the
        // branch. This check shares the revision write transaction.
        enforce_publication_fence(
            &transaction,
            lineage,
            &revision.plan_branch_id,
            revision.base_revision,
        )?;

        if revision.revision_seq == 1 {
            transaction.execute(
                "INSERT INTO plan_branches (plan_branch_id, head_revision)
                 VALUES (?1, 0)
                 ON CONFLICT(plan_branch_id) DO NOTHING",
                params![branch],
            )?;
        }

        let durable_head: Option<i64> = transaction
            .query_row(
                "SELECT head_revision FROM plan_branches WHERE plan_branch_id = ?1",
                params![branch],
                |row| row.get(0),
            )
            .optional()?;
        let Some(durable_head) = durable_head else {
            return Err(StorageError::NotFound(format!(
                "plan branch {branch} does not exist for non-genesis revision"
            )));
        };
        let durable_head = nonnegative_u64(durable_head, "plan branch head")?;
        if durable_head != revision.base_revision {
            return Err(StorageError::PlanStale {
                branch: branch.to_owned(),
                expected: revision.base_revision,
                found: durable_head,
            });
        }

        let receipt_ref = &revision.proposed_by.planning_cost_receipt_ref;
        match load_plan_cost_receipt(&transaction, receipt_ref)? {
            Some(stored) if stored == changeset.planning_cost_receipt => {}
            Some(_) => {
                return Err(StorageError::ConstraintViolation(format!(
                    "planning cost receipt {} does not match the accepted proposal receipt",
                    receipt_ref.as_str()
                )));
            }
            None => {
                return Err(StorageError::ConstraintViolation(format!(
                    "planning cost receipt {} does not resolve to durable evidence",
                    receipt_ref.as_str()
                )));
            }
        }

        transaction
            .execute(
                "INSERT INTO plan_revisions
                 (plan_branch_id, revision_seq, base_revision, changeset_hash, reason,
                  planner_adapter_id, planner_model_ref, planning_cost_receipt_ref,
                  validated_service_id, policy_fingerprint, created_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    branch,
                    revision_i64,
                    base_i64,
                    revision.changeset_hash,
                    revision.reason,
                    revision.proposed_by.planner.adapter_id.as_str(),
                    revision.proposed_by.planner.model_ref,
                    revision.proposed_by.planning_cost_receipt_ref.as_str(),
                    revision.validated_by.service_id,
                    revision.validated_by.policy_fingerprint,
                    created_at_i64,
                ],
            )
            .map_err(|error| map_constraint(error, "commit plan revision"))?;

        insert_revision_causes(&transaction, revision)?;
        insert_graph_snapshot(&transaction, revision, resulting_graph)?;
        transaction
            .execute(
                "INSERT INTO plan_revision_lineage
                 (plan_branch_id, revision_seq, parent_task_ref, decomposition_fingerprint,
                  trigger_evidence_key, planning_input_snapshot_id,
                  planning_input_fingerprint, planning_run_id, planning_run_generation)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    branch,
                    revision_i64,
                    lineage.parent_task_ref,
                    fingerprint,
                    lineage.trigger_evidence_key,
                    lineage.planning_input_snapshot_id.as_str(),
                    lineage.planning_input_fingerprint,
                    lineage.planning_run_id.as_str(),
                    sqlite_u64(lineage.planning_run_generation, "planning_run_generation")?,
                ],
            )
            .map_err(|error| map_constraint(error, "commit plan revision lineage"))?;
        reset_rejections(
            &transaction,
            &revision.plan_branch_id,
            &lineage.trigger_evidence_key,
        )?;

        let updated = transaction.execute(
            "UPDATE plan_branches SET head_revision = ?1
             WHERE plan_branch_id = ?2 AND head_revision = ?3",
            params![revision_i64, branch, base_i64],
        )?;
        if updated != 1 {
            let found: i64 = transaction.query_row(
                "SELECT head_revision FROM plan_branches WHERE plan_branch_id = ?1",
                params![branch],
                |row| row.get(0),
            )?;
            return Err(StorageError::PlanStale {
                branch: branch.to_owned(),
                expected: revision.base_revision,
                found: nonnegative_u64(found, "plan branch head")?,
            });
        }

        mark_publication_succeeded(&transaction, lineage, revision.created_at)?;

        let event = plan_revision_event(revision, evidence);
        let committed = insert_journal_event(&transaction, &event)?;
        transaction.commit()?;
        Ok(committed)
    }
}

fn validate_commit_binding(
    changeset: &PlanChangeSet,
    revision: &PlanRevision,
    evidence: &PlanCommitJournalEvidence,
) -> StorageResult<()> {
    if revision.base_revision != changeset.base_plan_revision {
        return Err(StorageError::ConstraintViolation(
            "PlanRevision.base_revision does not match accepted PlanChangeSet base".to_owned(),
        ));
    }
    if revision.revision_seq != revision.base_revision.saturating_add(1) {
        return Err(StorageError::ConstraintViolation(format!(
            "plan revision sequence {} is not contiguous after base {}",
            revision.revision_seq, revision.base_revision
        )));
    }
    if revision.changeset_hash != changeset.proposal_hash {
        return Err(StorageError::ConstraintViolation(
            "PlanRevision changeset_hash does not match accepted canonical proposal_hash"
                .to_owned(),
        ));
    }
    require_canonical_sha256(&revision.changeset_hash)?;
    if revision.reason != changeset.reason
        || revision.caused_by != changeset.source_evidence_refs
        || revision.proposed_by.planner != changeset.proposed_by
    {
        return Err(StorageError::ConstraintViolation(
            "PlanRevision provenance does not match accepted PlanChangeSet".to_owned(),
        ));
    }
    for (field, value) in [
        ("correlation_id", evidence.correlation_id.as_str()),
        ("causation_id", evidence.causation_id.as_str()),
        ("occurred_at", evidence.occurred_at.as_str()),
        ("provenance", evidence.provenance.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(StorageError::ConstraintViolation(format!(
                "plan commit journal evidence requires non-blank {field}"
            )));
        }
    }
    Ok(())
}

fn plan_revision_event(
    revision: &PlanRevision,
    evidence: &PlanCommitJournalEvidence,
) -> NewJournalEvent {
    NewJournalEvent {
        event_type: EventType::PlanRevisionCommitted,
        task_id: None,
        project_id: None,
        from_state: None,
        to_state: None,
        reason: Some(revision.reason.clone()),
        correlation_id: evidence.correlation_id.clone(),
        causation_id: evidence.causation_id.clone(),
        occurred_at: evidence.occurred_at.clone(),
        provenance: evidence.provenance.clone(),
        execution_id: None,
        attempt_id: None,
        attempt_outcome: None,
        workspace_id: None,
        from_workspace_state: None,
        to_workspace_state: None,
        plan_revision: Some(PlanRevisionJournalPayload {
            plan_branch_id: revision.plan_branch_id.clone(),
            revision_seq: revision.revision_seq,
            changeset_hash: revision.changeset_hash.clone(),
            caused_by: revision.caused_by.clone(),
            proposed_by: revision.proposed_by.clone(),
            validated_by: revision.validated_by.clone(),
        }),
    }
}

fn insert_revision_causes(connection: &Connection, revision: &PlanRevision) -> StorageResult<()> {
    for (position, evidence) in revision.caused_by.iter().enumerate() {
        connection.execute(
            "INSERT INTO plan_revision_causes
             (plan_branch_id, revision_seq, position, evidence_ref)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                revision.plan_branch_id.as_str(),
                sqlite_u64(revision.revision_seq, "revision_seq")?,
                sqlite_position(position)?,
                evidence.as_str(),
            ],
        )?;
    }
    Ok(())
}

fn insert_graph_snapshot(
    connection: &Connection,
    revision: &PlanRevision,
    graph: &PlanGraph,
) -> StorageResult<()> {
    let revision_seq = sqlite_u64(revision.revision_seq, "revision_seq")?;
    for (position, node) in graph.nodes.iter().enumerate() {
        connection.execute(
            "INSERT INTO plan_nodes
             (plan_branch_id, revision_seq, position, node_id, risk_class, required_gate)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                revision.plan_branch_id.as_str(),
                revision_seq,
                sqlite_position(position)?,
                node.id.as_str(),
                risk_name(node.risk_class),
                node.required_gate.as_ref().map(GateRef::as_str),
            ],
        )?;
        for (scope_position, root) in node.scope.roots().iter().enumerate() {
            connection.execute(
                "INSERT INTO plan_node_scopes
                 (plan_branch_id, revision_seq, node_id, position, scope_root)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    revision.plan_branch_id.as_str(),
                    revision_seq,
                    node.id.as_str(),
                    sqlite_position(scope_position)?,
                    root,
                ],
            )?;
        }
        for (criterion_position, criterion) in node.criteria.iter().enumerate() {
            connection.execute(
                "INSERT INTO plan_node_criteria
                 (plan_branch_id, revision_seq, node_id, position, criterion_id, verification_method)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    revision.plan_branch_id.as_str(),
                    revision_seq,
                    node.id.as_str(),
                    sqlite_position(criterion_position)?,
                    criterion.id.as_str(),
                    criterion.method.as_str(),
                ],
            )?;
        }
    }
    for (position, (from, to)) in graph.edges.iter().enumerate() {
        connection.execute(
            "INSERT INTO plan_edges
             (plan_branch_id, revision_seq, position, from_node_id, to_node_id)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                revision.plan_branch_id.as_str(),
                revision_seq,
                sqlite_position(position)?,
                from.as_str(),
                to.as_str(),
            ],
        )?;
    }
    Ok(())
}

fn load_validated_head(
    connection: &Connection,
    branch: &PlanBranchId,
) -> StorageResult<Option<u64>> {
    let raw_head: Option<i64> = connection
        .query_row(
            "SELECT head_revision FROM plan_branches WHERE plan_branch_id = ?1",
            params![branch.as_str()],
            |row| row.get(0),
        )
        .optional()?;
    let Some(raw_head) = raw_head else {
        return Ok(None);
    };
    let head = nonnegative_u64(raw_head, "plan branch head")?;
    let mut statement = connection.prepare(
        "SELECT revision_seq, base_revision FROM plan_revisions
         WHERE plan_branch_id = ?1 ORDER BY revision_seq",
    )?;
    let rows = statement.query_map(params![branch.as_str()], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
    })?;
    let mut observed = 0_u64;
    for row in rows {
        let (raw_seq, raw_base) = row?;
        let seq = positive_u64(raw_seq, "plan revision sequence")?;
        let base = nonnegative_u64(raw_base, "plan revision base")?;
        let expected_seq = observed
            .checked_add(1)
            .ok_or_else(|| StorageError::Corrupt("plan revision sequence overflow".to_owned()))?;
        if seq != expected_seq || base != observed {
            return Err(StorageError::Corrupt(format!(
                "plan branch {} stores non-contiguous lineage: expected revision {expected_seq} from base {observed}, found revision {seq} from base {base}",
                branch.as_str()
            )));
        }
        observed = seq;
    }
    if observed != head {
        return Err(StorageError::Corrupt(format!(
            "plan branch {} head is {head} but durable revision lineage ends at {observed}",
            branch.as_str()
        )));
    }
    Ok(Some(head))
}

#[allow(clippy::type_complexity)]
fn load_revision(
    connection: &Connection,
    branch: &PlanBranchId,
    revision_seq: u64,
) -> StorageResult<Option<PlanRevision>> {
    let row: Option<(
        i64,
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
            "SELECT revision_seq, base_revision, changeset_hash, reason,
                    planner_adapter_id, planner_model_ref, planning_cost_receipt_ref,
                    validated_service_id, policy_fingerprint, created_at_ms
             FROM plan_revisions
             WHERE plan_branch_id = ?1 AND revision_seq = ?2",
            params![branch.as_str(), sqlite_u64(revision_seq, "revision_seq")?],
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
                ))
            },
        )
        .optional()?;
    let Some((
        raw_seq,
        raw_base,
        changeset_hash,
        reason,
        adapter_id,
        model_ref,
        receipt_ref,
        service_id,
        policy_fingerprint,
        raw_created_at,
    )) = row
    else {
        return Ok(None);
    };
    require_canonical_sha256(&changeset_hash).map_err(|error| {
        StorageError::Corrupt(format!(
            "plan revision {}:{} stores invalid changeset hash: {error}",
            branch.as_str(),
            revision_seq
        ))
    })?;
    let seq = positive_u64(raw_seq, "plan revision sequence")?;
    let base = nonnegative_u64(raw_base, "plan revision base")?;
    let created_at = nonnegative_u64(raw_created_at, "plan revision created_at")?;
    let caused_by = load_revision_causes(connection, branch, seq)?;
    let proposed_by = RevisionProposedBy::new(
        PlannerIdentity::new(
            PlannerAdapterId::new(adapter_id).map_err(domain_reconstruction("planner adapter"))?,
            model_ref,
        )
        .map_err(domain_reconstruction("planner identity"))?,
        PlanningCostReceiptRef::new(receipt_ref)
            .map_err(domain_reconstruction("planning cost receipt reference"))?,
    );
    let validated_by = ValidatedBy::new(service_id, policy_fingerprint)
        .map_err(domain_reconstruction("plan validation provenance"))?;
    let restored = PlanRevision::new(
        seq,
        branch.clone(),
        base,
        changeset_hash,
        reason,
        caused_by,
        proposed_by,
        validated_by,
        created_at,
    )
    .map_err(domain_reconstruction("plan revision"))?;
    if restored.revision_seq != restored.base_revision.saturating_add(1) {
        return Err(StorageError::Corrupt(format!(
            "plan revision {}:{} is not contiguous after base {}",
            branch.as_str(),
            restored.revision_seq,
            restored.base_revision
        )));
    }
    Ok(Some(restored))
}

fn load_revision_causes(
    connection: &Connection,
    branch: &PlanBranchId,
    revision_seq: u64,
) -> StorageResult<Vec<EvidenceRef>> {
    let mut statement = connection.prepare(
        "SELECT position, evidence_ref FROM plan_revision_causes
         WHERE plan_branch_id = ?1 AND revision_seq = ?2 ORDER BY position",
    )?;
    let rows = statement.query_map(
        params![branch.as_str(), sqlite_u64(revision_seq, "revision_seq")?],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
    )?;
    let mut out = Vec::new();
    for (expected, row) in (0_i64..).zip(rows) {
        let (position, raw) = row?;
        if position != expected {
            return Err(StorageError::Corrupt(format!(
                "plan revision {}:{revision_seq} stores non-contiguous cause order",
                branch.as_str()
            )));
        }
        out.push(EvidenceRef::new(raw).map_err(domain_reconstruction("plan evidence reference"))?);
    }
    Ok(out)
}

fn load_graph(
    connection: &Connection,
    branch: &PlanBranchId,
    revision_seq: u64,
) -> StorageResult<PlanGraph> {
    let revision_i64 = sqlite_u64(revision_seq, "revision_seq")?;
    let mut statement = connection.prepare(
        "SELECT position, node_id, risk_class, required_gate FROM plan_nodes
         WHERE plan_branch_id = ?1 AND revision_seq = ?2 ORDER BY position",
    )?;
    let rows = statement.query_map(params![branch.as_str(), revision_i64], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?,
        ))
    })?;
    let mut nodes = Vec::new();
    for (expected_position, row) in (0_i64..).zip(rows) {
        let (position, raw_id, raw_risk, raw_gate) = row?;
        if position != expected_position {
            return Err(StorageError::Corrupt(format!(
                "plan graph {}:{revision_seq} stores non-contiguous node order",
                branch.as_str()
            )));
        }
        let id = PlanNodeId::new(raw_id).map_err(domain_reconstruction("plan node id"))?;
        let scope = load_node_scope(connection, branch, revision_seq, &id)?;
        let criteria = load_node_criteria(connection, branch, revision_seq, &id)?;
        let risk = parse_risk(&raw_risk)?;
        let gate = raw_gate
            .map(GateRef::new)
            .transpose()
            .map_err(domain_reconstruction("required gate"))?;
        nodes.push(
            PlanNode::new(id, scope, criteria, risk, gate)
                .map_err(domain_reconstruction("plan node"))?,
        );
    }

    let node_ids: BTreeSet<&str> = nodes.iter().map(|node| node.id.as_str()).collect();
    let mut edge_statement = connection.prepare(
        "SELECT position, from_node_id, to_node_id FROM plan_edges
         WHERE plan_branch_id = ?1 AND revision_seq = ?2 ORDER BY position",
    )?;
    let rows = edge_statement.query_map(params![branch.as_str(), revision_i64], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    let mut edges = Vec::new();
    for (expected_position, row) in (0_i64..).zip(rows) {
        let (position, raw_from, raw_to) = row?;
        if position != expected_position {
            return Err(StorageError::Corrupt(format!(
                "plan graph {}:{revision_seq} stores non-contiguous edge order",
                branch.as_str()
            )));
        }
        if !node_ids.contains(raw_from.as_str()) || !node_ids.contains(raw_to.as_str()) {
            return Err(StorageError::Corrupt(format!(
                "plan graph {}:{revision_seq} stores edge with missing endpoint",
                branch.as_str()
            )));
        }
        let from = PlanNodeId::new(raw_from).map_err(domain_reconstruction("edge source id"))?;
        let to = PlanNodeId::new(raw_to).map_err(domain_reconstruction("edge target id"))?;
        if from == to {
            return Err(StorageError::Corrupt(format!(
                "plan graph {}:{revision_seq} stores a self-edge",
                branch.as_str()
            )));
        }
        edges.push((from, to));
    }
    let graph = PlanGraph::new(nodes, edges).map_err(domain_reconstruction("plan graph"))?;
    validate_graph_snapshot(&graph)?;
    Ok(graph)
}

fn load_node_scope(
    connection: &Connection,
    branch: &PlanBranchId,
    revision_seq: u64,
    node: &PlanNodeId,
) -> StorageResult<plan_domain::PlanScope> {
    let mut statement = connection.prepare(
        "SELECT position, scope_root FROM plan_node_scopes
         WHERE plan_branch_id = ?1 AND revision_seq = ?2 AND node_id = ?3 ORDER BY position",
    )?;
    let rows = statement.query_map(
        params![
            branch.as_str(),
            sqlite_u64(revision_seq, "revision_seq")?,
            node.as_str()
        ],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
    )?;
    let mut roots = Vec::new();
    for (expected, row) in (0_i64..).zip(rows) {
        let (position, root) = row?;
        if position != expected {
            return Err(StorageError::Corrupt(format!(
                "plan node {} stores non-contiguous scope order",
                node.as_str()
            )));
        }
        roots.push(root);
    }
    let scope = plan_domain::PlanScope::new(roots.clone())
        .map_err(domain_reconstruction("plan node scope"))?;
    if scope.roots() != roots {
        return Err(StorageError::Corrupt(format!(
            "plan node {} stores non-canonical or duplicate scope roots",
            node.as_str()
        )));
    }
    Ok(scope)
}

fn load_node_criteria(
    connection: &Connection,
    branch: &PlanBranchId,
    revision_seq: u64,
    node: &PlanNodeId,
) -> StorageResult<Vec<CriterionDecl>> {
    let mut statement = connection.prepare(
        "SELECT position, criterion_id, verification_method FROM plan_node_criteria
         WHERE plan_branch_id = ?1 AND revision_seq = ?2 AND node_id = ?3 ORDER BY position",
    )?;
    let rows = statement.query_map(
        params![
            branch.as_str(),
            sqlite_u64(revision_seq, "revision_seq")?,
            node.as_str()
        ],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        },
    )?;
    let mut criteria = Vec::new();
    for (expected, row) in (0_i64..).zip(rows) {
        let (position, raw_id, raw_method) = row?;
        if position != expected {
            return Err(StorageError::Corrupt(format!(
                "plan node {} stores non-contiguous criterion order",
                node.as_str()
            )));
        }
        let id = CriterionId::new(raw_id).map_err(domain_reconstruction("criterion id"))?;
        let method = VerificationMethod::from_str(&raw_method).map_err(|_| {
            StorageError::Corrupt(format!(
                "plan node {} stores unknown verification method {raw_method:?}",
                node.as_str()
            ))
        })?;
        criteria.push(CriterionDecl::new(id, method));
    }
    Ok(criteria)
}

fn validate_graph_snapshot(graph: &PlanGraph) -> StorageResult<()> {
    let ids: BTreeSet<&str> = graph.nodes.iter().map(|node| node.id.as_str()).collect();
    if ids.len() != graph.nodes.len() {
        return Err(StorageError::ConstraintViolation(
            "resulting Plan graph contains duplicate node ids".to_owned(),
        ));
    }
    let mut edges = BTreeSet::new();
    for (from, to) in &graph.edges {
        if from == to || !ids.contains(from.as_str()) || !ids.contains(to.as_str()) {
            return Err(StorageError::ConstraintViolation(
                "resulting Plan graph contains an invalid edge".to_owned(),
            ));
        }
        if !edges.insert((from.as_str(), to.as_str())) {
            return Err(StorageError::ConstraintViolation(
                "resulting Plan graph contains duplicate edges".to_owned(),
            ));
        }
    }
    if graph_has_cycle(graph) {
        return Err(StorageError::Corrupt(
            "persisted/resulting Plan graph is cyclic".to_owned(),
        ));
    }
    Ok(())
}

fn graph_has_cycle(graph: &PlanGraph) -> bool {
    let mut indegree: BTreeMap<&str, usize> = graph
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), 0_usize))
        .collect();
    let mut outgoing: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (from, to) in &graph.edges {
        *indegree.entry(to.as_str()).or_default() += 1;
        outgoing.entry(from.as_str()).or_default().push(to.as_str());
    }
    let mut ready: Vec<&str> = indegree
        .iter()
        .filter_map(|(id, degree)| (*degree == 0).then_some(*id))
        .collect();
    let mut visited = 0_usize;
    while let Some(id) = ready.pop() {
        visited += 1;
        if let Some(next) = outgoing.get(id) {
            for target in next {
                if let Some(degree) = indegree.get_mut(target) {
                    *degree -= 1;
                    if *degree == 0 {
                        ready.push(target);
                    }
                }
            }
        }
    }
    visited != graph.nodes.len()
}

fn parse_risk(raw: &str) -> StorageResult<RiskClass> {
    match raw {
        "low" => Ok(RiskClass::Low),
        "medium" => Ok(RiskClass::Medium),
        "high" => Ok(RiskClass::High),
        "critical" => Ok(RiskClass::Critical),
        other => Err(StorageError::Corrupt(format!(
            "unknown persisted plan risk class {other:?}"
        ))),
    }
}

fn risk_name(risk: RiskClass) -> &'static str {
    match risk {
        RiskClass::Low => "low",
        RiskClass::Medium => "medium",
        RiskClass::High => "high",
        RiskClass::Critical => "critical",
    }
}

fn sqlite_u64(value: u64, field: &str) -> StorageResult<i64> {
    i64::try_from(value).map_err(|_| {
        StorageError::ConstraintViolation(format!("{field} exceeds SQLite INTEGER range"))
    })
}

fn sqlite_position(value: usize) -> StorageResult<i64> {
    i64::try_from(value).map_err(|_| {
        StorageError::ConstraintViolation("position exceeds SQLite INTEGER range".to_owned())
    })
}

fn nonnegative_u64(value: i64, field: &str) -> StorageResult<u64> {
    u64::try_from(value)
        .map_err(|_| StorageError::Corrupt(format!("{field} stores negative INTEGER {value}")))
}

fn positive_u64(value: i64, field: &str) -> StorageResult<u64> {
    let value = nonnegative_u64(value, field)?;
    if value == 0 {
        return Err(StorageError::Corrupt(format!("{field} stores zero")));
    }
    Ok(value)
}

fn missing_lineage(branch: &PlanBranchId, revision_seq: u64) -> StorageError {
    StorageError::Corrupt(format!(
        "plan branch {} revision {revision_seq} has no durable guardrail lineage; \
         V7/revision-cycle guardrails cannot be reconstructed",
        branch.as_str()
    ))
}

fn domain_reconstruction(
    field: &'static str,
) -> impl FnOnce(plan_domain::PlanDomainError) -> StorageError {
    move |error| StorageError::DomainReconstruction(format!("stored {field}: {error}"))
}
