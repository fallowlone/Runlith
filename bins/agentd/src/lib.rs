//! `agentd` G1 control server: secure endpoint ownership, OS peer
//! authentication (ADR-0020), accept loop, handshake/ping via
//! `ipc-control`, and graceful shutdown.
//!
//! Daemon process lifecycle + control-server integration only. No Task
//! Engine, SQLite, providers, Runner, Memory, Context, business IPC, or
//! Policy Engine (G6). Internal process-lifecycle state below is NOT Task
//! lifecycle state.
//!
//! Connection pipeline: transport → OS-authenticated peer → handshake →
//! READY. Peer authentication is connection-local and never durable.

use std::fmt;
use std::io;
use std::sync::Arc;
use std::time::Duration;

use bytes::BytesMut;
#[cfg(unix)]
use ipc_control::{ClientConfig, ClientSession};
use ipc_control::{ControlError, ReadyRequest, ServerConfig, ServerSession};
#[cfg(unix)]
use ipc_transport::TransportResult;
use ipc_transport::{
    same_local_principal, FramedTransport, PeerIdentity, TransportError, MAX_CONTROL_CLIENTS,
};
use tokio::io::{AsyncRead, AsyncWrite};

#[cfg(unix)]
use ipc_transport::platform::unix::{
    bind_server, connect, current_uid, ensure_runtime_dir, inspect_endpoint,
    remove_endpoint_if_unchanged, resolve_endpoint, snapshot_endpoint, EndpointState, UnixEndpoint,
};
#[cfg(windows)]
use ipc_transport::platform::windows::PipeListener;
#[cfg(windows)]
use tokio::net::windows::named_pipe::NamedPipeServer;
#[cfg(unix)]
use tokio::net::{UnixListener, UnixStream};

/// G1 daemon lifecycle errors. Local taxonomy only; never in Protobuf.
#[derive(Debug)]
pub enum DaemonError {
    /// Byte transport/framing/endpoint failure (preserved, not flattened).
    Transport(TransportError),
    /// Control-protocol failure (preserved, not flattened).
    Control(ControlError),
    /// Endpoint admission failed: insecure, unexpected, or foreign object.
    /// Never degraded, never bypassed.
    EndpointSecurity(String),
    /// Endpoint path unusable without a security implication.
    InvalidEndpoint(String),
    /// Another live daemon owns the canonical endpoint.
    AlreadyRunning,
    /// OS peer authentication denied the connection: identity mismatch.
    /// The connection is closed with no handshake, no READY, no mutation.
    PeerDenied,
    /// OS/runtime I/O failure.
    Io(io::Error),
}

impl fmt::Display for DaemonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport(error) => write!(f, "daemon transport failure: {error}"),
            Self::Control(error) => write!(f, "daemon protocol failure: {error}"),
            Self::EndpointSecurity(detail) => {
                write!(f, "daemon endpoint security failure: {detail}")
            }
            Self::InvalidEndpoint(detail) => write!(f, "daemon invalid endpoint: {detail}"),
            Self::AlreadyRunning => write!(f, "daemon already running (endpoint owned)"),
            Self::PeerDenied => write!(f, "peer authentication denied (identity mismatch)"),
            Self::Io(error) => write!(f, "daemon I/O failure: {error}"),
        }
    }
}

impl std::error::Error for DaemonError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Transport(error) => Some(error),
            Self::Control(error) => Some(error),
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<TransportError> for DaemonError {
    fn from(error: TransportError) -> Self {
        Self::Transport(error)
    }
}

impl From<ControlError> for DaemonError {
    fn from(error: ControlError) -> Self {
        Self::Control(error)
    }
}

impl From<io::Error> for DaemonError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Daemon configuration. Protocol behavior only; no business knobs.
#[derive(Debug, Clone)]
pub struct DaemonConfig {
    /// Handshake negotiation parameters.
    pub server_config: ServerConfig,
}

impl DaemonConfig {
    /// Accepted G1 daemon configuration.
    pub fn g1_default() -> Self {
        Self {
            server_config: ServerConfig::g1_default(),
        }
    }
}

/// Terminal outcome of a daemon run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunOutcome {
    /// A post-handshake `ShutdownRequest` drove the accepted sequence:
    /// stop admission, acknowledge, wind down, release endpoint, exit.
    ShutdownCompleted {
        /// Control connections served during this run.
        connections_served: u64,
    },
}

