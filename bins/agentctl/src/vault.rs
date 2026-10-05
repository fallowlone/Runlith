//! `agentctl vault`: operator commands for the ADR-0039 credential vault.
//!
//! Secrets (password, recovery phrase, credential) are read from the
//! terminal with echo off, never from argv or the environment. Commands that
//! print a recovery phrase refuse unless stdout is a terminal, before any
//! request is sent.

use std::process::ExitCode;

use ipc_contracts::proto::{VaultErrorCode, VaultState, VaultStatusResponse};
use ipc_contracts::{
    PROVIDER_CREDENTIAL_LIFETIME_DAYS, PROVIDER_CREDENTIAL_WARN_DAYS,
    VAULT_PROVIDER_CREDENTIAL_ENTRY,
};

pub const USAGE: &str = "agentctl vault <init|unlock|recover> [--idle-minutes <1-720>] | agentctl vault <lock|status|store|change-password|rotate>";

const SECONDS_PER_DAY: u64 = 86_400;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VaultCommand {
    Init { idle_minutes: u32 },
    Unlock { idle_minutes: u32 },
    Lock,
    Status,
    Store,
    ChangePassword,
    Recover { idle_minutes: u32 },
    Rotate,
}

/// Parses the words after `vault`. `idle_minutes = 0` asks the daemon for its
/// default; the daemon enforces the range.
pub fn parse(args: &[String]) -> Result<VaultCommand, ()> {
    let (command, rest) = args.split_first().ok_or(())?;
    match (command.as_str(), rest) {
        ("init", _) => Ok(VaultCommand::Init {
            idle_minutes: idle_option(rest)?,
        }),
        ("unlock", _) => Ok(VaultCommand::Unlock {
            idle_minutes: idle_option(rest)?,
        }),
        ("recover", _) => Ok(VaultCommand::Recover {
            idle_minutes: idle_option(rest)?,
        }),
        ("lock", []) => Ok(VaultCommand::Lock),
        ("status", []) => Ok(VaultCommand::Status),
        ("store", []) => Ok(VaultCommand::Store),
        ("change-password", []) => Ok(VaultCommand::ChangePassword),
        ("rotate", []) => Ok(VaultCommand::Rotate),
        _ => Err(()),
    }
}

fn idle_option(rest: &[String]) -> Result<u32, ()> {
    match rest {
        [] => Ok(0),
        [flag, value] if flag == "--idle-minutes" => value.parse().map_err(|_| ()),
        _ => Err(()),
    }
}

/// Human-readable status lines; `now_ms` is wall-clock Unix milliseconds.
pub fn status_lines(status: &VaultStatusResponse, now_ms: u64) -> Vec<String> {
    match status.state() {
        VaultState::VaultAbsent => {
            vec!["vault: absent; create it with `agentctl vault init`".to_owned()]
        }
        VaultState::VaultLocked => {
            vec!["vault: locked; unlock it with `agentctl vault unlock`".to_owned()]
        }
        VaultState::Unspecified => vec!["vault: state unknown".to_owned()],
        VaultState::VaultUnlocked => {
            let mut lines = vec![match status.unlocked_until_ms {
                Some(until_ms) => format!(
                    "vault: unlocked, locks in {} min",
                    until_ms.saturating_sub(now_ms).div_ceil(60_000)
                ),
                None => "vault: unlocked".to_owned(),
            }];
            if status.entries.is_empty() {
                lines.push(
                    "no credential stored; run `claude setup-token`, then `agentctl vault store`"
                        .to_owned(),
                );
            }
            for entry in &status.entries {
                let age_days = (now_ms / 1000).saturating_sub(entry.created_at_s) / SECONDS_PER_DAY;
                lines.push(format!("entry {}: stored {age_days} days ago", entry.name));
                if entry.name == VAULT_PROVIDER_CREDENTIAL_ENTRY {
                    lines.extend(age_warning(&entry.name, age_days));
                }
                if entry.rejected {
                    lines.push(format!(
                        "warning: {} was rejected by the provider; create a new token with \
                         `claude setup-token` and run `agentctl vault store`",
                        entry.name
                    ));
                }
            }
            lines
        }
    }
}

