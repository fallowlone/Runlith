use policy::*;

const DEFAULT_DENY_RULE_ID: &str = "policy.default_deny";

fn write(workspace: &str, path: &str) -> Capability {
    Capability::FilesystemWrite(FileScope::new(workspace, path).unwrap())
}

fn read(workspace: &str, path: &str) -> Capability {
    Capability::FilesystemRead(FileScope::new(workspace, path).unwrap())
}

fn request(capabilities: Vec<Capability>) -> PolicyRequest {
    PolicyRequest::new(
        "op-61",
        "agent-runtime",
        RequestOrigin::new("TASK-61", Some("E-61".into()), Some("A-61".into())).unwrap(),
        RequestedCapabilities::new(capabilities),
    )
    .unwrap()
}

fn rule(
    id: &str,
    reason: &str,
    layer: PolicyLayer,
    effect: PolicyOutcome,
    capability: Capability,
) -> PolicyRule {
    PolicyRule::new(id, reason, layer, effect, capability).unwrap()
}

fn compiled(rules: Vec<PolicyRule>) -> CompiledPolicy {
    CompiledPolicy::compile(rules).unwrap()
}

fn evaluate(
    request: &PolicyRequest,
    parent: EffectiveCapabilities,
    policy: &CompiledPolicy,
) -> Result<PolicyDecision, PolicyContractError> {
    PolicyEngine::evaluate(request, &parent, policy, 1_000)
}

#[test]
fn no_matching_rule_fails_closed() {
    let requested = request(vec![write("ws-1", "src")]);
    let decision = evaluate(
        &requested,
        EffectiveCapabilities::new(vec![write("ws-1", "src")]),
        &compiled(Vec::new()),
    )
    .unwrap();

    assert_eq!(decision.outcome(), PolicyOutcome::Deny);
    assert_eq!(decision.rule_id(), DEFAULT_DENY_RULE_ID);
    assert!(decision.effective().as_slice().is_empty());
}

#[test]
fn builtin_allow_grants_bounded_authority() {
    let requested = request(vec![write("ws-1", "src/api")]);
    let policy = compiled(vec![rule(
        "builtin.write-src",
        "builtin_allow",
        PolicyLayer::BuiltInHard,
        PolicyOutcome::Allow,
        write("ws-1", "src"),
    )]);
    let decision = evaluate(
        &requested,
        EffectiveCapabilities::new(vec![write("ws-1", "src")]),
        &policy,
    )
    .unwrap();

    assert_eq!(decision.outcome(), PolicyOutcome::Allow);
    assert!(decision.effective().permits(&write("ws-1", "src/api")));
    assert!(!decision.effective().permits(&write("ws-1", "src/web")));
}

#[test]
fn builtin_deny_is_final() {
    let requested = request(vec![write("ws-1", "src")]);
    let policy = compiled(vec![rule(
        "builtin.no-write",
        "hard_deny",
        PolicyLayer::BuiltInHard,
        PolicyOutcome::Deny,
        write("ws-1", "src"),
    )]);
    let decision = evaluate(
        &requested,
        EffectiveCapabilities::new(vec![write("ws-1", "src")]),
        &policy,
    )
    .unwrap();

    assert_eq!(decision.outcome(), PolicyOutcome::Deny);
    assert_eq!(decision.rule_id(), "builtin.no-write");
}

#[test]
fn builtin_require_approval_withholds_authority() {
    let requested = request(vec![write("ws-1", "src")]);
    let policy = compiled(vec![rule(
        "builtin.approve-write",
        "human_required",
        PolicyLayer::BuiltInHard,
        PolicyOutcome::RequireApproval,
        write("ws-1", "src"),
    )]);
    let decision = evaluate(
        &requested,
        EffectiveCapabilities::new(vec![write("ws-1", "src")]),
        &policy,
    )
    .unwrap();

    assert_eq!(decision.outcome(), PolicyOutcome::RequireApproval);
    assert!(decision.effective().as_slice().is_empty());
    assert!(decision
        .approval()
        .unwrap()
        .effective_if_approved()
        .permits(&write("ws-1", "src")));
    assert_eq!(
        decision.approval().unwrap().binding().policy_fingerprint(),
        policy.fingerprint()
    );
}

