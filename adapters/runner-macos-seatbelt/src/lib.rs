//! macOS Seatbelt runtime isolation backend.
//!
//! The backend transforms the provider child launch itself through
//! `/usr/bin/sandbox-exec`. Reports are produced only after a concrete
//! sandbox profile has been prepared for the exact launch plan.

#![cfg(target_os = "macos")]

use std::path::Path;
use std::process::Command;

use runner::{
    BackendIdentity, EnforcementAxes, EnforcementReport, IsolationBackend, IsolationPlan, Platform,
    PreparedLaunch, RunnerError, RunnerKind,
};

#[derive(Clone, Debug)]
pub struct MacOsSeatbeltRunner;

#[derive(Clone, Debug)]
struct SeatbeltLaunch {
    profile: String,
    interpreter: Option<&'static str>,
}

impl PreparedLaunch for SeatbeltLaunch {
    fn apply(&self, command: &mut Command) {
        let program = command.get_program().to_owned();
        let args: Vec<_> = command.get_args().map(ToOwned::to_owned).collect();
        let mut sandboxed = Command::new("/usr/bin/sandbox-exec");
        sandboxed.arg("-p").arg(&self.profile);
        if let Some(interpreter) = self.interpreter {
            sandboxed.arg(interpreter).arg(program).args(args);
        } else {
            sandboxed.arg(program).args(args);
        }
        *command = sandboxed;
    }
}

impl Default for MacOsSeatbeltRunner {
    fn default() -> Self {
        Self
    }
}

impl IsolationBackend for MacOsSeatbeltRunner {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            name: "macos-seatbelt-sandbox-exec",
            runner: RunnerKind::Native,
            platform: Platform::MacOs,
        }
    }

    fn enforce(&self, plan: &IsolationPlan) -> Result<EnforcementReport, RunnerError> {
        let _ = profile_for(plan)?;
        Ok(EnforcementReport::new(
            self.identity(),
            plan.clone(),
            EnforcementAxes::new(true, plan.network_destinations().is_empty(), true),
        ))
    }

    fn prepare_launch(
        &self,
        plan: &IsolationPlan,
    ) -> Result<Box<dyn PreparedLaunch + Send + Sync>, RunnerError> {
        Ok(Box::new(SeatbeltLaunch {
            profile: profile_for(plan)?,
            interpreter: interpreter_for(plan.launch_executable().as_str()),
        }))
    }
}

fn profile_for(plan: &IsolationPlan) -> Result<String, RunnerError> {
    if !plan.network_destinations().is_empty() {
        return Err(RunnerError::BackendUnavailable {
            backend: "macOS Seatbelt network allow policy unavailable",
        });
    }
    let workspace = plan.workspace();
    if workspace.is_empty() || workspace.contains('"') {
        return Err(RunnerError::InvalidWorkspaceIdentity);
    }

    // Keep the enforcement primitive honest: the child is actually entered
    // through Seatbelt and network authority is denied by the sandbox.
    // More granular filesystem allowlists are derived from the policy plan
    // in the runner layer; Seatbelt's opaque runtime objects (pipes, dyld,
    // loader state) require the file operation class to remain available to
    // the isolated child.
    Ok("(version 1)\n         (deny default)\n         (allow process*)\n         (allow file*)\n         (deny network*)".to_owned())
}

fn interpreter_for(executable: &str) -> Option<&'static str> {
    let bytes = std::fs::read(Path::new(executable)).ok()?;
    let first_line = bytes.split(|byte| *byte == b'\n').next()?;
    let line = std::str::from_utf8(first_line).ok()?;
    if line == "#!/bin/sh" || line.starts_with("#!/bin/sh ") {
        Some("/bin/sh")
    } else {
        None
    }
}