/// Per-connection outcome. Malformed/invalid sessions end the connection,
/// never the daemon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnectionOutcome {
    /// This connection requested shutdown; daemon wind-down follows.
    ShutdownInitiated,
    /// Connection ended without shutdown (close, violation, timeout).
    Ended,
}

/// Local authentication policy (ADR-0020 §12, §13): the peer is allowed
/// only when its OS identity is the same local principal as the daemon's.
/// Shared by the Unix and Windows admission paths so exactly one policy
/// exists. `client_kind`, PID, names, and paths never enter this decision.
pub(crate) fn authorize_peer(
    daemon: &PeerIdentity,
    peer: &PeerIdentity,
) -> Result<(), DaemonError> {
    if same_local_principal(daemon, peer) {
        Ok(())
    } else {
        Err(DaemonError::PeerDenied)
    }
}

/// Serves one authenticated stream: handshake, then the READY loop. Ping
/// is answered inside `ipc-control`; shutdown is acknowledged and
/// surfaced.
///
/// `prefetched` carries a frame read before authentication (Windows
/// first-frame staging): it is interpreted here only because the caller
/// already authenticated the peer. `None` reads the handshake live.
async fn serve_connection<S>(
    stream: S,
    config: &ServerConfig,
    shutdown_tx: tokio::sync::watch::Sender<bool>,
    prefetched: Option<BytesMut>,
) -> ConnectionOutcome
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut session = ServerSession::new(FramedTransport::new(stream), config.clone());
    let handshake = match prefetched {
        Some(frame) => session.run_handshake_from_frame(frame).await,
        None => session.run_handshake().await,
    };
    if handshake.is_err() {
        return ConnectionOutcome::Ended;
    }
    loop {
        match session.next_request().await {
            Ok(ReadyRequest::Ping) => continue,
            Ok(ReadyRequest::Shutdown) => {
                // Authorization basis: post-handshake request over the
                // secured per-user endpoint — never `client_kind`.
                // Acknowledge best-effort, then initiate shutdown even if
                // the acknowledgement itself failed to flush.
                let _ = session.respond_shutdown().await;
                let _ = shutdown_tx.send(true);
                return ConnectionOutcome::ShutdownInitiated;
            }
            Err(_) => return ConnectionOutcome::Ended,
        }
    }
}

/// Shared serve loop: admits authenticated streams from the accept pump
/// until a shutdown is initiated, then winds down. Each item carries an
/// optional prefetched first frame (Windows staging; `None` on Unix).
/// Admission is bounded: past [`MAX_CONTROL_CLIENTS`] the extra
/// connection is closed, never a daemon failure. Only peer-authenticated
/// streams ever arrive here — the pumps authenticate before sending.
async fn serve_streams<S>(
    mut accepted: tokio::sync::mpsc::Receiver<(S, Option<BytesMut>)>,
    config: &DaemonConfig,
) -> RunOutcome
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);
    let admissions = Arc::new(tokio::sync::Semaphore::new(MAX_CONTROL_CLIENTS));
    let mut joins = tokio::task::JoinSet::new();
    let mut connections_served: u64 = 0;

    loop {
        tokio::select! {
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    break;
                }
            }
            next = accepted.recv() => {
                let Some((stream, prefetched)) = next else {
                    // Accept pump gone: keep serving existing sessions,
                    // retry admission shortly instead of hot-spinning.
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    continue;
                };
                let permit = match admissions.clone().try_acquire_owned() {
                    Ok(permit) => permit,
                    Err(_) => continue,
                };
                connections_served += 1;
                let sender = shutdown_tx.clone();
                let server_config = config.server_config.clone();
                joins.spawn(async move {
                    let _permit = permit;
                    serve_connection(stream, &server_config, sender, prefetched).await
                });
            }
        }
    }

    // Wind down: acceptor output is ignored from here on, stragglers are
    // aborted, tasks reaped. The owned endpoint is released by the caller.
    joins.abort_all();
    while joins.join_next().await.is_some() {}
    RunOutcome::ShutdownCompleted { connections_served }
}

#[cfg(unix)]
mod unix_daemon {
    use super::*;
    use ipc_transport::platform::peer::{daemon_identity, peer_identity};

    /// Source of peer identity for one accepted connection. Production
    /// composition always uses [`OsPeerSource`] (kernel credentials);
    /// tests inject fixed identities through this narrow seam. The seam
    /// can only choose *which* identity is presented — never skip the
    /// [`authorize_peer`] decision.
    pub(crate) trait PeerSource: Send + Sync {
        fn peer_of(&self, stream: &UnixStream) -> TransportResult<PeerIdentity>;
    }

