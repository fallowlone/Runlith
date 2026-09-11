//! FakeAgent participation in the shared `ExecutionPort` contract suite
//! (TASK-0049, G5).
//!
//! The same [`execution_contract::run_execution_port_contract`] every
//! provider must pass, driven here by deterministic FakeAgent scenarios
//! (no process, no network, fully portable). FakeAgent-specific behavior
//! (scenario matrix, session handles, hang coordination) stays in the
//! existing G3 suites; this file answers only: does the FakeAgent-backed
//! executor satisfy the common execution seam?

use agent_runtime::ProviderRef;
use execution_contract::{run_execution_port_contract, ContractExecutors};
use fake_agent::{ExecutionControl, ExecutionRequest, FakeAgent, FakeScenario};
use scheduler::{ExecutionPort, ExecutionProblem, WorkEvidence, WorkItem};

/// Test-side FakeAgent→`ExecutionPort` wrapper. Adapter failures
/// normalize at this boundary by matching on `FakeAgentError` variants —
/// never on scenario names — mirroring the G3 failure-matrix wrapper.
struct ContractFakeExecutor {
    agent: FakeAgent,
    control: ExecutionControl,
}

impl ContractFakeExecutor {
    fn for_scenario(scenario: FakeScenario) -> Self {
        Self {
            agent: FakeAgent::for_scenario(scenario),
            control: ExecutionControl::fresh(),
        }
    }
}

impl ExecutionPort for ContractFakeExecutor {
    fn provider_ref(&self) -> ProviderRef {
        ProviderRef::new(fake_agent::FAKE_PROVIDER_REF).unwrap()
    }

    fn execute(&self, work: &WorkItem) -> Result<WorkEvidence, ExecutionProblem> {
        let request = ExecutionRequest::new(
            work.task_id.clone(),
            work.execution_id.clone(),
            work.attempt_id.clone(),
        );
        match self.agent.execute(&request, &self.control) {
            Ok(output) => Ok(WorkEvidence {
                summary: output.summary().to_owned(),
                evidence_ref: output.evidence_ref().to_owned(),
                partial: output.is_partial(),
            }),
            Err(fake_agent::FakeAgentError::TimedOut { budget }) => {
                Err(ExecutionProblem::TimedOut { detail: budget })
            }
            Err(fake_agent::FakeAgentError::Cancelled) => Err(ExecutionProblem::Cancelled),
            Err(other) => Err(ExecutionProblem::Failed {
                detail: other.to_string(),
            }),
        }
    }
}

#[test]
fn fake_agent_satisfies_the_shared_execution_port_contract() {
    // Timeout is virtual here (no wall-clock sleep): the scenario
    // reports budget exhaustion deterministically.
    let success = ContractFakeExecutor::for_scenario(FakeScenario::Success);
    let failure = ContractFakeExecutor::for_scenario(FakeScenario::Crash);
    let malformed = ContractFakeExecutor::for_scenario(FakeScenario::MalformedResult);
    let timeout = ContractFakeExecutor::for_scenario(FakeScenario::Timeout);
    run_execution_port_contract(&ContractExecutors {
        success: &success,
        failure: &failure,
        malformed: &malformed,
        timeout: &timeout,
    });
}
