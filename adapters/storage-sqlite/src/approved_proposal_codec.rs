//! Deterministic adapter-owned codec for an approved, not-yet-published
//! `PlanChangeSet` (TASK-0082 V9 RESUME).
//!
//! This is persistence representation only. Reconstructed data remains
//! untrusted until its canonical proposal hash and all current planning
//! fences are revalidated by the application service.

use std::collections::BTreeMap;
use std::str::FromStr;

use plan_domain::{
    verify_proposal_hash, BudgetDelta, CriterionDecl, CriterionId, EdgeAddition, EdgeRemoval,
    EvidenceRef, GateRef, GateUpdate, NodeAddition, NodeUpdate, PlanChangeSet, PlanNodeId,
    PlanScope, PlannerAdapterId, PlannerIdentity, PlanningCostReceipt, RiskClass, RiskDelta,
    VerificationMethod,
};

use crate::{StorageError, StorageResult};

const CODEC_VERSION: &str = "runlith-approved-plan-changeset-v1";

pub(crate) fn encode(changeset: &PlanChangeSet) -> Vec<u8> {
    let mut out = Vec::new();
    put_str(&mut out, CODEC_VERSION);
    put_u64(&mut out, changeset.base_plan_revision);
    put_str(&mut out, &changeset.reason);
    put_strings(
        &mut out,
        changeset
            .source_evidence_refs
            .iter()
            .map(EvidenceRef::as_str),
    );

    put_len(&mut out, changeset.node_additions.len());
    for node in &changeset.node_additions {
        put_str(&mut out, node.id.as_str());
        put_strings(&mut out, node.scope.roots().iter().map(String::as_str));
        put_len(&mut out, node.criteria.len());
        for criterion in &node.criteria {
            put_str(&mut out, criterion.id.as_str());
            put_str(&mut out, criterion.method.as_str());
        }
        put_str(&mut out, node.risk_class.as_str());
        put_option_str(&mut out, node.required_gate.as_ref().map(GateRef::as_str));
    }

    put_len(&mut out, changeset.node_updates.len());
    for update in &changeset.node_updates {
        put_str(&mut out, update.target.as_str());
        match &update.scope {
            Some(scope) => {
                put_u8(&mut out, 1);
                put_strings(&mut out, scope.roots().iter().map(String::as_str));
            }
            None => put_u8(&mut out, 0),
        }
        match update.risk_class {
            Some(risk) => {
                put_u8(&mut out, 1);
                put_str(&mut out, risk.as_str());
            }
            None => put_u8(&mut out, 0),
        }
        match &update.required_gate {
            None => put_u8(&mut out, 0),
            Some(GateUpdate::Clear) => put_u8(&mut out, 1),
            Some(GateUpdate::Set(gate)) => {
                put_u8(&mut out, 2);
                put_str(&mut out, gate.as_str());
            }
        }
    }

    put_strings(
        &mut out,
        changeset.node_removals.iter().map(PlanNodeId::as_str),
    );
    put_len(&mut out, changeset.edge_additions.len());
    for edge in &changeset.edge_additions {
        put_str(&mut out, edge.from.as_str());
        put_str(&mut out, edge.to.as_str());
    }
    put_len(&mut out, changeset.edge_removals.len());
    for edge in &changeset.edge_removals {
        put_str(&mut out, edge.from.as_str());
        put_str(&mut out, edge.to.as_str());
    }
    put_str(&mut out, &changeset.expected_outcome);
    put_len(&mut out, changeset.risk_delta.len());
    for delta in &changeset.risk_delta {
        put_str(&mut out, delta.node.as_str());
        put_str(&mut out, delta.from.as_str());
        put_str(&mut out, delta.to.as_str());
    }
    put_i64(&mut out, changeset.budget_delta.planning_cost_movement);
    put_i64(
        &mut out,
        changeset.budget_delta.expected_execution_cost_movement,
    );
    put_strings(
        &mut out,
        changeset.scope_delta.roots().iter().map(String::as_str),
    );
    put_len(&mut out, changeset.coverage_map.len());
    for (parent, children) in &changeset.coverage_map {
        put_str(&mut out, parent.as_str());
        put_strings(&mut out, children.iter().map(CriterionId::as_str));
    }
    put_str(&mut out, &changeset.proposal_hash);
    put_str(&mut out, changeset.proposed_by.adapter_id.as_str());
    put_str(&mut out, &changeset.proposed_by.model_ref);
    put_u32(&mut out, changeset.planning_cost_receipt.input_tokens);
    put_u32(&mut out, changeset.planning_cost_receipt.output_tokens);
    put_u64(&mut out, changeset.planning_cost_receipt.wall_time_ms);
    put_str(&mut out, &changeset.planning_cost_receipt.price_class);
    out
}