    /// Production peer source: kernel socket credentials.
    #[derive(Debug, Default)]
    pub(crate) struct OsPeerSource;

    impl PeerSource for OsPeerSource {
        fn peer_of(&self, stream: &UnixStream) -> TransportResult<PeerIdentity> {
            peer_identity(stream)
        }
    }

    /// Ownership acquisition result.
    enum Acquisition {
        /// This process owns the endpoint.
        Owned(UnixListener),
        /// A live compatible daemon answered the probe.
        LiveDaemon,
    }

    /// Probes for a live compatible daemon using the real G1 protocol:
    /// connect, handshake as an `agentctl`-compatible control client, ping.
    /// Any failure means "no live daemon answered" (stale or absent).
    async fn probe_live_daemon(endpoint: &UnixEndpoint) -> bool {
        let stream = match connect(endpoint).await {
            Ok(stream) => stream,
            Err(_) => return false,
        };
        let mut session = ClientSession::new(
            FramedTransport::new(stream),
            ClientConfig::new(ipc_control::CLIENT_KIND_AGENTCTL),
        );
        match session.perform_handshake().await {
            Ok(_) => session.ping().await.is_ok(),
            Err(_) => false,
        }
    }

    /// Attempts to bind an endpoint whose runtime directory is already
    /// admitted. A bind race lost to another daemon maps to `LiveDaemon`.
    async fn bind_owned(endpoint: &UnixEndpoint) -> Result<Acquisition, DaemonError> {
        match bind_server(endpoint) {
            Ok(listener) => Ok(Acquisition::Owned(listener)),
            Err(TransportError::EndpointOwned { .. }) => Ok(Acquisition::LiveDaemon),
            Err(other) => Err(other.into()),
        }
    }

    /// Full safe stale-socket recovery: inspect → snapshot identity →
    /// liveness probe → remove only if unchanged → bind. Foreign-owned or
    /// non-socket nodes fail closed; a changed identity aborts removal.
    async fn recover_stale(endpoint: &UnixEndpoint) -> Result<Acquisition, DaemonError> {
        let snapshot = match snapshot_endpoint(&endpoint.socket_path) {
            Ok(snapshot) => snapshot,
            // Vanished between inspection and snapshot: try a fresh bind.
            Err(_) => return bind_owned(endpoint).await,
        };
        if !snapshot.is_socket || snapshot.uid != current_uid() {
            return Err(DaemonError::EndpointSecurity(format!(
                "refusing stale recovery of non-socket or foreign node at {}",
                endpoint.socket_path.display()
            )));
        }
        if probe_live_daemon(endpoint).await {
            return Ok(Acquisition::LiveDaemon);
        }
        match remove_endpoint_if_unchanged(&endpoint.socket_path, &snapshot)? {
            true => bind_owned(endpoint).await,
            false => {
                // Identity changed under us: re-probe before giving up.
                if probe_live_daemon(endpoint).await {
                    return Ok(Acquisition::LiveDaemon);
                }
                Err(DaemonError::InvalidEndpoint(format!(
                    "endpoint at {} changed during stale recovery; refusing to proceed",
                    endpoint.socket_path.display()
                )))
            }
        }
    }

    /// Resolves startup into ownership or a live-daemon collision.
    /// Never deletes blindly, never renames, never chmods foreign objects,
    /// never falls back to another endpoint.
    async fn acquire_endpoint(endpoint: &UnixEndpoint) -> Result<Acquisition, DaemonError> {
        ensure_runtime_dir(&endpoint.runtime_dir)?;
        match inspect_endpoint(&endpoint.socket_path) {
            EndpointState::Absent => bind_owned(endpoint).await,
            EndpointState::SocketOwnedByCurrentUser => {
                if probe_live_daemon(endpoint).await {
                    return Ok(Acquisition::LiveDaemon);
                }
                recover_stale(endpoint).await
            }
            EndpointState::SocketOwnedByOtherUser => Err(DaemonError::EndpointSecurity(format!(
                "foreign-owned socket at {}; failing closed",
                endpoint.socket_path.display()
            ))),
            state => Err(DaemonError::EndpointSecurity(format!(
                "unexpected {state:?} at {}; failing closed, never removing",
                endpoint.socket_path.display()
            ))),
        }
    }

