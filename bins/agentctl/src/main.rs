//! `agentctl` entry point: minimal G1 CLI (`start`, `ping`, `shutdown`).
//!
//! Thin shell over the client lifecycle in the library. Concise,
//! deterministic stdout/stderr; no protobuf/frame/security-descriptor dumps.
//! Exit codes are G1-local, not a permanent standard.

use std::process::ExitCode;

use agentctl::{resolve_production_endpoint, spawn_sibling_daemon, ClientError};

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("start") => cmd_start().await,
        Some("ping") => cmd_ping().await,
        Some("shutdown") => cmd_shutdown().await,
        _ => {
            eprintln!("usage: agentctl <start|ping|shutdown>");
            ExitCode::FAILURE
        }
    }
}

#[cfg(unix)]
async fn cmd_start() -> ExitCode {
    let endpoint = match resolve_production_endpoint() {
        Ok(endpoint) => endpoint,
        Err(error) => return fail(&error),
    };
    match agentctl::connect_or_start(&endpoint, spawn_sibling_daemon).await {
        Ok(session) => {
            let version = session.negotiated_version().unwrap_or(0);
            println!("agentctl: daemon ready (protocol version {version})");
            ExitCode::SUCCESS
        }
        Err(error) => fail(&error),
    }
}

#[cfg(windows)]
async fn cmd_start() -> ExitCode {
    let pipe_name = match resolve_production_endpoint() {
        Ok(pipe_name) => pipe_name,
        Err(error) => return fail(&error),
    };
    match agentctl::connect_or_start(&pipe_name, spawn_sibling_daemon).await {
        Ok(session) => {
            let version = session.negotiated_version().unwrap_or(0);
            println!("agentctl: daemon ready (protocol version {version})");
            ExitCode::SUCCESS
        }
        Err(error) => fail(&error),
    }
}

#[cfg(unix)]
async fn cmd_ping() -> ExitCode {
    let endpoint = match resolve_production_endpoint() {
        Ok(endpoint) => endpoint,
        Err(error) => return fail(&error),
    };
    match agentctl::ping(&endpoint, spawn_sibling_daemon).await {
        Ok(()) => {
            println!("agentctl: pong");
            ExitCode::SUCCESS
        }
        Err(error) => fail(&error),
    }
}

#[cfg(windows)]
async fn cmd_ping() -> ExitCode {
    let pipe_name = match resolve_production_endpoint() {
        Ok(pipe_name) => pipe_name,
        Err(error) => return fail(&error),
    };
    match agentctl::ping(&pipe_name, spawn_sibling_daemon).await {
        Ok(()) => {
            println!("agentctl: pong");
            ExitCode::SUCCESS
        }
        Err(error) => fail(&error),
    }
}

#[cfg(unix)]
async fn cmd_shutdown() -> ExitCode {
    let endpoint = match resolve_production_endpoint() {
        Ok(endpoint) => endpoint,
        Err(error) => return fail(&error),
    };
    match agentctl::shutdown(&endpoint).await {
        Ok(()) => {
            println!("agentctl: shutdown complete");
            ExitCode::SUCCESS
        }
        Err(ClientError::NotRunning) => {
            eprintln!("agentctl: daemon not running");
            ExitCode::FAILURE
        }
        Err(error) => fail(&error),
    }
}

#[cfg(windows)]
async fn cmd_shutdown() -> ExitCode {
    let pipe_name = match resolve_production_endpoint() {
        Ok(pipe_name) => pipe_name,
        Err(error) => return fail(&error),
    };
    match agentctl::shutdown(&pipe_name).await {
        Ok(()) => {
            println!("agentctl: shutdown complete");
            ExitCode::SUCCESS
        }
        Err(ClientError::NotRunning) => {
            eprintln!("agentctl: daemon not running");
            ExitCode::FAILURE
        }
        Err(error) => fail(&error),
    }
}

fn fail(error: &ClientError) -> ExitCode {
    eprintln!("agentctl: failed: {error}");
    ExitCode::FAILURE
}
