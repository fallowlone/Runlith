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
    /// The provider refused the credential presented to it: the CLI
    /// reported `api_error_status` 401 in its structured result (ADR-0039
    /// P6b, measured on CLI 2.1.289). Provider response text is not kept.
    CredentialRejected {
        /// Bounded diagnostic (fixed text, never provider prose).
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
    /// Rate-limit/quota-exhaustion/overload/5xx signal (F5). Detected via
    /// a small documented stderr-marker table like [`Self::AuthRequired`];
    /// anything not matching stays [`ClaudeFailure::Failed`]. No
    /// machine-readable retry-after exists in CLI output, so none is
    /// invented: the detail carries the bounded evidence and retry
    /// routing stays an explicit operator/orchestration decision (the
    /// scheduler performs no automatic retry per TASK-0037).
    Transient {
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
            Self::CredentialRejected { .. } => "credential_rejected",
            Self::SpawnFailed { .. } => "spawn_failed",
            Self::LaunchUnmanaged { .. } => "launch_unmanaged",
            Self::TimedOut { .. } => "timed_out",
            Self::Cancelled => "cancelled",
            Self::Failed { .. } => "failed",
            Self::Transient { .. } => "transient",
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

    /// Wraps an arbitrary detail string into the transient failure,
    /// bounded on the way in.
    #[must_use]
    pub fn transient(detail: impl Into<String>) -> Self {
        Self::Transient {
            detail: bounded_detail(detail.into()),
        }
    }

    /// The provider refused the presented credential. Fixed text only: the
    /// provider's own response is never carried.
    #[must_use]
    pub fn credential_rejected() -> Self {
        Self::CredentialRejected {
            detail: "provider_credential_rejected: the provider refused the presented \
                     credential (api_error_status 401)"
                .to_owned(),
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
            | Self::CredentialRejected { detail }
            | Self::SpawnFailed { detail }
            | Self::TimedOut { detail }
            | Self::Failed { detail }
            | Self::Transient { detail }
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

/// Display prefix of [`ClaudeFailure::CredentialRejected`].
const CREDENTIAL_REJECTED_PREFIX: &str = "claude credential_rejected: ";

/// Whether a failure rendered by this adapter (`ClaudeFailure` display, as
/// carried by `ExecutionProblem::Failed` and planner `typed_cause`) is a
/// provider credential rejection. Matches the adapter-owned prefix only, so
/// provider text inside another failure's detail cannot forge it.
#[must_use]
pub fn is_credential_rejected(rendered: &str) -> bool {
    rendered.starts_with(CREDENTIAL_REJECTED_PREFIX)
}

/// Checks bounded stderr text against the auth-marker table.
/// Case-sensitive on purpose: these are CLI-prose fragments, and
/// case-folding would widen the match surface.
#[must_use]
pub fn looks_like_auth_failure(stderr_excerpt: &str) -> bool {
    AUTH_MARKERS
        .iter()
        .any(|marker| stderr_excerpt.contains(marker))
}

/// Bounded textual fallback: stderr markers that reliably indicate
/// rate limiting, quota exhaustion, overload, or a 5xx-class upstream
/// failure.
///
/// Documented, minimal, and unknown-safe like `AUTH_MARKERS`: matching
/// only ever produces `Transient`; non-matching output keeps its generic
/// class. Markers were observed against CLI 2.1.x prose ("model
/// overloaded", HTTP status digits in error text); if a future CLI
/// changes its wording, transient failures degrade to generic `Failed`
/// (loud, bounded, still fail-closed) — never to success.
const TRANSIENT_MARKERS: &[&str] = &[
    "overloaded",
    "Overloaded",
    "over capacity",
    "rate limit",
    "Rate limit",
    "rate_limit",
    "ratelimit",
    "quota exceeded",
    "quota exhausted",
    "429",
    "529",
    "500",
    "502",
    "503",
    "temporarily unavailable",
    "try again",
];

/// Checks bounded stderr text against the transient-marker table.
/// Case-sensitive on purpose, matching the auth-marker policy.
#[must_use]
pub fn looks_like_transient_failure(stderr_excerpt: &str) -> bool {
    TRANSIENT_MARKERS
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
            ClaudeFailure::CredentialRejected { detail: "x".into() },
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
                "credential_rejected",
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
    fn transient_markers_are_narrow_and_unknown_safe() {
        // F5: rate-limit/overload/5xx signals classify as Transient;
        // anything unrecognized stays a generic failure, never success.
        assert!(looks_like_transient_failure(
            "Error: model overloaded, try again later"
        ));
        assert!(looks_like_transient_failure(
            "Rate limit reached, retry later"
        ));
        assert!(looks_like_transient_failure(
            "claude exited with code Some(1): 529 overloaded"
        ));
        assert!(looks_like_transient_failure("503 Service Unavailable"));
        assert!(!looks_like_transient_failure(
            "Error: Invalid API key · Fix: run /login"
        ));
        assert!(!looks_like_transient_failure(""));
        assert!(!looks_like_transient_failure("exit code 1"));
        let transient = ClaudeFailure::transient("upstream overloaded");
        assert_eq!(transient.kind(), "transient");
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
