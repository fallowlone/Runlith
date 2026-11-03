//! One bounded, tool-less provider call whose answer is held to a JSON schema
//! (TASK-0230, G16).
//!
//! The caller owns the meaning of the answer: this adapter returns its text
//! untouched and the caller validates it as untrusted data (INV-093). Like the
//! planner, an unattached call fails closed: production must attach a verified
//! [`runner::SandboxedLaunch`] first.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use runner::{ChildSecretEnvironment, SandboxedLaunch};

use crate::config::ClaudeConfig;
use crate::executor::{ClaudeExecutor, ObservedPromptRun};
use crate::failure::ClaudeFailure;
use crate::process::CancelFlag;

/// A provider call with no tools, a fixed role and a fixed answer schema.
#[derive(Clone)]
pub struct ClaudeStructuredCall {
    executor: ClaudeExecutor,
}

impl ClaudeStructuredCall {
    /// Binds a call that cannot spawn until a verified launch is attached.
    pub fn bind(
        config: ClaudeConfig,
        role: &str,
        schema: &str,
        workdir: impl Into<PathBuf>,
        environment_base: impl Into<PathBuf>,
    ) -> Result<Self, String> {
        let config = structured_config(config, role, schema)?;
        Ok(Self {
            executor: ClaudeExecutor::bind(config, role, workdir.into(), environment_base.into())?,
        })
    }

    /// Unmanaged launch for fixture tests and explicitly trusted local hosts.
    #[cfg(any(test, feature = "trusted-host"))]
    pub fn bind_trusted_host(
        config: ClaudeConfig,
        role: &str,
        schema: &str,
        workdir: impl Into<PathBuf>,
        environment_base: impl Into<PathBuf>,
    ) -> Result<Self, String> {
        let config = structured_config(config, role, schema)?;
        Ok(Self {
            executor: ClaudeExecutor::bind_trusted_host(
                config,
                role,
                workdir.into(),
                environment_base.into(),
            )?,
        })
    }

    /// Cancelling this flag kills the provider child.
    #[must_use]
    pub fn with_cancel_flag(mut self, cancel: CancelFlag) -> Self {
        self.executor = self.executor.with_cancel_flag(cancel);
        self
    }

    /// Attaches the G6/Runner-verified launch and its late-bound secrets.
    #[must_use]
    pub fn with_sandboxed_launch_and_secrets(
        mut self,
        launch: Arc<SandboxedLaunch>,
        secrets: Option<ChildSecretEnvironment>,
    ) -> Self {
        self.executor = self
            .executor
            .with_sandboxed_launch_and_secrets(launch, secrets);
        self
    }

    /// Exact executable identity for G6 authorization and Runner planning.
    #[must_use]
    pub fn executable(&self) -> &std::ffi::OsStr {
        self.executor.executable()
    }

    /// Empty isolation directory bound at construction.
    #[must_use]
    pub fn workdir(&self) -> &Path {
        self.executor.workdir()
    }

    /// Exact explicit provider child environment for Runner planning.
    pub fn child_environment(
        &self,
    ) -> Result<Vec<(std::ffi::OsString, std::ffi::OsString)>, String> {
        self.executor.child_environment()
    }

    /// Provider-owned read roots required by the concrete isolation backend.
    pub fn isolation_support_read_roots(&self) -> Result<Vec<PathBuf>, String> {
        self.executor.isolation_support_read_roots()
    }

    /// Provider-owned write roots required by the concrete isolation backend.
    pub fn isolation_support_write_roots(&self) -> Result<Vec<PathBuf>, String> {
        self.executor.isolation_support_write_roots()
    }

    /// Runs the call and returns the provider's answer text, still untrusted.
    pub fn run(&self, prompt: &str) -> Result<String, ClaudeFailure> {
        match self.executor.run_prompt_observed(prompt) {
            ObservedPromptRun::NotStarted(failure) => Err(failure),
            ObservedPromptRun::Started { result, .. } => {
                result.map(|outcome| outcome.provider_result)
            }
        }
    }
}

fn structured_config(
    config: ClaudeConfig,
    role: &str,
    schema: &str,
) -> Result<ClaudeConfig, String> {
    config
        .proposal_only()
        .with_system_prompt(role)?
        .with_json_schema(schema)
}
