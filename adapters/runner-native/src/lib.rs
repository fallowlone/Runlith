//! Trusted-host NativeRunner adapter (TASK-0064, G6).
//!
//! This adapter intentionally installs no OS sandbox restrictions and reports
//! none. It can therefore satisfy only `IsolationRequirements::trusted_host()`.
//! Any required filesystem/network/process isolation is rejected by the shared
//! runner verifier instead of silently downgrading to host execution.
//!
//! It *does* install child environment isolation (TASK-SEC-R1 R1-H2): the
//! ambient environment is cleared and exactly the plan's explicit allowlist is
//! installed, and the report claims only that axis.

use runner::{
    BackendIdentity, ChildEnvironment, EnforcementAxes, EnforcementReport, IsolationBackend,
    IsolationPlan, Platform, PreparedLaunch, RunnerError, RunnerKind,
};
use std::process::Command;

#[derive(Debug, Default)]
struct NativeLaunch {
    environment: ChildEnvironment,
}

impl PreparedLaunch for NativeLaunch {
    fn apply(&self, command: &mut Command) {
        install_environment(command, &self.environment);
    }
}

/// Clears every inherited variable and installs exactly the explicit pairs.
/// Ambient credentials therefore cannot reach a governed child on this
/// backend, which enforces no other axis at all.
fn install_environment(command: &mut Command, environment: &ChildEnvironment) {
    command.env_clear();
    for (name, value) in environment.vars() {
        command.env(name, value);
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NativeRunner;

impl IsolationBackend for NativeRunner {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            name: "native-trusted-host",
            runner: RunnerKind::Native,
            platform: Platform::current(),
        }
    }

    fn enforce(&self, plan: &IsolationPlan) -> Result<EnforcementReport, RunnerError> {
        Ok(EnforcementReport::new(
            self.identity(),
            plan.clone(),
            EnforcementAxes::none(),
            true,
        ))
    }

    fn prepare_launch(
        &self,
        plan: &IsolationPlan,
    ) -> Result<Box<dyn PreparedLaunch + Send + Sync>, RunnerError> {
        Ok(Box::new(NativeLaunch {
            environment: plan.environment().clone(),
        }))
    }
}