#[test]
fn stronger_deny_beats_weaker_allow() {
    let requested = request(vec![write("ws-1", "src")]);
    let policy = compiled(vec![
        rule(
            "repo.allow",
            "repo_allow",
            PolicyLayer::Repository,
            PolicyOutcome::Allow,
            write("ws-1", "src"),
        ),
        rule(
            "builtin.deny",
            "hard_deny",
            PolicyLayer::BuiltInHard,
            PolicyOutcome::Deny,
            write("ws-1", "src"),
        ),
    ]);
    let decision = evaluate(
        &requested,
        EffectiveCapabilities::new(vec![write("ws-1", "src")]),
        &policy,
    )
    .unwrap();

    assert_eq!(decision.outcome(), PolicyOutcome::Deny);
    assert_eq!(decision.rule_id(), "builtin.deny");
}

#[test]
fn stronger_approval_beats_weaker_allow() {
    let requested = request(vec![write("ws-1", "src")]);
    let policy = compiled(vec![
        rule(
            "session.allow",
            "preference_allow",
            PolicyLayer::RequestSession,
            PolicyOutcome::Allow,
            write("ws-1", "src"),
        ),
        rule(
            "builtin.approval",
            "human_required",
            PolicyLayer::BuiltInHard,
            PolicyOutcome::RequireApproval,
            write("ws-1", "src"),
        ),
    ]);
    let decision = evaluate(
        &requested,
        EffectiveCapabilities::new(vec![write("ws-1", "src")]),
        &policy,
    )
    .unwrap();

    assert_eq!(decision.outcome(), PolicyOutcome::RequireApproval);
    assert_eq!(decision.rule_id(), "builtin.approval");
}

#[test]
fn weaker_layer_can_narrow_allow_to_deny() {
    let requested = request(vec![write("ws-1", "src/api")]);
    let policy = compiled(vec![
        rule(
            "builtin.allow",
            "base_allow",
            PolicyLayer::BuiltInHard,
            PolicyOutcome::Allow,
            write("ws-1", "src"),
        ),
        rule(
            "repo.deny-api",
            "repository_restriction",
            PolicyLayer::Repository,
            PolicyOutcome::Deny,
            write("ws-1", "src/api"),
        ),
    ]);
    let decision = evaluate(
        &requested,
        EffectiveCapabilities::new(vec![write("ws-1", "src")]),
        &policy,
    )
    .unwrap();

    assert_eq!(decision.outcome(), PolicyOutcome::Deny);
    assert_eq!(decision.rule_id(), "repo.deny-api");
}

#[test]
fn weaker_layer_can_narrow_allow_to_approval() {
    let requested = request(vec![write("ws-1", "src/api")]);
    let policy = compiled(vec![
        rule(
            "builtin.allow",
            "base_allow",
            PolicyLayer::BuiltInHard,
            PolicyOutcome::Allow,
            write("ws-1", "src"),
        ),
        rule(
            "workspace.approval",
            "workspace_approval",
            PolicyLayer::Workspace,
            PolicyOutcome::RequireApproval,
            write("ws-1", "src/api"),
        ),
    ]);
    let decision = evaluate(
        &requested,
        EffectiveCapabilities::new(vec![write("ws-1", "src")]),
        &policy,
    )
    .unwrap();

    assert_eq!(decision.outcome(), PolicyOutcome::RequireApproval);
    assert_eq!(decision.rule_id(), "workspace.approval");
}

#[test]
fn repository_allow_cannot_widen_user_deny() {
    let requested = request(vec![write("ws-1", "src")]);
    let policy = compiled(vec![
        rule(
            "user.deny",
            "user_deny",
            PolicyLayer::UserManaged,
            PolicyOutcome::Deny,
            write("ws-1", "src"),
        ),
        rule(
            "repo.allow",
            "repo_allow",
            PolicyLayer::Repository,
            PolicyOutcome::Allow,
            write("ws-1", "src"),
        ),
    ]);
    let decision = evaluate(
        &requested,
        EffectiveCapabilities::new(vec![write("ws-1", "src")]),
        &policy,
    )
    .unwrap();

    assert_eq!(decision.outcome(), PolicyOutcome::Deny);
    assert_eq!(decision.rule_id(), "user.deny");
}

