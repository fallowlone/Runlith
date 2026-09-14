//! Unix local transport: UDS streams, endpoint resolution, and endpoint
//! admission/security primitives for macOS and Linux.
//!
//! No TCP fallback exists anywhere here. The canonical socket path is the
//! singleton authority: a successful bind owns the daemon endpoint, and an
//! existing endpoint is never deleted by this layer (stale-socket probing
//! and removal belong to TASK-0020).

use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use tokio::net::{UnixListener, UnixStream};

use crate::framing::FramedTransport;
use crate::{TransportError, TransportResult};

/// Socket node inside the private runtime directory.
pub const SOCKET_FILE_NAME: &str = "agentd.sock";

/// Application-owned directory name inside the runtime parent.
const APP_DIR_NAME: &str = "ai-engineering-os";

/// Required mode of the application-owned runtime directory.
const RUNTIME_DIR_MODE: u32 = 0o700;

/// Required mode of the bound socket node.
const SOCKET_MODE: u32 = 0o600;

/// Environment override for the runtime parent directory (test seam).
///
/// When `AI_ENGINEERING_OS_RUNTIME_DIR` names an absolute path, endpoint
/// resolution uses it instead of the canonical per-user location, so
/// real-process tests run against a test-owned endpoint without touching
/// the user's runtime directory. The directory itself is still admitted
/// through [`ensure_runtime_dir`] (0700, current-user owned, no symlinks),
/// so the override selects *which* directory, never *whether* it is
/// secure. Production never sets this variable.
pub const RUNTIME_DIR_OVERRIDE_ENV: &str = "AI_ENGINEERING_OS_RUNTIME_DIR";

/// Resolved Unix daemon endpoint: socket node plus its runtime directory.
#[derive(Debug, Clone)]
pub struct UnixEndpoint {
    /// Full path of the `agentd.sock` node.
    pub socket_path: PathBuf,
    /// Application-owned private runtime directory holding the socket.
    pub runtime_dir: PathBuf,
}

/// Resolves the canonical daemon endpoint for this platform/user.
///
/// Linux prefers `$XDG_RUNTIME_DIR/ai-engineering-os/` when that variable
/// is usable for the current user, else `${TMPDIR:-/tmp}/ai-engineering-os-$UID/`.
/// macOS uses `$TMPDIR/ai-engineering-os/` with the platform temp-directory
/// fallback. The path is preflighted against the UDS length constraint;
/// over-long paths are an explicit error, never truncated.
pub fn resolve_endpoint() -> TransportResult<UnixEndpoint> {
    let runtime_dir = runtime_parent_override().unwrap_or_else(|| {
        #[cfg(target_os = "macos")]
        {
            macos_runtime_parent()
        }
        #[cfg(not(target_os = "macos"))]
        {
            xdg_runtime_parent().unwrap_or_else(fallback_runtime_parent)
        }
    });

    let socket_path = runtime_dir.join(SOCKET_FILE_NAME);
    preflight_socket_path(&socket_path)?;
    Ok(UnixEndpoint {
        socket_path,
        runtime_dir,
    })
}

/// Explicit absolute-parent override for tests (see
/// [`RUNTIME_DIR_OVERRIDE_ENV`]). Relative, empty, or missing values fall
/// back to canonical resolution.
fn runtime_parent_override() -> Option<PathBuf> {
    let dir = std::env::var_os(RUNTIME_DIR_OVERRIDE_ENV)?;
    if dir.is_empty() {
        return None;
    }
    let path = PathBuf::from(dir);
    if !path.is_absolute() {
        return None;
    }
    Some(path.join(APP_DIR_NAME))
}

/// Rejects socket paths that cannot be represented as a `sockaddr_un`.
/// Never truncates, hashes, or falls back.
pub fn preflight_socket_path(path: &Path) -> TransportResult<()> {
    let total = std::mem::size_of::<libc::sockaddr_un>();
    let offset = std::mem::offset_of!(libc::sockaddr_un, sun_path);
    // One byte is reserved for the NUL terminator.
    let usable = total.saturating_sub(offset).saturating_sub(1);
    if path.as_os_str().as_bytes().len() > usable {
        return Err(TransportError::PathTooLong {
            path: path.to_path_buf(),
        });
    }
    Ok(())
}