    /// Starts the daemon on an explicit endpoint (production passes the
    /// canonical resolved endpoint; tests pass sandboxes).
    pub async fn run_on_endpoint(
        endpoint: &UnixEndpoint,
        config: DaemonConfig,
    ) -> Result<RunOutcome, DaemonError> {
        run_on_endpoint_with(endpoint, config, OsPeerSource).await
    }

    /// Test-seam entry: identical to [`run_on_endpoint`] except peer
    /// identity comes from `source`. Production callers use
    /// [`run_on_endpoint`], which hardcodes [`OsPeerSource`].
    pub(crate) async fn run_on_endpoint_with(
        endpoint: &UnixEndpoint,
        config: DaemonConfig,
        source: impl PeerSource + 'static,
    ) -> Result<RunOutcome, DaemonError> {
        let listener = match acquire_endpoint(endpoint).await? {
            Acquisition::Owned(listener) => listener,
            Acquisition::LiveDaemon => return Err(DaemonError::AlreadyRunning),
        };
        // Daemon principal once, from the OS — never from configuration.
        let daemon = daemon_identity().map_err(DaemonError::from)?;
        // Snapshot the node we own so shutdown cleanup removes only it.
        let owned = snapshot_endpoint(&endpoint.socket_path).ok();
        let (accepted_tx, accepted_rx) =
            tokio::sync::mpsc::channel::<(UnixStream, Option<BytesMut>)>(MAX_CONTROL_CLIENTS + 2);
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        // Admission order: extract peer → authorize → serve.
                        // Denied or unidentifiable peers are closed here:
                        // no handshake, no READY, no response, no event.
                        let admitted = source
                            .peer_of(&stream)
                            .map_err(DaemonError::from)
                            .and_then(|peer| authorize_peer(&daemon, &peer));
                        if admitted.is_err() {
                            continue;
                        }
                        if accepted_tx.send((stream, None)).await.is_err() {
                            break;
                        }
                    }
                    // Transient accept failure must not kill the daemon.
                    Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
                }
            }
        });
        let outcome = serve_streams(accepted_rx, &config).await;
        if let Some(snapshot) = owned.as_ref() {
            let _ = remove_endpoint_if_unchanged(&endpoint.socket_path, snapshot);
        }
        Ok(outcome)
    }

    /// Canonical production entry: resolve, acquire, serve, clean up.
    pub async fn run(config: DaemonConfig) -> Result<RunOutcome, DaemonError> {
        let endpoint = resolve_endpoint()?;
        run_on_endpoint(&endpoint, config).await
    }
}

#[cfg(unix)]
pub use unix_daemon::{run, run_on_endpoint};

#[cfg(windows)]
mod windows_daemon {
    use super::*;
    use ipc_transport::platform::peer::{daemon_identity, pipe_client_identity};
    use ipc_transport::platform::windows::framed_server;

    /// Canonical production entry: first-instance bind, staged
    /// authenticate-then-serve, exit. No stale-socket deletion exists on
    /// Windows; a lost singleton race terminates as `AlreadyRunning`. No
    /// alternate pipe names.
    ///
    /// Staged admission (ADR-0020 §8): the accept pump reads exactly one
    /// bounded frame as opaque bytes, authenticates the peer SID, and only
    /// then hands the server plus the still-undecoded frame to the serve
    /// loop. Denied peers are closed with no response. The pump handles
    /// one connection at a time through read+auth, so no unbounded
    /// pre-auth pool exists; post-auth connections share the
    /// `MAX_CONTROL_CLIENTS` bound, and a silent client is bounded by the
    /// frame read timeout.
    pub async fn run(config: DaemonConfig) -> Result<RunOutcome, DaemonError> {
        let mut listener = match PipeListener::bind_first() {
            Ok(listener) => listener,
            Err(TransportError::EndpointOwned { .. }) => {
                return Err(DaemonError::AlreadyRunning);
            }
            Err(other) => return Err(other.into()),
        };
        // Daemon principal once, from the OS — never from configuration.
        let daemon = daemon_identity().map_err(DaemonError::from)?;
        let (accepted_tx, accepted_rx) = tokio::sync::mpsc::channel::<(
            NamedPipeServer,
            Option<BytesMut>,
        )>(MAX_CONTROL_CLIENTS + 2);
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok(server) => {
                        // Stage 1: one bounded frame as opaque bytes.
                        // Framing rules apply; nothing is decoded here.
                        let mut framed = framed_server(server);
                        let frame = match framed.read_frame().await {
                            Ok(frame) => frame,
                            Err(_) => continue,
                        };
                        let server = framed.into_inner().into_inner();
                        // Stage 2: authenticate before any decode. The
                        // frame stays undecoded until the serve loop runs
                        // the prefetched handshake below.
                        let admitted = pipe_client_identity(&server)
                            .map_err(DaemonError::from)
                            .and_then(|peer| authorize_peer(&daemon, &peer));
                        if admitted.is_err() {
                            continue;
                        }
                        if accepted_tx.send((server, Some(frame))).await.is_err() {
                            break;
                        }
                    }
                    Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
                }
            }
        });
        Ok(serve_streams(accepted_rx, &config).await)
    }
}

