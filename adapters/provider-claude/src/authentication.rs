//! Pre-launch authentication state for the Claude CLI (providers.md §9a).
//!
//! Authentication is a precondition verified before a launch, never a
//! failure discovered during one. The probe obeys the §2 detection
//! discipline: one fixed executable, fixed argv, no shell, stdin closed,
//! bounded capture, short timeout. It answers from local state, consumes no
//! quota, and can never present an interactive prompt.
//!
//! This observes *that* the provider is signed in, never *what* it is signed
//! in with: the parsed field is a boolean, and no other field is read.
//! Credentials are not read, persisted, logged, forwarded, or repaired (§7).

use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::config::bounded_detail;
use crate::detect::find_on_path;
use crate::json::parse_json;

/// Timeout for the auth-state probe: the CLI answers from local state, so
/// anything slower is a broken backend rather than a slow one.
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// Bound for probe stdout: a small status object, never a dump.
const PROBE_MAX_BYTES: usize = 8192;

/// Fixed argv. Never a login, never a mutation, never interactive.
const PROBE_ARGV: [&str; 2] = ["auth", "status"];

/// Accepted provider-neutral authentication vocabulary (providers.md §9a) —
/// exactly these states, no additions, no removals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AuthenticationState {
    /// The provider reports a usable signed-in identity.
    Authenticated,
    /// The provider reports no signed-in identity.
    Unauthenticated,
    /// Neither could be established. Not permission to proceed.
    Unknown,
}

impl AuthenticationState {
    /// Stable machine-readable name (the providers.md §9a vocabulary).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Authenticated => "AUTHENTICATED",
            Self::Unauthenticated => "UNAUTHENTICATED",
            Self::Unknown => "UNKNOWN",
        }
    }

    /// Whether a launch requiring authentication may proceed. `Unknown`
    /// refuses like `Unauthenticated`: an unestablished state is not consent.
    #[must_use]
    pub const fn permits_launch(self) -> bool {
        matches!(self, Self::Authenticated)
    }
}

/// Observed authentication state plus a bounded operator-facing note.
#[derive(Debug, Clone)]
pub struct Authentication {
    /// The observed state.
    pub state: AuthenticationState,
    /// Bounded diagnostic (never a credential, never a transcript).
    pub detail: String,
}

impl Authentication {
    /// The refusal text for a launch blocked on authentication. It names the
    /// operator action, because the runtime never performs a login itself.
    #[must_use]
    pub fn refusal(&self) -> String {
        bounded_detail(format!(
            "provider authentication required ({}): {}; run `claude auth login` \
             in an interactive terminal, then retry",
            self.state.as_str(),
            self.detail
        ))
    }
}

/// Runs `<executable> auth status` and classifies the outcome.
///
/// `executable` may be a bare name (resolved via [`find_on_path`]) or an
/// explicit path. A missing executable is [`AuthenticationState::Unknown`]
/// rather than `Unauthenticated`: absence of the CLI is an installation fact
/// that [`crate::detect`] owns, and this probe must not claim to have
/// observed a signed-out identity it never asked.
#[must_use]
pub fn probe_authentication(executable: &OsStr) -> Authentication {
    let Some(resolved) = find_on_path(executable) else {
        return Authentication {
            state: AuthenticationState::Unknown,
            detail: bounded_detail(format!(
                "claude executable not found: {}",
                executable.to_string_lossy()
            )),
        };
    };
    match probe(&resolved.into_os_string()) {
        Ok(raw) => classify(&raw),
        Err(detail) => Authentication {
            state: AuthenticationState::Unknown,
            detail,
        },
    }
}

/// Maps probe stdout onto the §9a vocabulary. Only `loggedIn` is read.
fn classify(raw: &str) -> Authentication {
    let Ok(json) = parse_json(raw) else {
        return Authentication {
            state: AuthenticationState::Unknown,
            detail: bounded_detail("claude auth probe output was not JSON".to_owned()),
        };
    };
    match json.get("loggedIn").and_then(crate::json::Json::as_bool) {
        Some(true) => Authentication {
            state: AuthenticationState::Authenticated,
            detail: bounded_detail("claude CLI reports a signed-in identity".to_owned()),
        },
        Some(false) => Authentication {
            state: AuthenticationState::Unauthenticated,
            detail: bounded_detail("claude CLI reports no signed-in identity".to_owned()),
        },
        None => Authentication {
            state: AuthenticationState::Unknown,
            detail: bounded_detail(
                "claude auth probe output carried no usable loggedIn field".to_owned(),
            ),
        },
    }
}