/// Creates the application-owned runtime directory when absent and admits
/// an existing one only when it is owned by the current effective UID, is
/// not a symlink, and has exactly mode `0700`. Anything else fails closed:
/// this function never chmods a foreign directory, never follows symlinks,
/// and never replaces unexpected filesystem objects.
pub fn ensure_runtime_dir(dir: &Path) -> TransportResult<()> {
    match fs::symlink_metadata(dir) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(dir).map_err(|error| match error.kind() {
                io::ErrorKind::PermissionDenied => TransportError::EndpointSecurity(format!(
                    "cannot create runtime directory {}: permission denied",
                    dir.display()
                )),
                io::ErrorKind::NotFound => TransportError::InvalidEndpoint(format!(
                    "runtime parent for {} does not exist",
                    dir.display()
                )),
                _ => TransportError::Io(error),
            })?;
            // Harden against umask: enforce 0700 explicitly, then re-validate.
            fs::set_permissions(dir, fs::Permissions::from_mode(RUNTIME_DIR_MODE))?;
            validate_runtime_dir(dir)
        }
        Err(error) => Err(TransportError::Io(error)),
        Ok(_) => validate_runtime_dir(dir),
    }
}

fn validate_runtime_dir(dir: &Path) -> TransportResult<()> {
    // `symlink_metadata` does not follow symlinks: a symlink endpoint is
    // observed as a symlink and rejected, never traversed.
    let metadata = fs::symlink_metadata(dir)?;
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        return Err(TransportError::EndpointSecurity(format!(
            "runtime directory {} is a symlink; refusing to follow",
            dir.display()
        )));
    }
    if !file_type.is_dir() {
        return Err(TransportError::InvalidEndpoint(format!(
            "runtime path {} exists but is not a directory",
            dir.display()
        )));
    }
    if metadata.uid() != current_uid() {
        return Err(TransportError::EndpointSecurity(format!(
            "runtime directory {} is not owned by the current user",
            dir.display()
        )));
    }
    if metadata.mode() & 0o7777 != RUNTIME_DIR_MODE {
        return Err(TransportError::EndpointSecurity(format!(
            "runtime directory {} must have mode 0700",
            dir.display()
        )));
    }
    Ok(())
}

/// Observable state of an endpoint path, inspected without following
/// symlinks. Inspection primitives only: liveness probing and stale-socket
/// removal belong to TASK-0020.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointState {
    /// Nothing exists at the path; a bind may proceed after admission.
    Absent,
    /// A socket node owned by the current user (collision for binders).
    SocketOwnedByCurrentUser,
    /// A socket node owned by someone else (foreign/insecure endpoint).
    SocketOwnedByOtherUser,
    /// A symlink: never followed, never unlinked by this layer.
    UnexpectedSymlink,
    /// A regular file: never treated as a socket, never unlinked.
    UnexpectedFile,
    /// A directory where the socket node should be.
    UnexpectedDirectory,
    /// Anything else, including uninspectable paths.
    UnexpectedOther,
}

/// Classifies the endpoint path without following symlinks and without
/// mutating anything.
#[must_use]
pub fn inspect_endpoint(path: &Path) -> EndpointState {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => EndpointState::Absent,
        Err(_) => EndpointState::UnexpectedOther,
        Ok(metadata) => {
            let file_type = metadata.file_type();
            if file_type.is_symlink() {
                EndpointState::UnexpectedSymlink
            } else if file_type.is_socket() {
                if metadata.uid() == current_uid() {
                    EndpointState::SocketOwnedByCurrentUser
                } else {
                    EndpointState::SocketOwnedByOtherUser
                }
            } else if file_type.is_file() {
                EndpointState::UnexpectedFile
            } else if file_type.is_dir() {
                EndpointState::UnexpectedDirectory
            } else {
                EndpointState::UnexpectedOther
            }
        }
    }
}

