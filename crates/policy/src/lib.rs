//! Provider-neutral G6 security authority contracts.
//!
//! This crate owns no persistence, UI, provider behavior, or OS sandboxing.
//! It only defines deterministic values used to request, narrow, decide and
//! audit authority. Enforcement remains at the operation boundary.

use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;

mod engine;
mod review_authority;
mod risk_signals;
mod selector_catalog;
mod test_mutation;

pub use engine::{CompiledPolicy, PolicyEngine, PolicyLayer, PolicyRule, PolicyRuleEvidence};
pub use review_authority::{
    HumanPrincipal, ImplementationPrincipal, ReviewAuthorityError, ReviewPath, ReviewPathSetting,
    ReviewVerdict,
};
pub use risk_signals::{classify_risk_signals, RiskAdvisory, RiskSignal};
pub use selector_catalog::{
    BoundCandidateParams, CandidateSelectorCatalog, CandidateSelectorCatalogRevision,
    SelectorCatalogError, SELECTOR_FILE_CONTAINS_V1,
};
pub use test_mutation::{TestMutationPolicy, TestMutationPolicyError, TestMutationPolicyRevision};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CapabilityKind {
    FilesystemRead,
    FilesystemWrite,
    ProcessExecute,
    NetworkAccess,
    GitMutate,
    SecretAccess,
    ToolInvoke,
    ExternalAction,
}

impl FromStr for CapabilityKind {
    type Err = PolicyContractError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "filesystem.read" => Ok(Self::FilesystemRead),
            "filesystem.write" => Ok(Self::FilesystemWrite),
            "process.execute" => Ok(Self::ProcessExecute),
            "network.access" => Ok(Self::NetworkAccess),
            "git.mutate" => Ok(Self::GitMutate),
            "secret.access" => Ok(Self::SecretAccess),
            "tool.invoke" => Ok(Self::ToolInvoke),
            "external.action" => Ok(Self::ExternalAction),
            other => Err(PolicyContractError::UnknownCapability(other.to_owned())),
        }
    }
}

