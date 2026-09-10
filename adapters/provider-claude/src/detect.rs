//! Deterministic Claude CLI discovery (TASK-0046, G5).
//!
//! Discovery never mutates repository state, never starts coding work,
//! never triggers interactive login, and never interpolates through a
//! shell: one fixed executable plus fixed argv (`--version`), PATH lookup
//! by explicit directory scan, bounded capture, short timeout.
//!
//! Authentication is deliberately NOT probed here: no probe can observe
//! subscription state without burning quota or risking an interactive
//! login prompt, so auth surfaces at execution time as a bounded
//! [`crate::ClaudeFailure`] instead. Detection answers installation and
//! version compatibility only.

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::config::{bounded_detail, MINIMUM_VERSION, TESTED_MAJOR_VERSION};

/// Timeout for the `--version` probe: version output is instant and local;
/// anything slower is a broken backend, not a slow one.
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// Bound for `--version` stdout: a version line, never a dump.
const PROBE_MAX_BYTES: usize = 4096;

/// Provider-neutral availability vocabulary, exactly the closed set from
/// providers.md §9 — no additions (`AUTH_REQUIRED` is intentionally NOT a
/// status: auth state is execution-time failure evidence, not an
/// installation fact, and the accepted vocabulary must not grow per
/// provider).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProviderAvailability {
    /// CLI present, version compatible.
    Available,
    /// No executable found.
    Missing,
    /// Present but answering wrongly (probe failed, output unusable).
    Degraded,
    /// Provider reports quota exhaustion.
    QuotaExhausted,
    /// Provider reports rate limiting.
    RateLimited,
    /// Provider reports policy refusal.
    PolicyBlocked,
    /// Present but version-incompatible.
    Incompatible,
    /// None of the above could be established.
    Unknown,
}

impl ProviderAvailability {
    /// Stable machine-readable name (the providers.md vocabulary).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Available => "AVAILABLE",
            Self::Missing => "MISSING",
            Self::Degraded => "DEGRADED",
            Self::QuotaExhausted => "QUOTA_EXHAUSTED",
            Self::RateLimited => "RATE_LIMITED",
            Self::PolicyBlocked => "POLICY_BLOCKED",
            Self::Incompatible => "INCOMPATIBLE",
            Self::Unknown => "UNKNOWN",
        }
    }
}

/// Parsed CLI version (plain data).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CliVersion {
    /// Major component.
    pub major: u64,
    /// Minor component.
    pub minor: u64,
    /// Patch component.
    pub patch: u64,
}

impl CliVersion {
    /// Whether this version satisfies the adapter's compatibility policy:
    /// same tested major, at least the minimum minor. Patch drift never
    /// gates (future patches stay tolerated; schema drift fails closed at
    /// parse time instead).
    #[must_use]
    pub fn is_compatible(self) -> bool {
        self.major == TESTED_MAJOR_VERSION && self.minor >= MINIMUM_VERSION.minor
    }
}

/// Detection outcome: availability plus optional version evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detection {
    /// The observed availability status.
    pub status: ProviderAvailability,
    /// Parsed version, present when the probe answered parseably.
    pub version: Option<CliVersion>,
    /// Bounded diagnostic note (never a credential, never a transcript).
    pub detail: String,
}

