//! Argv construction for the Claude CLI (TASK-0046, G5).
//!
//! Direct executable invocation only: the returned vector becomes
//! `Command::new(executable).args(argv)` with no shell in between, so
//! prompt text, paths, and session references containing spaces, quotes,
//! `$`, backticks, or newlines remain ordinary argv values. There is no
//! code path that joins these into a command string.
//!
//! Non-interactive machine-readable mode: `-p --output-format json` is
//! the smallest stable mode sufficient for the canonical contract (one
//! result object, no ANSI terminal scraping, no PTY).

use std::ffi::OsString;

use crate::config::ClaudeConfig;

/// Builds the full argument vector for one provider execution.
///
/// Layout: `-p --safe-mode --output-format json [--permission-mode M] [--resume S]
/// <prompt>`. The prompt is always the final positional value and is
/// never split, interpolated, or shell-escaped — argv carries it whole.
/// Only flags probed against the installed CLI are ever emitted (see
/// [`crate::probe_capabilities`]); unknown future flags are never
/// guessed here.
#[must_use]
pub fn build_argv(config: &ClaudeConfig, prompt: &str) -> Vec<OsString> {
    let mut argv = vec![
        OsString::from("-p"),
        OsString::from("--safe-mode"),
        OsString::from("--output-format"),
        OsString::from("json"),
    ];
    if let Some(mode) = config.permission_mode() {
        argv.push(OsString::from("--permission-mode"));
        argv.push(OsString::from(mode));
    }
    if let Some(session) = config.resume_session() {
        argv.push(OsString::from("--resume"));
        argv.push(OsString::from(session));
    }
    argv.push(OsString::from(prompt));
    argv
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn config() -> ClaudeConfig {
        ClaudeConfig::default()
    }

    #[test]
    fn minimal_argv_is_print_plus_json_plus_prompt() {
        let argv = build_argv(&config(), "do the thing");
        assert_eq!(argv[0], OsString::from("-p"));
        assert_eq!(argv[1], OsString::from("--safe-mode"));
        assert_eq!(argv[2], OsString::from("--output-format"));
        assert_eq!(argv[3], OsString::from("json"));
        assert_eq!(argv.last().unwrap(), &OsString::from("do the thing"));
    }

    #[test]
    fn prompt_travels_as_one_argv_value_never_split() {
        // Injection-safety core: hostile prompt text stays a single argv
        // element. There is no shell to interpret `$(...)`, backticks,
        // quotes, leading dashes, or newlines.
        let hostile = "$(rm -rf /) `evil` \"quoted\" 'single' --dangerous-flag\nnewline; semi";
        let argv = build_argv(&config(), hostile);
        let prompt_args: Vec<&OsString> = argv.iter().filter(|a| *a == hostile).collect();
        assert_eq!(
            prompt_args.len(),
            1,
            "prompt must appear exactly once, whole"
        );
        assert_eq!(argv.len(), 5, "no extra argv elements may appear");
    }

    #[test]
    fn optional_flags_emit_only_when_configured() {
        let plain = build_argv(&config(), "p");
        assert!(!plain.iter().any(|a| a == "--permission-mode"));
        assert!(!plain.iter().any(|a| a == "--resume"));

        let with_both = ClaudeConfig::default()
            .with_permission_mode("plan")
            .unwrap()
            .with_resume_session("sess-1")
            .unwrap();
        let argv = build_argv(&with_both, "p");
        let texts: Vec<String> = argv
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            texts,
            [
                "-p",
                "--safe-mode",
                "--output-format",
                "json",
                "--permission-mode",
                "plan",
                "--resume",
                "sess-1",
                "p"
            ]
        );
    }

    #[test]
    fn no_shell_primitives_anywhere_in_argv() {
        let configured = config()
            .with_timeout(Duration::from_secs(5))
            .unwrap()
            .with_executable("/usr/local/bin/claude")
            .unwrap();
        let argv = build_argv(&configured, "prompt with spaces");
        for arg in &argv {
            let text = arg.to_string_lossy();
            assert!(
                !text.contains("sh -c"),
                "shell interpolation must never appear"
            );
            assert!(
                !text.contains("bash -c"),
                "shell interpolation must never appear"
            );
        }
    }
}