fn age_warning(name: &str, age_days: u64) -> Option<String> {
    const RENEW: &str =
        "create a new token with `claude setup-token` and run `agentctl vault store`";
    if age_days >= PROVIDER_CREDENTIAL_LIFETIME_DAYS {
        Some(format!(
            "warning: {name} is past its one-year lifetime; {RENEW}"
        ))
    } else if age_days >= PROVIDER_CREDENTIAL_LIFETIME_DAYS - PROVIDER_CREDENTIAL_WARN_DAYS {
        Some(format!(
            "warning: {name} reaches its one-year lifetime in {} days; {RENEW}",
            PROVIDER_CREDENTIAL_LIFETIME_DAYS - age_days
        ))
    } else {
        None
    }
}

/// Stable snake-case name of a wire error code for stderr output.
pub fn error_name(code: i32) -> &'static str {
    match VaultErrorCode::try_from(code) {
        Ok(VaultErrorCode::VaultErrorAbsent) => "absent",
        Ok(VaultErrorCode::VaultErrorAlreadyExists) => "already_exists",
        Ok(VaultErrorCode::VaultErrorLocked) => "locked",
        Ok(VaultErrorCode::VaultErrorUnlockFailed) => "unlock_failed",
        Ok(VaultErrorCode::VaultErrorRateLimited) => "rate_limited",
        Ok(VaultErrorCode::VaultErrorWeakPassword) => "weak_password",
        Ok(VaultErrorCode::VaultErrorInvalidPhrase) => "invalid_phrase",
        Ok(VaultErrorCode::VaultErrorEntryNotAllowed) => "entry_not_allowed",
        Ok(VaultErrorCode::VaultErrorEntryRejected) => "entry_rejected",
        Ok(VaultErrorCode::VaultErrorCorrupt) => "corrupt",
        Ok(VaultErrorCode::VaultErrorInvalidRequest) => "invalid_request",
        Ok(VaultErrorCode::VaultErrorUnavailable) => "unavailable",
        Ok(VaultErrorCode::Unspecified) | Err(_) => "unknown",
    }
}

pub async fn cmd(args: Vec<String>) -> ExitCode {
    let Ok(command) = parse(&args) else {
        eprintln!("usage: {USAGE}");
        return ExitCode::FAILURE;
    };
    run(command).await
}

#[cfg(not(unix))]
async fn run(_command: VaultCommand) -> ExitCode {
    eprintln!("agentctl vault: the credential vault is supported on unix only");
    ExitCode::FAILURE
}