/// Resolves an executable name through `PATH` by explicit directory scan
/// (no shell, no `which` subprocess). Returns the first executable file
/// found. An explicit path (containing a separator) is returned as-is
/// when it names an existing file.
#[must_use]
pub fn find_on_path(name: &OsStr) -> Option<PathBuf> {
    let raw = name.as_encoded_bytes();
    if raw.is_empty() || raw.contains(&0) {
        return None;
    }
    let candidate = PathBuf::from(name);
    if raw.contains(&b'/') || raw.contains(&b'\\') {
        return candidate.is_file().then_some(candidate);
    }
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        let candidate = dir.join(name);
        if candidate.is_file() && is_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

#[cfg(unix)]
fn is_executable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    // Execute-bit heuristic for PATH lookup only: the spawn itself is the
    // real check (permission errors surface as bounded probe failures).
    // No new dependency for a lookup heuristic.
    path.is_file()
        && std::fs::metadata(path).is_ok_and(|meta| meta.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &std::path::Path) -> bool {
    path.is_file()
}

/// Runs `<executable> --version` and classifies the outcome.
///
/// Never triggers login, never starts work: `--version` is a local,
/// non-interactive probe. `executable` may be a bare name (resolved via
/// [`find_on_path`]) or an explicit path.
#[must_use]
pub fn detect(executable: &OsStr) -> Detection {
    let resolved = match find_on_path(executable) {
        Some(path) => path.into_os_string(),
        None => {
            return Detection {
                status: ProviderAvailability::Missing,
                version: None,
                detail: bounded_detail(format!(
                    "claude executable not found: {}",
                    executable.to_string_lossy()
                )),
            };
        }
    };
    match probe_version(&resolved) {
        Ok((version, raw)) => {
            if version.is_compatible() {
                Detection {
                    status: ProviderAvailability::Available,
                    version: Some(version),
                    detail: bounded_detail(format!("claude CLI {raw}")),
                }
            } else {
                Detection {
                    status: ProviderAvailability::Incompatible,
                    version: Some(version),
                    detail: bounded_detail(format!(
                        "claude CLI {raw} is outside the tested {}.{} line",
                        MINIMUM_VERSION.major, MINIMUM_VERSION.minor
                    )),
                }
            }
        }
        Err(detail) => Detection {
            status: ProviderAvailability::Unknown,
            version: None,
            detail,
        },
    }
}

/// Runs the version probe with a short timeout and parses the answer.
fn probe_version(executable: &OsString) -> Result<(CliVersion, String), String> {
    let mut child = Command::new(executable)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| {
            bounded_detail(format!("claude version probe failed to spawn: {error}"))
        })?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return Err(bounded_detail(
                        "claude version probe exited non-zero; treating as unknown".to_owned(),
                    ));
                }
                let stdout = child
                    .stdout
                    .take()
                    .ok_or_else(|| bounded_detail("claude probe has no stdout".to_owned()))?;
                let mut buf = Vec::new();
                use std::io::Read;
                stdout
                    .take((PROBE_MAX_BYTES + 1) as u64)
                    .read_to_end(&mut buf)
                    .map_err(|error| {
                        bounded_detail(format!("claude probe output unreadable: {error}"))
                    })?;
                if buf.len() > PROBE_MAX_BYTES {
                    return Err(bounded_detail(
                        "claude version probe output exceeded bound".to_owned(),
                    ));
                }
                let _ = child.wait();
                let raw = String::from_utf8_lossy(&buf).trim().to_owned();
                let version = parse_version(&raw)
                    .ok_or_else(|| bounded_detail(format!("claude version unparseable: {raw}")))?;
                return Ok((version, raw));
            }
            Ok(None) => {
                if start.elapsed() > PROBE_TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(bounded_detail(
                        "claude version probe timed out; treating as unknown".to_owned(),
                    ));
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => {
                return Err(bounded_detail(format!("claude probe wait failed: {error}")));
            }
        }
    }
}

/// Parses the first `major.minor.patch` triple in `raw`.
///
/// Tolerant by design: `2.1.263 (Claude Code)`, `2.1.263`, and
/// `claude 2.1.263` all parse; future patch releases parse without
/// adapter changes. Non-numeric or missing triples return `None`.
#[must_use]
pub fn parse_version(raw: &str) -> Option<CliVersion> {
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let (major, j) = take_number(bytes, i)?;
            let j = skip_dot(bytes, j)?;
            let (minor, j) = take_number(bytes, j)?;
            let j = skip_dot(bytes, j)?;
            let (patch, _) = take_number(bytes, j)?;
            return Some(CliVersion {
                major,
                minor,
                patch,
            });
        }
        i += 1;
    }
    None
}

