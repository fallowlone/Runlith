//! Process-test daemon with explicit test composition; never a shipped binary.

use std::process::ExitCode;

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args
        .first()
        .is_some_and(|arg| arg.starts_with("__candidate-"))
    {
        return agentd::run_candidate_helper(&args);
    }
    let installed = std::env::current_exe()
        .ok()
        .and_then(|exe| {
            exe.parent()
                .map(|parent| parent.join(".confirmed-test-daemon").is_file())
        })
        .unwrap_or(false);
    if !args.is_empty() || !installed {
        eprintln!("confirmed daemon fixture requires a test-owned installation");
        return ExitCode::FAILURE;
    }
    let database = match agentd::resolve_database_path() {
        Ok(path) => path,
        Err(error) => {
            agentd::startup_log::report_fatal(&format!("fixture database: {error}"));
            return ExitCode::FAILURE;
        }
    };
    let config =
        agentd::DaemonConfig::with_execution(database).with_confirmed_user_presence_for_tests();
    match agentd::run(config).await {
        Ok(_) => ExitCode::SUCCESS,
        Err(agentd::DaemonError::AlreadyRunning) => ExitCode::from(2),
        Err(error) => {
            agentd::startup_log::report_fatal(&format!("fixture daemon: {error}"));
            ExitCode::FAILURE
        }
    }
}
