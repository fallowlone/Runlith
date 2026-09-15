//! G6 trusted integration boundary (TASK-0067).
//!
//! Policy remains the only authorization authority. This service composes the
//! already-accepted Policy/Approval/Runner/Secrets contracts with the durable
//! Security Audit so privileged operations fail closed when authorization,
//! enforcement, or audit evidence cannot be established.

use std::fmt;
use std::path::Path;

use policy::{
    CompiledPolicy, EffectiveCapabilities, PolicyContractError, PolicyDecision, PolicyEngine,
    PolicyOutcome, PolicyRequest, ScopeId,
};
use runner::{ChildEnvironment, IsolationBackend, IsolationRequirements, RunnerError};
use secrets::{
    SecretBackend, SecretBindingId, SecretBrokerError, SecretDelivery, SecretDeliveryTarget,
    SecretMetadataStore, SecretsBroker,
};
use security_audit::{
    AuditCapability, NewSecurityAuditRecord, SecurityAuditAuthor, SecurityAuditEvent,
    SecurityAuditOutcome, SecurityAuditSubject, SecurityAuditSubjectKind,
};
use storage_sqlite::{ApprovalState, SqliteStore, StorageError};

use crate::{ApprovalService, Clock, ServiceError};

#[derive(Clone, Debug)]
pub enum G6Authorization {
    Authorized {
        decision: PolicyDecision,
        effective: EffectiveCapabilities,
    },
    ApprovalRequired {
        decision: PolicyDecision,
        request_id: String,
    },
    Denied {
        decision: PolicyDecision,
    },
}

impl G6Authorization {
    #[must_use]
    pub fn effective(&self) -> Option<&EffectiveCapabilities> {
        match self {
            Self::Authorized { effective, .. } => Some(effective),
            Self::ApprovalRequired { .. } | Self::Denied { .. } => None,
        }
    }
}

/// Correlation/provenance supplied by trusted product state for one privileged
/// operation. Provider/model text never enters this value.
#[derive(Clone, Debug)]
pub struct PrivilegedOperation {
    pub operation_id: String,
    pub task_id: String,
    pub execution_id: Option<String>,
    pub attempt_id: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ProcessSecuritySpec {
    workspace: String,
    executable: ScopeId,
    requirements: IsolationRequirements,
    environment: ChildEnvironment,
    provider: String,
}

impl ProcessSecuritySpec {
    #[must_use]
    pub fn new(
        workspace: impl Into<String>,
        executable: ScopeId,
        requirements: IsolationRequirements,
        environment: ChildEnvironment,
        provider: impl Into<String>,
    ) -> Self {
        Self {
            workspace: workspace.into(),
            executable,
            requirements,
            environment,
            provider: provider.into(),
        }
    }
}

impl PrivilegedOperation {
    #[must_use]
    pub fn from_policy_request(request: &PolicyRequest) -> Self {
        Self {
            operation_id: request.operation_id().to_owned(),
            task_id: request.origin().task_id().to_owned(),
            execution_id: request.origin().execution_id().map(str::to_owned),
            attempt_id: request.origin().attempt_id().map(str::to_owned),
        }
    }
}

#[derive(Debug)]
pub enum G6SecurityError {
    Service(ServiceError),
    Storage(StorageError),
    Policy(PolicyContractError),
    Runner(RunnerError),
    Secret(SecretBrokerError),
}

impl fmt::Display for G6SecurityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Service(error) => write!(formatter, "security service failure: {error}"),
            Self::Storage(error) => write!(formatter, "security audit failure: {error}"),
            Self::Policy(error) => write!(formatter, "policy decision failure: {error}"),
            Self::Runner(error) => write!(formatter, "runner enforcement failure: {error}"),
            Self::Secret(error) => write!(formatter, "secret release failure: {error}"),
        }
    }
}

impl std::error::Error for G6SecurityError {}

impl From<ServiceError> for G6SecurityError {
    fn from(error: ServiceError) -> Self {
        Self::Service(error)
    }
}

