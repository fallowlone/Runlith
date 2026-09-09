//! `agentd` entry point: thin CLI shell over the G1 control server.
//!
//! Running `agentd` starts the daemon on its canonical endpoint. An
//! already-running compatible daemon produces a distinguishable
//! AlreadyRunning result (exit code 2). Exit codes are G1-local, not a
//! permanent standard.

use std::process::ExitCode;

use agentd::{DaemonConfig, DaemonError, RunOutcome};

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("usage: agentd");
        println!("  Starts the local G1 control daemon on its canonical endpoint.");
        return ExitCode::SUCCESS;
    }
    if !args.is_empty() {
        eprintln!("agentd: unexpected arguments (usage: agentd)");
        return ExitCode::FAILURE;
    }
    match agentd::run(DaemonConfig::g1_default()).await {
        Ok(RunOutcome::ShutdownCompleted { connections_served }) => {
            println!("agentd: shutdown complete (connections served: {connections_served})");
            ExitCode::SUCCESS
        }
        Err(DaemonError::AlreadyRunning) => {
            eprintln!("agentd: already running (endpoint owned)");
            ExitCode::from(2)
        }
        Err(error) => {
            eprintln!("agentd: failed: {error}");
            ExitCode::FAILURE
        }
    }
}
