//! OS-authenticated peer identity (ADR-0020 transport mechanics).
//!
//! This module extracts kernel/token-backed identity and compares it —
//! nothing more. Policy (allow/reject) belongs to `agentd`; protocol
//! semantics belong to `ipc-control`. Identity here is never a username,
//! PID, process name, binary path, environment, or `client_kind` claim:
//! Unix peers are effective UIDs from kernel socket credentials,
//! Windows peers are owned user-SID bytes from the client access token.

use crate::{TransportError, TransportResult};

/// Authenticated OS-user identity of one IPC peer.
///
/// Platform representations differ, but the meaning is fixed: the local
/// OS user principal. Opaque to callers: construct via
/// [`daemon_identity`] / [`peer_identity`] (Unix) or the Windows token
/// extraction; [`from_uid`] / [`from_sid`] exist solely as the narrow
/// policy-test seam and production composition never uses them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerIdentity {
    #[cfg(unix)]
    uid: u32,
    #[cfg(windows)]
    sid: Vec<u8>,
}

// ---------------------------------------------------------------------------
// Unix: effective UID from kernel socket credentials.
// ---------------------------------------------------------------------------

/// Policy-test seam: identity for an explicit UID. Production paths use
/// [`peer_identity`] (kernel credentials), never this constructor.
#[cfg(unix)]
#[must_use]
pub fn from_uid(uid: u32) -> PeerIdentity {
    PeerIdentity { uid }
}

/// Effective UID carried by this identity (diagnostics/tests only —
/// never authentication evidence on its own).
#[cfg(unix)]
#[must_use]
pub fn peer_uid(identity: &PeerIdentity) -> u32 {
    identity.uid
}

/// The daemon's own principal: current effective UID from the trusted OS
/// API. Never from argv, environment, config, or protocol data.
#[cfg(unix)]
pub fn daemon_identity() -> TransportResult<PeerIdentity> {
    Ok(from_uid(super::unix::current_uid()))
}

/// Kernel-authenticated UID of the connected peer.
///
/// Linux reads `SO_PEERCRED`; macOS reads `getpeereid()`. PID and process
/// metadata are ignored even where the kernel provides them. Any other
/// Unix fails closed: no credentials, no authentication.
#[cfg(unix)]
pub fn peer_identity(stream: &tokio::net::UnixStream) -> TransportResult<PeerIdentity> {
    Ok(from_uid(peer_uid_of(stream)?))
}

#[cfg(target_os = "linux")]
fn peer_uid_of(stream: &tokio::net::UnixStream) -> TransportResult<u32> {
    use std::os::unix::io::{AsFd, AsRawFd};

    let fd = stream.as_fd().as_raw_fd();
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // Safety: `fd` is a live connected socket owned by `stream`;
    // `getsockopt` writes exactly `size_of::<ucred>()` bytes for
    // `SO_PEERCRED` on Linux.
    let ret = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut cred as *mut libc::ucred).cast::<libc::c_void>(),
            &mut len,
        )
    };
    if ret != 0 {
        return Err(TransportError::PeerAuth(format!(
            "SO_PEERCRED unavailable: {}",
            std::io::Error::last_os_error()
        )));
    }
    Ok(cred.uid)
}

