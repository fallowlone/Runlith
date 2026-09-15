//! Claude planner participation in the shared provider-neutral PlannerPort
//! contract. Deterministic fixture executables exercise the real adapter and
//! provider parsing path without network or Claude authentication.

#![cfg(unix)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use plan_domain::{DecompositionRequest, PlannerFault, PlannerPort, PlanningBudget, ReplanRequest};
use planner_contract::{
    contract_decomposition_request, contract_replan_request, run_planner_port_contract,
    ContractPlanners,
};
use provider_claude::{ClaudeConfig, ClaudePlanner};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct Case {
    dir: PathBuf,
}

impl Case {
    fn named(name: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "runlith-claude-planner-{name}-{}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("case dir");
        Self { dir }
    }

    fn fixture(&self, name: &str, body: &str) -> PathBuf {
        let path = self.dir.join(name);
        let mut file = std::fs::File::create(&path).expect("fixture file");
        file.write_all(b"#!/bin/sh\n").expect("shebang");
        file.write_all(body.as_bytes()).expect("fixture body");
        drop(file);
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("fixture chmod");
        path
    }

    fn workdir(&self) -> PathBuf {
        let path = self.dir.join("work");
        std::fs::create_dir_all(&path).expect("workdir");
        path
    }
}

impl Drop for Case {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn planner_for(fixture: &Path, case: &Case) -> ClaudePlanner {
    let config = ClaudeConfig::default()
        .with_executable(fixture)
        .expect("fixture executable")
        .with_timeout(Duration::from_secs(10))
        .expect("timeout");
    ClaudePlanner::bind_trusted_host(config, case.workdir(), case.dir.join("provider-env"))
        .expect("planner")
}

fn valid_fixture_body() -> &'static str {
    r#"
last=""
for arg in "$@"; do last="$arg"; done
case "$last" in
  *assess_atomicity*)
    result='{"verdict":"DECOMPOSABLE","rationale":"fixture needs decomposition","evidence_refs":["evidence-contract"]}'
    ;;
  *propose_replan*)
    result='{"base_plan_revision":1,"reason":"fixture replan","node_additions":[{"id":"replanned-child","scope":["repo:src"],"criteria":[{"id":"replan-criterion","method":"automated-test"}],"risk_class":"low","required_gate":null}],"node_updates":[],"node_removals":[],"edge_additions":[],"edge_removals":[],"expected_outcome":"replacement leaf","risk_delta":[],"budget_delta":{"planning_cost_movement":0,"expected_execution_cost_movement":0},"scope_delta":["repo:src"],"coverage_map":{}}'
    ;;
  *)
    result='{"base_plan_revision":0,"reason":"fixture decomposition","node_additions":[{"id":"planned-child","scope":["repo:src"],"criteria":[{"id":"criterion-child","method":"automated-test"}],"risk_class":"low","required_gate":null}],"node_updates":[],"node_removals":[],"edge_additions":[],"edge_removals":[],"expected_outcome":"child leaf","risk_delta":[],"budget_delta":{"planning_cost_movement":0,"expected_execution_cost_movement":0},"scope_delta":["repo:src"],"coverage_map":{"criterion-root":["criterion-child"]}}'
    ;;
esac
escaped=$(printf '%s' "$result" | sed 's/\\/\\\\/g; s/"/\\"/g')
printf '{"type":"result","subtype":"success","is_error":false,"result":"%s","session_id":"planner-contract-session","usage":{"input_tokens":100,"output_tokens":100}}' "$escaped"
"#
}

#[test]
fn claude_planner_satisfies_shared_planner_port_contract() {
    let case = Case::named("contract");
    let valid_path = case.fixture("valid.sh", valid_fixture_body());
    let failure_path = case.fixture("failure.sh", "echo 'planner failed' >&2\nexit 3\n");
    let malformed_path = case.fixture(
        "malformed.sh",
        r#"printf '{"type":"result","subtype":"success","is_error":false,"result":"not planner json","session_id":"planner-malformed","usage":{"input_tokens":1,"output_tokens":1}}'"#,
    );
    let valid = planner_for(&valid_path, &case);
    let failure = planner_for(&failure_path, &case);
    let malformed = planner_for(&malformed_path, &case);

    run_planner_port_contract(&ContractPlanners {
        valid: &valid,
        failure: &failure,
        malformed: &malformed,
    });
}

#[test]
fn depth_and_budget_refusal_happen_before_provider_invocation() {
    let case = Case::named("preflight");
    let marker = case.dir.join("called");
    let script = format!("touch {:?}\nexit 99\n", marker);
    let fixture = case.fixture("must-not-run.sh", &script);
    let planner = planner_for(&fixture, &case);

    let mut depth: DecompositionRequest = contract_decomposition_request();
    depth.depth_remaining = 0;
    assert!(matches!(
        planner.propose_decomposition(depth),
        Err(PlannerFault::DepthExhausted)
    ));

    let mut budget: ReplanRequest = contract_replan_request();
    budget.budget_remaining = PlanningBudget {
        calls_remaining: 0,
        input_tokens_remaining: 1,
    };
    assert!(matches!(
        planner.propose_replan(budget),
        Err(PlannerFault::BudgetExhausted)
    ));
    assert!(!marker.exists(), "pre-call refusal must not spawn provider");
}

#[test]
fn unattached_planner_cannot_fall_back_to_direct_host_spawn() {
    let case = Case::named("unattached");
    let marker = case.dir.join("called");
    let script = format!("touch {:?}\nexit 0\n", marker);
    let fixture = case.fixture("must-not-run.sh", &script);
    let config = ClaudeConfig::default()
        .with_executable(&fixture)
        .expect("fixture executable");
    let planner = ClaudePlanner::bind(config, case.workdir(), case.dir.join("provider-env"))
        .expect("unattached planner");

    let error = planner
        .propose_decomposition(contract_decomposition_request())
        .expect_err("unattached production planner must refuse");
    assert!(matches!(error, PlannerFault::ProviderFailure { .. }));
    assert!(
        !marker.exists(),
        "unattached planner must not spawn directly"
    );
}

#[test]
fn missing_provider_reported_usage_fails_closed() {
    let case = Case::named("missing-usage");
    let fixture = case.fixture(
        "missing-usage.sh",
        r#"
result='{"base_plan_revision":0,"reason":"fixture decomposition","node_additions":[],"node_updates":[],"node_removals":[],"edge_additions":[],"edge_removals":[],"expected_outcome":"leaf","risk_delta":[],"budget_delta":{"planning_cost_movement":0,"expected_execution_cost_movement":0},"scope_delta":["repo:src"],"coverage_map":{}}'
escaped=$(printf '%s' "$result" | sed 's/\\/\\\\/g; s/"/\\"/g')
printf '{"type":"result","subtype":"success","is_error":false,"result":"%s","session_id":"planner-no-usage"}' "$escaped"
"#,
    );
    let planner = planner_for(&fixture, &case);

    let error = planner
        .propose_decomposition(contract_decomposition_request())
        .expect_err("planner usage is mandatory");
    match error {
        PlannerFault::ProviderFailure { typed_cause } => {
            assert!(
                typed_cause.contains("token usage") || typed_cause.contains("usage"),
                "missing usage must remain an explicit provider failure: {typed_cause}"
            );
        }
        other => panic!("expected provider failure, got {other:?}"),
    }
}