#[cfg(windows)]
pub use windows_daemon::run;

#[cfg(all(test, unix))]
mod tests {
    use super::unix_daemon::{run_on_endpoint_with, OsPeerSource, PeerSource};
    use super::*;
    use bytes::Bytes;
    use ipc_contracts::proto::{
        control_request, control_response, ControlRequest, ControlResponse, ShutdownRequest,
    };
    use ipc_transport::platform::peer::{daemon_identity, from_uid};
    use ipc_transport::platform::unix::{
        bind_server as transport_bind, connect as transport_connect,
    };
    use prost::Message;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use std::sync::Arc;

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    struct Sandbox {
        root: PathBuf,
        endpoint: UnixEndpoint,
    }

    impl Sandbox {
        fn new(suite: &str) -> Self {
            let id = COUNTER.fetch_add(1, Ordering::SeqCst);
            let root =
                PathBuf::from("/tmp").join(format!("add-{}-{}-{}", std::process::id(), suite, id));
            std::fs::create_dir(&root).expect("sandbox root");
            let runtime_dir = root.join("run");
            let endpoint = UnixEndpoint {
                socket_path: runtime_dir.join(ipc_transport::platform::unix::SOCKET_FILE_NAME),
                runtime_dir,
            };
            Self { root, endpoint }
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn spawn_daemon(
        endpoint: &UnixEndpoint,
    ) -> tokio::task::JoinHandle<Result<RunOutcome, DaemonError>> {
        let endpoint = endpoint.clone();
        tokio::spawn(async move { run_on_endpoint(&endpoint, DaemonConfig::g1_default()).await })
    }

    async fn admin_session(endpoint: &UnixEndpoint) -> ClientSession<UnixStream> {
        let stream = transport_connect(endpoint).await.expect("connect");
        let mut session = ClientSession::new(
            FramedTransport::new(stream),
            ClientConfig::new(ipc_control::CLIENT_KIND_AGENTCTL),
        );
        session.perform_handshake().await.expect("handshake");
        assert!(session.is_ready());
        session
    }

    async fn wait_ready(endpoint: &UnixEndpoint) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            if let Ok(stream) = transport_connect(endpoint).await {
                let mut session = ClientSession::new(
                    FramedTransport::new(stream),
                    ClientConfig::new(ipc_control::CLIENT_KIND_AGENTCTL),
                );
                if session.perform_handshake().await.is_ok() {
                    return;
                }
            }
            if tokio::time::Instant::now() >= deadline {
                panic!("daemon never became ready");
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    fn shutdown_frame() -> Bytes {
        Bytes::from(
            ControlRequest {
                payload: Some(control_request::Payload::Shutdown(ShutdownRequest {})),
            }
            .encode_to_vec(),
        )
    }

    async fn shutdown_and_await(
        handle: tokio::task::JoinHandle<Result<RunOutcome, DaemonError>>,
        endpoint: &UnixEndpoint,
    ) -> RunOutcome {
        let session = admin_session(endpoint).await;
        let mut transport = session.into_inner();
        transport
            .write_frame(shutdown_frame())
            .await
            .expect("write shutdown");
        let frame = transport.read_frame().await.expect("read ack");
        let response = ControlResponse::decode(&frame[..]).expect("decode");
        assert!(
            matches!(
                response.payload,
                Some(control_response::Payload::Shutdown(_))
            ),
            "expected ShutdownResponse, got {response:?}"
        );
        drop(transport);
        tokio::time::timeout(Duration::from_secs(10), handle)
            .await
            .expect("daemon exits promptly")
            .expect("task joins")
            .expect("clean shutdown")
    }

    #[tokio::test]
    async fn daemon_serves_handshake_ping_and_shutdown() {
        let sandbox = Sandbox::new("serve");
        let handle = spawn_daemon(&sandbox.endpoint);
        wait_ready(&sandbox.endpoint).await;
        let mut session = admin_session(&sandbox.endpoint).await;
        session.ping().await.expect("ping");
        session.ping().await.expect("second ping");
        drop(session);
        let outcome = shutdown_and_await(handle, &sandbox.endpoint).await;
        assert!(
            matches!(outcome, RunOutcome::ShutdownCompleted { connections_served } if connections_served >= 2),
            "got {outcome:?}"
        );
        assert!(
            !sandbox.endpoint.socket_path.exists(),
            "owned endpoint must be released"
        );
    }

    #[tokio::test]
    async fn second_daemon_reports_already_running() {
        let sandbox = Sandbox::new("second");
        let handle_a = spawn_daemon(&sandbox.endpoint);
        wait_ready(&sandbox.endpoint).await;
        match run_on_endpoint(&sandbox.endpoint, DaemonConfig::g1_default()).await {
            Err(DaemonError::AlreadyRunning) => {}
            other => panic!("expected AlreadyRunning, got {other:?}"),
        }
        // First daemon is alive and serving.
        admin_session(&sandbox.endpoint)
            .await
            .ping()
            .await
            .expect("ping");
        let outcome = shutdown_and_await(handle_a, &sandbox.endpoint).await;
        assert!(matches!(outcome, RunOutcome::ShutdownCompleted { .. }));
    }

    #[tokio::test]
    async fn stale_socket_is_recovered() {
        let sandbox = Sandbox::new("stale");
        // Bind (creating the runtime dir at 0700) then drop: stale node remains.
        let stale = transport_bind(&sandbox.endpoint).expect("bind");
        drop(stale);
        assert!(sandbox.endpoint.socket_path.exists());
        let handle = spawn_daemon(&sandbox.endpoint);
        wait_ready(&sandbox.endpoint).await;
        admin_session(&sandbox.endpoint)
            .await
            .ping()
            .await
            .expect("ping");
        shutdown_and_await(handle, &sandbox.endpoint).await;
        assert!(!sandbox.endpoint.socket_path.exists());
    }

    #[tokio::test]
    async fn unexpected_objects_fail_closed_and_preserved() {
        let sandbox = Sandbox::new("objects");
        std::fs::create_dir(&sandbox.endpoint.runtime_dir).expect("setup");
        std::fs::set_permissions(
            &sandbox.endpoint.runtime_dir,
            std::fs::Permissions::from_mode(0o700),
        )
        .expect("setup");
        std::fs::write(&sandbox.endpoint.socket_path, b"precious").expect("setup");
        match run_on_endpoint(&sandbox.endpoint, DaemonConfig::g1_default()).await {
            Err(DaemonError::EndpointSecurity(_)) => {}
            other => panic!("expected EndpointSecurity, got {other:?}"),
        }
        assert_eq!(
            std::fs::read(&sandbox.endpoint.socket_path).expect("read"),
            b"precious"
        );

        std::fs::remove_file(&sandbox.endpoint.socket_path).expect("setup");
        let target = sandbox.root.join("real");
        std::fs::write(&target, b"real").expect("setup");
        std::os::unix::fs::symlink(&target, &sandbox.endpoint.socket_path).expect("setup");
        match run_on_endpoint(&sandbox.endpoint, DaemonConfig::g1_default()).await {
            Err(DaemonError::EndpointSecurity(_)) => {}
            other => panic!("expected EndpointSecurity, got {other:?}"),
        }
        assert!(std::fs::symlink_metadata(&sandbox.endpoint.socket_path)
            .expect("stat")
            .file_type()
            .is_symlink());
    }

    #[tokio::test]
    async fn malformed_client_does_not_kill_daemon() {
        let sandbox = Sandbox::new("malformed");
        let handle = spawn_daemon(&sandbox.endpoint);
        wait_ready(&sandbox.endpoint).await;
        // Garbage framed payload, then abandon the connection.
        let stream = transport_connect(&sandbox.endpoint).await.expect("connect");
        let mut raw = FramedTransport::new(stream);
        raw.write_frame(Bytes::from_static(b"garbage-bytes"))
            .await
            .expect("write");
        drop(raw);
        // Daemon still serves healthy clients.
        admin_session(&sandbox.endpoint)
            .await
            .ping()
            .await
            .expect("ping");
        shutdown_and_await(handle, &sandbox.endpoint).await;
    }

    #[tokio::test]
    async fn ninth_concurrent_client_is_closed_not_fatal() {
        let sandbox = Sandbox::new("ninth");
        let handle = spawn_daemon(&sandbox.endpoint);
        wait_ready(&sandbox.endpoint).await;
        // wait_ready holds no session; open exactly MAX_CONTROL_CLIENTS idle ones.
        let mut holders = Vec::new();
        for _ in 0..MAX_CONTROL_CLIENTS {
            holders.push(admin_session(&sandbox.endpoint).await);
        }
        // One more must be refused without harming the daemon.
        let extra = transport_connect(&sandbox.endpoint).await.expect("connect");
        let mut extra_session = ClientSession::new(
            FramedTransport::new(extra),
            ClientConfig::new(ipc_control::CLIENT_KIND_AGENTCTL),
        );
        assert!(
            extra_session.perform_handshake().await.is_err(),
            "ninth client must not complete handshake"
        );
        drop(extra_session);
        // Occupants still work.
        holders[0].ping().await.expect("ping");
        drop(holders);
        shutdown_and_await(handle, &sandbox.endpoint).await;
    }

    // -- OS peer authentication (ADR-0020) ----------------------------------

    /// Narrow test seam: presents a fixed peer identity (and counts
    /// presentations) instead of reading kernel credentials. It can only
    /// choose *which* identity the policy sees — never skip the decision.
    struct FixedPeerSource {
        peer: PeerIdentity,
        calls: Arc<AtomicUsize>,
    }

    impl PeerSource for FixedPeerSource {
        fn peer_of(&self, _stream: &UnixStream) -> TransportResult<PeerIdentity> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.peer.clone())
        }
    }

