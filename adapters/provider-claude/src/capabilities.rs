//! Provider-neutral capability discovery (TASK-0046, G5).
//!
//! Only capabilities proven by the installed CLI are advertised: each
//! flag the argv builder may emit is probed live against `claude --help`
//! output. Unknown != supported — an unobserved flag reads as absent, and
//! a failed probe degrades the whole set instead of guessing.
//!
//! Version-to-capability mapping stays inside this adapter: core sees only
//! the resulting booleans, never CLI versions or flag spellings.

use std::ffi::OsStr;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::config::bounded_detail;
use crate::detect::CliVersion;

/// Timeout for the `--help` probe (local, non-interactive, no quota).
const HELP_TIMEOUT: Duration = Duration::from_secs(10);

/// Bound for `--help` capture: help text is kilobytes, never megabytes.
const HELP_MAX_BYTES: usize = 256 * 1024;

/// Provider-neutral capabilities actually proven by the installed CLI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeCapabilities {
    /// Non-interactive execution (`-p`/`--print`).
    pub non_interactive: bool,
    /// Machine-readable single-result output (`--output-format` + `json`).
    pub structured_json: bool,
    /// Realtime streaming output (`stream-json`).
    pub streaming: bool,
    /// Session continuation (`--resume`).
    pub session_resume: bool,
    /// Explicit session identity (`--session-id`).
    pub session_control: bool,
    /// Permission-mode selection (`--permission-mode`).
    pub permission_modes: bool,
    /// Provider-native safe mode (`--safe-mode`).
    pub safe_mode: bool,
    /// Execution inside a caller-bound working directory. Always true:
    /// cwd binding is adapter-enforced (`current_dir`), not CLI-advertised.
    pub workspace_execution: bool,
    /// Process-handle cancellation (`kill`, never `pkill`). Always true:
    /// implemented by the adapter's child ownership, not by the CLI.
    pub cancellation: bool,
    /// Usage reporting via the JSON result (`usage` object). True when
    /// structured output is proven; each result still carries its own
    /// provenance (`reported` vs `unknown`) — absence is never fabricated.
    pub usage_reporting: bool,
    /// The CLI version these answers were probed against, if known.
    pub probed_version: Option<CliVersion>,
    /// Bounded diagnostic note.
    pub detail: String,
}

impl ClaudeCapabilities {
    /// All-probe-failed degradation: nothing advertised beyond the two
    /// adapter-implemented mechanics (which need no CLI cooperation).
    fn degraded(detail: String) -> Self {
        Self {
            non_interactive: false,
            structured_json: false,
            streaming: false,
            session_resume: false,
            session_control: false,
            permission_modes: false,
            safe_mode: false,
            workspace_execution: true,
            cancellation: true,
            usage_reporting: false,
            probed_version: None,
            detail,
        }
    }
}

/// Probes `<executable> --help` and derives capabilities from observed
/// flags. `version` ties the answers to a detected CLI version (may be
/// `None` when detection could not parse one).
#[must_use]
pub fn probe_capabilities(executable: &OsStr, version: Option<CliVersion>) -> ClaudeCapabilities {
    let help = match probe_help(executable) {
        Some(help) => help,
        None => {
            return ClaudeCapabilities::degraded(bounded_detail(
                "claude --help probe failed; no CLI-advertised capabilities".to_owned(),
            ));
        }
    };
    let structured_json = help.contains("--output-format") && help.contains("json");
    ClaudeCapabilities {
        non_interactive: help.contains("--print") || help.contains(" -p"),
        structured_json,
        streaming: help.contains("stream-json"),
        session_resume: help.contains("--resume"),
        session_control: help.contains("--session-id"),
        permission_modes: help.contains("--permission-mode"),
        safe_mode: help.contains("--safe-mode"),
        workspace_execution: true,
        cancellation: true,
        usage_reporting: structured_json,
        probed_version: version,
        detail: bounded_detail("capabilities probed from installed claude --help".to_owned()),
    }
}

/// Runs `claude --help` with a short timeout and bounded capture.
/// `None` on any transport failure (spawn, timeout, over-bound output).
fn probe_help(executable: &OsStr) -> Option<String> {
    let resolved = crate::detect::find_on_path(executable)?;
    let mut child = Command::new(&resolved)
        .arg("--help")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                let stdout = child.stdout.take()?;
                let mut buf = Vec::new();
                use std::io::Read;
                stdout
                    .take((HELP_MAX_BYTES + 1) as u64)
                    .read_to_end(&mut buf)
                    .ok()?;
                let _ = child.wait();
                if buf.len() > HELP_MAX_BYTES {
                    return None;
                }
                return Some(String::from_utf8_lossy(&buf).into_owned());
            }
            Ok(None) => {
                if start.elapsed() > HELP_TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(_) => return None,
        }
    }
}

/// The exact argv flags [`crate::build_argv`] may emit, for the probe
/// contract test below: capability discovery must cover every flag the
/// adapter can pass, so no flag is ever emitted unproven.
#[cfg(test)]
pub(crate) const EMITTED_FLAGS: &[&str] = &[
    "-p",
    "--safe-mode",
    "--output-format",
    "--permission-mode",
    "--resume",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_emitted_flag_has_a_capability_probe() {
        // If argv ever gains a flag, this test forces the capability
        // struct (and the help probe) to account for it.
        let covered = [
            ("-p", "non_interactive"),
            ("--safe-mode", "safe_mode"),
            ("--output-format", "structured_json"),
            ("--permission-mode", "permission_modes"),
            ("--resume", "session_resume"),
        ];
        for flag in EMITTED_FLAGS {
            assert!(
                covered.iter().any(|(f, _)| f == flag),
                "emitted flag {flag:?} has no capability probe"
            );
        }
    }

    #[test]
    fn missing_executable_degrades_without_guessing() {
        let caps = probe_capabilities(OsStr::new("runlith-definitely-not-a-binary-xyz"), None);
        assert!(!caps.non_interactive);
        assert!(!caps.structured_json);
        assert!(!caps.streaming);
        assert!(!caps.session_resume);
        assert!(!caps.safe_mode);
        assert!(!caps.usage_reporting);
        // Adapter-implemented mechanics survive: they need no CLI.
        assert!(caps.workspace_execution);
        assert!(caps.cancellation);
    }

    #[test]
    fn real_help_probe_advertises_print_and_json_here() {
        // Environment-coupled: skips gracefully when no CLI is installed
        // (deterministic CI stays green); proves the flags argv depends
        // on when one is present.
        if crate::detect::find_on_path(OsStr::new("claude")).is_none() {
            return;
        }
        let detection = crate::detect::detect(OsStr::new("claude"));
        let caps = probe_capabilities(OsStr::new("claude"), detection.version);
        assert!(caps.non_interactive, "installed CLI must support -p");
        assert!(
            caps.structured_json,
            "installed CLI must support json output"
        );
        assert!(caps.session_resume, "installed CLI must support --resume");
        assert!(caps.safe_mode, "installed CLI must support --safe-mode");
    }

    #[test]
    fn executable_name_is_not_part_of_capabilities() {
        // Provider-neutral shape: no Claude paths, versions, or flag
        // spellings leak through the struct fields core might read.
        let caps = ClaudeCapabilities::degraded("test".to_owned());
        let debug = format!("{caps:?}");
        assert!(!debug.contains("claude"));
    }
}