pub(crate) fn decode(bytes: &[u8]) -> StorageResult<PlanChangeSet> {
    let mut input = Decoder::new(bytes);
    if input.string()? != CODEC_VERSION {
        return Err(StorageError::Corrupt(
            "approved PlanChangeSet has unknown codec version".to_owned(),
        ));
    }
    let base_plan_revision = input.u64()?;
    let reason = input.string()?;
    let source_evidence_refs = input
        .strings()?
        .into_iter()
        .map(EvidenceRef::new)
        .collect::<Result<Vec<_>, _>>()
        .map_err(domain)?;

    let addition_count = input.len()?;
    let mut node_additions = Vec::with_capacity(addition_count);
    for _ in 0..addition_count {
        let id = PlanNodeId::new(input.string()?).map_err(domain)?;
        let scope = PlanScope::new(input.strings()?).map_err(domain)?;
        let criterion_count = input.len()?;
        let mut criteria = Vec::with_capacity(criterion_count);
        for _ in 0..criterion_count {
            let id = CriterionId::new(input.string()?).map_err(domain)?;
            let method = VerificationMethod::from_str(&input.string()?).map_err(domain)?;
            criteria.push(CriterionDecl::new(id, method));
        }
        let risk_class = RiskClass::parse(&input.string()?).map_err(domain)?;
        let required_gate = input
            .option_string()?
            .map(GateRef::new)
            .transpose()
            .map_err(domain)?;
        node_additions.push(NodeAddition {
            id,
            scope,
            criteria,
            risk_class,
            required_gate,
        });
    }

    let update_count = input.len()?;
    let mut node_updates = Vec::with_capacity(update_count);
    for _ in 0..update_count {
        let target = PlanNodeId::new(input.string()?).map_err(domain)?;
        let scope = match input.u8()? {
            0 => None,
            1 => Some(PlanScope::new(input.strings()?).map_err(domain)?),
            value => return invalid_tag("node update scope", value),
        };
        let risk_class = match input.u8()? {
            0 => None,
            1 => Some(RiskClass::parse(&input.string()?).map_err(domain)?),
            value => return invalid_tag("node update risk", value),
        };
        let required_gate = match input.u8()? {
            0 => None,
            1 => Some(GateUpdate::Clear),
            2 => Some(GateUpdate::Set(
                GateRef::new(input.string()?).map_err(domain)?,
            )),
            value => return invalid_tag("node update gate", value),
        };
        node_updates.push(NodeUpdate {
            target,
            scope,
            risk_class,
            required_gate,
        });
    }

    let node_removals = input
        .strings()?
        .into_iter()
        .map(PlanNodeId::new)
        .collect::<Result<Vec<_>, _>>()
        .map_err(domain)?;
    let edge_addition_count = input.len()?;
    let mut edge_additions = Vec::with_capacity(edge_addition_count);
    for _ in 0..edge_addition_count {
        edge_additions.push(EdgeAddition {
            from: PlanNodeId::new(input.string()?).map_err(domain)?,
            to: PlanNodeId::new(input.string()?).map_err(domain)?,
        });
    }
    let edge_removal_count = input.len()?;
    let mut edge_removals = Vec::with_capacity(edge_removal_count);
    for _ in 0..edge_removal_count {
        edge_removals.push(EdgeRemoval {
            from: PlanNodeId::new(input.string()?).map_err(domain)?,
            to: PlanNodeId::new(input.string()?).map_err(domain)?,
        });
    }
    let expected_outcome = input.string()?;
    let risk_count = input.len()?;
    let mut risk_delta = Vec::with_capacity(risk_count);
    for _ in 0..risk_count {
        risk_delta.push(RiskDelta {
            node: PlanNodeId::new(input.string()?).map_err(domain)?,
            from: RiskClass::parse(&input.string()?).map_err(domain)?,
            to: RiskClass::parse(&input.string()?).map_err(domain)?,
        });
    }
    let budget_delta = BudgetDelta::new(input.i64()?, input.i64()?);
    let scope_delta = PlanScope::new(input.strings()?).map_err(domain)?;
    let coverage_count = input.len()?;
    let mut coverage_map = BTreeMap::new();
    for _ in 0..coverage_count {
        let parent = CriterionId::new(input.string()?).map_err(domain)?;
        let children = input
            .strings()?
            .into_iter()
            .map(CriterionId::new)
            .collect::<Result<Vec<_>, _>>()
            .map_err(domain)?;
        if coverage_map.insert(parent, children).is_some() {
            return Err(StorageError::Corrupt(
                "approved PlanChangeSet contains duplicate coverage parent".to_owned(),
            ));
        }
    }
    let proposal_hash = input.string()?;
    let proposed_by = PlannerIdentity::new(
        PlannerAdapterId::new(input.string()?).map_err(domain)?,
        input.string()?,
    )
    .map_err(domain)?;
    let planning_cost_receipt =
        PlanningCostReceipt::new(input.u32()?, input.u32()?, input.u64()?, input.string()?)
            .map_err(domain)?;
    input.finish()?;

    let changeset = PlanChangeSet {
        base_plan_revision,
        reason,
        source_evidence_refs,
        node_additions,
        node_updates,
        node_removals,
        edge_additions,
        edge_removals,
        expected_outcome,
        risk_delta,
        budget_delta,
        scope_delta,
        coverage_map,
        proposal_hash,
        proposed_by,
        planning_cost_receipt,
    };
    changeset.check_wellformed().map_err(domain)?;
    if !verify_proposal_hash(&changeset) {
        return Err(StorageError::Corrupt(
            "approved PlanChangeSet proposal_hash does not match durable bytes".to_owned(),
        ));
    }
    Ok(changeset)
}