#[test]
fn session_allow_cannot_widen_repository_restriction() {
    let requested = request(vec![write("ws-1", "src")]);
    let policy = compiled(vec![
        rule(
            "builtin.allow",
            "base_allow",
            PolicyLayer::BuiltInHard,
            PolicyOutcome::Allow,
            write("ws-1", "src"),
        ),
        rule(
            "repo.deny",
            "repo_deny",
            PolicyLayer::Repository,
            PolicyOutcome::Deny,
            write("ws-1", "src"),
        ),
        rule(
            "session.allow",
            "session_allow",
            PolicyLayer::RequestSession,
            PolicyOutcome::Allow,
            write("ws-1", "src"),
        ),
    ]);
    let decision = evaluate(
        &requested,
        EffectiveCapabilities::new(vec![write("ws-1", "src")]),
        &policy,
    )
    .unwrap();

    assert_eq!(decision.outcome(), PolicyOutcome::Deny);
    assert_eq!(decision.rule_id(), "repo.deny");
}

#[test]
fn repository_policy_cannot_self_grant() {
    let requested = request(vec![write("ws-1", "src")]);
    let policy = compiled(vec![rule(
        "repo.allow",
        "repo_allow",
        PolicyLayer::Repository,
        PolicyOutcome::Allow,
        write("ws-1", "src"),
    )]);
    let decision = evaluate(
        &requested,
        EffectiveCapabilities::new(vec![write("ws-1", "src")]),
        &policy,
    )
    .unwrap();

    assert_eq!(decision.outcome(), PolicyOutcome::Deny);
    assert_eq!(decision.rule_id(), DEFAULT_DENY_RULE_ID);
}

#[test]
fn parent_scope_widening_is_refused_before_policy() {
    let requested = request(vec![write("ws-1", "src")]);
    let policy = compiled(vec![rule(
        "builtin.allow",
        "base_allow",
        PolicyLayer::BuiltInHard,
        PolicyOutcome::Allow,
        write("ws-1", "src"),
    )]);

    assert!(matches!(
        evaluate(
            &requested,
            EffectiveCapabilities::new(vec![write("ws-1", "src/api")]),
            &policy,
        ),
        Err(PolicyContractError::AuthorityExpansion(_))
    ));
}

#[test]
fn filesystem_subscope_matching_is_typed() {
    let requested = request(vec![write("ws-1", "src/api/routes")]);
    let policy = compiled(vec![rule(
        "builtin.allow-src",
        "base_allow",
        PolicyLayer::BuiltInHard,
        PolicyOutcome::Allow,
        write("ws-1", "src/api"),
    )]);
    let decision = evaluate(
        &requested,
        EffectiveCapabilities::new(vec![write("ws-1", "src")]),
        &policy,
    )
    .unwrap();

    assert_eq!(decision.outcome(), PolicyOutcome::Allow);
}

#[test]
fn filesystem_string_prefix_confusion_does_not_match() {
    let requested = request(vec![write("ws-1", "src/api2")]);
    let policy = compiled(vec![rule(
        "builtin.allow-api",
        "base_allow",
        PolicyLayer::BuiltInHard,
        PolicyOutcome::Allow,
        write("ws-1", "src/api"),
    )]);
    let decision = evaluate(
        &requested,
        EffectiveCapabilities::new(vec![write("ws-1", "src")]),
        &policy,
    )
    .unwrap();

    assert_eq!(decision.outcome(), PolicyOutcome::Deny);
}

#[test]
fn restrictive_subscope_blocks_broader_request_atomically() {
    let requested = request(vec![write("ws-1", "src")]);
    let policy = compiled(vec![
        rule(
            "builtin.allow-src",
            "base_allow",
            PolicyLayer::BuiltInHard,
            PolicyOutcome::Allow,
            write("ws-1", "src"),
        ),
        rule(
            "repo.deny-api",
            "repo_restriction",
            PolicyLayer::Repository,
            PolicyOutcome::Deny,
            write("ws-1", "src/api"),
        ),
    ]);
    let decision = evaluate(
        &requested,
        EffectiveCapabilities::new(vec![write("ws-1", "src")]),
        &policy,
    )
    .unwrap();

    assert_eq!(decision.outcome(), PolicyOutcome::Deny);
    assert!(decision.effective().as_slice().is_empty());
}

