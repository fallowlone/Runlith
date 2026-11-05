//! `agentd` entry point: thin CLI shell over the G2 control + business server.
//!
//! Running `agentd` resolves the canonical durable database path, opens
//! (creating/migrating) the store, and starts the daemon on its canonical
//! endpoint. An already-running compatible daemon produces a
//! distinguishable AlreadyRunning result (exit code 2). A database or
//! data-directory failure fails clearly without serving. Exit codes are
//! G1-local, not a permanent standard.

use std::process::ExitCode;

use agentd::{DaemonConfig, DaemonError, RunOutcome};

fn main() -> ExitCode {
    #[cfg(unix)]
    if let Err(error) = agentd::harden_process() {
        agentd::startup_log::report_fatal(&format!("agentd: {error}"));
        return ExitCode::FAILURE;
    }
    run()
}

#[tokio::main(flavor = "current_thread")]
async fn run() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args
        .first()
        .is_some_and(|arg| arg.starts_with("__candidate-"))
    {
        return agentd::run_candidate_helper(&args);
    }
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("usage: agentd");
        println!("  Starts the local G2 daemon on its canonical endpoint.");
        println!("  The durable database resolves per ADR-0021 section 15;");
        println!("  AI_ENGINEERING_OS_DATA_DIR overrides the data parent for tests.");
        return ExitCode::SUCCESS;
    }
    if !args.is_empty() {
        eprintln!("agentd: unexpected arguments (usage: agentd)");
        return ExitCode::FAILURE;
    }
    let database_path = match agentd::resolve_database_path() {
        Ok(path) => path,
        Err(error) => {
            agentd::startup_log::report_fatal(&format!(
                "agentd: cannot resolve durable database: {error}"
            ));
            return ExitCode::FAILURE;
        }
    };
    match agentd::run(DaemonConfig::with_execution(database_path)).await {
        Ok(RunOutcome::ShutdownCompleted { connections_served }) => {
            println!("agentd: shutdown complete (connections served: {connections_served})");
            ExitCode::SUCCESS
        }
        Err(DaemonError::AlreadyRunning) => {
            eprintln!("agentd: already running (endpoint owned)");
            ExitCode::from(2)
        }
        Err(error) => {
            agentd::startup_log::report_fatal(&format!("agentd: failed: {error}"));
            ExitCode::FAILURE
        }
    }
}