/// Filesystem identity snapshot of an endpoint node, captured with
/// `symlink_metadata` (never following symlinks).
///
/// Stale-socket recovery flow (owned by the daemon lifecycle layer):
/// snapshot → liveness probe → re-snapshot → remove only if unchanged.
/// The `(device, inode)` pair detects replacement between inspection and
/// removal; owner and file-type checks keep the removal scoped to our own
/// socket nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EndpointSnapshot {
    /// Filesystem device number.
    pub device: u64,
    /// Inode number on that device.
    pub inode: u64,
    /// Owning UID.
    pub uid: u32,
    /// Whether the node was a socket.
    pub is_socket: bool,
}

/// Captures the identity of the node at `path` without following symlinks.
/// Fails when nothing exists (callers treat absence as "nothing stale").
pub fn snapshot_endpoint(path: &Path) -> TransportResult<EndpointSnapshot> {
    let metadata = fs::symlink_metadata(path).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => TransportError::InvalidEndpoint(format!(
            "no endpoint node at {} to snapshot",
            path.display()
        )),
        _ => TransportError::Io(error),
    })?;
    Ok(EndpointSnapshot {
        device: metadata.dev(),
        inode: metadata.ino(),
        uid: metadata.uid(),
        is_socket: metadata.file_type().is_socket(),
    })
}

/// Removes the node at `path` only if it is still the exact node captured
/// in `snapshot` (same device, inode, owner, and socket file type) and only
/// if that node is a socket owned by the current user.
///
/// Returns `Ok(true)` when the stale node was removed, `Ok(false)` when the
/// path changed identity (replacement is preserved — possible TOCTOU
/// interference) or vanished (someone else already cleaned up). Never
/// removes symlinks, regular files, directories, or foreign-owned nodes:
/// those fail closed with [`TransportError::EndpointSecurity`].
pub fn remove_endpoint_if_unchanged(
    path: &Path,
    snapshot: &EndpointSnapshot,
) -> TransportResult<bool> {
    let current = match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(TransportError::Io(error)),
        Ok(metadata) => metadata,
    };
    let now = EndpointSnapshot {
        device: current.dev(),
        inode: current.ino(),
        uid: current.uid(),
        is_socket: current.file_type().is_socket(),
    };
    if now != *snapshot {
        return Ok(false);
    }
    if !snapshot.is_socket {
        return Err(TransportError::EndpointSecurity(format!(
            "refusing to remove non-socket node at {}; never blindly unlinking",
            path.display()
        )));
    }
    if snapshot.uid != current_uid() {
        return Err(TransportError::EndpointSecurity(format!(
            "refusing to remove foreign-owned node at {}",
            path.display()
        )));
    }
    fs::remove_file(path)?;
    Ok(true)
}

/// Binds the canonical socket endpoint.
///
/// Admits the runtime directory, refuses to replace or unlink any existing
/// node (socket collision maps to [`TransportError::EndpointOwned`] so
/// TASK-0020 can report `AlreadyRunning`), binds, then restricts the new
/// socket node to mode `0600`.
///
/// Requires a Tokio runtime context (the listener registers with the
/// reactor at bind time).
pub fn bind_server(endpoint: &UnixEndpoint) -> TransportResult<UnixListener> {
    ensure_runtime_dir(&endpoint.runtime_dir)?;
    match inspect_endpoint(&endpoint.socket_path) {
        EndpointState::Absent => {}
        EndpointState::SocketOwnedByCurrentUser | EndpointState::SocketOwnedByOtherUser => {
            return Err(TransportError::EndpointOwned {
                endpoint: endpoint.socket_path.display().to_string(),
            });
        }
        state => {
            return Err(TransportError::EndpointSecurity(format!(
                "refusing to replace {state:?} at {}; never blindly unlinking",
                endpoint.socket_path.display()
            )));
        }
    }
    let listener =
        UnixListener::bind(&endpoint.socket_path).map_err(|error| match error.kind() {
            io::ErrorKind::AddrInUse => TransportError::EndpointOwned {
                endpoint: endpoint.socket_path.display().to_string(),
            },
            io::ErrorKind::PermissionDenied => TransportError::EndpointSecurity(format!(
                "cannot bind {}: permission denied",
                endpoint.socket_path.display()
            )),
            _ => TransportError::Io(error),
        })?;
    fs::set_permissions(
        &endpoint.socket_path,
        fs::Permissions::from_mode(SOCKET_MODE),
    )?;
    Ok(listener)
}