fn domain(error: plan_domain::PlanDomainError) -> StorageError {
    StorageError::DomainReconstruction(format!(
        "approved PlanChangeSet reconstruction failed: {error}"
    ))
}

fn invalid_tag<T>(field: &str, value: u8) -> StorageResult<T> {
    Err(StorageError::Corrupt(format!(
        "approved PlanChangeSet has invalid {field} tag {value}"
    )))
}

fn put_u8(out: &mut Vec<u8>, value: u8) {
    out.push(value);
}

fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn put_i64(out: &mut Vec<u8>, value: i64) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn put_len(out: &mut Vec<u8>, value: usize) {
    put_u64(out, value as u64);
}

fn put_str(out: &mut Vec<u8>, value: &str) {
    put_len(out, value.len());
    out.extend_from_slice(value.as_bytes());
}

fn put_option_str(out: &mut Vec<u8>, value: Option<&str>) {
    match value {
        Some(value) => {
            put_u8(out, 1);
            put_str(out, value);
        }
        None => put_u8(out, 0),
    }
}

fn put_strings<'a>(out: &mut Vec<u8>, values: impl Iterator<Item = &'a str>) {
    let values = values.collect::<Vec<_>>();
    put_len(out, values.len());
    for value in values {
        put_str(out, value);
    }
}

struct Decoder<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Decoder<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, len: usize) -> StorageResult<&'a [u8]> {
        let end = self.offset.checked_add(len).ok_or_else(|| {
            StorageError::Corrupt("approved PlanChangeSet length overflow".to_owned())
        })?;
        let slice = self.bytes.get(self.offset..end).ok_or_else(|| {
            StorageError::Corrupt("approved PlanChangeSet is truncated".to_owned())
        })?;
        self.offset = end;
        Ok(slice)
    }

    fn u8(&mut self) -> StorageResult<u8> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> StorageResult<u32> {
        let bytes: [u8; 4] = self.take(4)?.try_into().map_err(|_| {
            StorageError::Corrupt("approved PlanChangeSet has bad u32 width".to_owned())
        })?;
        Ok(u32::from_be_bytes(bytes))
    }

    fn u64(&mut self) -> StorageResult<u64> {
        let bytes: [u8; 8] = self.take(8)?.try_into().map_err(|_| {
            StorageError::Corrupt("approved PlanChangeSet has bad u64 width".to_owned())
        })?;
        Ok(u64::from_be_bytes(bytes))
    }

    fn i64(&mut self) -> StorageResult<i64> {
        let bytes: [u8; 8] = self.take(8)?.try_into().map_err(|_| {
            StorageError::Corrupt("approved PlanChangeSet has bad i64 width".to_owned())
        })?;
        Ok(i64::from_be_bytes(bytes))
    }

    fn len(&mut self) -> StorageResult<usize> {
        usize::try_from(self.u64()?).map_err(|_| {
            StorageError::Corrupt("approved PlanChangeSet length exceeds host range".to_owned())
        })
    }

    fn string(&mut self) -> StorageResult<String> {
        let len = self.len()?;
        String::from_utf8(self.take(len)?.to_vec()).map_err(|_| {
            StorageError::Corrupt("approved PlanChangeSet contains invalid UTF-8".to_owned())
        })
    }

    fn strings(&mut self) -> StorageResult<Vec<String>> {
        let len = self.len()?;
        let mut values = Vec::with_capacity(len);
        for _ in 0..len {
            values.push(self.string()?);
        }
        Ok(values)
    }

    fn option_string(&mut self) -> StorageResult<Option<String>> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.string()?)),
            value => invalid_tag("optional string", value),
        }
    }

    fn finish(&self) -> StorageResult<()> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(StorageError::Corrupt(
                "approved PlanChangeSet has trailing bytes".to_owned(),
            ))
        }
    }
}
