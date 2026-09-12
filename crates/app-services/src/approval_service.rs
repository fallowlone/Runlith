//! Trusted durable approval lifecycle for G6 TASK-0062.
//!
//! Only a canonical `PolicyDecision::RequireApproval` can register or consume
//! authority here. The control plane may observe and resolve a pending row,
//! but cannot mint a grant or change its binding.

use std::path::Path;

use policy::{
    ApprovalResolution, Capability, EffectiveCapabilities, HumanDecision, PolicyContractError,
    PolicyDecision, PolicyOutcome,
};
use security_audit::{
    AuditCapability, NewSecurityAuditRecord, SecurityAuditAuthor, SecurityAuditEvent,
    SecurityAuditOutcome, SecurityAuditSubject, SecurityAuditSubjectKind,
};
use storage_sqlite::{
    ApprovalCapabilityRecord, ApprovalRecord, ApprovalState, NewApprovalRecord, SqliteStore,
    StorageError,
};

use crate::{Clock, ServiceError, ServiceResult};

pub struct ApprovalService<C> {
    store: SqliteStore,
    clock: C,
}

impl<C: Clock> ApprovalService<C> {
    pub fn open(path: impl AsRef<Path>, clock: C) -> ServiceResult<Self> {
        Ok(Self::attach(SqliteStore::open(path)?, clock))
    }

    pub fn attach(store: SqliteStore, clock: C) -> Self {
        Self { store, clock }
    }

    pub fn get(&self, request_id: &str) -> ServiceResult<Option<ApprovalRecord>> {
        Ok(self.store.get_approval(request_id)?)
    }

    /// Persists the exact approval requirement produced by canonical policy.
    /// ALLOW cannot manufacture approval state; DENY remains final.
    pub fn register(&mut self, decision: &PolicyDecision) -> ServiceResult<ApprovalRecord> {
        match decision.outcome() {
            PolicyOutcome::Deny => return Err(PolicyContractError::HardDenied.into()),
            PolicyOutcome::Allow => return Err(PolicyContractError::ApprovalNotRequired.into()),
            PolicyOutcome::RequireApproval => {}
        }
        let requirement = decision
            .approval()
            .ok_or(PolicyContractError::ApprovalNotRequired)?;
        let binding = requirement.binding();
        let request = binding.request();
        let record = NewApprovalRecord {
            request_id: requirement.request_id().to_owned(),
            binding_fingerprint: requirement.binding_fingerprint()?,
            operation_id: request.operation_id().to_owned(),
            requester: request.requester().to_owned(),
            task_id: request.origin().task_id().to_owned(),
            execution_id: request.origin().execution_id().map(str::to_owned),
            attempt_id: request.origin().attempt_id().map(str::to_owned),
            policy_fingerprint: binding.policy_fingerprint().to_owned(),
            expires_at_ms: binding.expires_at_ms(),
            requested: request
                .requested()
                .as_slice()
                .iter()
                .map(capability_record)
                .collect::<Result<_, _>>()?,
            effective: requirement
                .effective_if_approved()
                .as_slice()
                .iter()
                .map(capability_record)
                .collect::<Result<_, _>>()?,
        };
        let audit = approval_audit(
            ApprovalAuditSource::from_new(&record),
            SecurityAuditEvent::ApprovalRequested,
            SecurityAuditOutcome::ApprovalRequired,
            self.clock.now_millis(),
        )?;
        Ok(self.store.create_approval_with_audit(&record, &audit)?)
    }

    /// Records one human resolution. Binding and authority are immutable;
    /// callers provide only the request id, decision and approver provenance.
    pub fn decide(
        &mut self,
        request_id: &str,
        decision: HumanDecision,
        approver: &str,
    ) -> ServiceResult<ApprovalRecord> {
        let state = match decision {
            HumanDecision::Approve => ApprovalState::Approved,
            HumanDecision::Deny => ApprovalState::Denied,
        };
        let current = self.store.get_approval(request_id)?.ok_or_else(|| {
            StorageError::NotFound(format!("approval {request_id} does not exist"))
        })?;
        let now = self.clock.now_millis();
        let audit = approval_audit(
            ApprovalAuditSource::from_record(&current),
            SecurityAuditEvent::ApprovalDecided,
            match decision {
                HumanDecision::Approve => SecurityAuditOutcome::Approved,
                HumanDecision::Deny => SecurityAuditOutcome::Denied,
            },
            now,
        )?;
        Ok(self
            .store
            .decide_approval_with_audit(request_id, state, approver, now, &audit)?)
    }