/// Connects a local client stream. No TCP, no retries, no spawn: lifecycle
/// belongs to TASK-0020.
pub async fn connect(endpoint: &UnixEndpoint) -> TransportResult<UnixStream> {
    UnixStream::connect(&endpoint.socket_path)
        .await
        .map_err(|error| match error.kind() {
            io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused => {
                TransportError::EndpointUnavailable {
                    endpoint: endpoint.socket_path.display().to_string(),
                }
            }
            io::ErrorKind::PermissionDenied => TransportError::EndpointSecurity(format!(
                "cannot connect to {}: permission denied",
                endpoint.socket_path.display()
            )),
            _ => TransportError::Io(error),
        })
}

/// Accepts one inbound byte stream. Handshake/session handling belongs to
/// TASK-0019.
pub async fn accept(listener: &UnixListener) -> TransportResult<UnixStream> {
    listener
        .accept()
        .await
        .map(|(stream, _)| stream)
        .map_err(TransportError::from)
}

/// Wraps a connected stream with G1 framing.
pub fn framed(stream: UnixStream) -> FramedTransport<UnixStream> {
    FramedTransport::new(stream)
}

/// Current effective UID. Exported for lifecycle layers that must verify
/// socket/dir ownership without taking on filesystem mechanics.
#[must_use]
pub fn current_uid() -> u32 {
    // Safety: `geteuid` has no preconditions; it always returns the
    // effective UID of the calling process and touches no memory.
    unsafe { libc::geteuid() }
}

/// `$XDG_RUNTIME_DIR/ai-engineering-os/` when the variable names a usable
/// directory for the current user; `None` selects the documented fallback.
/// (Linux resolution; macOS uses [`macos_runtime_parent`].)
#[cfg(not(target_os = "macos"))]
fn xdg_runtime_parent() -> Option<PathBuf> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")?;
    if dir.is_empty() {
        return None;
    }
    let path = PathBuf::from(dir);
    if !path.is_absolute() {
        return None;
    }
    let metadata = fs::symlink_metadata(&path).ok()?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return None;
    }
    if metadata.uid() != current_uid() {
        return None;
    }
    Some(path.join(APP_DIR_NAME))
}