#[cfg(unix)]
async fn run(command: VaultCommand) -> ExitCode {
    use ipc_contracts::proto::{
        vault_core_request::Payload as Request, vault_core_response::Payload as Response,
        VaultChangePasswordRequest, VaultCoreRequest, VaultCreateRequest, VaultLockRequest,
        VaultRecoverRequest, VaultRotateRequest, VaultStatusRequest, VaultStoreRequest,
        VaultUnlockRequest,
    };

    let needs_secret = !matches!(command, VaultCommand::Lock | VaultCommand::Status);
    if needs_secret && !terminal::is_terminal(&std::io::stdin()) {
        eprintln!("agentctl vault: secrets are read from a terminal; stdin is not a terminal");
        return ExitCode::FAILURE;
    }
    let shows_phrase = matches!(command, VaultCommand::Init { .. } | VaultCommand::Rotate);
    if shows_phrase && !terminal::is_terminal(&std::io::stdout()) {
        eprintln!(
            "agentctl vault: the recovery phrase is printed only to a terminal; stdout is not a terminal"
        );
        return ExitCode::FAILURE;
    }
    let request = match command {
        VaultCommand::Status => Ok(Request::Status(VaultStatusRequest {})),
        VaultCommand::Lock => Ok(Request::Lock(VaultLockRequest {})),
        VaultCommand::Init { idle_minutes } => new_password().map(|password| {
            Request::Create(VaultCreateRequest {
                password,
                idle_minutes,
            })
        }),
        VaultCommand::Unlock { idle_minutes } => {
            terminal::read_secret("Vault password: ").map(|password| {
                Request::Unlock(VaultUnlockRequest {
                    password,
                    idle_minutes,
                })
            })
        }
        VaultCommand::Store => terminal::read_secret("Vault password: ").and_then(|password| {
            let value = terminal::read_secret("Token printed by `claude setup-token`: ")?;
            let value = trim_ascii(value);
            if value.is_empty() {
                return Err("the token is empty".to_owned());
            }
            Ok(Request::Store(VaultStoreRequest {
                password,
                name: VAULT_PROVIDER_CREDENTIAL_ENTRY.to_owned(),
                value,
            }))
        }),
        VaultCommand::ChangePassword => {
            terminal::read_secret("Current vault password: ").and_then(|current_password| {
                Ok(Request::ChangePassword(VaultChangePasswordRequest {
                    current_password,
                    new_password: new_password()?,
                }))
            })
        }
        VaultCommand::Recover { idle_minutes } => terminal::read_secret("Recovery phrase: ")
            .and_then(|recovery_phrase| {
                Ok(Request::Recover(VaultRecoverRequest {
                    recovery_phrase,
                    new_password: new_password()?,
                    idle_minutes,
                }))
            }),
        VaultCommand::Rotate => terminal::read_secret("Vault password: ")
            .map(|password| Request::Rotate(VaultRotateRequest { password })),
    };
    let request = match request {
        Ok(request) => request,
        Err(message) => {
            eprintln!("agentctl vault: {message}");
            return ExitCode::FAILURE;
        }
    };
    let endpoint = match agentctl::resolve_production_endpoint() {
        Ok(endpoint) => endpoint,
        Err(error) => return crate::fail(&error),
    };
    let request = VaultCoreRequest {
        payload: Some(request),
    };
    let response =
        match agentctl::vault_request(&endpoint, agentctl::spawn_sibling_daemon, request).await {
            Ok(response) => response,
            Err(error) => return crate::fail(&error),
        };
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0);
    match response.payload {
        Some(Response::Status(status)) => {
            print_lines(&status_lines(&status, now_ms));
            ExitCode::SUCCESS
        }
        Some(Response::Phrase(phrase)) => {
            let words = zeroize::Zeroizing::new(phrase.recovery_phrase);
            println!("Recovery phrase (shown once; write it down and keep it offline):");
            println!();
            println!("    {}", String::from_utf8_lossy(&words));
            println!();
            if command == VaultCommand::Rotate {
                println!("The previous recovery phrase no longer opens the vault.");
            }
            if let Some(status) = phrase.status {
                print_lines(&status_lines(&status, now_ms));
            }
            ExitCode::SUCCESS
        }
        Some(Response::Error(error)) => {
            eprintln!("error {}: {}", error_name(error.code), error.message);
            if let Some(retry_after_ms) = error.retry_after_ms {
                eprintln!("retry in {} s", retry_after_ms.div_ceil(1000));
            }
            ExitCode::FAILURE
        }
        None => crate::fail(&agentctl::ClientError::BusinessAmbiguous(
            "daemon sent an empty vault response".to_owned(),
        )),
    }
}

fn print_lines(lines: &[String]) {
    for line in lines {
        println!("{line}");
    }
}

/// Asks twice and requires both entries to match.
#[cfg(unix)]
fn new_password() -> Result<Vec<u8>, String> {
    let first = terminal::read_secret("New vault password (at least 12 characters): ")?;
    let second = zeroize::Zeroizing::new(terminal::read_secret("Repeat the new password: ")?);
    if first != *second {
        drop(zeroize::Zeroizing::new(first));
        return Err("the two passwords differ".to_owned());
    }
    Ok(first)
}