impl From<StorageError> for G6SecurityError {
    fn from(error: StorageError) -> Self {
        Self::Storage(error)
    }
}

impl From<PolicyContractError> for G6SecurityError {
    fn from(error: PolicyContractError) -> Self {
        Self::Policy(error)
    }
}

impl From<RunnerError> for G6SecurityError {
    fn from(error: RunnerError) -> Self {
        Self::Runner(error)
    }
}

impl From<SecretBrokerError> for G6SecurityError {
    fn from(error: SecretBrokerError) -> Self {
        Self::Secret(error)
    }
}

pub struct G6SecurityService<C> {
    audit: SqliteStore,
    approvals: ApprovalService<C>,
    clock: C,
}

impl<C: Clock + Clone> G6SecurityService<C> {
    pub fn open(path: impl AsRef<Path>, clock: C) -> Result<Self, G6SecurityError> {
        Ok(Self {
            audit: SqliteStore::open(&path)?,
            approvals: ApprovalService::open(&path, clock.clone())?,
            clock,
        })
    }

    /// Evaluates the current exact request, records product-authored policy
    /// evidence, and consumes a matching durable approval only when the
    /// canonical decision still requires that exact approval.
    pub fn authorize(
        &mut self,
        request: &PolicyRequest,
        parent: &EffectiveCapabilities,
        policy: &CompiledPolicy,
        approval_expires_at_ms: u64,
    ) -> Result<G6Authorization, G6SecurityError> {
        let operation = PrivilegedOperation::from_policy_request(request);
        self.append(NewSecurityAuditRecord {
            event: SecurityAuditEvent::PolicyRequest,
            author: SecurityAuditAuthor::PolicyEngine,
            outcome: Some(SecurityAuditOutcome::Requested),
            occurred_at_ms: self.clock.now_millis(),
            correlation_id: operation.operation_id.clone(),
            causation_id: None,
            operation_id: Some(operation.operation_id.clone()),
            task_id: Some(operation.task_id.clone()),
            execution_id: operation.execution_id.clone(),
            attempt_id: operation.attempt_id.clone(),
            subject: Some(SecurityAuditSubject {
                kind: SecurityAuditSubjectKind::Operation,
                id: operation.operation_id.clone(),
            }),
            policy_fingerprint: None,
            rule_id: None,
            reason_code: None,
            approval_request_id: None,
            capabilities: audit_capabilities(request.requested().as_slice()),
        })?;

        let decision = match PolicyEngine::evaluate(request, parent, policy, approval_expires_at_ms)
        {
            Ok(decision) => decision,
            Err(error) => {
                self.append(denied_operation(
                    &operation,
                    request.requested().as_slice(),
                    "policy_contract_rejected",
                    self.clock.now_millis(),
                ))?;
                return Err(error.into());
            }
        };
        self.append(policy_decision_record(&decision, self.clock.now_millis()))?;

        match decision.outcome() {
            PolicyOutcome::Deny => {
                self.append(denied_operation(
                    &operation,
                    request.requested().as_slice(),
                    decision.reason_code(),
                    self.clock.now_millis(),
                ))?;
                Ok(G6Authorization::Denied { decision })
            }
            PolicyOutcome::Allow => {
                let effective = decision.effective().clone();
                self.append(effective_authority_record(
                    &operation,
                    &effective,
                    self.clock.now_millis(),
                ))?;
                Ok(G6Authorization::Authorized {
                    decision,
                    effective,
                })
            }
            PolicyOutcome::RequireApproval => {
                let request_id = decision
                    .approval()
                    .expect("policy approval outcome carries requirement")
                    .request_id()
                    .to_owned();
                match self.approvals.get(&request_id)? {
                    None => {
                        self.approvals.register(&decision)?;
                        Ok(G6Authorization::ApprovalRequired {
                            decision,
                            request_id,
                        })
                    }
                    Some(record) if record.state == ApprovalState::Pending => {
                        Ok(G6Authorization::ApprovalRequired {
                            decision,
                            request_id,
                        })
                    }
                    Some(_) => {
                        let effective = match self.approvals.authorize(&decision) {
                            Ok(effective) => effective,
                            Err(error) => {
                                self.append(denied_operation(
                                    &operation,
                                    request.requested().as_slice(),
                                    "approval_refused",
                                    self.clock.now_millis(),
                                ))?;
                                return Err(error.into());
                            }
                        };
                        self.append(effective_authority_record(
                            &operation,
                            &effective,
                            self.clock.now_millis(),
                        ))?;
                        Ok(G6Authorization::Authorized {
                            decision,
                            effective,
                        })
                    }
                }
            }
        }
    }

