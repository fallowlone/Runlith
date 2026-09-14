//! Adapter-local Claude failure taxonomy (TASK-0046, G5).
//!
//! Claude-specific failures normalized at the adapter boundary into
//! stable variants. Classification uses structured signals first (spawn
//! errors, exit status, JSON `type`/`subtype`/`is_error` fields); the one
//! unavoidable textual fallback (authentication hints in stderr, which the
//! CLI reports with no stable code) is bounded, tested, adapter-local,
//! and unknown-safe — anything unrecognized stays an explicit generic
//! failure, never misclassified as success.
//!
//! This taxonomy is adapter-local, not a core contract: orchestration
//! sees only `TimedOut` / `Cancelled` / `Failed{detail}` through the
//! provider-neutral seam, and a future second provider defines its own
//! adapter-local mapping onto the same seam.

use crate::config::bounded_detail;

/// How one Claude execution failed (or which pre-execution gate refused).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaudeFailure {
    /// No `claude` executable found.
    CliMissing {
        /// Bounded diagnostic.
        detail: String,
    },
    /// CLI present but outside the tested version line.
    Incompatible {
        /// Bounded diagnostic (observed version text).
        detail: String,
    },
    /// CLI reports missing/invalid authentication. Detected via a small
    /// documented stderr-marker table (no stable exit code exists);
    /// anything not matching stays [`ClaudeFailure::Failed`].
    AuthRequired {
        /// Bounded diagnostic (marker-derived, never credential content).
        detail: String,
    },
    /// The child process could not be spawned.
    SpawnFailed {
        /// Bounded diagnostic.
        detail: String,
    },
    /// The executor was asked to spawn with no Runner sandbox attached
    /// (TASK-SEC-R1 R1-H1). Fail closed: an unattached launch is refused
    /// instead of silently running the provider child unmanaged.
    LaunchUnmanaged {
        /// Bounded diagnostic.
        detail: String,
    },
    /// The child did not conclude within the configured bound.
    TimedOut {
        /// Bounded diagnostic (configured bound).
        detail: String,
    },
    /// Cancelled through the canonical cancellation path.
    Cancelled,
    /// Non-zero exit with (or without) parseable error output.
    Failed {
        /// Bounded diagnostic (exit code + stderr excerpt / subtype).
        detail: String,
    },
    /// Exit zero but output unusable: empty, non-JSON, wrong shape, or
    /// blank result text. Never trusted as evidence.
    MalformedOutput {
        /// Bounded diagnostic (what was wrong, structurally).
        detail: String,
    },
    /// The bound workspace directory was missing or unusable. Fail closed:
    /// the provider never falls back to another directory.
    WorkspaceBinding {
        /// Bounded diagnostic.
        detail: String,
    },
    /// Mandatory provider-environment isolation could not be prepared.
    EnvironmentIsolation {
        /// Bounded diagnostic without environment values or secret material.
        detail: String,
    },
}

impl ClaudeFailure {
    /// Stable adapter-local diagnostic class (never CLI prose).
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::CliMissing { .. } => "cli_missing",
            Self::Incompatible { .. } => "incompatible",
            Self::AuthRequired { .. } => "auth_required",
            Self::SpawnFailed { .. } => "spawn_failed",
            Self::LaunchUnmanaged { .. } => "launch_unmanaged",
            Self::TimedOut { .. } => "timed_out",
            Self::Cancelled => "cancelled",
            Self::Failed { .. } => "failed",
            Self::MalformedOutput { .. } => "malformed_output",
            Self::WorkspaceBinding { .. } => "workspace_binding",
            Self::EnvironmentIsolation { .. } => "environment_isolation",
        }
    }

    /// Wraps an arbitrary detail string into the generic failure,
    /// bounded on the way in.
    #[must_use]
    pub fn failed(detail: impl Into<String>) -> Self {
        Self::Failed {
            detail: bounded_detail(detail.into()),
        }
    }

    /// Wraps an arbitrary detail string into malformed output, bounded.
    #[must_use]
    pub fn malformed(detail: impl Into<String>) -> Self {
        Self::MalformedOutput {
            detail: bounded_detail(detail.into()),
        }
    }
}

