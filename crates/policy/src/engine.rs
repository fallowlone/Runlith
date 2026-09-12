use super::*;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

const DEFAULT_DENY_RULE_ID: &str = "policy.default_deny";
const DEFAULT_DENY_REASON: &str = "no_authorizing_rule";

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum PolicyLayer {
    BuiltInHard,
    UserManaged,
    Workspace,
    Repository,
    RequestSession,
}

impl PolicyLayer {
    fn as_str(self) -> &'static str {
        match self {
            Self::BuiltInHard => "builtin-hard",
            Self::UserManaged => "user-managed",
            Self::Workspace => "workspace",
            Self::Repository => "repository",
            Self::RequestSession => "request-session",
        }
    }

    fn can_grant(self) -> bool {
        matches!(self, Self::BuiltInHard | Self::UserManaged)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PolicyRule {
    id: String,
    reason_code: String,
    layer: PolicyLayer,
    effect: PolicyOutcome,
    capability: Capability,
}

impl PolicyRule {
    pub fn new(
        id: impl Into<String>,
        reason_code: impl Into<String>,
        layer: PolicyLayer,
        effect: PolicyOutcome,
        capability: Capability,
    ) -> Result<Self, PolicyContractError> {
        Ok(Self {
            id: checked_id("rule_id", id.into())?,
            reason_code: checked_id("reason_code", reason_code.into())?,
            layer,
            effect,
            capability,
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn reason_code(&self) -> &str {
        &self.reason_code
    }

    pub fn layer(&self) -> PolicyLayer {
        self.layer
    }

    pub fn effect(&self) -> PolicyOutcome {
        self.effect
    }

    pub fn capability(&self) -> &Capability {
        &self.capability
    }

    fn matches(&self, requested: &Capability) -> bool {
        match self.effect {
            PolicyOutcome::Allow => requested.is_subset_of(&self.capability),
            PolicyOutcome::Deny | PolicyOutcome::RequireApproval => {
                capabilities_overlap(requested, &self.capability)
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PolicyRuleEvidence {
    layer: PolicyLayer,
    rule_id: String,
    reason_code: String,
    effect: PolicyOutcome,
    requested_capability: Capability,
    rule_capability: Capability,
}

impl PolicyRuleEvidence {
    pub fn layer(&self) -> PolicyLayer {
        self.layer
    }

    pub fn rule_id(&self) -> &str {
        &self.rule_id
    }

    pub fn reason_code(&self) -> &str {
        &self.reason_code
    }

    pub fn effect(&self) -> PolicyOutcome {
        self.effect
    }

    pub fn requested_capability(&self) -> &Capability {
        &self.requested_capability
    }

    pub fn rule_capability(&self) -> &Capability {
        &self.rule_capability
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompiledPolicy {
    rules: Vec<PolicyRule>,
    fingerprint: String,
}

impl CompiledPolicy {
    pub fn compile(rules: Vec<PolicyRule>) -> Result<Self, PolicyContractError> {
        let mut ids = BTreeSet::new();
        for rule in &rules {
            if !ids.insert(rule.id.clone()) {
                return Err(PolicyContractError::DuplicateRuleId(rule.id.clone()));
            }
        }

        let mut canonical = rules
            .into_iter()
            .map(|rule| Ok((canonical_rule_bytes(&rule)?, rule)))
            .collect::<Result<Vec<_>, PolicyContractError>>()?;
        canonical.sort_by(|left, right| {
            layer_rank(left.1.layer)
                .cmp(&layer_rank(right.1.layer))
                .then_with(|| left.0.cmp(&right.0))
        });

        let mut hasher = Sha256::new();
        push_field(&mut hasher, b"policy-fingerprint-v1");
        for (bytes, _) in &canonical {
            push_field(&mut hasher, bytes);
        }
        let fingerprint = format!("sha256:{:x}", hasher.finalize());

        Ok(Self {
            rules: canonical.into_iter().map(|(_, rule)| rule).collect(),
            fingerprint,
        })
    }

    pub fn rules(&self) -> &[PolicyRule] {
        &self.rules
    }

    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PolicyEngine;

impl PolicyEngine {
    pub fn evaluate(
        request: &PolicyRequest,
        parent: &EffectiveCapabilities,
        policy: &CompiledPolicy,
        approval_expires_at_ms: u64,
    ) -> Result<PolicyDecision, PolicyContractError> {
        let bounded = parent.narrow(request.requested.clone())?;
        let mut capability_results = Vec::new();
        let mut evidence = Vec::new();

        for requested in request.requested.as_slice() {
            let result = evaluate_capability(requested, policy, &mut evidence);
            capability_results.push(result);
        }

        let aggregate = aggregate_results(&capability_results);
        let fingerprint = policy.fingerprint.clone();

        match aggregate.outcome {
            PolicyOutcome::Deny => Ok(PolicyDecision {
                outcome: PolicyOutcome::Deny,
                rule_id: aggregate.rule_id,
                reason_code: aggregate.reason_code,
                request: request.clone(),
                effective: EffectiveCapabilities::none(),
                approval: None,
                policy_fingerprint: Some(fingerprint),
                rule_evidence: evidence,
            }),
            PolicyOutcome::Allow => Ok(PolicyDecision {
                outcome: PolicyOutcome::Allow,
                rule_id: aggregate.rule_id,
                reason_code: aggregate.reason_code,
                request: request.clone(),
                effective: bounded,
                approval: None,
                policy_fingerprint: Some(fingerprint),
                rule_evidence: evidence,
            }),
            PolicyOutcome::RequireApproval => {
                let request_id =
                    approval_request_id(request, &fingerprint, approval_expires_at_ms)?;
                let binding = ApprovalBinding::new(
                    request.clone(),
                    fingerprint.clone(),
                    approval_expires_at_ms,
                )?;
                let approval = ApprovalRequirement::new(request_id, binding, bounded)?;
                Ok(PolicyDecision {
                    outcome: PolicyOutcome::RequireApproval,
                    rule_id: aggregate.rule_id,
                    reason_code: aggregate.reason_code,
                    request: request.clone(),
                    effective: EffectiveCapabilities::none(),
                    approval: Some(approval),
                    policy_fingerprint: Some(fingerprint),
                    rule_evidence: evidence,
                })
            }
        }
    }
}

#[derive(Clone, Debug)]
struct CapabilityResult {
    outcome: PolicyOutcome,
    rule_id: String,
    reason_code: String,
    layer: Option<PolicyLayer>,
}

fn evaluate_capability(
    requested: &Capability,
    policy: &CompiledPolicy,
    evidence: &mut Vec<PolicyRuleEvidence>,
) -> CapabilityResult {
    let mut result: Option<CapabilityResult> = None;

    for rule in policy.rules() {
        if !rule.matches(requested) {
            continue;
        }

        evidence.push(PolicyRuleEvidence {
            layer: rule.layer,
            rule_id: rule.id.clone(),
            reason_code: rule.reason_code.clone(),
            effect: rule.effect,
            requested_capability: requested.clone(),
            rule_capability: rule.capability.clone(),
        });

        match rule.effect {
            PolicyOutcome::Deny => {
                if !matches!(
                    result.as_ref().map(|result| result.outcome),
                    Some(PolicyOutcome::Deny)
                ) {
                    result = Some(from_rule(rule));
                }
            }
            PolicyOutcome::RequireApproval => match result.as_ref().map(|result| result.outcome) {
                Some(PolicyOutcome::Deny | PolicyOutcome::RequireApproval) => {}
                Some(PolicyOutcome::Allow) => result = Some(from_rule(rule)),
                None if rule.layer.can_grant() => result = Some(from_rule(rule)),
                None => {}
            },
            PolicyOutcome::Allow => {
                if result.is_none() && rule.layer.can_grant() {
                    result = Some(from_rule(rule));
                }
            }
        }
    }

    result.unwrap_or_else(default_deny)
}

fn aggregate_results(results: &[CapabilityResult]) -> CapabilityResult {
    if results.is_empty() {
        return default_deny();
    }

    let outcome = results
        .iter()
        .fold(PolicyOutcome::Allow, |current, result| {
            current.most_restrictive(result.outcome)
        });

    results
        .iter()
        .filter(|result| result.outcome == outcome)
        .min_by(|left, right| cause_key(left).cmp(&cause_key(right)))
        .cloned()
        .unwrap_or_else(default_deny)
}

fn cause_key(result: &CapabilityResult) -> (u8, &str, &str) {
    let layer = result.layer.map(layer_rank).unwrap_or(u8::MAX);
    (layer, &result.rule_id, &result.reason_code)
}

fn layer_rank(layer: PolicyLayer) -> u8 {
    match layer {
        PolicyLayer::BuiltInHard => 0,
        PolicyLayer::UserManaged => 1,
        PolicyLayer::Workspace => 2,
        PolicyLayer::Repository => 3,
        PolicyLayer::RequestSession => 4,
    }
}

fn from_rule(rule: &PolicyRule) -> CapabilityResult {
    CapabilityResult {
        outcome: rule.effect,
        rule_id: rule.id.clone(),
        reason_code: rule.reason_code.clone(),
        layer: Some(rule.layer),
    }
}

fn default_deny() -> CapabilityResult {
    CapabilityResult {
        outcome: PolicyOutcome::Deny,
        rule_id: DEFAULT_DENY_RULE_ID.to_owned(),
        reason_code: DEFAULT_DENY_REASON.to_owned(),
        layer: None,
    }
}

fn capabilities_overlap(left: &Capability, right: &Capability) -> bool {
    match (left, right) {
        (Capability::FilesystemRead(left), Capability::FilesystemRead(right))
        | (Capability::FilesystemWrite(left), Capability::FilesystemWrite(right)) => {
            left.workspace == right.workspace
                && (left.is_subset_of(right) || right.is_subset_of(left))
        }
        (Capability::ProcessExecute(left), Capability::ProcessExecute(right))
        | (Capability::NetworkAccess(left), Capability::NetworkAccess(right))
        | (Capability::GitMutate(left), Capability::GitMutate(right))
        | (Capability::SecretAccess(left), Capability::SecretAccess(right))
        | (Capability::ToolInvoke(left), Capability::ToolInvoke(right))
        | (Capability::ExternalAction(left), Capability::ExternalAction(right)) => left == right,
        _ => false,
    }
}

fn canonical_rule_bytes(rule: &PolicyRule) -> Result<Vec<u8>, PolicyContractError> {
    let mut bytes = Vec::new();
    append_field(&mut bytes, b"policy-rule-v1");
    append_field(&mut bytes, rule.layer.as_str().as_bytes());
    append_field(&mut bytes, rule.id.as_bytes());
    append_field(&mut bytes, rule.reason_code.as_bytes());
    append_field(&mut bytes, outcome_name(rule.effect).as_bytes());
    append_field(&mut bytes, &canonical_capability_bytes(&rule.capability)?);
    Ok(bytes)
}

fn canonical_request_bytes(request: &PolicyRequest) -> Result<Vec<u8>, PolicyContractError> {
    let mut bytes = Vec::new();
    append_field(&mut bytes, b"policy-request-v1");
    append_field(&mut bytes, request.operation_id.as_bytes());
    append_field(&mut bytes, request.requester.as_bytes());
    append_field(&mut bytes, request.origin.task_id.as_bytes());
    append_optional_field(&mut bytes, request.origin.execution_id.as_deref());
    append_optional_field(&mut bytes, request.origin.attempt_id.as_deref());
    for capability in request.requested.as_slice() {
        append_field(&mut bytes, &canonical_capability_bytes(capability)?);
    }
    Ok(bytes)
}

fn canonical_capability_bytes(capability: &Capability) -> Result<Vec<u8>, PolicyContractError> {
    let mut bytes = Vec::new();
    match capability {
        Capability::FilesystemRead(scope) => {
            append_field(&mut bytes, b"filesystem.read");
            append_file_scope(&mut bytes, scope)?;
        }
        Capability::FilesystemWrite(scope) => {
            append_field(&mut bytes, b"filesystem.write");
            append_file_scope(&mut bytes, scope)?;
        }
        Capability::ProcessExecute(scope) => {
            append_exact_scope(&mut bytes, b"process.execute", scope)
        }
        Capability::NetworkAccess(scope) => {
            append_exact_scope(&mut bytes, b"network.access", scope)
        }
        Capability::GitMutate(scope) => append_exact_scope(&mut bytes, b"git.mutate", scope),
        Capability::SecretAccess(scope) => append_exact_scope(&mut bytes, b"secret.access", scope),
        Capability::ToolInvoke(scope) => append_exact_scope(&mut bytes, b"tool.invoke", scope),
        Capability::ExternalAction(scope) => {
            append_exact_scope(&mut bytes, b"external.action", scope)
        }
    }
    Ok(bytes)
}

fn append_file_scope(bytes: &mut Vec<u8>, scope: &FileScope) -> Result<(), PolicyContractError> {
    append_field(bytes, scope.workspace.as_bytes());
    let root = scope
        .root
        .to_str()
        .ok_or_else(|| PolicyContractError::MalformedScope {
            field: "filesystem_root_utf8",
            value: "<non-utf8>".to_owned(),
        })?;
    append_field(bytes, root.as_bytes());
    Ok(())
}

fn append_exact_scope(bytes: &mut Vec<u8>, kind: &[u8], scope: &ScopeId) {
    append_field(bytes, kind);
    append_field(bytes, scope.0.as_bytes());
}

fn append_optional_field(bytes: &mut Vec<u8>, value: Option<&str>) {
    match value {
        Some(value) => {
            bytes.push(1);
            append_field(bytes, value.as_bytes());
        }
        None => bytes.push(0),
    }
}

fn append_field(bytes: &mut Vec<u8>, value: &[u8]) {
    bytes.extend_from_slice(&(value.len() as u64).to_be_bytes());
    bytes.extend_from_slice(value);
}

fn push_field(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}

fn outcome_name(outcome: PolicyOutcome) -> &'static str {
    match outcome {
        PolicyOutcome::Allow => "allow",
        PolicyOutcome::Deny => "deny",
        PolicyOutcome::RequireApproval => "require-approval",
    }
}

fn approval_request_id(
    request: &PolicyRequest,
    policy_fingerprint: &str,
    expires_at_ms: u64,
) -> Result<String, PolicyContractError> {
    let mut hasher = Sha256::new();
    push_field(&mut hasher, b"approval-request-v1");
    push_field(&mut hasher, &canonical_request_bytes(request)?);
    push_field(&mut hasher, policy_fingerprint.as_bytes());
    push_field(&mut hasher, &expires_at_ms.to_be_bytes());
    Ok(format!("approval-{:x}", hasher.finalize()))
}