    /// Applies the accepted Runner boundary immediately around a process
    /// operation. Isolation evidence is committed before the operation; a
    /// backend refusal records a violation and never invokes the callback.
    pub fn enforce_process_operation<T, E>(
        &mut self,
        operation: &PrivilegedOperation,
        backend: &dyn IsolationBackend,
        effective: &EffectiveCapabilities,
        spec: &ProcessSecuritySpec,
        invoke: impl FnOnce() -> Result<T, E>,
    ) -> Result<Result<T, E>, G6SecurityError> {
        let backend_identity = backend.identity();
        let effective_audit = audit_capabilities(effective.as_slice());
        let guarded = runner::enforce_process_operation(
            backend,
            effective,
            spec.workspace.clone(),
            spec.executable.clone(),
            spec.requirements,
            spec.environment.clone(),
            |_plan| -> Result<Result<T, E>, G6SecurityError> {
                self.append(NewSecurityAuditRecord {
                    event: SecurityAuditEvent::IsolationEnforcement,
                    author: SecurityAuditAuthor::Runner,
                    outcome: Some(SecurityAuditOutcome::Enforced),
                    occurred_at_ms: self.clock.now_millis(),
                    correlation_id: operation.operation_id.clone(),
                    causation_id: None,
                    operation_id: Some(operation.operation_id.clone()),
                    task_id: Some(operation.task_id.clone()),
                    execution_id: operation.execution_id.clone(),
                    attempt_id: operation.attempt_id.clone(),
                    subject: Some(SecurityAuditSubject {
                        kind: SecurityAuditSubjectKind::RunnerBackend,
                        id: backend_identity.name.to_owned(),
                    }),
                    policy_fingerprint: None,
                    rule_id: None,
                    reason_code: None,
                    approval_request_id: None,
                    capabilities: effective_audit.clone(),
                })?;
                self.append(provider_record(
                    operation,
                    &spec.provider,
                    SecurityAuditOutcome::Requested,
                    self.clock.now_millis(),
                ))?;
                let result = invoke();
                self.append(provider_record(
                    operation,
                    &spec.provider,
                    if result.is_ok() {
                        SecurityAuditOutcome::Succeeded
                    } else {
                        SecurityAuditOutcome::Failed
                    },
                    self.clock.now_millis(),
                ))?;
                Ok(result)
            },
        );
        match guarded {
            Ok(result) => result,
            Err(error) => {
                self.append(NewSecurityAuditRecord {
                    event: SecurityAuditEvent::IsolationViolation,
                    author: SecurityAuditAuthor::Runner,
                    outcome: Some(SecurityAuditOutcome::Violated),
                    occurred_at_ms: self.clock.now_millis(),
                    correlation_id: operation.operation_id.clone(),
                    causation_id: None,
                    operation_id: Some(operation.operation_id.clone()),
                    task_id: Some(operation.task_id.clone()),
                    execution_id: operation.execution_id.clone(),
                    attempt_id: operation.attempt_id.clone(),
                    subject: Some(SecurityAuditSubject {
                        kind: SecurityAuditSubjectKind::RunnerBackend,
                        id: backend_identity.name.to_owned(),
                    }),
                    policy_fingerprint: None,
                    rule_id: None,
                    reason_code: Some("runner_enforcement_refused".to_owned()),
                    approval_request_id: None,
                    capabilities: effective_audit,
                })?;
                Err(error.into())
            }
        }
    }