impl CapabilityKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FilesystemRead => "filesystem.read",
            Self::FilesystemWrite => "filesystem.write",
            Self::ProcessExecute => "process.execute",
            Self::NetworkAccess => "network.access",
            Self::GitMutate => "git.mutate",
            Self::SecretAccess => "secret.access",
            Self::ToolInvoke => "tool.invoke",
            Self::ExternalAction => "external.action",
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct FileScope {
    workspace: String,
    root: PathBuf,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ScopeId(String);

impl ScopeId {
    pub fn new(value: impl Into<String>) -> Result<Self, PolicyContractError> {
        Ok(Self(checked_id("scope_id", value.into())?))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FileScope {
    pub fn new(
        workspace: impl Into<String>,
        root: impl AsRef<Path>,
    ) -> Result<Self, PolicyContractError> {
        let workspace = checked_id("workspace", workspace.into())?;
        let root = normalize_relative_path(root.as_ref())?;
        // Ensure the serialized scope (root.to_string_lossy()) would pass the
        // audit schema CHECK `length(trim(scope)) > 0`. Scoped file names are
        // attacker-controlled (in live worktree scans), so refuse any root whose
        // trimmed string representation is empty.
        let scope_serialized = root.to_string_lossy();
        if scope_serialized.trim().is_empty() {
            return Err(PolicyContractError::MalformedScope {
                field: "filesystem_root",
                value: scope_serialized.into_owned(),
            });
        }
        Ok(Self { workspace, root })
    }

    pub fn workspace(&self) -> &str {
        &self.workspace
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn is_subset_of(&self, parent: &Self) -> bool {
        self.workspace == parent.workspace && path_is_within(&self.root, &parent.root)
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum Capability {
    FilesystemRead(FileScope),
    FilesystemWrite(FileScope),
    ProcessExecute(ScopeId),
    NetworkAccess(ScopeId),
    GitMutate(ScopeId),
    SecretAccess(ScopeId),
    ToolInvoke(ScopeId),
    ExternalAction(ScopeId),
}

impl Capability {
    pub fn process(executable: impl Into<String>) -> Result<Self, PolicyContractError> {
        Ok(Self::ProcessExecute(ScopeId::new(executable)?))
    }

    pub fn network(host: impl Into<String>) -> Result<Self, PolicyContractError> {
        Ok(Self::NetworkAccess(ScopeId::new(host)?))
    }

    pub fn git(repository: impl Into<String>) -> Result<Self, PolicyContractError> {
        Ok(Self::GitMutate(ScopeId::new(repository)?))
    }

    pub fn secret(secret: impl Into<String>) -> Result<Self, PolicyContractError> {
        Ok(Self::SecretAccess(ScopeId::new(secret)?))
    }

    pub fn tool(tool: impl Into<String>) -> Result<Self, PolicyContractError> {
        Ok(Self::ToolInvoke(ScopeId::new(tool)?))
    }

    pub fn external_action(action: impl Into<String>) -> Result<Self, PolicyContractError> {
        Ok(Self::ExternalAction(ScopeId::new(action)?))
    }

    pub fn kind(&self) -> CapabilityKind {
        match self {
            Self::FilesystemRead(_) => CapabilityKind::FilesystemRead,
            Self::FilesystemWrite(_) => CapabilityKind::FilesystemWrite,
            Self::ProcessExecute(_) => CapabilityKind::ProcessExecute,
            Self::NetworkAccess(_) => CapabilityKind::NetworkAccess,
            Self::GitMutate(_) => CapabilityKind::GitMutate,
            Self::SecretAccess(_) => CapabilityKind::SecretAccess,
            Self::ToolInvoke(_) => CapabilityKind::ToolInvoke,
            Self::ExternalAction(_) => CapabilityKind::ExternalAction,
        }
    }

    pub fn is_subset_of(&self, parent: &Self) -> bool {
        match (self, parent) {
            (Self::FilesystemRead(child), Self::FilesystemRead(parent))
            | (Self::FilesystemWrite(child), Self::FilesystemWrite(parent)) => {
                child.is_subset_of(parent)
            }
            (Self::ProcessExecute(child), Self::ProcessExecute(parent))
            | (Self::NetworkAccess(child), Self::NetworkAccess(parent))
            | (Self::GitMutate(child), Self::GitMutate(parent))
            | (Self::SecretAccess(child), Self::SecretAccess(parent))
            | (Self::ToolInvoke(child), Self::ToolInvoke(parent))
            | (Self::ExternalAction(child), Self::ExternalAction(parent)) => child == parent,
            _ => false,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RequestedCapabilities(Vec<Capability>);

impl RequestedCapabilities {
    pub fn new(capabilities: Vec<Capability>) -> Self {
        Self(dedupe(capabilities))
    }

    pub fn as_slice(&self) -> &[Capability] {
        &self.0
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EffectiveCapabilities(Vec<Capability>);

impl EffectiveCapabilities {
    pub fn new(capabilities: Vec<Capability>) -> Self {
        Self(dedupe(capabilities))
    }

    pub fn none() -> Self {
        Self::default()
    }

    pub fn as_slice(&self) -> &[Capability] {
        &self.0
    }

    pub fn permits(&self, requested: &Capability) -> bool {
        self.0.iter().any(|parent| requested.is_subset_of(parent))
    }

    pub fn narrow(&self, requested: RequestedCapabilities) -> Result<Self, PolicyContractError> {
        for capability in requested.as_slice() {
            if !self.permits(capability) {
                return Err(PolicyContractError::AuthorityExpansion(capability.clone()));
            }
        }
        Ok(Self::new(requested.0))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestOrigin {
    task_id: String,
    execution_id: Option<String>,
    attempt_id: Option<String>,
}

impl RequestOrigin {
    pub fn new(
        task_id: impl Into<String>,
        execution_id: Option<String>,
        attempt_id: Option<String>,
    ) -> Result<Self, PolicyContractError> {
        Ok(Self {
            task_id: checked_id("task_id", task_id.into())?,
            execution_id: checked_optional_id("execution_id", execution_id)?,
            attempt_id: checked_optional_id("attempt_id", attempt_id)?,
        })
    }

    pub fn task_id(&self) -> &str {
        &self.task_id
    }

    pub fn execution_id(&self) -> Option<&str> {
        self.execution_id.as_deref()
    }

    pub fn attempt_id(&self) -> Option<&str> {
        self.attempt_id.as_deref()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PolicyRequest {
    operation_id: String,
    requester: String,
    origin: RequestOrigin,
    requested: RequestedCapabilities,
}

impl PolicyRequest {
    pub fn new(
        operation_id: impl Into<String>,
        requester: impl Into<String>,
        origin: RequestOrigin,
        requested: RequestedCapabilities,
    ) -> Result<Self, PolicyContractError> {
        Ok(Self {
            operation_id: checked_id("operation_id", operation_id.into())?,
            requester: checked_id("requester", requester.into())?,
            origin,
            requested,
        })
    }

    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }

    pub fn requester(&self) -> &str {
        &self.requester
    }

    pub fn origin(&self) -> &RequestOrigin {
        &self.origin
    }

    pub fn requested(&self) -> &RequestedCapabilities {
        &self.requested
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyOutcome {
    Allow,
    Deny,
    RequireApproval,
}

impl PolicyOutcome {
    pub fn most_restrictive(self, other: Self) -> Self {
        use PolicyOutcome::{Allow, Deny, RequireApproval};
        match (self, other) {
            (Deny, _) | (_, Deny) => Deny,
            (RequireApproval, _) | (_, RequireApproval) => RequireApproval,
            (Allow, Allow) => Allow,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApprovalBinding {
    request: PolicyRequest,
    policy_fingerprint: String,
    expires_at_ms: u64,
}

impl ApprovalBinding {
    pub fn new(
        request: PolicyRequest,
        policy_fingerprint: impl Into<String>,
        expires_at_ms: u64,
    ) -> Result<Self, PolicyContractError> {
        Ok(Self {
            request,
            policy_fingerprint: checked_id("policy_fingerprint", policy_fingerprint.into())?,
            expires_at_ms,
        })
    }

    pub fn request(&self) -> &PolicyRequest {
        &self.request
    }

    pub fn policy_fingerprint(&self) -> &str {
        &self.policy_fingerprint
    }

    pub fn expires_at_ms(&self) -> u64 {
        self.expires_at_ms
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApprovalRequirement {
    request_id: String,
    binding: ApprovalBinding,
    effective_if_approved: EffectiveCapabilities,
}

impl ApprovalRequirement {
    pub fn new(
        request_id: impl Into<String>,
        binding: ApprovalBinding,
        effective_if_approved: EffectiveCapabilities,
    ) -> Result<Self, PolicyContractError> {
        ensure_effective_within_request(&binding.request.requested, &effective_if_approved)?;
        Ok(Self {
            request_id: checked_id("approval_request_id", request_id.into())?,
            binding,
            effective_if_approved,
        })
    }

    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    pub fn binding(&self) -> &ApprovalBinding {
        &self.binding
    }

    pub fn effective_if_approved(&self) -> &EffectiveCapabilities {
        &self.effective_if_approved
    }

    /// Stable exact-binding identity used by durable approval state.
    /// Secret capabilities contribute only their opaque reference id.
    pub fn binding_fingerprint(&self) -> Result<String, PolicyContractError> {
        engine::approval_requirement_fingerprint(self)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HumanDecision {
    Approve,
    Deny,
}

impl HumanDecision {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Approve => "approve",
            Self::Deny => "deny",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApprovalResolution {
    request_id: String,
    binding: ApprovalBinding,
    decision: HumanDecision,
    approver: String,
    decided_at_ms: u64,
}

impl ApprovalResolution {
    pub fn new(
        request_id: impl Into<String>,
        binding: ApprovalBinding,
        decision: HumanDecision,
        approver: impl Into<String>,
        decided_at_ms: u64,
    ) -> Result<Self, PolicyContractError> {
        Ok(Self {
            request_id: checked_id("approval_request_id", request_id.into())?,
            binding,
            decision,
            approver: checked_id("approver", approver.into())?,
            decided_at_ms,
        })
    }

    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    pub fn binding(&self) -> &ApprovalBinding {
        &self.binding
    }

    pub fn decision(&self) -> HumanDecision {
        self.decision
    }

    pub fn approver(&self) -> &str {
        &self.approver
    }

    pub fn decided_at_ms(&self) -> u64 {
        self.decided_at_ms
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PolicyDecision {
    outcome: PolicyOutcome,
    rule_id: String,
    reason_code: String,
    request: PolicyRequest,
    effective: EffectiveCapabilities,
    approval: Option<ApprovalRequirement>,
    policy_fingerprint: Option<String>,
    rule_evidence: Vec<PolicyRuleEvidence>,
}

impl PolicyDecision {
    pub fn allow(
        request: PolicyRequest,
        effective: EffectiveCapabilities,
        rule_id: impl Into<String>,
        reason_code: impl Into<String>,
    ) -> Result<Self, PolicyContractError> {
        ensure_effective_within_request(&request.requested, &effective)?;
        Ok(Self {
            outcome: PolicyOutcome::Allow,
            rule_id: checked_id("rule_id", rule_id.into())?,
            reason_code: checked_id("reason_code", reason_code.into())?,
            request,
            effective,
            approval: None,
            policy_fingerprint: None,
            rule_evidence: Vec::new(),
        })
    }

    pub fn deny(
        request: PolicyRequest,
        rule_id: impl Into<String>,
        reason_code: impl Into<String>,
    ) -> Result<Self, PolicyContractError> {
        Ok(Self {
            outcome: PolicyOutcome::Deny,
            rule_id: checked_id("rule_id", rule_id.into())?,
            reason_code: checked_id("reason_code", reason_code.into())?,
            request,
            effective: EffectiveCapabilities::none(),
            approval: None,
            policy_fingerprint: None,
            rule_evidence: Vec::new(),
        })
    }

    pub fn require_approval(
        request: PolicyRequest,
        approval: ApprovalRequirement,
        rule_id: impl Into<String>,
        reason_code: impl Into<String>,
    ) -> Result<Self, PolicyContractError> {
        if approval.binding.request != request {
            return Err(PolicyContractError::ApprovalScopeMismatch);
        }
        Ok(Self {
            outcome: PolicyOutcome::RequireApproval,
            rule_id: checked_id("rule_id", rule_id.into())?,
            reason_code: checked_id("reason_code", reason_code.into())?,
            request,
            effective: EffectiveCapabilities::none(),
            approval: Some(approval),
            policy_fingerprint: None,
            rule_evidence: Vec::new(),
        })
    }

    pub fn apply_approval(
        &self,
        resolution: &ApprovalResolution,
        now_ms: u64,
    ) -> Result<EffectiveCapabilities, PolicyContractError> {
        if self.outcome == PolicyOutcome::Deny {
            return Err(PolicyContractError::HardDenied);
        }
        let requirement = self
            .approval
            .as_ref()
            .ok_or(PolicyContractError::ApprovalNotRequired)?;
        if requirement.request_id != resolution.request_id
            || requirement.binding != resolution.binding
        {
            return Err(PolicyContractError::ApprovalScopeMismatch);
        }
        if now_ms > requirement.binding.expires_at_ms {
            return Err(PolicyContractError::ApprovalExpired);
        }
        if resolution.decision != HumanDecision::Approve {
            return Err(PolicyContractError::ApprovalDenied);
        }
        Ok(requirement.effective_if_approved.clone())
    }

    pub fn outcome(&self) -> PolicyOutcome {
        self.outcome
    }

    pub fn rule_id(&self) -> &str {
        &self.rule_id
    }

    pub fn reason_code(&self) -> &str {
        &self.reason_code
    }

    pub fn request(&self) -> &PolicyRequest {
        &self.request
    }

    pub fn effective(&self) -> &EffectiveCapabilities {
        &self.effective
    }

    pub fn approval(&self) -> Option<&ApprovalRequirement> {
        self.approval.as_ref()
    }

    pub fn policy_fingerprint(&self) -> Option<&str> {
        self.policy_fingerprint.as_deref()
    }

    pub fn rule_evidence(&self) -> &[PolicyRuleEvidence] {
        &self.rule_evidence
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecurityAuditEventKind {
    PolicyRequested,
    PolicyDecided,
    ApprovalRequested,
    ApprovalDecided,
    SecretAccessed,
    PrivilegeElevation,
    OperationDenied,
    IsolationViolation,
    PrivilegedInvocation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SecurityAuditRecord {
    pub kind: SecurityAuditEventKind,
    pub operation_id: String,
    pub request: Option<PolicyRequest>,
    pub outcome: Option<PolicyOutcome>,
    pub rule_id: Option<String>,
    pub reason_code: Option<String>,
    pub effective: EffectiveCapabilities,
    pub approval_request_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PolicyContractError {
    UnknownCapability(String),
    MalformedScope { field: &'static str, value: String },
    DuplicateRuleId(String),
    AuthorityExpansion(Capability),
    EffectiveAuthorityOutsideRequest(Capability),
    ApprovalScopeMismatch,
    ApprovalExpired,
    ApprovalDenied,
    ApprovalNotRequired,
    HardDenied,
    InvalidTestMutationPolicy(String),
    InvalidReviewAuthority(String),
    InvalidSelectorCatalog(String),
}

impl fmt::Display for PolicyContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for PolicyContractError {}

fn ensure_effective_within_request(
    requested: &RequestedCapabilities,
    effective: &EffectiveCapabilities,
) -> Result<(), PolicyContractError> {
    for capability in effective.as_slice() {
        if !requested
            .as_slice()
            .iter()
            .any(|requested| capability.is_subset_of(requested))
        {
            return Err(PolicyContractError::EffectiveAuthorityOutsideRequest(
                capability.clone(),
            ));
        }
    }
    Ok(())
}

fn checked_optional_id(
    field: &'static str,
    value: Option<String>,
) -> Result<Option<String>, PolicyContractError> {
    value.map(|value| checked_id(field, value)).transpose()
}

fn checked_id(field: &'static str, value: String) -> Result<String, PolicyContractError> {
    if value.trim().is_empty() || value.chars().any(char::is_control) {
        return Err(PolicyContractError::MalformedScope { field, value });
    }
    Ok(value)
}

fn normalize_relative_path(path: &Path) -> Result<PathBuf, PolicyContractError> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(component) => normalized.push(component),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(PolicyContractError::MalformedScope {
                    field: "filesystem_root",
                    value: path.display().to_string(),
                });
            }
        }
    }
    if normalized.as_os_str().is_empty() {
        Ok(PathBuf::from("."))
    } else {
        Ok(normalized)
    }
}

fn path_is_within(child: &Path, parent: &Path) -> bool {
    if parent == Path::new(".") {
        return true;
    }
    child.starts_with(parent)
}

fn dedupe(capabilities: Vec<Capability>) -> Vec<Capability> {
    let mut unique = Vec::with_capacity(capabilities.len());
    for capability in capabilities {
        if !unique.contains(&capability) {
            unique.push(capability);
        }
    }
    unique
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file_write(path: &str) -> Capability {
        Capability::FilesystemWrite(FileScope::new("ws-1", path).unwrap())
    }

    fn request(capabilities: Vec<Capability>) -> PolicyRequest {
        PolicyRequest::new(
            "op-1",
            "agent-runtime",
            RequestOrigin::new("T-1", Some("E-1".into()), Some("A-1".into())).unwrap(),
            RequestedCapabilities::new(capabilities),
        )
        .unwrap()
    }

    #[test]
    fn policy_decision_represents_all_three_outcomes() {
        let requested = request(vec![file_write("src")]);
        let effective = EffectiveCapabilities::new(vec![file_write("src")]);
        let allow =
            PolicyDecision::allow(requested.clone(), effective, "allow.workspace", "ok").unwrap();
        let deny = PolicyDecision::deny(requested.clone(), "hard.no-write", "hard_deny").unwrap();
        let binding = ApprovalBinding::new(requested.clone(), "policy-v1", 100).unwrap();
        let approval = ApprovalRequirement::new(
            "approval-1",
            binding,
            EffectiveCapabilities::new(vec![file_write("src")]),
        )
        .unwrap();
        let needs_approval = PolicyDecision::require_approval(
            requested,
            approval,
            "approval.write",
            "human_required",
        )
        .unwrap();

        assert_eq!(allow.outcome, PolicyOutcome::Allow);
        assert_eq!(deny.outcome, PolicyOutcome::Deny);
        assert_eq!(needs_approval.outcome, PolicyOutcome::RequireApproval);
    }

    #[test]
    fn child_equal_authority_is_allowed() {
        let parent = EffectiveCapabilities::new(vec![file_write("src")]);
        assert_eq!(
            parent
                .narrow(RequestedCapabilities::new(vec![file_write("src")]))
                .unwrap(),
            parent
        );
    }

    #[test]
    fn child_strict_subset_is_allowed() {
        let parent = EffectiveCapabilities::new(vec![file_write("src")]);
        let child = parent
            .narrow(RequestedCapabilities::new(vec![file_write("src/api")]))
            .unwrap();
        assert!(child.permits(&file_write("src/api/routes")));
        assert!(!child.permits(&file_write("src/web")));
    }

    #[test]
    fn child_scope_widening_is_refused() {
        let parent = EffectiveCapabilities::new(vec![file_write("src/api")]);
        assert!(matches!(
            parent.narrow(RequestedCapabilities::new(vec![file_write("src")])),
            Err(PolicyContractError::AuthorityExpansion(_))
        ));
    }

    #[test]
    fn child_unrelated_capability_is_refused() {
        let parent = EffectiveCapabilities::new(vec![file_write("src")]);
        assert!(matches!(
            parent.narrow(RequestedCapabilities::new(vec![Capability::network(
                "example.com"
            )
            .unwrap()])),
            Err(PolicyContractError::AuthorityExpansion(_))
        ));
    }

    #[test]
    fn provider_request_cannot_broaden_effective_authority() {
        let parent = EffectiveCapabilities::new(vec![Capability::process("cargo").unwrap()]);
        let provider_request = RequestedCapabilities::new(vec![
            Capability::process("cargo").unwrap(),
            Capability::network("example.com").unwrap(),
        ]);
        assert!(matches!(
            parent.narrow(provider_request),
            Err(PolicyContractError::AuthorityExpansion(_))
        ));
    }

    #[test]
    fn child_cannot_cross_workspace_scope() {
        let parent = EffectiveCapabilities::new(vec![Capability::FilesystemWrite(
            FileScope::new("ws-1", "src").unwrap(),
        )]);
        let child = RequestedCapabilities::new(vec![Capability::FilesystemWrite(
            FileScope::new("ws-2", "src/api").unwrap(),
        )]);
        assert!(matches!(
            parent.narrow(child),
            Err(PolicyContractError::AuthorityExpansion(_))
        ));
    }

    #[test]
    fn parent_with_no_capability_cannot_delegate() {
        assert!(matches!(
            EffectiveCapabilities::none()
                .narrow(RequestedCapabilities::new(vec![file_write("src")])),
            Err(PolicyContractError::AuthorityExpansion(_))
        ));
    }

    #[test]
    fn exact_approval_authorizes_only_bound_scope() {
        let requested = request(vec![file_write("src/api")]);
        let binding = ApprovalBinding::new(requested.clone(), "policy-v1", 100).unwrap();
        let requirement = ApprovalRequirement::new(
            "approval-1",
            binding.clone(),
            EffectiveCapabilities::new(vec![file_write("src/api")]),
        )
        .unwrap();
        let decision = PolicyDecision::require_approval(
            requested,
            requirement,
            "approval.write",
            "human_required",
        )
        .unwrap();
        let resolution = ApprovalResolution::new(
            "approval-1",
            binding,
            HumanDecision::Approve,
            "local-user",
            50,
        )
        .unwrap();

        let effective = decision.apply_approval(&resolution, 50).unwrap();
        assert!(effective.permits(&file_write("src/api/routes")));
        assert!(!effective.permits(&file_write("src/web")));
    }

    #[test]
    fn approval_binding_fingerprint_is_exact_and_deterministic() {
        let requested = request(vec![file_write("src/api")]);
        let requirement = ApprovalRequirement::new(
            "approval-1",
            ApprovalBinding::new(requested.clone(), "policy-v1", 100).unwrap(),
            EffectiveCapabilities::new(vec![file_write("src/api")]),
        )
        .unwrap();
        assert_eq!(
            requirement.binding_fingerprint().unwrap(),
            requirement.clone().binding_fingerprint().unwrap()
        );

        let wider = ApprovalRequirement::new(
            "approval-1",
            ApprovalBinding::new(request(vec![file_write("src")]), "policy-v1", 100).unwrap(),
            EffectiveCapabilities::new(vec![file_write("src")]),
        )
        .unwrap();
        let revised_policy = ApprovalRequirement::new(
            "approval-1",
            ApprovalBinding::new(requested, "policy-v2", 100).unwrap(),
            EffectiveCapabilities::new(vec![file_write("src/api")]),
        )
        .unwrap();
        assert_ne!(
            requirement.binding_fingerprint().unwrap(),
            wider.binding_fingerprint().unwrap()
        );
        assert_ne!(
            requirement.binding_fingerprint().unwrap(),
            revised_policy.binding_fingerprint().unwrap()
        );
    }

    #[test]
    fn approval_required_authority_is_not_effective_before_approval() {
        let requested = request(vec![file_write("src/api")]);
        let binding = ApprovalBinding::new(requested.clone(), "policy-v1", 100).unwrap();
        let requirement = ApprovalRequirement::new(
            "approval-1",
            binding,
            EffectiveCapabilities::new(vec![file_write("src/api")]),
        )
        .unwrap();
        let decision = PolicyDecision::require_approval(
            requested,
            requirement,
            "approval.write",
            "human_required",
        )
        .unwrap();

        assert!(decision.effective().as_slice().is_empty());
        assert!(matches!(
            decision
                .effective()
                .narrow(RequestedCapabilities::new(vec![file_write("src/api")])),
            Err(PolicyContractError::AuthorityExpansion(_))
        ));
    }

    #[test]
    fn stale_or_widened_approval_is_refused() {
        let requested = request(vec![file_write("src/api")]);
        let binding = ApprovalBinding::new(requested.clone(), "policy-v1", 100).unwrap();
        let requirement = ApprovalRequirement::new(
            "approval-1",
            binding,
            EffectiveCapabilities::new(vec![file_write("src/api")]),
        )
        .unwrap();
        let decision = PolicyDecision::require_approval(
            requested,
            requirement,
            "approval.write",
            "human_required",
        )
        .unwrap();
        let wider_request = request(vec![file_write("src")]);
        let wider_binding = ApprovalBinding::new(wider_request, "policy-v1", 100).unwrap();
        let widened = ApprovalResolution::new(
            "approval-1",
            wider_binding,
            HumanDecision::Approve,
            "local-user",
            50,
        )
        .unwrap();
        assert_eq!(
            decision.apply_approval(&widened, 50),
            Err(PolicyContractError::ApprovalScopeMismatch)
        );
        let exact = ApprovalResolution::new(
            "approval-1",
            decision.approval.as_ref().unwrap().binding.clone(),
            HumanDecision::Approve,
            "local-user",
            50,
        )
        .unwrap();
        assert_eq!(
            decision.apply_approval(&exact, 101),
            Err(PolicyContractError::ApprovalExpired)
        );
    }

    #[test]
    fn hard_deny_cannot_be_approved() {
        let requested = request(vec![file_write("src")]);
        let deny = PolicyDecision::deny(requested.clone(), "hard.no-write", "hard_deny").unwrap();
        let binding = ApprovalBinding::new(requested, "policy-v1", 100).unwrap();
        let resolution = ApprovalResolution::new(
            "approval-1",
            binding,
            HumanDecision::Approve,
            "local-user",
            50,
        )
        .unwrap();
        assert_eq!(
            deny.apply_approval(&resolution, 50),
            Err(PolicyContractError::HardDenied)
        );
    }

    #[test]
    fn hard_policy_deny_outranks_repository_allow() {
        assert_eq!(
            PolicyOutcome::Deny.most_restrictive(PolicyOutcome::Allow),
            PolicyOutcome::Deny
        );
    }

    #[test]
    fn unknown_capability_fails_closed() {
        assert!(matches!(
            "filesystem.teleport".parse::<CapabilityKind>(),
            Err(PolicyContractError::UnknownCapability(_))
        ));
    }

    #[test]
    fn malformed_scope_fails_closed() {
        assert!(matches!(
            FileScope::new("ws-1", "../outside"),
            Err(PolicyContractError::MalformedScope { .. })
        ));
        assert!(matches!(
            FileScope::new("ws-1", "/tmp/outside"),
            Err(PolicyContractError::MalformedScope { .. })
        ));
    }

    #[test]
    fn unauditable_space_only_scope_is_refused() {
        // G6 audit schema CHECK requires length(trim(scope)) > 0. A file path
        // consisting only of spaces would fail this CHECK and be unauditable.
        // FileScope::new must refuse it.
        assert!(matches!(
            FileScope::new("ws-1", " "),
            Err(PolicyContractError::MalformedScope {
                field: "filesystem_root",
                ..
            })
        ));
    }

    #[test]
    fn unauditable_whitespace_only_scope_is_refused() {
        // Tabs and multiple spaces all trim to empty and are unauditable.
        assert!(matches!(
            FileScope::new("ws-1", "\t"),
            Err(PolicyContractError::MalformedScope {
                field: "filesystem_root",
                ..
            })
        ));
        assert!(matches!(
            FileScope::new("ws-1", "   "),
            Err(PolicyContractError::MalformedScope {
                field: "filesystem_root",
                ..
            })
        ));
    }

    #[test]
    fn scope_with_leading_trailing_spaces_is_accepted() {
        // A path like " a " is valid: trim("  a  ") has length > 0.
        // Normalize-relative-path treats it as Normal component " a ".
        let scope = FileScope::new("ws-1", " a ").unwrap();
        assert_eq!(scope.root(), std::path::Path::new(" a "));
    }

    #[test]
    fn scope_with_spaces_in_middle_is_accepted() {
        // "a b" is auditable: trim("a b") = "a b" with length 3.
        let scope = FileScope::new("ws-1", "a b").unwrap();
        assert_eq!(scope.root(), std::path::Path::new("a b"));
    }

    #[test]
    fn requested_and_effective_authority_are_distinct_types() {
        let requested = request(vec![file_write("src")]);
        let effective = EffectiveCapabilities::new(vec![file_write("src/api")]);
        let decision =
            PolicyDecision::allow(requested, effective.clone(), "allow.api", "ok").unwrap();
        assert_eq!(decision.effective, effective);
    }

    #[test]
    fn effective_authority_cannot_exceed_request() {
        let requested = request(vec![file_write("src/api")]);
        assert!(matches!(
            PolicyDecision::allow(
                requested,
                EffectiveCapabilities::new(vec![file_write("src")]),
                "allow.bad",
                "bad"
            ),
            Err(PolicyContractError::EffectiveAuthorityOutsideRequest(_))
        ));
    }

    #[test]
    fn policy_primitives_are_deterministic_for_identical_inputs() {
        let parent = EffectiveCapabilities::new(vec![file_write("src")]);
        let requested = RequestedCapabilities::new(vec![file_write("src/api")]);
        assert_eq!(
            parent.narrow(requested.clone()).unwrap(),
            parent.narrow(requested).unwrap()
        );
    }
}
