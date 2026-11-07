//! Client-side authentication of the daemon endpoint (Unix).
//!
//! A client must prove the endpoint belongs to the current user BEFORE any
//! byte (handshake, secrets) is written: the runtime directory and socket
//! node are checked without following symlinks, and after `connect` the
//! kernel-reported peer UID must equal the expected UID. Every failure is
//! closed. Windows parity is handled separately in `windows.rs`.

use std::fs;
use std::io;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::Path;

use crate::{TransportError, TransportResult};

/// Rejects a connected peer whose UID differs from `expected_uid`.
pub(super) fn verify_peer_uid(peer_uid: u32, expected_uid: u32) -> TransportResult<()> {
    if peer_uid == expected_uid {
        return Ok(());
    }
    Err(TransportError::PeerAuth(format!(
        "daemon peer uid {peer_uid} does not match expected uid {expected_uid}; refusing to send data"
    )))
}

fn deny(msg: String) -> TransportError {
    TransportError::EndpointSecurity(msg)
}

/// Checks runtime dir (real dir, owner, exactly 0700) and socket node
/// (real socket, owner, no group/other access) with `symlink_metadata`.
/// An absent node is not an error here: `connect` reports unavailability.
pub(super) fn verify_endpoint_nodes(
    runtime_dir: &Path,
    socket_path: &Path,
    expected_uid: u32,
) -> TransportResult<()> {
    let dir = match fs::symlink_metadata(runtime_dir) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(TransportError::Io(e)),
        Ok(m) => m,
    };
    if dir.file_type().is_symlink() || !dir.is_dir() {
        return Err(deny(format!(
            "runtime directory {} is a symlink or not a directory",
            runtime_dir.display()
        )));
    }
    if dir.uid() != expected_uid {
        return Err(deny(format!(
            "runtime directory {} is not owned by the current user",
            runtime_dir.display()
        )));
    }
    if dir.mode() & 0o7777 != 0o700 {
        return Err(deny(format!(
            "runtime directory {} must have mode 0700",
            runtime_dir.display()
        )));
    }
    let sock = match fs::symlink_metadata(socket_path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(TransportError::Io(e)),
        Ok(m) => m,
    };
    if !sock.file_type().is_socket() {
        return Err(deny(format!(
            "endpoint {} is a symlink or not a socket",
            socket_path.display()
        )));
    }
    if sock.uid() != expected_uid {
        return Err(deny(format!(
            "endpoint {} is not owned by the current user",
            socket_path.display()
        )));
    }
    if sock.mode() & 0o077 != 0 {
        return Err(deny(format!(
            "endpoint {} must not be accessible by group/other",
            socket_path.display()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::unix::{
        bind_server, connect, current_uid, ensure_runtime_dir, UnixEndpoint, SOCKET_FILE_NAME,
    };
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::path::PathBuf;

    fn sandbox(name: &str) -> (PathBuf, UnixEndpoint) {
        let root = PathBuf::from("/tmp").join(format!("ica-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir(&root).unwrap();
        let runtime_dir = root.join("run");
        let ep = UnixEndpoint {
            socket_path: runtime_dir.join(SOCKET_FILE_NAME),
            runtime_dir,
        };
        (root, ep)
    }

    #[test]
    fn peer_uid_mismatch_is_refused() {
        assert!(verify_peer_uid(5, 5).is_ok());
        assert!(matches!(
            verify_peer_uid(0, 501),
            Err(TransportError::PeerAuth(_))
        ));
    }

    #[tokio::test]
    async fn happy_path_connects_and_nodes_verify() {
        let (root, ep) = sandbox("ok");
        let _l = bind_server(&ep).unwrap();
        verify_endpoint_nodes(&ep.runtime_dir, &ep.socket_path, current_uid()).unwrap();
        connect(&ep).await.unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn foreign_owner_injected_is_refused() {
        let (root, ep) = sandbox("own");
        ensure_runtime_dir(&ep.runtime_dir).unwrap();
        let other = current_uid().wrapping_add(1);
        assert!(matches!(
            verify_endpoint_nodes(&ep.runtime_dir, &ep.socket_path, other),
            Err(TransportError::EndpointSecurity(_))
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn wrong_socket_mode_is_refused_by_connect() {
        let (root, ep) = sandbox("mode");
        let _l = bind_server(&ep).unwrap();
        fs::set_permissions(&ep.socket_path, fs::Permissions::from_mode(0o666)).unwrap();
        assert!(matches!(
            connect(&ep).await,
            Err(TransportError::EndpointSecurity(_))
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn wrong_dir_mode_is_refused_by_connect() {
        let (root, ep) = sandbox("dmode");
        ensure_runtime_dir(&ep.runtime_dir).unwrap();
        fs::set_permissions(&ep.runtime_dir, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(matches!(
            connect(&ep).await,
            Err(TransportError::EndpointSecurity(_))
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn symlinked_socket_is_refused_by_connect() {
        let (root, ep) = sandbox("sym");
        let l = bind_server(&ep).unwrap();
        let real = ep.runtime_dir.join("real.sock");
        fs::rename(&ep.socket_path, &real).unwrap();
        symlink(&real, &ep.socket_path).unwrap();
        assert!(matches!(
            connect(&ep).await,
            Err(TransportError::EndpointSecurity(_))
        ));
        drop(l);
        let _ = fs::remove_dir_all(root);
    }
}