fn take_number(bytes: &[u8], mut i: usize) -> Option<(u64, usize)> {
    let start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == start || i - start > 10 {
        return None;
    }
    std::str::from_utf8(&bytes[start..i])
        .ok()?
        .parse()
        .ok()
        .map(|n| (n, i))
}

fn skip_dot(bytes: &[u8], i: usize) -> Option<usize> {
    if bytes.get(i) == Some(&b'.') {
        Some(i + 1)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_parsing_tolerates_vendor_suffixes() {
        assert_eq!(
            parse_version("2.1.263 (Claude Code)"),
            Some(CliVersion {
                major: 2,
                minor: 1,
                patch: 263
            })
        );
        assert_eq!(
            parse_version("2.1.263"),
            Some(CliVersion {
                major: 2,
                minor: 1,
                patch: 263
            })
        );
        assert_eq!(
            parse_version("claude version 10.20.30"),
            Some(CliVersion {
                major: 10,
                minor: 20,
                patch: 30
            })
        );
        assert_eq!(parse_version("no version here"), None);
        assert_eq!(parse_version("2.1"), None);
        assert_eq!(parse_version(""), None);
        assert_eq!(parse_version("v2.x.3"), None);
    }

    #[test]
    fn compatibility_pins_tested_major_tolerates_patches() {
        let current = CliVersion {
            major: 2,
            minor: 1,
            patch: 263,
        };
        assert!(current.is_compatible());
        assert!(CliVersion {
            major: 2,
            minor: 5,
            patch: 0
        }
        .is_compatible());
        assert!(CliVersion {
            major: 2,
            minor: 2,
            patch: 0
        }
        .is_compatible());
        assert!(!CliVersion {
            major: 2,
            minor: 0,
            patch: 9
        }
        .is_compatible());
        assert!(!CliVersion {
            major: 1,
            minor: 9,
            patch: 9
        }
        .is_compatible());
        assert!(!CliVersion {
            major: 3,
            minor: 0,
            patch: 0
        }
        .is_compatible());
    }

    #[test]
    fn availability_vocabulary_matches_providers_md() {
        let statuses = [
            ProviderAvailability::Available,
            ProviderAvailability::Missing,
            ProviderAvailability::Degraded,
            ProviderAvailability::QuotaExhausted,
            ProviderAvailability::RateLimited,
            ProviderAvailability::PolicyBlocked,
            ProviderAvailability::Incompatible,
            ProviderAvailability::Unknown,
        ];
        let names: Vec<&str> = statuses.iter().map(|s| s.as_str()).collect();
        assert_eq!(
            names,
            [
                "AVAILABLE",
                "MISSING",
                "DEGRADED",
                "QUOTA_EXHAUSTED",
                "RATE_LIMITED",
                "POLICY_BLOCKED",
                "INCOMPATIBLE",
                "UNKNOWN",
            ]
        );
    }

    #[test]
    fn missing_executable_is_missing_never_unknown() {
        let detection = detect(OsStr::new("runlith-definitely-not-a-binary-xyz"));
        assert_eq!(detection.status, ProviderAvailability::Missing);
        assert_eq!(detection.version, None);
    }

    #[test]
    fn find_on_path_rejects_empty_and_nul() {
        assert!(find_on_path(OsStr::new("")).is_none());
        assert!(find_on_path(OsStr::new("a\0b")).is_none());
    }

    #[test]
    fn real_claude_detection_reports_available_here() {
        // Environment-coupled documentation test: when a CLI is installed
        // it must satisfy the compatibility policy (fails loudly on
        // INCOMPATIBLE/UNKNOWN so drift is visible). A missing
        // installation skips gracefully — deterministic CI without the
        // CLI must stay green.
        let detection = detect(OsStr::new("claude"));
        if detection.status == ProviderAvailability::Missing {
            return;
        }
        assert_eq!(detection.status, ProviderAvailability::Available);
        assert!(detection.version.is_some_and(|v| v.is_compatible()));
    }
}