/// Removes surrounding ASCII whitespace in place (pasted tokens often carry
/// a trailing space or newline) without copying the secret.
#[cfg(unix)]
fn trim_ascii(mut value: Vec<u8>) -> Vec<u8> {
    let end = value
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map_or(0, |index| index + 1);
    value.truncate(end);
    let start = value
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(value.len());
    value.drain(..start);
    value
}

#[cfg(unix)]
mod terminal {
    use std::io::{self, BufRead, Write};

    use rustix::termios::{self, LocalModes, OptionalActions};
    use zeroize::Zeroizing;

    /// Room for any expected secret, so the line buffer never reallocates
    /// and leaves an unzeroized copy behind.
    const LINE_CAPACITY: usize = 4096;

    pub fn is_terminal(stream: &impl std::os::fd::AsFd) -> bool {
        termios::isatty(stream)
    }

    /// Reads one line from the terminal with echo off and returns its bytes
    /// without the line ending. The terminal mode is restored on every
    /// return; a signal that kills the process mid-read leaves echo off
    /// (`stty echo` repairs it).
    pub fn read_secret(prompt: &str) -> Result<Vec<u8>, String> {
        read_hidden(prompt).map_err(|error| format!("terminal input failed: {error}"))
    }

    fn read_hidden(prompt: &str) -> io::Result<Vec<u8>> {
        let stdin = io::stdin();
        let original = termios::tcgetattr(&stdin)?;
        let mut hidden = original.clone();
        hidden.local_modes.remove(LocalModes::ECHO);
        hidden.local_modes.insert(LocalModes::ECHONL);
        eprint!("{prompt}");
        io::stderr().flush()?;
        termios::tcsetattr(&stdin, OptionalActions::Flush, &hidden)?;
        let mut line = Zeroizing::new(String::with_capacity(LINE_CAPACITY));
        let read = stdin.lock().read_line(&mut line);
        termios::tcsetattr(&stdin, OptionalActions::Now, &original)?;
        read?;
        let end = line.trim_end_matches(['\r', '\n']).len();
        line.truncate(end);
        Ok(std::mem::take(&mut *line).into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ipc_contracts::proto::VaultEntryDto;

    const NOW_MS: u64 = 1_800_000_000_000;

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| (*word).to_owned()).collect()
    }

    fn unlocked(entries: Vec<VaultEntryDto>, until_ms: Option<u64>) -> VaultStatusResponse {
        VaultStatusResponse {
            state: VaultState::VaultUnlocked as i32,
            entries,
            unlocked_until_ms: until_ms,
        }
    }

    fn token_aged(days: u64) -> VaultEntryDto {
        VaultEntryDto {
            name: VAULT_PROVIDER_CREDENTIAL_ENTRY.to_owned(),
            created_at_s: NOW_MS / 1000 - days * SECONDS_PER_DAY,
            rejected: false,
        }
    }

    fn token_rejected() -> VaultEntryDto {
        VaultEntryDto {
            name: VAULT_PROVIDER_CREDENTIAL_ENTRY.to_owned(),
            created_at_s: NOW_MS / 1000 - 10 * SECONDS_PER_DAY,
            rejected: true,
        }
    }

    #[test]
    fn parses_every_command_and_the_idle_option() {
        assert_eq!(
            parse(&args(&["init"])),
            Ok(VaultCommand::Init { idle_minutes: 0 })
        );
        assert_eq!(
            parse(&args(&["unlock", "--idle-minutes", "90"])),
            Ok(VaultCommand::Unlock { idle_minutes: 90 })
        );
        assert_eq!(
            parse(&args(&["recover", "--idle-minutes", "5"])),
            Ok(VaultCommand::Recover { idle_minutes: 5 })
        );
        assert_eq!(parse(&args(&["lock"])), Ok(VaultCommand::Lock));
        assert_eq!(parse(&args(&["status"])), Ok(VaultCommand::Status));
        assert_eq!(parse(&args(&["store"])), Ok(VaultCommand::Store));
        assert_eq!(
            parse(&args(&["change-password"])),
            Ok(VaultCommand::ChangePassword)
        );
        assert_eq!(parse(&args(&["rotate"])), Ok(VaultCommand::Rotate));
    }