#[test]
fn exact_scope_id_matching_is_exact() {
    let policy = compiled(vec![rule(
        "builtin.cargo",
        "cargo_allowed",
        PolicyLayer::BuiltInHard,
        PolicyOutcome::Allow,
        Capability::process("cargo").unwrap(),
    )]);
    let cargo = request(vec![Capability::process("cargo").unwrap()]);
    let rustc = request(vec![Capability::process("rustc").unwrap()]);

    assert_eq!(
        evaluate(
            &cargo,
            EffectiveCapabilities::new(vec![Capability::process("cargo").unwrap()]),
            &policy,
        )
        .unwrap()
        .outcome(),
        PolicyOutcome::Allow
    );
    assert_eq!(
        evaluate(
            &rustc,
            EffectiveCapabilities::new(vec![Capability::process("rustc").unwrap()]),
            &policy,
        )
        .unwrap()
        .outcome(),
        PolicyOutcome::Deny
    );
}

#[test]
fn mixed_capability_deny_exposes_no_partial_authority() {
    let requested = request(vec![
        write("ws-1", "src"),
        Capability::network("example.com").unwrap(),
    ]);
    let policy = compiled(vec![
        rule(
            "builtin.write",
            "write_allowed",
            PolicyLayer::BuiltInHard,
            PolicyOutcome::Allow,
            write("ws-1", "src"),
        ),
        rule(
            "builtin.network",
            "network_denied",
            PolicyLayer::BuiltInHard,
            PolicyOutcome::Deny,
            Capability::network("example.com").unwrap(),
        ),
    ]);
    let parent = EffectiveCapabilities::new(vec![
        write("ws-1", "src"),
        Capability::network("example.com").unwrap(),
    ]);
    let decision = evaluate(&requested, parent, &policy).unwrap();

    assert_eq!(decision.outcome(), PolicyOutcome::Deny);
    assert!(decision.effective().as_slice().is_empty());
}

#[test]
fn mixed_capability_approval_withholds_all_authority_until_approval() {
    let requested = request(vec![read("ws-1", "src"), write("ws-1", "src")]);
    let policy = compiled(vec![
        rule(
            "builtin.read",
            "read_allowed",
            PolicyLayer::BuiltInHard,
            PolicyOutcome::Allow,
            read("ws-1", "src"),
        ),
        rule(
            "builtin.write",
            "write_requires_approval",
            PolicyLayer::BuiltInHard,
            PolicyOutcome::RequireApproval,
            write("ws-1", "src"),
        ),
    ]);
    let parent = EffectiveCapabilities::new(vec![read("ws-1", "src"), write("ws-1", "src")]);
    let decision = evaluate(&requested, parent, &policy).unwrap();

    assert_eq!(decision.outcome(), PolicyOutcome::RequireApproval);
    assert!(decision.effective().as_slice().is_empty());
    assert_eq!(
        decision
            .approval()
            .unwrap()
            .effective_if_approved()
            .as_slice()
            .len(),
        2
    );
}

#[test]
fn identical_inputs_produce_identical_decisions() {
    let requested = request(vec![write("ws-1", "src/api")]);
    let policy = compiled(vec![rule(
        "builtin.allow",
        "base_allow",
        PolicyLayer::BuiltInHard,
        PolicyOutcome::Allow,
        write("ws-1", "src"),
    )]);
    let parent = EffectiveCapabilities::new(vec![write("ws-1", "src")]);

    assert_eq!(
        PolicyEngine::evaluate(&requested, &parent, &policy, 1_000).unwrap(),
        PolicyEngine::evaluate(&requested, &parent, &policy, 1_000).unwrap()
    );
}

#[test]
fn fingerprint_is_independent_of_rule_insertion_order() {
    let a = rule(
        "builtin.read",
        "read",
        PolicyLayer::BuiltInHard,
        PolicyOutcome::Allow,
        read("ws-1", "src"),
    );
    let b = rule(
        "user.write",
        "write",
        PolicyLayer::UserManaged,
        PolicyOutcome::RequireApproval,
        write("ws-1", "src"),
    );

    assert_eq!(
        compiled(vec![a.clone(), b.clone()]).fingerprint(),
        compiled(vec![b, a]).fingerprint()
    );
}

