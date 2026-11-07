//! Durable record of fatal startup errors (audit F6): appended to a 0600
//! file in the private runtime directory so a daemon that died before
//! serving leaves its reason behind, whoever launched it.

/// Prints `message` to stderr and appends it, timestamped, to the startup
/// log. Logging failures are ignored: the error is already on stderr.
pub fn report_fatal(message: &str) {
    eprintln!("{message}");
    #[cfg(unix)]
    let _ = append(message);
}

#[cfg(unix)]
fn append(message: &str) -> std::io::Result<()> {
    use std::io::Write;

    const MAX_LOG_BYTES: u64 = 64 * 1024;
    let endpoint = ipc_transport::platform::unix::resolve_endpoint()
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    ipc_transport::platform::unix::ensure_runtime_dir(&endpoint.runtime_dir)
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    let mut file = ipc_transport::platform::unix::open_runtime_log(
        &endpoint,
        ipc_transport::STARTUP_LOG_FILE_NAME,
        true,
    )
    .map_err(|error| std::io::Error::other(error.to_string()))?;
    if file.metadata()?.len() > MAX_LOG_BYTES {
        file.set_len(0)?;
    }
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    writeln!(file, "{message} (unix time {seconds})")
}
