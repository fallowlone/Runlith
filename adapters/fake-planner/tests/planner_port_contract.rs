use fake_planner::{FakePlanner, FakePlannerScenario};
use planner_contract::{run_planner_port_contract, ContractPlanners};

#[test]
fn fake_planner_satisfies_shared_planner_port_contract() {
    let valid = FakePlanner::for_scenario(FakePlannerScenario::Valid);
    let failure = FakePlanner::for_scenario(FakePlannerScenario::ProviderFailure);
    let malformed = FakePlanner::for_scenario(FakePlannerScenario::MalformedProposal);
    run_planner_port_contract(&ContractPlanners {
        valid: &valid,
        failure: &failure,
        malformed: &malformed,
    });
}