/// Bounded, non-interactive capture of the probe's stdout.
fn probe(executable: &OsString) -> Result<String, String> {
    let mut child = Command::new(executable)
        .args(PROBE_ARGV)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| bounded_detail(format!("claude auth probe failed to spawn: {error}")))?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return Err(bounded_detail(
                        "claude auth probe exited non-zero; state not established".to_owned(),
                    ));
                }
                let stdout = child
                    .stdout
                    .take()
                    .ok_or_else(|| bounded_detail("claude auth probe has no stdout".to_owned()))?;
                let mut buf = Vec::new();
                stdout
                    .take((PROBE_MAX_BYTES + 1) as u64)
                    .read_to_end(&mut buf)
                    .map_err(|error| {
                        bounded_detail(format!("claude auth probe output unreadable: {error}"))
                    })?;
                let _ = child.wait();
                if buf.len() > PROBE_MAX_BYTES {
                    return Err(bounded_detail(
                        "claude auth probe output exceeded bound".to_owned(),
                    ));
                }
                return Ok(String::from_utf8_lossy(&buf).trim().to_owned());
            }
            Ok(None) => {
                if start.elapsed() > PROBE_TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(bounded_detail(
                        "claude auth probe timed out; state not established".to_owned(),
                    ));
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(bounded_detail(format!(
                    "claude auth probe could not be awaited: {error}"
                )));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_in_state_permits_launch_and_others_refuse() {
        assert!(AuthenticationState::Authenticated.permits_launch());
        assert!(!AuthenticationState::Unauthenticated.permits_launch());
        // An unestablished state is not consent: this is the whole point of
        // verifying before launch rather than discovering during one.
        assert!(!AuthenticationState::Unknown.permits_launch());
    }

    #[test]
    fn classify_reads_only_the_signed_in_flag() {
        assert_eq!(
            classify(r#"{"loggedIn":true,"authMethod":"claude.ai"}"#).state,
            AuthenticationState::Authenticated
        );
        assert_eq!(
            classify(r#"{"loggedIn":false,"authMethod":"none"}"#).state,
            AuthenticationState::Unauthenticated
        );
        assert_eq!(
            classify(r#"{"authMethod":"claude.ai"}"#).state,
            AuthenticationState::Unknown,
            "a missing flag is unestablished, never a signed-in assumption"
        );
        assert_eq!(
            classify("Not logged in · Please run /login").state,
            AuthenticationState::Unknown,
            "non-JSON output is unestablished, not a parsed verdict"
        );
    }

    #[test]
    fn classified_state_never_carries_credential_material() {
        // The probe output of a real signed-in CLI includes account
        // identifiers. None of them may reach the detail an operator sees.
        let raw = r#"{"loggedIn":true,"authMethod":"claude.ai",
            "email":"person@example.invalid","orgId":"org-secret-identifier"}"#;
        let observed = classify(raw);
        assert_eq!(observed.state, AuthenticationState::Authenticated);
        assert!(!observed.detail.contains("example.invalid"));
        assert!(!observed.detail.contains("org-secret-identifier"));
        assert!(!observed.refusal().contains("org-secret-identifier"));
    }

    #[test]
    fn missing_executable_is_unestablished_not_signed_out() {
        let observed = probe_authentication(OsStr::new("claude-not-installed-anywhere"));
        assert_eq!(
            observed.state,
            AuthenticationState::Unknown,
            "absence of the CLI is an installation fact detect() owns"
        );
    }

    #[test]
    fn refusal_names_the_operator_action() {
        let refusal = Authentication {
            state: AuthenticationState::Unauthenticated,
            detail: "claude CLI reports no signed-in identity".to_owned(),
        }
        .refusal();
        assert!(refusal.contains("UNAUTHENTICATED"));
        assert!(
            refusal.contains("claude auth login"),
            "the runtime never logs in itself, so the refusal must say who does: {refusal}"
        );
    }
}