#[test]
fn relevant_policy_change_changes_fingerprint() {
    let allow = compiled(vec![rule(
        "builtin.write",
        "write",
        PolicyLayer::BuiltInHard,
        PolicyOutcome::Allow,
        write("ws-1", "src"),
    )]);
    let deny = compiled(vec![rule(
        "builtin.write",
        "write",
        PolicyLayer::BuiltInHard,
        PolicyOutcome::Deny,
        write("ws-1", "src"),
    )]);

    assert_ne!(allow.fingerprint(), deny.fingerprint());
}

#[test]
fn stable_rule_and_reason_provenance_is_retained() {
    let requested = request(vec![write("ws-1", "src/api")]);
    let policy = compiled(vec![
        rule(
            "repo.review-api",
            "repo_requires_review",
            PolicyLayer::Repository,
            PolicyOutcome::RequireApproval,
            write("ws-1", "src/api"),
        ),
        rule(
            "builtin.allow",
            "base_allow",
            PolicyLayer::BuiltInHard,
            PolicyOutcome::Allow,
            write("ws-1", "src"),
        ),
    ]);
    let decision = evaluate(
        &requested,
        EffectiveCapabilities::new(vec![write("ws-1", "src")]),
        &policy,
    )
    .unwrap();

    assert_eq!(decision.rule_id(), "repo.review-api");
    assert_eq!(decision.reason_code(), "repo_requires_review");
    assert_eq!(decision.policy_fingerprint(), Some(policy.fingerprint()));
    assert!(decision.rule_evidence().iter().any(|evidence| {
        evidence.rule_id() == "repo.review-api"
            && evidence.reason_code() == "repo_requires_review"
            && evidence.layer() == PolicyLayer::Repository
            && evidence.effect() == PolicyOutcome::RequireApproval
    }));
}

#[test]
fn duplicate_rule_identity_fails_compilation() {
    let first = rule(
        "duplicate",
        "allow",
        PolicyLayer::BuiltInHard,
        PolicyOutcome::Allow,
        write("ws-1", "src"),
    );
    let second = rule(
        "duplicate",
        "deny",
        PolicyLayer::Repository,
        PolicyOutcome::Deny,
        write("ws-1", "src/api"),
    );

    assert_eq!(
        CompiledPolicy::compile(vec![first, second]),
        Err(PolicyContractError::DuplicateRuleId("duplicate".into()))
    );
}

#[test]
fn adding_stronger_deny_never_increases_effective_authority() {
    let requested = request(vec![write("ws-1", "src")]);
    let allow = compiled(vec![rule(
        "user.allow",
        "allow",
        PolicyLayer::UserManaged,
        PolicyOutcome::Allow,
        write("ws-1", "src"),
    )]);
    let deny = compiled(vec![
        rule(
            "user.allow",
            "allow",
            PolicyLayer::UserManaged,
            PolicyOutcome::Allow,
            write("ws-1", "src"),
        ),
        rule(
            "builtin.deny",
            "deny",
            PolicyLayer::BuiltInHard,
            PolicyOutcome::Deny,
            write("ws-1", "src"),
        ),
    ]);
    let parent = EffectiveCapabilities::new(vec![write("ws-1", "src")]);
    let allowed = PolicyEngine::evaluate(&requested, &parent, &allow, 1_000).unwrap();
    let denied = PolicyEngine::evaluate(&requested, &parent, &deny, 1_000).unwrap();

    assert_eq!(allowed.effective().as_slice().len(), 1);
    assert!(denied.effective().as_slice().is_empty());
}

#[test]
fn deterministic_corpus_preserves_parent_and_request_bounds() {
    let policy = compiled(vec![rule(
        "builtin.allow-src",
        "allow",
        PolicyLayer::BuiltInHard,
        PolicyOutcome::Allow,
        write("ws-1", "src"),
    )]);
    let parent = EffectiveCapabilities::new(vec![write("ws-1", "src")]);

    for path in ["src", "src/api", "src/api/routes", "src/web"] {
        let requested = request(vec![write("ws-1", path)]);
        let decision = PolicyEngine::evaluate(&requested, &parent, &policy, 1_000).unwrap();
        assert_eq!(decision.outcome(), PolicyOutcome::Allow);
        for effective in decision.effective().as_slice() {
            assert!(requested
                .requested()
                .as_slice()
                .iter()
                .any(|requested| effective.is_subset_of(requested)));
            assert!(parent.permits(effective));
        }
    }
}
