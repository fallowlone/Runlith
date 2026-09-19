//! Adapter-owned Claude provider configuration (TASK-0046, G5).
//!
//! Every field here is provider-specific and lives at the composition
//! layer: executable selection, version policy, process bounds, and
//! provider-native execution switches. Nothing here enters Task
//! persistence, the Event Journal, or domain semantics.

use std::ffi::{OsStr, OsString};
use std::time::Duration;

/// Default executable name resolved through `PATH` lookup.
pub const DEFAULT_EXECUTABLE: &str = "claude";

/// Tested CLI major version. Capability and schema assumptions in this
/// adapter were proven against a 2.x CLI; other majors are reported
/// `INCOMPATIBLE` (fail closed — a future major may change flags or the
/// JSON contract). Minor/patch drift within the major is tolerated:
/// schema drift surfaces as bounded `MalformedOutput` at runtime, never
/// as silent misinterpretation.
pub const TESTED_MAJOR_VERSION: u64 = 2;

/// Minimum accepted version within the tested major.
pub const MINIMUM_VERSION: CliMinimum = CliMinimum {
    major: TESTED_MAJOR_VERSION,
    minor: 1,
};

/// Minimum accepted CLI version (plain data, no behavior).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CliMinimum {
    /// Required major (exact: other majors are incompatible).
    pub major: u64,
    /// Required minor (this and newer within the major).
    pub minor: u64,
}

/// Default wall-clock bound for one provider execution.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(300);

/// Default grace period for capture finalization after the child exits:
/// once the CLI parent is reaped, drain threads get this long to observe
/// pipe EOF before the adapter finalizes with the bytes collected so far
/// (TASK-0047, G5). Bounds daemon reconciliation against a double-forked
/// daemon grandchild holding an inherited pipe open.
pub const DEFAULT_CAPTURE_GRACE: Duration = Duration::from_secs(5);

/// Upper bound for captured CLI stdout/stderr each. Matches the `git-cli`
/// precedent: commands return bounded evidence, never unbounded dumps.
pub const DEFAULT_MAX_OUTPUT_BYTES: usize = 1024 * 1024;

/// Upper bound for the canonical evidence summary handed to orchestration.
/// Matches the `fake-agent` precedent (`MAX_SUMMARY_LEN`): operational
/// evidence, never a transcript.
pub const DEFAULT_MAX_SUMMARY_CHARS: usize = 1024;

/// Upper bound for the task prompt handed to the CLI.
pub const MAX_PROMPT_CHARS: usize = 8192;

/// Upper bound for diagnostic detail strings handed to higher layers.
/// Matches the `git-cli` precedent (`MAX_DETAIL_CHARS`).
pub const MAX_DETAIL_CHARS: usize = 500;

/// Adapter-owned provider configuration.
///
/// Constructed by the composition layer (or tests); consumed only inside
/// this adapter. The orchestrator never sees it.
#[derive(Debug, Clone)]
pub struct ClaudeConfig {
    /// CLI executable: name resolved via `PATH`, or an explicit override
    /// path (tests, hermetic fixtures). Never a shell string.
    executable: OsString,
    /// Wall-clock bound for one execution (kill + `TimedOut` past it).
    timeout: Duration,
    /// Per-stream capture bound (stdout/stderr each).
    max_output_bytes: usize,
    /// Canonical summary bound.
    max_summary_chars: usize,
    /// Provider-native permission mode passed as `--permission-mode`, if
    /// set. `None` keeps provider-native default behavior (no override).
    /// Values that would weaken the provider's own safety controls
    /// (`bypassPermissions`, and any mode spelling containing it) are
    /// refused at construction — G5 must not normalize YOLO execution.
    permission_mode: Option<String>,
    /// Opaque provider session to resume via `--resume`, if set. Never a
    /// Task/Execution/Attempt identity — provider state only, validated as
    /// opaque bounded text.
    resume_session: Option<String>,
    /// Session system prompt passed as `--system-prompt`, replacing the
    /// provider's default. Adapter-authored only: it states the role the
    /// session is, so the role stops competing with the provider's own
    /// prompt from inside the message body (TASK-0106 D5).
    system_prompt: Option<String>,
    /// Structural proposal-only mode. Emits `--tools ""` plus
    /// `--disallowedTools "mcp__*"`, so the child has no tool to call at
    /// all. Stronger than a permission mode, which only asks the model to
    /// refrain (TASK-0109).
    proposal_only: bool,
    /// Grace period for capture finalization after natural child exit.
    /// Bounds the join of the stdout/stderr drain threads once the child
    /// is reaped; detached threads never block execution finalization.
    capture_grace: Duration,
}