    /// No secret is ever accepted as an argument.
    #[test]
    fn rejects_unknown_commands_extra_words_and_bad_idle_values() {
        for words in [
            &[][..],
            &["open"][..],
            &["store", "sk-ant-oat01-value"][..],
            &["unlock", "my-password"][..],
            &["lock", "--idle-minutes", "5"][..],
            &["unlock", "--idle-minutes"][..],
            &["unlock", "--idle-minutes", "ten"][..],
            &["unlock", "--idle-minutes", "-1"][..],
        ] {
            assert_eq!(parse(&args(words)), Err(()), "{words:?}");
        }
    }

    #[test]
    fn absent_and_locked_vaults_say_what_to_do() {
        let absent = VaultStatusResponse {
            state: VaultState::VaultAbsent as i32,
            entries: Vec::new(),
            unlocked_until_ms: None,
        };
        let locked = VaultStatusResponse {
            state: VaultState::VaultLocked as i32,
            ..absent.clone()
        };

        assert!(status_lines(&absent, NOW_MS)[0].contains("agentctl vault init"));
        assert!(status_lines(&locked, NOW_MS)[0].contains("agentctl vault unlock"));
    }

    #[test]
    fn unlocked_vault_reports_minutes_until_it_locks() {
        let status = unlocked(Vec::new(), Some(NOW_MS + 29 * 60_000 + 1));

        let lines = status_lines(&status, NOW_MS);

        assert_eq!(lines[0], "vault: unlocked, locks in 30 min");
        assert!(lines[1].contains("no credential stored"));
    }

    #[test]
    fn credential_age_warns_in_the_last_month_and_after_the_lifetime() {
        let fresh = status_lines(&unlocked(vec![token_aged(10)], None), NOW_MS);
        assert_eq!(
            fresh[1],
            format!("entry {VAULT_PROVIDER_CREDENTIAL_ENTRY}: stored 10 days ago")
        );
        assert_eq!(fresh.len(), 2);

        let ageing = status_lines(
            &unlocked(
                vec![token_aged(
                    PROVIDER_CREDENTIAL_LIFETIME_DAYS - PROVIDER_CREDENTIAL_WARN_DAYS + 5,
                )],
                None,
            ),
            NOW_MS,
        );
        assert!(ageing[2].starts_with("warning:"));
        assert!(ageing[2].contains("in 25 days"));

        let expired = status_lines(&unlocked(vec![token_aged(400)], None), NOW_MS);
        assert!(expired[2].starts_with("warning:"));
        assert!(expired[2].contains("past its one-year lifetime"));
        assert!(expired[2].contains("claude setup-token"));
    }

    #[test]
    fn rejected_credential_is_flagged_in_status() {
        let plain = status_lines(&unlocked(vec![token_aged(10)], None), NOW_MS);
        assert!(
            !plain.iter().any(|line| line.contains("rejected")),
            "{plain:?}"
        );

        let flagged = status_lines(&unlocked(vec![token_rejected()], None), NOW_MS);
        assert!(
            flagged
                .iter()
                .any(|line| line.contains("rejected")
                    && line.contains(VAULT_PROVIDER_CREDENTIAL_ENTRY)),
            "{flagged:?}"
        );
    }

    #[test]
    fn error_codes_have_stable_names() {
        assert_eq!(
            error_name(VaultErrorCode::VaultErrorRateLimited as i32),
            "rate_limited"
        );
        assert_eq!(error_name(999), "unknown");
    }
}