    /// Secret access integration for trusted execution/tool owners. A durable
    /// requested record is committed before backend material is loaded; only
    /// opaque SecretRef identity enters the audit.
    pub fn release_secret<M, B>(
        &mut self,
        operation: &PrivilegedOperation,
        broker: &SecretsBroker<M, B>,
        binding_id: &SecretBindingId,
        target: &SecretDeliveryTarget,
        effective: &EffectiveCapabilities,
        delivery: &mut dyn SecretDelivery,
    ) -> Result<(), G6SecurityError>
    where
        M: SecretMetadataStore,
        B: SecretBackend,
    {
        let binding = broker.binding(binding_id)?;
        let subject = SecurityAuditSubject {
            kind: SecurityAuditSubjectKind::SecretRef,
            id: binding.secret().as_str().to_owned(),
        };
        let capability = policy::Capability::secret(binding.secret().as_str())?;
        self.append(secret_record(
            operation,
            subject.clone(),
            &capability,
            SecurityAuditOutcome::Requested,
            self.clock.now_millis(),
        ))?;
        match broker.release(binding_id, target, effective, delivery) {
            Ok(()) => {
                self.append(secret_record(
                    operation,
                    subject,
                    &capability,
                    SecurityAuditOutcome::Succeeded,
                    self.clock.now_millis(),
                ))?;
                Ok(())
            }
            Err(error) => {
                let outcome = match error {
                    SecretBrokerError::Unauthorized | SecretBrokerError::BindingTargetMismatch => {
                        SecurityAuditOutcome::Denied
                    }
                    _ => SecurityAuditOutcome::Failed,
                };
                self.append(secret_record(
                    operation,
                    subject,
                    &capability,
                    outcome,
                    self.clock.now_millis(),
                ))?;
                Err(error.into())
            }
        }
    }

    pub fn list_audit_ordered(
        &self,
    ) -> Result<Vec<security_audit::SecurityAuditRecord>, G6SecurityError> {
        Ok(self.audit.list_security_audit_ordered()?)
    }

    fn append(&mut self, record: NewSecurityAuditRecord) -> Result<(), G6SecurityError> {
        self.audit.append_security_audit(&record)?;
        Ok(())
    }
}

fn audit_capabilities(capabilities: &[policy::Capability]) -> Vec<AuditCapability> {
    capabilities
        .iter()
        .map(AuditCapability::from_capability)
        .collect()
}

fn policy_decision_record(decision: &PolicyDecision, now: u64) -> NewSecurityAuditRecord {
    let request = decision.request();
    NewSecurityAuditRecord {
        event: SecurityAuditEvent::PolicyDecision,
        author: SecurityAuditAuthor::PolicyEngine,
        outcome: Some(match decision.outcome() {
            PolicyOutcome::Allow => SecurityAuditOutcome::Allowed,
            PolicyOutcome::Deny => SecurityAuditOutcome::Denied,
            PolicyOutcome::RequireApproval => SecurityAuditOutcome::ApprovalRequired,
        }),
        occurred_at_ms: now,
        correlation_id: request.operation_id().to_owned(),
        causation_id: None,
        operation_id: Some(request.operation_id().to_owned()),
        task_id: Some(request.origin().task_id().to_owned()),
        execution_id: request.origin().execution_id().map(str::to_owned),
        attempt_id: request.origin().attempt_id().map(str::to_owned),
        subject: Some(SecurityAuditSubject {
            kind: SecurityAuditSubjectKind::PolicyRule,
            id: decision.rule_id().to_owned(),
        }),
        policy_fingerprint: decision.policy_fingerprint().map(str::to_owned),
        rule_id: Some(decision.rule_id().to_owned()),
        reason_code: Some(decision.reason_code().to_owned()),
        approval_request_id: decision
            .approval()
            .map(|approval| approval.request_id().to_owned()),
        capabilities: audit_capabilities(request.requested().as_slice()),
    }
}