impl Default for ClaudeConfig {
    fn default() -> Self {
        Self {
            executable: OsString::from(DEFAULT_EXECUTABLE),
            timeout: DEFAULT_TIMEOUT,
            max_output_bytes: DEFAULT_MAX_OUTPUT_BYTES,
            max_summary_chars: DEFAULT_MAX_SUMMARY_CHARS,
            permission_mode: None,
            resume_session: None,
            system_prompt: None,
            proposal_only: false,
            capture_grace: DEFAULT_CAPTURE_GRACE,
        }
    }
}

impl ClaudeConfig {
    /// Explicit executable path or name (fixture doubles, custom installs).
    /// Must be non-blank and NUL-free; resolution happens at spawn time.
    pub fn with_executable(mut self, executable: impl Into<OsString>) -> Result<Self, String> {
        let executable = executable.into();
        if executable.is_empty()
            || executable.as_encoded_bytes().contains(&0)
            || executable.to_string_lossy().trim().is_empty()
        {
            return Err("claude executable must be non-blank and NUL-free".to_owned());
        }
        self.executable = executable;
        Ok(self)
    }

    /// Wall-clock bound for one execution. Must be non-zero.
    pub fn with_timeout(mut self, timeout: Duration) -> Result<Self, String> {
        if timeout.is_zero() {
            return Err("claude execution timeout must be non-zero".to_owned());
        }
        self.timeout = timeout;
        Ok(self)
    }

    /// Provider-native `--permission-mode` override. Refuses values that
    /// would bypass the provider's own permission system.
    /// Replaces the provider's default system prompt for this session.
    /// Adapter-authored text only; never operator or model input.
    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Result<Self, String> {
        let prompt = prompt.into();
        if prompt.trim().is_empty() {
            return Err("system prompt must not be blank".to_owned());
        }
        if prompt.chars().count() > MAX_PROMPT_CHARS {
            return Err("system prompt exceeds the provider prompt bound".to_owned());
        }
        self.system_prompt = Some(prompt);
        Ok(self)
    }

    /// Removes every built-in and MCP tool from the child process.
    #[must_use]
    pub fn proposal_only(mut self) -> Self {
        self.proposal_only = true;
        self
    }

    /// Session system prompt, when one replaces the provider default.
    #[must_use]
    pub fn system_prompt(&self) -> Option<&str> {
        self.system_prompt.as_deref()
    }

    /// Whether this configuration removes all tool capability.
    #[must_use]
    pub fn is_proposal_only(&self) -> bool {
        self.proposal_only
    }

    pub fn with_permission_mode(mut self, mode: impl Into<String>) -> Result<Self, String> {
        let mode = mode.into();
        let trimmed = mode.trim();
        if trimmed.is_empty() || trimmed.len() > 64 || trimmed.contains('\0') {
            return Err("claude permission mode must be non-blank bounded text".to_owned());
        }
        let lowered = trimmed.to_ascii_lowercase();
        if lowered.contains("bypass") {
            return Err("claude permission bypass modes are never set by this adapter".to_owned());
        }
        self.permission_mode = Some(trimmed.to_owned());
        Ok(self)
    }

    /// Opaque provider session reference for `--resume`. Validated as
    /// bounded opaque text only — never interpreted, never a durable id.
    pub fn with_resume_session(mut self, session: impl Into<String>) -> Result<Self, String> {
        let session = session.into();
        let trimmed = session.trim();
        if trimmed.is_empty()
            || trimmed.len() > 256
            || trimmed.contains('\0')
            || trimmed.starts_with('-')
        {
            return Err("claude resume session must be non-blank bounded text".to_owned());
        }
        self.resume_session = Some(trimmed.to_owned());
        Ok(self)
    }

    /// The configured executable (name or path).
    #[must_use]
    pub fn executable(&self) -> &OsStr {
        &self.executable
    }

    /// The execution wall-clock bound.
    #[must_use]
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Per-stream capture bound.
    #[must_use]
    pub fn max_output_bytes(&self) -> usize {
        self.max_output_bytes
    }

