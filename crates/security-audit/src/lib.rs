//! Provider-neutral Security Audit contracts for G6.
//!
//! Records contain bounded structured product facts only. There is no generic
//! provider/model text, secret payload, environment dump, transcript, or log
//! field. Persistence and tamper evidence are adapter responsibilities.

use std::fmt;
use std::str::FromStr;

use policy::{Capability, CapabilityKind, FileScope, PolicyContractError, ScopeId};

pub const SECURITY_AUDIT_RECORD_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecurityAuditEvent {
    PolicyRequest,
    PolicyDecision,
    EffectiveAuthority,
    ApprovalRequested,
    ApprovalDecided,
    HumanDecisionRequested,
    HumanDecisionDecided,
    HumanDecisionApplied,
    HumanDecisionSuperseded,
    SecretAccess,
    PrivilegeElevation,
    DeniedOperation,
    IsolationEnforcement,
    IsolationViolation,
    ProviderInvocation,
    ToolInvocation,
}

impl SecurityAuditEvent {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PolicyRequest => "policy.requested",
            Self::PolicyDecision => "policy.decided",
            Self::EffectiveAuthority => "authority.effective",
            Self::ApprovalRequested => "approval.requested",
            Self::ApprovalDecided => "approval.decided",
            Self::HumanDecisionRequested => "human_decision.requested",
            Self::HumanDecisionDecided => "human_decision.decided",
            Self::HumanDecisionApplied => "human_decision.applied",
            Self::HumanDecisionSuperseded => "human_decision.superseded",
            Self::SecretAccess => "secret.access",
            Self::PrivilegeElevation => "privilege.elevation",
            Self::DeniedOperation => "operation.denied",
            Self::IsolationEnforcement => "isolation.enforced",
            Self::IsolationViolation => "isolation.violation",
            Self::ProviderInvocation => "provider.invoked",
            Self::ToolInvocation => "tool.invoked",
        }
    }

    pub fn parse(value: &str) -> Result<Self, SecurityAuditError> {
        match value {
            "policy.requested" => Ok(Self::PolicyRequest),
            "policy.decided" => Ok(Self::PolicyDecision),
            "authority.effective" => Ok(Self::EffectiveAuthority),
            "approval.requested" => Ok(Self::ApprovalRequested),
            "approval.decided" => Ok(Self::ApprovalDecided),
            "human_decision.requested" => Ok(Self::HumanDecisionRequested),
            "human_decision.decided" => Ok(Self::HumanDecisionDecided),
            "human_decision.applied" => Ok(Self::HumanDecisionApplied),
            "human_decision.superseded" => Ok(Self::HumanDecisionSuperseded),
            "secret.access" => Ok(Self::SecretAccess),
            "privilege.elevation" => Ok(Self::PrivilegeElevation),
            "operation.denied" => Ok(Self::DeniedOperation),
            "isolation.enforced" => Ok(Self::IsolationEnforcement),
            "isolation.violation" => Ok(Self::IsolationViolation),
            "provider.invoked" => Ok(Self::ProviderInvocation),
            "tool.invoked" => Ok(Self::ToolInvocation),
            _ => Err(SecurityAuditError::Corrupt("unknown audit event")),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecurityAuditAuthor {
    PolicyEngine,
    ApprovalService,
    HumanDecisionService,
    SecretsBroker,
    Runner,
    ProviderEnvironment,
    Runtime,
    ToolService,
}

impl SecurityAuditAuthor {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PolicyEngine => "policy-engine",
            Self::ApprovalService => "approval-service",
            Self::HumanDecisionService => "human-decision-service",
            Self::SecretsBroker => "secrets-broker",
            Self::Runner => "runner",
            Self::ProviderEnvironment => "provider-environment",
            Self::Runtime => "runtime",
            Self::ToolService => "tool-service",
        }
    }

    pub fn parse(value: &str) -> Result<Self, SecurityAuditError> {
        match value {
            "policy-engine" => Ok(Self::PolicyEngine),
            "approval-service" => Ok(Self::ApprovalService),
            "human-decision-service" => Ok(Self::HumanDecisionService),
            "secrets-broker" => Ok(Self::SecretsBroker),
            "runner" => Ok(Self::Runner),
            "provider-environment" => Ok(Self::ProviderEnvironment),
            "runtime" => Ok(Self::Runtime),
            "tool-service" => Ok(Self::ToolService),
            _ => Err(SecurityAuditError::Corrupt("unknown audit author")),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecurityAuditOutcome {
    Requested,
    Allowed,
    Denied,
    ApprovalRequired,
    Approved,
    Rejected,
    Resolved,
    Superseded,
    Consumed,
    Succeeded,
    Failed,
    Enforced,
    Violated,
}

impl SecurityAuditOutcome {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Requested => "requested",
            Self::Allowed => "allowed",
            Self::Denied => "denied",
            Self::ApprovalRequired => "approval_required",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
            Self::Resolved => "resolved",
            Self::Superseded => "superseded",
            Self::Consumed => "consumed",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Enforced => "enforced",
            Self::Violated => "violated",
        }
    }

    pub fn parse(value: &str) -> Result<Self, SecurityAuditError> {
        match value {
            "requested" => Ok(Self::Requested),
            "allowed" => Ok(Self::Allowed),
            "denied" => Ok(Self::Denied),
            "approval_required" => Ok(Self::ApprovalRequired),
            "approved" => Ok(Self::Approved),
            "rejected" => Ok(Self::Rejected),
            "resolved" => Ok(Self::Resolved),
            "superseded" => Ok(Self::Superseded),
            "consumed" => Ok(Self::Consumed),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "enforced" => Ok(Self::Enforced),
            "violated" => Ok(Self::Violated),
            _ => Err(SecurityAuditError::Corrupt("unknown audit outcome")),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecurityAuditSubjectKind {
    PolicyRule,
    Approval,
    HumanDecision,
    SecretRef,
    RunnerBackend,
    Provider,
    Tool,
    Operation,
}

impl SecurityAuditSubjectKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PolicyRule => "policy_rule",
            Self::Approval => "approval",
            Self::HumanDecision => "human_decision",
            Self::SecretRef => "secret_ref",
            Self::RunnerBackend => "runner_backend",
            Self::Provider => "provider",
            Self::Tool => "tool",
            Self::Operation => "operation",
        }
    }

    pub fn parse(value: &str) -> Result<Self, SecurityAuditError> {
        match value {
            "policy_rule" => Ok(Self::PolicyRule),
            "approval" => Ok(Self::Approval),
            "human_decision" => Ok(Self::HumanDecision),
            "secret_ref" => Ok(Self::SecretRef),
            "runner_backend" => Ok(Self::RunnerBackend),
            "provider" => Ok(Self::Provider),
            "tool" => Ok(Self::Tool),
            "operation" => Ok(Self::Operation),
            _ => Err(SecurityAuditError::Corrupt("unknown audit subject kind")),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SecurityAuditSubject {
    pub kind: SecurityAuditSubjectKind,
    pub id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditCapability {
    pub kind: CapabilityKind,
    pub workspace: Option<String>,
    pub scope: String,
}

impl AuditCapability {
    #[must_use]
    pub fn from_capability(capability: &Capability) -> Self {
        match capability {
            Capability::FilesystemRead(scope) | Capability::FilesystemWrite(scope) => Self {
                kind: capability.kind(),
                workspace: Some(scope.workspace().to_owned()),
                scope: scope.root().to_string_lossy().into_owned(),
            },
            Capability::ProcessExecute(scope)
            | Capability::NetworkAccess(scope)
            | Capability::GitMutate(scope)
            | Capability::SecretAccess(scope)
            | Capability::ToolInvoke(scope)
            | Capability::ExternalAction(scope) => Self {
                kind: capability.kind(),
                workspace: None,
                scope: scope.as_str().to_owned(),
            },
        }
    }

    pub fn to_capability(&self) -> Result<Capability, PolicyContractError> {
        Ok(match self.kind {
            CapabilityKind::FilesystemRead => Capability::FilesystemRead(FileScope::new(
                self.workspace
                    .clone()
                    .ok_or_else(|| PolicyContractError::MalformedScope {
                        field: "workspace",
                        value: String::new(),
                    })?,
                &self.scope,
            )?),
            CapabilityKind::FilesystemWrite => Capability::FilesystemWrite(FileScope::new(
                self.workspace
                    .clone()
                    .ok_or_else(|| PolicyContractError::MalformedScope {
                        field: "workspace",
                        value: String::new(),
                    })?,
                &self.scope,
            )?),
            CapabilityKind::ProcessExecute => {
                Capability::ProcessExecute(ScopeId::new(&self.scope)?)
            }
            CapabilityKind::NetworkAccess => Capability::NetworkAccess(ScopeId::new(&self.scope)?),
            CapabilityKind::GitMutate => Capability::GitMutate(ScopeId::new(&self.scope)?),
            CapabilityKind::SecretAccess => Capability::SecretAccess(ScopeId::new(&self.scope)?),
            CapabilityKind::ToolInvoke => Capability::ToolInvoke(ScopeId::new(&self.scope)?),
            CapabilityKind::ExternalAction => {
                Capability::ExternalAction(ScopeId::new(&self.scope)?)
            }
        })
    }

    pub fn parse(
        kind: &str,
        workspace: Option<String>,
        scope: String,
    ) -> Result<Self, SecurityAuditError> {
        let kind = CapabilityKind::from_str(kind)
            .map_err(|_| SecurityAuditError::Corrupt("unknown audit capability kind"))?;
        let capability = Self {
            kind,
            workspace,
            scope,
        };
        capability
            .to_capability()
            .map_err(|_| SecurityAuditError::Corrupt("invalid audit capability scope"))?;
        Ok(capability)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewSecurityAuditRecord {
    pub event: SecurityAuditEvent,
    pub author: SecurityAuditAuthor,
    pub outcome: Option<SecurityAuditOutcome>,
    pub occurred_at_ms: u64,
    pub correlation_id: String,
    pub causation_id: Option<String>,
    pub operation_id: Option<String>,
    pub task_id: Option<String>,
    pub execution_id: Option<String>,
    pub attempt_id: Option<String>,
    pub subject: Option<SecurityAuditSubject>,
    pub policy_fingerprint: Option<String>,
    pub rule_id: Option<String>,
    pub reason_code: Option<String>,
    pub approval_request_id: Option<String>,
    pub capabilities: Vec<AuditCapability>,
}

impl NewSecurityAuditRecord {
    pub fn validated(&self) -> Result<Self, SecurityAuditError> {
        validate_id("correlation_id", &self.correlation_id)?;
        for (name, value) in [
            ("causation_id", self.causation_id.as_deref()),
            ("operation_id", self.operation_id.as_deref()),
            ("task_id", self.task_id.as_deref()),
            ("execution_id", self.execution_id.as_deref()),
            ("attempt_id", self.attempt_id.as_deref()),
            ("policy_fingerprint", self.policy_fingerprint.as_deref()),
            ("rule_id", self.rule_id.as_deref()),
            ("reason_code", self.reason_code.as_deref()),
            ("approval_request_id", self.approval_request_id.as_deref()),
        ] {
            if let Some(value) = value {
                validate_id(name, value)?;
            }
        }
        if let Some(subject) = &self.subject {
            validate_id("subject_id", &subject.id)?;
        }
        for capability in &self.capabilities {
            capability
                .to_capability()
                .map_err(|_| SecurityAuditError::Invalid("invalid capability"))?;
        }
        validate_shape(self)?;

        let mut normalized = self.clone();
        normalized
            .capabilities
            .sort_by(|left, right| capability_key(left).cmp(&capability_key(right)));
        normalized.capabilities.dedup();
        Ok(normalized)
    }
}

fn validate_shape(record: &NewSecurityAuditRecord) -> Result<(), SecurityAuditError> {
    use SecurityAuditEvent as E;
    use SecurityAuditOutcome as O;
    use SecurityAuditSubjectKind as S;

    let require = |present: bool, field| {
        present
            .then_some(())
            .ok_or(SecurityAuditError::Missing(field))
    };
    let require_subject = |kind| {
        require(
            matches!(record.subject.as_ref(), Some(subject) if subject.kind == kind),
            "subject",
        )
    };
    match record.event {
        E::PolicyRequest => require(record.operation_id.is_some(), "operation_id")?,
        E::PolicyDecision => {
            require(record.operation_id.is_some(), "operation_id")?;
            require(record.policy_fingerprint.is_some(), "policy_fingerprint")?;
            require(record.rule_id.is_some(), "rule_id")?;
            require(record.reason_code.is_some(), "reason_code")?;
            require(
                matches!(
                    record.outcome,
                    Some(O::Allowed | O::Denied | O::ApprovalRequired)
                ),
                "policy outcome",
            )?;
        }
        E::EffectiveAuthority => require(record.operation_id.is_some(), "operation_id")?,
        E::ApprovalRequested => {
            require(record.approval_request_id.is_some(), "approval_request_id")?;
            require(
                matches!(record.outcome, Some(O::ApprovalRequired)),
                "approval outcome",
            )?;
        }
        E::ApprovalDecided => {
            require(record.approval_request_id.is_some(), "approval_request_id")?;
            require(
                matches!(
                    record.outcome,
                    Some(O::Approved | O::Denied | O::Rejected | O::Consumed)
                ),
                "approval outcome",
            )?;
        }
        E::HumanDecisionRequested => {
            require_subject(S::HumanDecision)?;
            require(record.task_id.is_some(), "task_id")?;
            require(
                matches!(record.outcome, Some(O::Requested)),
                "human decision outcome",
            )?;
        }
        E::HumanDecisionDecided => {
            require_subject(S::HumanDecision)?;
            require(record.task_id.is_some(), "task_id")?;
            require(
                matches!(record.outcome, Some(O::Resolved)),
                "human decision outcome",
            )?;
        }
        E::HumanDecisionApplied => {
            require_subject(S::HumanDecision)?;
            require(record.task_id.is_some(), "task_id")?;
            require(
                matches!(record.outcome, Some(O::Consumed)),
                "human decision outcome",
            )?;
        }
        E::HumanDecisionSuperseded => {
            require_subject(S::HumanDecision)?;
            require(record.task_id.is_some(), "task_id")?;
            require(
                matches!(record.outcome, Some(O::Superseded)),
                "human decision outcome",
            )?;
        }
        E::SecretAccess => require_subject(S::SecretRef)?,
        E::PrivilegeElevation => require(
            matches!(
                record.outcome,
                Some(O::ApprovalRequired | O::Approved | O::Denied)
            ),
            "elevation outcome",
        )?,
        E::DeniedOperation => require(matches!(record.outcome, Some(O::Denied)), "denied outcome")?,
        E::IsolationEnforcement => {
            require_subject(S::RunnerBackend)?;
            require(
                matches!(record.outcome, Some(O::Enforced | O::Failed)),
                "isolation outcome",
            )?;
        }
        E::IsolationViolation => {
            require_subject(S::RunnerBackend)?;
            require(
                matches!(record.outcome, Some(O::Violated)),
                "isolation outcome",
            )?;
        }
        E::ProviderInvocation => require_subject(S::Provider)?,
        E::ToolInvocation => require_subject(S::Tool)?,
    }
    Ok(())
}

fn capability_key(capability: &AuditCapability) -> (&str, Option<&str>, &str) {
    (
        capability.kind.as_str(),
        capability.workspace.as_deref(),
        &capability.scope,
    )
}

fn validate_id(field: &'static str, value: &str) -> Result<(), SecurityAuditError> {
    if value.trim().is_empty() || value.len() > 512 || value.chars().any(char::is_control) {
        return Err(SecurityAuditError::Invalid(field));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SecurityAuditId(i64);

impl SecurityAuditId {
    pub fn from_seq(seq: i64) -> Result<Self, SecurityAuditError> {
        if seq > 0 {
            Ok(Self(seq))
        } else {
            Err(SecurityAuditError::Corrupt("invalid audit sequence"))
        }
    }

    pub fn parse(raw: &str) -> Result<Self, SecurityAuditError> {
        raw.strip_prefix("aud-")
            .and_then(|digits| digits.parse::<i64>().ok())
            .filter(|seq| *seq > 0)
            .map(Self)
            .ok_or(SecurityAuditError::Corrupt("malformed audit identity"))
    }

    #[must_use]
    pub const fn seq(self) -> i64 {
        self.0
    }
}

impl fmt::Display for SecurityAuditId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "aud-{}", self.0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SecurityAuditRecord {
    pub id: SecurityAuditId,
    pub seq: i64,
    pub fact: NewSecurityAuditRecord,
    pub previous_hash: String,
    pub record_hash: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecurityAuditError {
    Invalid(&'static str),
    Missing(&'static str),
    Corrupt(&'static str),
}

impl fmt::Display for SecurityAuditError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(field) => write!(formatter, "invalid security audit {field}"),
            Self::Missing(field) => write!(formatter, "missing security audit {field}"),
            Self::Corrupt(detail) => write!(formatter, "corrupt security audit record: {detail}"),
        }
    }
}

impl std::error::Error for SecurityAuditError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy_record() -> NewSecurityAuditRecord {
        NewSecurityAuditRecord {
            event: SecurityAuditEvent::PolicyDecision,
            author: SecurityAuditAuthor::PolicyEngine,
            outcome: Some(SecurityAuditOutcome::Denied),
            occurred_at_ms: 1,
            correlation_id: "corr-1".into(),
            causation_id: Some("cause-1".into()),
            operation_id: Some("op-1".into()),
            task_id: Some("TASK-1".into()),
            execution_id: None,
            attempt_id: None,
            subject: Some(SecurityAuditSubject {
                kind: SecurityAuditSubjectKind::PolicyRule,
                id: "rule-1".into(),
            }),
            policy_fingerprint: Some("sha256:abc".into()),
            rule_id: Some("rule-1".into()),
            reason_code: Some("blocked".into()),
            approval_request_id: None,
            capabilities: vec![AuditCapability::from_capability(
                &Capability::process("cargo").unwrap(),
            )],
        }
    }

    #[test]
    fn policy_decision_requires_canonical_provenance_fields() {
        assert!(policy_record().validated().is_ok());
        let mut missing = policy_record();
        missing.policy_fingerprint = None;
        assert_eq!(
            missing.validated().unwrap_err(),
            SecurityAuditError::Missing("policy_fingerprint")
        );
    }

    #[test]
    fn capabilities_are_canonicalized_as_a_set() {
        let mut record = policy_record();
        record.capabilities.push(AuditCapability::from_capability(
            &Capability::process("cargo").unwrap(),
        ));
        assert_eq!(record.validated().unwrap().capabilities.len(), 1);
    }
}