#[cfg(target_os = "macos")]
fn peer_uid_of(stream: &tokio::net::UnixStream) -> TransportResult<u32> {
    use std::os::unix::io::{AsFd, AsRawFd};

    let fd = stream.as_fd().as_raw_fd();
    let mut euid: libc::uid_t = 0;
    let mut egid: libc::gid_t = 0;
    // Safety: `fd` is a live connected socket; `getpeereid` writes both
    // out-parameters on success and touches nothing on failure.
    let ret = unsafe { libc::getpeereid(fd, &mut euid, &mut egid) };
    if ret != 0 {
        return Err(TransportError::PeerAuth(format!(
            "getpeereid failed: {}",
            std::io::Error::last_os_error()
        )));
    }
    Ok(euid)
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn peer_uid_of(_stream: &tokio::net::UnixStream) -> TransportResult<u32> {
    Err(TransportError::PeerAuth(
        "kernel peer credentials unavailable on this Unix platform; failing closed".to_owned(),
    ))
}

// ---------------------------------------------------------------------------
// Windows: user SID from the client access token.
// ---------------------------------------------------------------------------

/// Policy-test seam: identity for explicit SID bytes. Production paths
/// use [`pipe_client_identity`] (impersonation + token query), never this
/// constructor.
#[cfg(windows)]
#[must_use]
pub fn from_sid(sid: Vec<u8>) -> PeerIdentity {
    PeerIdentity { sid }
}

/// The daemon's own principal: current process-user SID from the trusted
/// OS token API. Never from argv, environment, config, or protocol data.
#[cfg(windows)]
pub fn daemon_identity() -> TransportResult<PeerIdentity> {
    Ok(from_sid(super::windows::daemon_user_sid()?))
}

/// Narrow RAII guard for pipe-client impersonation.
///
/// The normal path calls [`ImpersonationGuard::revert`] explicitly so a
/// revert failure is observed and fails the connection closed; [`Drop`]
/// reverts best-effort as a backstop so no exit path — including panics —
/// leaks an impersonating thread context. No `.await` and no application
/// work may run while a guard is armed.
#[cfg(windows)]
pub struct ImpersonationGuard {
    armed: bool,
}

#[cfg(windows)]
impl ImpersonationGuard {
    /// Impersonates the client connected to `server`. Failure fails
    /// closed before any token is touched.
    pub fn impersonate(
        server: &tokio::net::windows::named_pipe::NamedPipeServer,
    ) -> TransportResult<Self> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::Pipes::ImpersonateNamedPipeClient;

        let handle = server.as_raw_handle() as windows_sys::Win32::Foundation::HANDLE;
        // Safety: `handle` is a live connected pipe server instance owned
        // by `server`; the call impersonates only the calling thread.
        let ok = unsafe { ImpersonateNamedPipeClient(handle) };
        if ok == 0 {
            return Err(TransportError::PeerAuth(format!(
                "ImpersonateNamedPipeClient failed: {}",
                std::io::Error::last_os_error()
            )));
        }
        Ok(Self { armed: true })
    }

    /// Reverts to self, checking the result. A revert failure is a severe
    /// local security failure: the caller must fail the connection closed
    /// and run no further request processing on this context.
    pub fn revert(mut self) -> TransportResult<()> {
        use windows_sys::Win32::Security::RevertToSelf;

        // Safety: paired with a successful `impersonate` on this thread;
        // disarmed below so `Drop` cannot revert twice.
        let ok = unsafe { RevertToSelf() };
        self.armed = false;
        if ok == 0 {
            return Err(TransportError::PeerAuth(format!(
                "RevertToSelf failed: {}",
                std::io::Error::last_os_error()
            )));
        }
        Ok(())
    }
}

#[cfg(windows)]
impl Drop for ImpersonationGuard {
    fn drop(&mut self) {
        if self.armed {
            // Safety: backstop only; normal paths revert explicitly above.
            unsafe {
                windows_sys::Win32::Security::RevertToSelf();
            }
        }
    }
}

/// Extracts the connected client's user SID via impersonation +
/// thread-token query, reverting on every path.
///
/// Synchronous start to finish: no `.await`, no application work while
/// impersonating. Every failure — impersonation, token open, token
/// query, SID copy, revert — fails closed with no identity.
#[cfg(windows)]
pub fn pipe_client_identity(
    server: &tokio::net::windows::named_pipe::NamedPipeServer,
) -> TransportResult<PeerIdentity> {
    let guard = ImpersonationGuard::impersonate(server)?;
    let sid = client_token_sid_bytes();
    let reverted = guard.revert();
    // A revert failure dominates: an uncertain thread context must never
    // continue as though authentication succeeded.
    let sid = match (sid, reverted) {
        (Ok(sid), Ok(())) => sid,
        (Err(error), _) => return Err(error),
        (Ok(_), Err(error)) => return Err(error),
    };
    Ok(from_sid(sid))
}