    /// Canonical summary bound.
    #[must_use]
    pub fn max_summary_chars(&self) -> usize {
        self.max_summary_chars
    }

    /// The configured permission-mode override, if any.
    #[must_use]
    pub fn permission_mode(&self) -> Option<&str> {
        self.permission_mode.as_deref()
    }

    /// The configured resume session, if any.
    #[must_use]
    pub fn resume_session(&self) -> Option<&str> {
        self.resume_session.as_deref()
    }

    /// Grace period for capture finalization after natural child exit.
    /// Must be non-zero: a zero grace would discard racing tail output
    /// on every fast exit.
    pub fn with_capture_grace(mut self, grace: Duration) -> Result<Self, String> {
        if grace.is_zero() {
            return Err("claude capture grace must be non-zero".to_owned());
        }
        self.capture_grace = grace;
        Ok(self)
    }

    /// The capture-finalization grace period.
    #[must_use]
    pub fn capture_grace(&self) -> Duration {
        self.capture_grace
    }
}

/// Truncates diagnostic text to [`MAX_DETAIL_CHARS`] so provider output can
/// only ever travel upward as bounded evidence.
#[must_use]
pub fn bounded_detail(text: String) -> String {
    if text.chars().count() <= MAX_DETAIL_CHARS {
        return text;
    }
    let truncated: String = text.chars().take(MAX_DETAIL_CHARS).collect();
    format!("{truncated}…[truncated]")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_keep_provider_native_behavior() {
        let config = ClaudeConfig::default();
        assert_eq!(config.executable(), OsStr::new("claude"));
        assert_eq!(config.timeout(), DEFAULT_TIMEOUT);
        assert_eq!(config.permission_mode(), None);
        assert_eq!(config.resume_session(), None);
    }

    #[test]
    fn executable_rejects_blank_and_nul() {
        assert!(ClaudeConfig::default().with_executable("").is_err());
        assert!(ClaudeConfig::default().with_executable("  ").is_err());
        assert!(ClaudeConfig::default().with_executable("a\0b").is_err());
    }

    #[test]
    fn timeout_rejects_zero() {
        assert!(ClaudeConfig::default()
            .with_timeout(Duration::ZERO)
            .is_err());
        assert!(ClaudeConfig::default()
            .with_timeout(Duration::from_secs(1))
            .is_ok());
    }

    #[test]
    fn permission_bypass_modes_are_refused() {
        for mode in ["bypassPermissions", "BYPASS", "acceptBypassEdits"] {
            assert!(
                ClaudeConfig::default().with_permission_mode(mode).is_err(),
                "{mode:?} must be refused"
            );
        }
        assert!(ClaudeConfig::default().with_permission_mode("plan").is_ok());
        assert!(ClaudeConfig::default().with_permission_mode("").is_err());
    }

    #[test]
    fn resume_session_is_opaque_bounded_text() {
        assert!(ClaudeConfig::default().with_resume_session("").is_err());
        assert!(ClaudeConfig::default()
            .with_resume_session("x".repeat(257))
            .is_err());
        assert!(ClaudeConfig::default()
            .with_resume_session("--dangerous-provider-flag")
            .is_err());
        let ok = ClaudeConfig::default()
            .with_resume_session("550e8400-e29b-41d4-a716-446655440000")
            .unwrap();
        assert_eq!(
            ok.resume_session(),
            Some("550e8400-e29b-41d4-a716-446655440000")
        );
    }

    #[test]
    fn detail_is_bounded() {
        let long = "x".repeat(MAX_DETAIL_CHARS + 100);
        let out = bounded_detail(long);
        assert!(out.chars().count() <= MAX_DETAIL_CHARS + "…[truncated]".chars().count());
        assert!(out.ends_with("…[truncated]"));
    }

    #[test]
    fn capture_grace_defaults_and_rejects_zero() {
        assert_eq!(
            ClaudeConfig::default().capture_grace(),
            DEFAULT_CAPTURE_GRACE
        );
        assert!(ClaudeConfig::default()
            .with_capture_grace(Duration::ZERO)
            .is_err());
        let ok = ClaudeConfig::default()
            .with_capture_grace(Duration::from_secs(1))
            .unwrap();
        assert_eq!(ok.capture_grace(), Duration::from_secs(1));
    }
}
