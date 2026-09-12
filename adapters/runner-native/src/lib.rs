//! Trusted-host NativeRunner adapter (TASK-0064, G6).
//!
//! This adapter intentionally installs no OS sandbox restrictions and reports
//! none. It can therefore satisfy only `IsolationRequirements::trusted_host()`.
//! Any required filesystem/network/process isolation is rejected by the shared
//! runner verifier instead of silently downgrading to host execution.

use runner::{
    BackendIdentity, EnforcementAxes, EnforcementReport, IsolationBackend, IsolationPlan, Platform,
    RunnerError, RunnerKind,
};

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
        ))
    }
}