#[cfg(windows)]
fn client_token_sid_bytes() -> TransportResult<Vec<u8>> {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::TOKEN_QUERY;
    use windows_sys::Win32::System::Threading::{GetCurrentThread, OpenThreadToken};

    use std::ptr;

    // Safety: `GetCurrentThread` pseudo-handle is always valid;
    // `OpenAsSelf = TRUE` checks access against the process context while
    // still opening the thread's impersonation (client) token. The token
    // handle is closed on every path below.
    unsafe {
        let mut token: HANDLE = ptr::null_mut();
        if OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut token) == 0 {
            return Err(TransportError::PeerAuth(format!(
                "OpenThreadToken failed: {}",
                std::io::Error::last_os_error()
            )));
        }
        let result = super::windows::token_sid_bytes(token);
        CloseHandle(token);
        result
    }
}

// ---------------------------------------------------------------------------
// Same-principal policy: the single comparison boundary.
// ---------------------------------------------------------------------------

/// Returns whether `peer` is the same local OS principal as `daemon`:
/// UID equality on Unix, OS SID equality on Windows. Anything else —
/// mismatch, incompatible representation — denies. No permissive
/// fallback exists by construction: there is no third outcome.
#[must_use]
pub fn same_local_principal(daemon: &PeerIdentity, peer: &PeerIdentity) -> bool {
    #[cfg(unix)]
    {
        daemon.uid == peer.uid
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Security::EqualSid;
        use windows_sys::Win32::Security::PSID;

        if daemon.sid.len() != peer.sid.len() {
            return false;
        }
        // Safety: both buffers are complete OS-produced (or well-formed
        // test) SIDs of equal length; `EqualSid` reads only within them.
        unsafe { EqualSid(daemon.sid.as_ptr() as PSID, peer.sid.as_ptr() as PSID) != 0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(unix)]
    fn same_uid_allows_different_uid_denies() {
        let daemon = from_uid(1000);
        assert!(same_local_principal(&daemon, &from_uid(1000)));
        assert!(!same_local_principal(&daemon, &from_uid(1001)));
        assert!(!same_local_principal(&daemon, &from_uid(0)));
    }

    #[test]
    #[cfg(unix)]
    fn daemon_identity_is_current_euid() {
        let daemon = daemon_identity().expect("euid always available");
        assert_eq!(peer_uid(&daemon), super::super::unix::current_uid());
    }

    /// Real OS mechanism (not a fake identity): a bound socket plus a
    /// genuine client connection yields kernel credentials whose UID is
    /// the daemon's own euid.
    #[tokio::test]
    #[cfg(unix)]
    async fn real_peer_credentials_report_same_user() {
        use std::sync::atomic::{AtomicU64, Ordering};

        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "ipt-peer-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir(&dir).expect("sandbox");
        let path = dir.join("agentd.sock");
        let listener = tokio::net::UnixListener::bind(&path).expect("bind");
        let client = tokio::net::UnixStream::connect(&path)
            .await
            .expect("connect");
        let (server, _) = listener.accept().await.expect("accept");
        let peer = peer_identity(&server).expect("kernel peer credentials");
        let daemon = daemon_identity().expect("daemon principal");
        assert_eq!(peer_uid(&peer), super::super::unix::current_uid());
        assert!(same_local_principal(&daemon, &peer));
        drop(client);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Minimal well-formed SID (`S-1-5-21`): revision 1, one
    /// sub-authority, NT authority. Hand-built so policy tests need no OS.
    #[cfg(windows)]
    fn test_sid(last: u8) -> Vec<u8> {
        vec![1, 1, 0, 0, 0, 0, 0, 5, 21, 0, 0, last]
    }

    #[test]
    #[cfg(windows)]
    fn same_sid_allows_different_sid_denies() {
        let daemon = from_sid(test_sid(1));
        assert!(same_local_principal(&daemon, &from_sid(test_sid(1))));
        assert!(!same_local_principal(&daemon, &from_sid(test_sid(2))));
        assert!(!same_local_principal(&daemon, &from_sid(vec![1, 1])));
    }
}