fn effective_authority_record(
    operation: &PrivilegedOperation,
    effective: &EffectiveCapabilities,
    now: u64,
) -> NewSecurityAuditRecord {
    NewSecurityAuditRecord {
        event: SecurityAuditEvent::EffectiveAuthority,
        author: SecurityAuditAuthor::PolicyEngine,
        outcome: Some(SecurityAuditOutcome::Allowed),
        occurred_at_ms: now,
        correlation_id: operation.operation_id.clone(),
        causation_id: None,
        operation_id: Some(operation.operation_id.clone()),
        task_id: Some(operation.task_id.clone()),
        execution_id: operation.execution_id.clone(),
        attempt_id: operation.attempt_id.clone(),
        subject: Some(SecurityAuditSubject {
            kind: SecurityAuditSubjectKind::Operation,
            id: operation.operation_id.clone(),
        }),
        policy_fingerprint: None,
        rule_id: None,
        reason_code: None,
        approval_request_id: None,
        capabilities: audit_capabilities(effective.as_slice()),
    }
}

fn denied_operation(
    operation: &PrivilegedOperation,
    capabilities: &[policy::Capability],
    reason_code: &str,
    now: u64,
) -> NewSecurityAuditRecord {
    NewSecurityAuditRecord {
        event: SecurityAuditEvent::DeniedOperation,
        author: SecurityAuditAuthor::PolicyEngine,
        outcome: Some(SecurityAuditOutcome::Denied),
        occurred_at_ms: now,
        correlation_id: operation.operation_id.clone(),
        causation_id: None,
        operation_id: Some(operation.operation_id.clone()),
        task_id: Some(operation.task_id.clone()),
        execution_id: operation.execution_id.clone(),
        attempt_id: operation.attempt_id.clone(),
        subject: Some(SecurityAuditSubject {
            kind: SecurityAuditSubjectKind::Operation,
            id: operation.operation_id.clone(),
        }),
        policy_fingerprint: None,
        rule_id: None,
        reason_code: Some(reason_code.to_owned()),
        approval_request_id: None,
        capabilities: audit_capabilities(capabilities),
    }
}

fn secret_record(
    operation: &PrivilegedOperation,
    subject: SecurityAuditSubject,
    capability: &policy::Capability,
    outcome: SecurityAuditOutcome,
    now: u64,
) -> NewSecurityAuditRecord {
    NewSecurityAuditRecord {
        event: SecurityAuditEvent::SecretAccess,
        author: SecurityAuditAuthor::SecretsBroker,
        outcome: Some(outcome),
        occurred_at_ms: now,
        correlation_id: operation.operation_id.clone(),
        causation_id: None,
        operation_id: Some(operation.operation_id.clone()),
        task_id: Some(operation.task_id.clone()),
        execution_id: operation.execution_id.clone(),
        attempt_id: operation.attempt_id.clone(),
        subject: Some(subject),
        policy_fingerprint: None,
        rule_id: None,
        reason_code: None,
        approval_request_id: None,
        capabilities: vec![AuditCapability::from_capability(capability)],
    }
}

fn provider_record(
    operation: &PrivilegedOperation,
    provider: &str,
    outcome: SecurityAuditOutcome,
    now: u64,
) -> NewSecurityAuditRecord {
    NewSecurityAuditRecord {
        event: SecurityAuditEvent::ProviderInvocation,
        author: SecurityAuditAuthor::Runtime,
        outcome: Some(outcome),
        occurred_at_ms: now,
        correlation_id: operation.operation_id.clone(),
        causation_id: None,
        operation_id: Some(operation.operation_id.clone()),
        task_id: Some(operation.task_id.clone()),
        execution_id: operation.execution_id.clone(),
        attempt_id: operation.attempt_id.clone(),
        subject: Some(SecurityAuditSubject {
            kind: SecurityAuditSubjectKind::Provider,
            id: provider.to_owned(),
        }),
        policy_fingerprint: None,
        rule_id: None,
        reason_code: None,
        approval_request_id: None,
        capabilities: Vec::new(),
    }
}