/// `${TMPDIR:-/tmp}/ai-engineering-os-$UID/` (Linux fallback).
///
/// Mirrors the macOS fallback: both append the current UID so that two users
/// sharing the same temp directory get distinct runtime parents (TASK-SEC-R1
/// R1-M4).
#[cfg(not(target_os = "macos"))]
fn fallback_runtime_parent() -> PathBuf {
    let tmp = std::env::var_os("TMPDIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    runtime_parent_for(tmp, current_uid())
}

/// `$TMPDIR/ai-engineering-os-$UID/` (macOS).
///
/// Appends the current UID, exactly like the Linux fallback, so that two users
/// sharing the same temp directory never collide on the singleton daemon
/// socket (TASK-SEC-R1 R1-M4).
#[cfg(target_os = "macos")]
fn macos_runtime_parent() -> PathBuf {
    let tmp = std::env::var_os("TMPDIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(std::env::temp_dir);
    runtime_parent_for(tmp, current_uid())
}

/// Builds the per-UID runtime directory for a given temp directory.
///
/// Shared by the macOS and Linux fallbacks so they produce per-user distinct
/// paths. The XDG and override paths intentionally omit the UID because those
/// directories are already per-user; only the shared temp fallbacks need the
/// disambiguation.
fn runtime_parent_for(tmp: PathBuf, uid: u32) -> PathBuf {
    tmp.join(format!("{}-{}", APP_DIR_NAME, uid))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    static ENV_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Test-owned sandbox under `/tmp` with a short unique name: macOS
    /// `sun_path` fits 104 bytes, so sandboxes must stay short. Removed on
    /// drop (best effort) so runs never pollute the real runtime endpoint.
    struct Sandbox {
        root: PathBuf,
        endpoint: UnixEndpoint,
    }

    impl Sandbox {
        fn new(suite: &str) -> Self {
            let id = COUNTER.fetch_add(1, Ordering::SeqCst);
            let root =
                PathBuf::from("/tmp").join(format!("ipt-{}-{}-{}", std::process::id(), suite, id));
            fs::create_dir(&root).expect("sandbox root");
            let runtime_dir = root.join("run");
            let endpoint = UnixEndpoint {
                socket_path: runtime_dir.join(SOCKET_FILE_NAME),
                runtime_dir,
            };
            Self { root, endpoint }
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    /// Short scratch directory (no endpoint layout implied).
    fn scratch_dir(suite: &str) -> PathBuf {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir =
            PathBuf::from("/tmp").join(format!("ipt-{}-{}-{}", std::process::id(), suite, id));
        fs::create_dir(&dir).expect("scratch dir");
        dir
    }

    fn dir_mode(path: &Path) -> u32 {
        fs::symlink_metadata(path).expect("stat").mode() & 0o7777
    }

    #[test]
    fn runtime_dir_is_created_with_mode_0700() {
        let sandbox = Sandbox::new("create");
        ensure_runtime_dir(&sandbox.endpoint.runtime_dir).expect("create");
        assert!(sandbox.endpoint.runtime_dir.is_dir());
        assert_eq!(dir_mode(&sandbox.endpoint.runtime_dir), 0o700);
    }

    #[test]
    fn existing_insecure_runtime_dir_is_rejected() {
        let sandbox = Sandbox::new("insecure");
        fs::create_dir(&sandbox.endpoint.runtime_dir).expect("setup");
        fs::set_permissions(
            &sandbox.endpoint.runtime_dir,
            fs::Permissions::from_mode(0o755),
        )
        .expect("setup perms");
        match ensure_runtime_dir(&sandbox.endpoint.runtime_dir) {
            Err(TransportError::EndpointSecurity(_)) => {}
            other => panic!("expected EndpointSecurity, got {other:?}"),
        }
        // Rejected, not repaired: still 0755.
        assert_eq!(dir_mode(&sandbox.endpoint.runtime_dir), 0o755);
    }

    #[test]
    fn symlinked_runtime_dir_is_rejected_without_following() {
        let root = scratch_dir("symlinkdir");
        let target = root.join("target");
        fs::create_dir(&target).expect("setup");
        let link = root.join("run");
        std::os::unix::fs::symlink(&target, &link).expect("setup symlink");
        match ensure_runtime_dir(&link) {
            Err(TransportError::EndpointSecurity(_)) => {}
            other => panic!("expected EndpointSecurity, got {other:?}"),
        }
        assert!(fs::symlink_metadata(&link)
            .expect("stat")
            .file_type()
            .is_symlink());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn existing_file_as_runtime_dir_is_invalid() {
        let root = scratch_dir("filedir");
        let file = root.join("run");
        fs::write(&file, b"not a dir").expect("setup");
        match ensure_runtime_dir(&file) {
            Err(TransportError::InvalidEndpoint(_)) => {}
            other => panic!("expected InvalidEndpoint, got {other:?}"),
        }
        let _ = fs::remove_dir_all(&root);
    }

    // Note: `UnixListener::bind` registers with the Tokio reactor, so bind
    // tests run inside a runtime even though `bind_server` itself is sync.
    #[tokio::test]
    async fn socket_bind_sets_mode_0600() {
        let sandbox = Sandbox::new("bind");
        let _listener = bind_server(&sandbox.endpoint).expect("bind");
        let mode = fs::symlink_metadata(&sandbox.endpoint.socket_path)
            .expect("stat")
            .mode()
            & 0o7777;
        assert_eq!(mode, 0o600);
        assert_eq!(
            inspect_endpoint(&sandbox.endpoint.socket_path),
            EndpointState::SocketOwnedByCurrentUser
        );
    }

    #[tokio::test]
    async fn second_bind_reports_collision_without_deleting() {
        let sandbox = Sandbox::new("collision");
        let _first = bind_server(&sandbox.endpoint).expect("first bind");
        match bind_server(&sandbox.endpoint) {
            Err(TransportError::EndpointOwned { .. }) => {}
            other => panic!("expected EndpointOwned, got {other:?}"),
        }
        // Still a socket, still ours.
        assert_eq!(
            inspect_endpoint(&sandbox.endpoint.socket_path),
            EndpointState::SocketOwnedByCurrentUser
        );
    }

    #[test]
    fn existing_regular_file_is_never_unlinked() {
        let sandbox = Sandbox::new("regfile");
        fs::create_dir(&sandbox.endpoint.runtime_dir).expect("setup");
        fs::set_permissions(
            &sandbox.endpoint.runtime_dir,
            fs::Permissions::from_mode(0o700),
        )
        .expect("setup");
        fs::write(&sandbox.endpoint.socket_path, b"precious").expect("setup");
        match bind_server(&sandbox.endpoint) {
            Err(TransportError::EndpointSecurity(_)) => {}
            other => panic!("expected EndpointSecurity, got {other:?}"),
        }
        assert_eq!(
            fs::read(&sandbox.endpoint.socket_path).expect("read"),
            b"precious"
        );
        assert_eq!(
            inspect_endpoint(&sandbox.endpoint.socket_path),
            EndpointState::UnexpectedFile
        );
    }

    #[test]
    fn symlink_endpoint_is_rejected_and_preserved() {
        let sandbox = Sandbox::new("symsock");
        fs::create_dir(&sandbox.endpoint.runtime_dir).expect("setup");
        fs::set_permissions(
            &sandbox.endpoint.runtime_dir,
            fs::Permissions::from_mode(0o700),
        )
        .expect("setup");
        let target = sandbox.root.join("real");
        fs::write(&target, b"real").expect("setup");
        std::os::unix::fs::symlink(&target, &sandbox.endpoint.socket_path).expect("setup symlink");
        match bind_server(&sandbox.endpoint) {
            Err(TransportError::EndpointSecurity(_)) => {}
            other => panic!("expected EndpointSecurity, got {other:?}"),
        }
        assert!(fs::symlink_metadata(&sandbox.endpoint.socket_path)
            .expect("stat")
            .file_type()
            .is_symlink());
        assert_eq!(
            inspect_endpoint(&sandbox.endpoint.socket_path),
            EndpointState::UnexpectedSymlink
        );
    }

    #[test]
    fn runtime_dir_override_selects_test_endpoint() {
        let _guard = ENV_GUARD.lock().expect("env guard");
        let sandbox_root = scratch_dir("override");
        let previous = std::env::var_os(RUNTIME_DIR_OVERRIDE_ENV);
        unsafe {
            std::env::set_var(RUNTIME_DIR_OVERRIDE_ENV, &sandbox_root);
        }
        let resolved = resolve_endpoint();
        match previous {
            Some(value) => unsafe { std::env::set_var(RUNTIME_DIR_OVERRIDE_ENV, value) },
            None => unsafe { std::env::remove_var(RUNTIME_DIR_OVERRIDE_ENV) },
        }
        let endpoint = resolved.expect("resolve");
        assert_eq!(endpoint.runtime_dir, sandbox_root.join("ai-engineering-os"));
        assert_eq!(
            endpoint.socket_path.file_name().and_then(|n| n.to_str()),
            Some(SOCKET_FILE_NAME)
        );
        let _ = fs::remove_dir_all(&sandbox_root);
    }

    #[test]
    fn overlong_socket_path_is_rejected() {
        let long = PathBuf::from("/tmp")
            .join("a".repeat(400))
            .join(SOCKET_FILE_NAME);
        match preflight_socket_path(&long) {
            Err(TransportError::PathTooLong { path }) => assert_eq!(path, long),
            other => panic!("expected PathTooLong, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn stale_socket_snapshot_remove_round_trip() {
        let sandbox = Sandbox::new("stalesnap");
        fs::create_dir(&sandbox.endpoint.runtime_dir).expect("setup");
        // Bind then drop the listener: stale socket node remains, no owner.
        let listener = UnixListener::bind(&sandbox.endpoint.socket_path).expect("bind");
        let snapshot = snapshot_endpoint(&sandbox.endpoint.socket_path).expect("snapshot");
        assert!(snapshot.is_socket);
        drop(listener);
        assert_eq!(
            inspect_endpoint(&sandbox.endpoint.socket_path),
            EndpointState::SocketOwnedByCurrentUser
        );
        assert!(
            remove_endpoint_if_unchanged(&sandbox.endpoint.socket_path, &snapshot).expect("remove"),
            "unchanged stale socket must be removed"
        );
        assert_eq!(
            inspect_endpoint(&sandbox.endpoint.socket_path),
            EndpointState::Absent
        );
        // Second removal is a no-op, not an error.
        assert!(
            !remove_endpoint_if_unchanged(&sandbox.endpoint.socket_path, &snapshot)
                .expect("second remove"),
        );
    }

    #[tokio::test]
    async fn replaced_endpoint_is_preserved_by_remove_guard() {
        let sandbox = Sandbox::new("toctou");
        fs::create_dir(&sandbox.endpoint.runtime_dir).expect("setup");
        let listener = UnixListener::bind(&sandbox.endpoint.socket_path).expect("bind");
        let snapshot = snapshot_endpoint(&sandbox.endpoint.socket_path).expect("snapshot");
        assert!(snapshot.is_socket);
        drop(listener);
        // The path is replaced by a different object class before cleanup
        // runs. The stored socket identity can never match a regular file,
        // so this is deterministic (no inode-reuse gamble).
        fs::remove_file(&sandbox.endpoint.socket_path).expect("setup");
        fs::write(&sandbox.endpoint.socket_path, b"replacement").expect("setup");
        match remove_endpoint_if_unchanged(&sandbox.endpoint.socket_path, &snapshot) {
            Ok(false) => {}
            other => panic!("expected refusal (Ok(false)), got {other:?}"),
        }
        assert_eq!(
            fs::read(&sandbox.endpoint.socket_path).expect("read"),
            b"replacement",
            "replacement must be preserved"
        );
    }

    #[test]
    fn remove_guard_rejects_non_socket_and_symlink() {
        let sandbox = Sandbox::new("noguard");
        fs::create_dir(&sandbox.endpoint.runtime_dir).expect("setup");
        fs::write(&sandbox.endpoint.socket_path, b"plain file").expect("setup");
        let snapshot = snapshot_endpoint(&sandbox.endpoint.socket_path).expect("snapshot");
        assert!(!snapshot.is_socket);
        match remove_endpoint_if_unchanged(&sandbox.endpoint.socket_path, &snapshot) {
            Err(TransportError::EndpointSecurity(_)) => {}
            other => panic!("expected EndpointSecurity, got {other:?}"),
        }
        assert_eq!(
            fs::read(&sandbox.endpoint.socket_path).expect("read"),
            b"plain file"
        );

        let target = sandbox.root.join("real");
        fs::write(&target, b"real").expect("setup");
        let link = sandbox.endpoint.runtime_dir.join("link.sock");
        std::os::unix::fs::symlink(&target, &link).expect("setup");
        let link_snapshot = snapshot_endpoint(&link).expect("snapshot");
        assert!(!link_snapshot.is_socket);
        match remove_endpoint_if_unchanged(&link, &link_snapshot) {
            Err(TransportError::EndpointSecurity(_)) => {}
            other => panic!("expected EndpointSecurity, got {other:?}"),
        }
        assert!(fs::symlink_metadata(&link)
            .expect("stat")
            .file_type()
            .is_symlink());
    }

    #[test]
    fn snapshot_of_absent_path_is_invalid() {
        let sandbox = Sandbox::new("snapabsent");
        match snapshot_endpoint(&sandbox.endpoint.socket_path) {
            Err(TransportError::InvalidEndpoint(_)) => {}
            other => panic!("expected InvalidEndpoint, got {other:?}"),
        }
    }

    #[test]
    fn inspect_endpoint_covers_absent_and_directory() {
        let sandbox = Sandbox::new("inspect");
        let missing = sandbox.root.join("missing").join(SOCKET_FILE_NAME);
        assert_eq!(inspect_endpoint(&missing), EndpointState::Absent);
        assert_eq!(
            inspect_endpoint(&sandbox.root),
            EndpointState::UnexpectedDirectory
        );
    }

    #[test]
    fn connect_to_absent_endpoint_is_unavailable() {
        let sandbox = Sandbox::new("connabsent");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        match runtime.block_on(connect(&sandbox.endpoint)) {
            Err(TransportError::EndpointUnavailable { .. }) => {}
            other => panic!("expected EndpointUnavailable, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn framed_payload_crosses_unix_stream() {
        use bytes::Bytes;

        let sandbox = Sandbox::new("xstream");
        let listener = bind_server(&sandbox.endpoint).expect("bind");
        let mut client = framed(connect(&sandbox.endpoint).await.expect("connect"));
        let server_stream = accept(&listener).await.expect("accept");
        let mut server = framed(server_stream);

        client
            .write_frame(Bytes::from_static(b"hello-daemon"))
            .await
            .expect("client write");
        let got = server.read_frame().await.expect("server read");
        assert_eq!(&got[..], b"hello-daemon");

        server
            .write_frame(Bytes::from_static(b"hello-client"))
            .await
            .expect("server write");
        let back = client.read_frame().await.expect("client read");
        assert_eq!(&back[..], b"hello-client");
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn resolve_endpoint_uses_tmpdir_on_macos() {
        let _guard = ENV_GUARD.lock().expect("env guard");
        let sandbox_root = scratch_dir("resolvemac");
        let previous = std::env::var_os("TMPDIR");
        unsafe {
            std::env::set_var("TMPDIR", &sandbox_root);
        }
        let resolved = resolve_endpoint();
        match previous {
            Some(value) => unsafe { std::env::set_var("TMPDIR", value) },
            None => unsafe { std::env::remove_var("TMPDIR") },
        }
        let endpoint = resolved.expect("resolve");
        assert_eq!(
            endpoint.runtime_dir,
            sandbox_root.join(format!("{}-{}", APP_DIR_NAME, current_uid()))
        );
        assert_eq!(
            endpoint.socket_path.file_name().and_then(|n| n.to_str()),
            Some(SOCKET_FILE_NAME)
        );
        let _ = fs::remove_dir_all(&sandbox_root);
    }

    /// TASK-SEC-R1 R1-M4: two distinct UIDs must produce distinct runtime
    /// parents so that two users sharing the same temp directory never collide
    /// on the per-user daemon socket. Tests the shared UID-suffixing logic
    /// directly with synthetic UIDs (platform-independent).
    #[test]
    fn two_synthetic_uids_get_distinct_runtime_parents() {
        let tmp = PathBuf::from("/tmp");
        let a = runtime_parent_for(tmp.clone(), 1000);
        let b = runtime_parent_for(tmp, 1001);
        assert_ne!(a, b, "UID suffix must disambiguate runtime parents");
        assert!(
            a.ends_with("ai-engineering-os-1000"),
            "expected UID suffix on {a:?}"
        );
        assert!(
            b.ends_with("ai-engineering-os-1001"),
            "expected UID suffix on {b:?}"
        );
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn resolve_endpoint_prefers_usable_xdg_runtime_dir() {
        let _guard = ENV_GUARD.lock().expect("env guard");
        fs::set_permissions(&sandbox_root, fs::Permissions::from_mode(0o700)).expect("setup");
        let previous = std::env::var_os("XDG_RUNTIME_DIR");
        unsafe {
            std::env::set_var("XDG_RUNTIME_DIR", &sandbox_root);
        }
        let resolved = resolve_endpoint();
        match previous {
            Some(value) => unsafe { std::env::set_var("XDG_RUNTIME_DIR", value) },
            None => unsafe { std::env::remove_var("XDG_RUNTIME_DIR") },
        }
        let endpoint = resolved.expect("resolve");
        assert_eq!(endpoint.runtime_dir, sandbox_root.join("ai-engineering-os"));
        let _ = fs::remove_dir_all(&sandbox_root);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn resolve_endpoint_falls_back_without_xdg() {
        let _guard = ENV_GUARD.lock().expect("env guard");
        let previous = std::env::var_os("XDG_RUNTIME_DIR");
        unsafe {
            std::env::remove_var("XDG_RUNTIME_DIR");
        }
        let resolved = resolve_endpoint();
        match previous {
            Some(value) => unsafe { std::env::set_var("XDG_RUNTIME_DIR", value) },
            None => unsafe { std::env::remove_var("XDG_RUNTIME_DIR") },
        }
        let endpoint = resolved.expect("resolve");
        let dir = endpoint.runtime_dir.to_string_lossy().into_owned();
        assert!(
            dir.contains(&format!("ai-engineering-os-{}", libc::geteuid())),
            "fallback must carry the UID: {dir}"
        );
    }
}