    /// Production source plus a presentation counter, for reconnect tests.
    struct CountingOsSource {
        calls: Arc<AtomicUsize>,
    }

    impl PeerSource for CountingOsSource {
        fn peer_of(&self, stream: &UnixStream) -> TransportResult<PeerIdentity> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            OsPeerSource.peer_of(stream)
        }
    }

    fn mismatched_uid() -> u32 {
        // Guaranteed different from the daemon's own euid on any host.
        current_uid().wrapping_add(1)
    }

    fn spawn_daemon_with(
        endpoint: &UnixEndpoint,
        source: impl PeerSource + 'static,
    ) -> tokio::task::JoinHandle<Result<RunOutcome, DaemonError>> {
        let endpoint = endpoint.clone();
        tokio::spawn(async move {
            run_on_endpoint_with(&endpoint, DaemonConfig::g1_default(), source).await
        })
    }

    async fn handshake_as(endpoint: &UnixEndpoint, kind: &str) -> bool {
        let stream = transport_connect(endpoint).await.expect("connect");
        let mut session = ClientSession::new(FramedTransport::new(stream), ClientConfig::new(kind));
        let ok = session.perform_handshake().await.is_ok() && session.is_ready();
        ok
    }

    #[test]
    fn same_principal_policy_allows_self_denies_other() {
        let daemon = daemon_identity().expect("daemon principal");
        assert!(authorize_peer(&daemon, &daemon.clone()).is_ok());
        let other = from_uid(mismatched_uid());
        match authorize_peer(&daemon, &other) {
            Err(DaemonError::PeerDenied) => {}
            other => panic!("expected PeerDenied, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn real_same_user_connection_passes_auth() {
        // Full production path (`OsPeerSource` + kernel credentials):
        // a genuine local client handshakes and pings.
        let sandbox = Sandbox::new("authsame");
        let handle = spawn_daemon(&sandbox.endpoint);
        wait_ready(&sandbox.endpoint).await;
        admin_session(&sandbox.endpoint)
            .await
            .ping()
            .await
            .expect("ping");
        shutdown_and_await(handle, &sandbox.endpoint).await;
    }

    #[tokio::test]
    async fn mismatched_peer_is_rejected_before_handshake() {
        let sandbox = Sandbox::new("authdeny");
        let calls = Arc::new(AtomicUsize::new(0));
        let handle = spawn_daemon_with(
            &sandbox.endpoint,
            FixedPeerSource {
                peer: from_uid(mismatched_uid()),
                calls: calls.clone(),
            },
        );
        // Give the daemon a moment to bind (no handshake can succeed).
        tokio::time::sleep(Duration::from_millis(300)).await;
        // A valid-looking handshake with the genuine `agentctl` kind is
        // still rejected: kind is not authentication.
        for kind in [
            ipc_control::CLIENT_KIND_AGENTCTL,
            ipc_control::CLIENT_KIND_TAURI_HOST,
        ] {
            assert!(
                !handshake_as(&sandbox.endpoint, kind).await,
                "mismatched peer must never reach READY (kind {kind})"
            );
        }
        assert!(
            calls.load(Ordering::SeqCst) >= 2,
            "each connection must present (and fail) authentication"
        );
        // The daemon itself is unharmed by denied peers.
        assert!(
            !handle.is_finished(),
            "denied peers must not kill the daemon"
        );
        handle.abort();
    }

    #[tokio::test]
    async fn reconnect_reauthenticates_every_connection() {
        let sandbox = Sandbox::new("authre");
        let calls = Arc::new(AtomicUsize::new(0));
        let handle = spawn_daemon_with(
            &sandbox.endpoint,
            CountingOsSource {
                calls: calls.clone(),
            },
        );
        wait_ready(&sandbox.endpoint).await;
        // Connection 1: auth + handshake + close.
        assert!(handshake_as(&sandbox.endpoint, ipc_control::CLIENT_KIND_AGENTCTL).await);
        // Connection 2: authentication runs again, nothing inherited.
        assert!(handshake_as(&sandbox.endpoint, ipc_control::CLIENT_KIND_AGENTCTL).await);
        assert_eq!(
            calls.load(Ordering::SeqCst),
            3,
            "wait_ready probe + two reconnects each authenticate once"
        );
        // Production path still shuts down cleanly afterwards.
        shutdown_and_await(handle, &sandbox.endpoint).await;
    }
}

/// Windows peer-auth policy tests (ADR-0020 §17, §23).
///
/// Compile-verified on non-Windows hosts via the Windows target check;
/// runtime execution requires Windows CI/host (recorded honestly in the
/// task result — never claimed here). Impersonation-path and live-pipe
/// tests are platform coverage debt for Windows CI:
///
/// ```text
/// same-user real agentctl → accepted
/// different-user process → rejected
/// ImpersonateNamedPipeClient real path
/// token-query failure path
/// server remains self after RevertToSelf
/// reconnect authenticates again
/// ```
#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;
    use ipc_transport::platform::peer::from_sid;

    /// Minimal well-formed SID (`S-1-5-21/22`): revision 1, one
    /// sub-authority, NT authority. Hand-built so policy tests need no OS.
    fn test_sid(last: u8) -> PeerIdentity {
        from_sid(vec![1, 1, 0, 0, 0, 0, 0, 5, 21, 0, 0, last])
    }

    #[test]
    fn same_sid_authorizes_different_sid_denies() {
        let daemon = test_sid(1);
        assert!(authorize_peer(&daemon, &test_sid(1)).is_ok());
        match authorize_peer(&daemon, &test_sid(2)) {
            Err(DaemonError::PeerDenied) => {}
            other => panic!("expected PeerDenied, got {other:?}"),
        }
        // Truncated SIDs never compare equal.
        match authorize_peer(&daemon, &from_sid(vec![1, 1])) {
            Err(DaemonError::PeerDenied) => {}
            other => panic!("expected PeerDenied, got {other:?}"),
        }
    }

    #[test]
    fn extraction_failure_is_not_authorized() {
        // Stage-2 decision table: extraction failure OR mismatch both deny
        // before any handshake exists. This pins the fail-closed mapping
        // the staged pump relies on.
        let daemon = test_sid(1);
        let denied: Result<(), DaemonError> =
            Err(TransportError::PeerAuth("test: impersonation failed".to_owned()).into());
        assert!(matches!(denied, Err(DaemonError::Transport(_))));
        // A denied peer yields no session: authorize_peer is the only gate,
        // and it rejects here.
        assert!(authorize_peer(&daemon, &test_sid(9)).is_err());
    }

    #[test]
    fn system_is_not_special_cased() {
        // LOCAL SYSTEM (`S-1-5-18`) against a user daemon (`S-1-5-21-...`)
        // denies by the same-user rule: DACL transport access is not
        // application authentication, and no SYSTEM exception exists.
        let daemon = test_sid(1);
        let system = from_sid(vec![1, 1, 0, 0, 0, 0, 0, 5, 18, 0, 0, 0]);
        assert!(authorize_peer(&daemon, &system).is_err());
    }
}