    /// Returns effective authority only after the currently evaluated policy
    /// decision exactly matches a durable approved binding, remains unexpired,
    /// and wins the atomic single-use consume step.
    pub fn authorize(&mut self, decision: &PolicyDecision) -> ServiceResult<EffectiveCapabilities> {
        if decision.outcome() == PolicyOutcome::Deny {
            return Err(PolicyContractError::HardDenied.into());
        }
        let requirement = decision
            .approval()
            .ok_or(PolicyContractError::ApprovalNotRequired)?;
        let fingerprint = requirement.binding_fingerprint()?;
        let current = self
            .store
            .get_approval(requirement.request_id())?
            .ok_or_else(|| {
                StorageError::NotFound(format!(
                    "approval {} does not exist",
                    requirement.request_id()
                ))
            })?;
        if current.binding_fingerprint != fingerprint {
            return Err(PolicyContractError::ApprovalScopeMismatch.into());
        }
        let now = self.clock.now_millis();
        if now > current.expires_at_ms {
            return Err(PolicyContractError::ApprovalExpired.into());
        }
        match current.state {
            ApprovalState::Denied => return Err(PolicyContractError::ApprovalDenied.into()),
            ApprovalState::Approved => {}
            ApprovalState::Pending | ApprovalState::Consumed => {
                return Err(ServiceError::Storage(StorageError::ConstraintViolation(
                    format!(
                        "approval {} is {} and cannot authorize",
                        requirement.request_id(),
                        current.state.as_str()
                    ),
                )))
            }
        }
        let approver = current.approver.as_deref().ok_or_else(|| {
            StorageError::Corrupt(format!(
                "approved approval {} has no approver",
                requirement.request_id()
            ))
        })?;
        let decided_at_ms = current.decided_at_ms.ok_or_else(|| {
            StorageError::Corrupt(format!(
                "approved approval {} has no decision time",
                requirement.request_id()
            ))
        })?;
        let resolution = ApprovalResolution::new(
            requirement.request_id(),
            requirement.binding().clone(),
            HumanDecision::Approve,
            approver,
            decided_at_ms,
        )?;
        let effective = decision.apply_approval(&resolution, now)?;
        let audit = approval_audit(
            ApprovalAuditSource::from_record(&current),
            SecurityAuditEvent::ApprovalDecided,
            SecurityAuditOutcome::Consumed,
            now,
        )?;
        self.store.consume_approval_with_audit(
            requirement.request_id(),
            &fingerprint,
            now,
            &audit,
        )?;
        Ok(effective)
    }
}

struct ApprovalAuditSource<'a> {
    operation_id: &'a str,
    task_id: &'a str,
    execution_id: &'a Option<String>,
    attempt_id: &'a Option<String>,
    request_id: &'a str,
    policy_fingerprint: &'a str,
    capabilities: &'a [ApprovalCapabilityRecord],
}

impl<'a> ApprovalAuditSource<'a> {
    fn from_new(record: &'a NewApprovalRecord) -> Self {
        Self {
            operation_id: &record.operation_id,
            task_id: &record.task_id,
            execution_id: &record.execution_id,
            attempt_id: &record.attempt_id,
            request_id: &record.request_id,
            policy_fingerprint: &record.policy_fingerprint,
            capabilities: &record.effective,
        }
    }

    fn from_record(record: &'a ApprovalRecord) -> Self {
        Self {
            operation_id: &record.operation_id,
            task_id: &record.task_id,
            execution_id: &record.execution_id,
            attempt_id: &record.attempt_id,
            request_id: &record.request_id,
            policy_fingerprint: &record.policy_fingerprint,
            capabilities: &record.effective,
        }
    }
}

fn approval_audit(
    source: ApprovalAuditSource<'_>,
    event: SecurityAuditEvent,
    outcome: SecurityAuditOutcome,
    occurred_at_ms: u64,
) -> ServiceResult<NewSecurityAuditRecord> {
    let capabilities = source
        .capabilities
        .iter()
        .map(|capability| {
            AuditCapability::parse(
                &capability.kind,
                capability.workspace.clone(),
                capability.scope.clone(),
            )
            .map_err(|error| StorageError::Corrupt(error.to_string()).into())
        })
        .collect::<ServiceResult<Vec<_>>>()?;
    Ok(NewSecurityAuditRecord {
        event,
        author: SecurityAuditAuthor::ApprovalService,
        outcome: Some(outcome),
        occurred_at_ms,
        correlation_id: source.operation_id.to_owned(),
        causation_id: None,
        operation_id: Some(source.operation_id.to_owned()),
        task_id: Some(source.task_id.to_owned()),
        execution_id: source.execution_id.clone(),
        attempt_id: source.attempt_id.clone(),
        subject: Some(SecurityAuditSubject {
            kind: SecurityAuditSubjectKind::Approval,
            id: source.request_id.to_owned(),
        }),
        policy_fingerprint: Some(source.policy_fingerprint.to_owned()),
        rule_id: None,
        reason_code: None,
        approval_request_id: Some(source.request_id.to_owned()),
        capabilities,
    })
}

fn capability_record(
    capability: &Capability,
) -> Result<ApprovalCapabilityRecord, PolicyContractError> {
    let (workspace, scope) = match capability {
        Capability::FilesystemRead(scope) | Capability::FilesystemWrite(scope) => (
            Some(scope.workspace().to_owned()),
            scope
                .root()
                .to_str()
                .ok_or_else(|| PolicyContractError::MalformedScope {
                    field: "filesystem_root",
                    value: scope.root().display().to_string(),
                })?
                .to_owned(),
        ),
        Capability::ProcessExecute(scope)
        | Capability::NetworkAccess(scope)
        | Capability::GitMutate(scope)
        | Capability::SecretAccess(scope)
        | Capability::ToolInvoke(scope)
        | Capability::ExternalAction(scope) => (None, scope.as_str().to_owned()),
    };
    Ok(ApprovalCapabilityRecord {
        kind: capability.kind().as_str().to_owned(),
        workspace,
        scope,
    })
}
