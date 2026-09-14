//! Trusted-host NativeRunner adapter (TASK-0064, G6).
//!
//! This adapter intentionally installs no OS sandbox restrictions and reports
//! none. It can therefore satisfy only `IsolationRequirements::trusted_host()`.
//! Any required filesystem/network/process isolation is rejected by the shared
//! runner verifier instead of silently downgrading to host execution.
//!
//! It *does* install child environment isolation (TASK-SEC-R1 R1-H2) and it
//! owns the launch path (TASK-SEC-R1 R1-H1): the ambient environment is
//! cleared and exactly the plan's explicit allowlist is installed, and the
//! backend hands back the sandboxed launch recipe instead of a command object
//! any caller could spawn unmodified.

use runner::{
    BackendIdentity, ChildEnvironment, CommandSpec, EnforcementAxes, EnforcementReport,
    IsolationBackend, IsolationPlan, LaunchRecipe, Platform, RunnerError, RunnerKind,
};
use std::ffi::OsString;

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

    /// No OS sandbox axis is installed on this backend, so the child may run
    /// wherever the host allows. The environment is still enforced: the recipe
    /// carries exactly the plan's explicit pairs and nothing ambient.
    fn sandbox(
        &self,
        plan: &IsolationPlan,
        spec: &CommandSpec,
    ) -> Result<LaunchRecipe, RunnerError> {
        Ok(LaunchRecipe::new(
            spec.program(),
            spec.args().iter().cloned(),
            environment_pairs(plan.environment()),
        ))
    }
}

/// Clears every inherited variable and installs exactly the explicit pairs.
/// Ambient credentials therefore cannot reach a governed child on this
/// backend, which enforces no other axis at all.
fn environment_pairs(environment: &ChildEnvironment) -> Vec<(OsString, OsString)> {
    environment.vars().to_vec()
}