impl std::fmt::Display for ClaudeFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => write!(f, "claude execution cancelled"),
            Self::CliMissing { detail }
            | Self::Incompatible { detail }
            | Self::AuthRequired { detail }
            | Self::SpawnFailed { detail }
            | Self::TimedOut { detail }
            | Self::Failed { detail }
            | Self::MalformedOutput { detail }
            | Self::WorkspaceBinding { detail }
            | Self::EnvironmentIsolation { detail } => {
                write!(f, "claude {}: {detail}", self.kind())
            }
            Self::LaunchUnmanaged { detail } => {
                write!(f, "claude {}: {detail}", self.kind())
            }
        }
    }
}

impl std::error::Error for ClaudeFailure {}

/// Bounded textual fallback: stderr markers that reliably indicate
/// missing/invalid subscription authentication.
///
/// Documented, minimal, and unknown-safe: matching only ever produces
/// `AuthRequired`; non-matching output keeps its generic class. Markers
/// were observed against CLI 2.1.x; if a future CLI changes its wording,
/// auth failures degrade to generic `Failed` (loud, bounded, still
/// fail-closed) — never to success.
const AUTH_MARKERS: &[&str] = &[
    "Invalid API key",
    "invalid x-api-key",
    "authentication required",
    "not logged in",
    "Please run /login",
    "OAuth token",
];

/// Checks bounded stderr text against the auth-marker table.
/// Case-sensitive on purpose: these are CLI-prose fragments, and
/// case-folding would widen the match surface.
#[must_use]
pub fn looks_like_auth_failure(stderr_excerpt: &str) -> bool {
    AUTH_MARKERS
        .iter()
        .any(|marker| stderr_excerpt.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_are_stable_adapter_local_names() {
        let failures = [
            ClaudeFailure::CliMissing { detail: "x".into() },
            ClaudeFailure::Incompatible { detail: "x".into() },
            ClaudeFailure::AuthRequired { detail: "x".into() },
            ClaudeFailure::SpawnFailed { detail: "x".into() },
            ClaudeFailure::TimedOut { detail: "x".into() },
            ClaudeFailure::Cancelled,
            ClaudeFailure::Failed { detail: "x".into() },
            ClaudeFailure::MalformedOutput { detail: "x".into() },
            ClaudeFailure::WorkspaceBinding { detail: "x".into() },
            ClaudeFailure::EnvironmentIsolation { detail: "x".into() },
        ];
        let kinds: Vec<&str> = failures.iter().map(|f| f.kind()).collect();
        assert_eq!(
            kinds,
            [
                "cli_missing",
                "incompatible",
                "auth_required",
                "spawn_failed",
                "timed_out",
                "cancelled",
                "failed",
                "malformed_output",
                "workspace_binding",
                "environment_isolation",
            ]
        );
    }

    #[test]
    fn auth_markers_are_narrow_and_unknown_safe() {
        assert!(looks_like_auth_failure(
            "Error: Invalid API key · Fix: run /login"
        ));
        assert!(looks_like_auth_failure("... not logged in ..."));
        // Unknown output never matches: it stays a generic failure.
        assert!(!looks_like_auth_failure(
            "Error: model overloaded, try again"
        ));
        assert!(!looks_like_auth_failure(""));
        assert!(!looks_like_auth_failure("exit code 1"));
        // Case-folding is deliberately absent.
        assert!(!looks_like_auth_failure("invalid api key"));
    }

    #[test]
    fn constructors_bound_detail() {
        let long = "y".repeat(crate::config::MAX_DETAIL_CHARS + 10);
        let failed = ClaudeFailure::failed(long.clone());
        let malformed = ClaudeFailure::malformed(long);
        for failure in [failed, malformed] {
            let text = failure.to_string();
            assert!(
                text.chars().count()
                    <= "claude malformed_output: ".len()
                        + crate::config::MAX_DETAIL_CHARS
                        + "…[truncated]".len()
            );
        }
    }
}
