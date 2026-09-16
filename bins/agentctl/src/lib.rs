//! `agentctl` G2 client: connect-first launcher behavior, daemon
//! spawning, bounded startup/reconnect retry, ping, graceful shutdown
//! against an existing daemon only, plus version-1 business requests
//! (`project ...`, `task ...`) through the trusted daemon dispatcher.
//!
//! Launcher/client integration only. No Task APIs of its own, no SQLite,
//! no storage access, no reconnect state reuse (every reconnect is a new
//! transport plus a new handshake), and never a blind replay of a
//! business mutation after an ambiguous failure.

use std::fmt;
use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use bytes::Bytes;
use ipc_contracts::proto::{
    control_request, control_response, ControlRequest, ControlResponse, ExecutionCoreRequest,
    ExecutionCoreResponse, HumanDecisionCoreRequest, HumanDecisionCoreResponse,
    PlanningCoreRequest, PlanningCoreResponse, PolicyCoreRequest, PolicyCoreResponse,
    ShutdownRequest, TaskAuthoringCoreRequest, TaskAuthoringCoreResponse, TaskCoreRequest,
    TaskCoreResponse,
};
use ipc_control::{ClientConfig, ClientSession, ControlError, CLIENT_KIND_AGENTCTL};
use ipc_transport::{FramedTransport, TransportError};
use prost::Message;
use tokio::io::{AsyncRead, AsyncWrite};

#[cfg(unix)]
use ipc_transport::platform::unix::{
    connect as transport_connect, inspect_endpoint, resolve_endpoint, EndpointState, UnixEndpoint,
};
#[cfg(windows)]
use ipc_transport::platform::windows::{connect_client, pipe_name};
#[cfg(windows)]
use tokio::net::windows::named_pipe::NamedPipeClient;
#[cfg(unix)]
use tokio::net::UnixStream;

/// Total startup/reconnect budget: 10 seconds, monotonic clock.
pub const STARTUP_BUDGET: Duration = Duration::from_secs(10);

/// Deadline for the `ShutdownResponse` acknowledgement.
pub const SHUTDOWN_RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);

/// Deadline for daemon endpoint/process disappearance after acknowledgement.
pub const DISAPPEAR_TIMEOUT: Duration = Duration::from_secs(5);

/// Deadline for a business response after the request is written.
pub const BUSINESS_RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);

/// Deadline for an execution response after the request is written.
/// `run` commits its durable entry synchronously (service open plus
/// three atomic commits) before the provider half starts in the
/// background, so this stays a fast-exchange bound — never provider
/// time. Terminal results arrive via `status` polling.
pub const EXECUTION_RESPONSE_TIMEOUT: Duration = Duration::from_secs(15);

/// Deadline for one local approval control-plane exchange.
pub const POLICY_RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);

/// Deadline for one local Human Decision Queue exchange.
pub const HUMAN_DECISION_RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);
/// Planning requests may cross the provider boundary, but never replay.
pub const PLANNING_RESPONSE_TIMEOUT: Duration = Duration::from_secs(125);

/// Poll interval while waiting for endpoint disappearance.
const ABSENCE_POLL: Duration = Duration::from_millis(50);

/// Accepted retry delays: 50ms, 100ms, 200ms, 400ms, 800ms, then 1000ms
/// until the budget expires. No jitter in G1.
pub fn backoff_sequence() -> impl Iterator<Item = Duration> {
    std::iter::successors(Some(Duration::from_millis(50)), |previous| {
        Some((*previous * 2).min(Duration::from_millis(1000)))
    })
}

/// Concrete delay schedule that fits inside `budget` (pure, deterministic,
/// unit-tested). Production timing constants are never weakened for tests.
#[must_use]
pub fn planned_delays(budget: Duration) -> Vec<Duration> {
    let mut delays = Vec::new();
    let mut spent = Duration::ZERO;
    for delay in backoff_sequence() {
        if spent + delay > budget {
            break;
        }
        spent += delay;
        delays.push(delay);
    }
    delays
}

/// G1 client lifecycle errors. Local taxonomy only; never in Protobuf.
#[derive(Debug)]
pub enum ClientError {
    /// Byte transport/framing/endpoint failure (preserved, not flattened).
    Transport(TransportError),
    /// Control-protocol failure (preserved, not flattened).
    Control(ControlError),
    /// No listener reachable (absent or refused) where one was expected.
    DaemonUnavailable(String),
    /// Shutdown requested but no daemon is running. Never spawns.
    NotRunning,
    /// The daemon process could not be spawned.
    SpawnFailed(String),
    /// The startup/reconnect budget expired without a usable daemon.
    StartupTimeout,
    /// Endpoint security, ownership, or permission failure. Fail closed:
    /// never retried, never bypassed with another endpoint.
    Security(String),
    /// A shutdown deadline expired (acknowledgement or disappearance).
    /// Never silently claimed as success, never replayed.
    ShutdownTimeout {
        /// Which stage expired: `"response"` or `"disappearance"`.
        stage: &'static str,
    },
    /// The shutdown command was sent (or may have been sent) but its
    /// outcome is unconfirmed. The command is NOT replayed: callers must
    /// re-probe daemon state instead. Anticipates the broader
    /// no-blind-side-effect-replay invariant.
    ShutdownAmbiguous(String),
    /// A business mutation was sent (or may have been sent) but its
    /// outcome is unconfirmed: lost reply, undecodable reply, wrong
    /// variant, or timeout after the write. The mutation is NOT replayed:
    /// callers must re-probe with a read (get/list/events) instead.
    /// G2 provides no generic automatic mutation retry.
    BusinessAmbiguous(String),
    /// An execution exchange was sent (or may have been sent) but its
    /// outcome is unconfirmed. `run` is NOT replayed: the caller re-probes
    /// with `status` on the client-minted execution id instead (the id is
    /// known even when the reply is lost). `status` is a pure read and
    /// may be retried freely; `cancel` is idempotent and may be re-issued.
    ExecutionAmbiguous(String),
    /// A G6 approval request may have reached the daemon but its reply is
    /// unconfirmed. Human decisions are never replayed automatically.
    PolicyAmbiguous(String),
    /// A G7 human-decision request may have reached the daemon but its
    /// reply is unconfirmed. A decision mutation is never replayed
    /// automatically; callers must re-probe the request by id.
    HumanDecisionAmbiguous(String),
    /// OS/runtime I/O failure.
    Io(io::Error),
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport(error) => write!(f, "client transport failure: {error}"),
            Self::Control(error) => write!(f, "client protocol failure: {error}"),
            Self::DaemonUnavailable(endpoint) => write!(f, "daemon unavailable at {endpoint}"),
            Self::NotRunning => write!(f, "daemon not running"),
            Self::SpawnFailed(detail) => write!(f, "failed to spawn daemon: {detail}"),
            Self::StartupTimeout => write!(f, "daemon startup timed out"),
            Self::Security(detail) => write!(f, "client security failure: {detail}"),
            Self::ShutdownTimeout { stage } => {
                write!(f, "shutdown timed out waiting for {stage}")
            }
            Self::ShutdownAmbiguous(detail) => {
                write!(f, "ambiguous shutdown (not replayed): {detail}")
            }
            Self::BusinessAmbiguous(detail) => {
                write!(f, "ambiguous business result (not replayed): {detail}")
            }
            Self::ExecutionAmbiguous(detail) => {
                write!(f, "ambiguous execution result (run not replayed): {detail}")
            }
            Self::PolicyAmbiguous(detail) => {
                write!(f, "ambiguous approval result (not replayed): {detail}")
            }
            Self::HumanDecisionAmbiguous(detail) => {
                write!(
                    f,
                    "ambiguous human-decision result (not replayed): {detail}"
                )
            }
            Self::Io(error) => write!(f, "client I/O failure: {error}"),
        }
    }
}

impl std::error::Error for ClientError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Transport(error) => Some(error),
            Self::Control(error) => Some(error),
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<TransportError> for ClientError {
    fn from(error: TransportError) -> Self {
        Self::Transport(error)
    }
}

impl From<ControlError> for ClientError {
    fn from(error: ControlError) -> Self {
        Self::Control(error)
    }
}

impl From<io::Error> for ClientError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Maps transport failures to security versus plain-transport errors.
/// Security failures fail closed and are never retried.
fn classify_transport(error: TransportError) -> ClientError {
    match error {
        TransportError::EndpointSecurity(detail) => ClientError::Security(detail),
        TransportError::Io(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            ClientError::Security(format!("permission denied: {error}"))
        }
        other => ClientError::Transport(other),
    }
}

/// Maps protocol failures, preserving transport security signals.
fn classify_control(error: ControlError) -> ClientError {
    match error {
        ControlError::Transport(inner) => classify_transport(inner),
        other => ClientError::Control(other),
    }
}

/// Reports whether a failure can plausibly resolve during daemon
/// startup/reconnect. Security, ownership, permission, incompatibility,
/// and malformed-configuration failures are never retried.
fn is_retryable(error: &ClientError) -> bool {
    match error {
        ClientError::DaemonUnavailable(_) => true,
        ClientError::Transport(inner) => is_retryable_transport(inner),
        ClientError::Control(ControlError::Transport(inner)) => is_retryable_transport(inner),
        ClientError::Control(ControlError::Timeout { .. }) => true,
        _ => false,
    }
}

/// Retryable transport subset: endpoint-not-ready races and transient I/O.
/// Frame-limit, security, ownership, and malformed signals stay fatal.
fn is_retryable_transport(error: &TransportError) -> bool {
    match error {
        TransportError::EndpointUnavailable { .. } => true,
        TransportError::Timeout { .. } => true,
        TransportError::Io(error) => matches!(
            error.kind(),
            io::ErrorKind::NotFound
                | io::ErrorKind::ConnectionRefused
                | io::ErrorKind::ConnectionReset
                | io::ErrorKind::BrokenPipe
                | io::ErrorKind::TimedOut
                | io::ErrorKind::Interrupted
        ),
        _ => false,
    }
}

/// Performs handshake over a connected stream with a fresh session.
/// Reconnects always build a new session: no socket-local state is reused.
async fn handshake_stream<S>(stream: S) -> Result<ClientSession<S>, ClientError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut session = ClientSession::new(
        FramedTransport::new(stream),
        ClientConfig::new(CLIENT_KIND_AGENTCTL),
    );
    session
        .perform_handshake()
        .await
        .map_err(classify_control)?;
    Ok(session)
}

/// Connects to an existing daemon and handshakes. No spawning, no retries.
#[cfg(unix)]
pub async fn connect_and_handshake(
    endpoint: &UnixEndpoint,
) -> Result<ClientSession<UnixStream>, ClientError> {
    let stream = transport_connect(endpoint)
        .await
        .map_err(|error| match error {
            TransportError::EndpointUnavailable { endpoint } => {
                ClientError::DaemonUnavailable(endpoint)
            }
            other => classify_transport(other),
        })?;
    handshake_stream(stream).await
}

/// Connects to an existing daemon and handshakes. No spawning, no retries.
#[cfg(windows)]
pub async fn connect_and_handshake(
    pipe_name: &str,
) -> Result<ClientSession<NamedPipeClient>, ClientError> {
    let stream = connect_client(pipe_name).map_err(|error| match error {
        TransportError::EndpointUnavailable { endpoint } => {
            ClientError::DaemonUnavailable(endpoint)
        }
        other => classify_transport(other),
    })?;
    handshake_stream(stream).await
}

/// Performs a version-1 handshake over a connected stream with a fresh
/// session. Reconnects always build a new session: no socket-local state
/// is reused, and v1/auth state is never inherited from a dead session.
async fn handshake_stream_v1<S>(stream: S) -> Result<ClientSession<S>, ClientError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut session = ClientSession::new(
        FramedTransport::new(stream),
        ClientConfig::business_default(CLIENT_KIND_AGENTCTL),
    );
    session
        .perform_handshake()
        .await
        .map_err(classify_control)?;
    if !session.business_allowed() {
        return Err(ClientError::Control(ControlError::UnsupportedProtocol {
            negotiated: session.negotiated_version().unwrap_or(0),
            required: 1,
        }));
    }
    Ok(session)
}

/// Connects to an existing daemon and handshakes at protocol v1. No
/// spawning, no retries.
#[cfg(unix)]
pub async fn connect_and_handshake_v1(
    endpoint: &UnixEndpoint,
) -> Result<ClientSession<UnixStream>, ClientError> {
    let stream = transport_connect(endpoint)
        .await
        .map_err(|error| match error {
            TransportError::EndpointUnavailable { endpoint } => {
                ClientError::DaemonUnavailable(endpoint)
            }
            other => classify_transport(other),
        })?;
    handshake_stream_v1(stream).await
}

/// Connects to an existing daemon and handshakes at protocol v1. No
/// spawning, no retries.
#[cfg(windows)]
pub async fn connect_and_handshake_v1(
    pipe_name: &str,
) -> Result<ClientSession<NamedPipeClient>, ClientError> {
    let stream = connect_client(pipe_name).map_err(|error| match error {
        TransportError::EndpointUnavailable { endpoint } => {
            ClientError::DaemonUnavailable(endpoint)
        }
        other => classify_transport(other),
    })?;
    handshake_stream_v1(stream).await
}

fn business_request_frame(request: TaskCoreRequest) -> Bytes {
    Bytes::from(
        ControlRequest {
            payload: Some(control_request::Payload::TaskCore(request)),
        }
        .encode_to_vec(),
    )
}

/// Sends one business request and validates the business reply. Shared
/// across platforms over any connected v1 session.
///
/// Connect/handshake failures happen before any business byte is written
/// and stay retryable under the existing policy. After the request is
/// written, any failure (lost response, undecodable reply, wrong
/// variant, timeout) is [`ClientError::BusinessAmbiguous`] and the
/// mutation is never replayed — callers re-probe with a read instead.
/// A well-formed reply — including a typed business-error payload — is a
/// successful exchange, not an ambiguity.
async fn business_session<S>(
    session: ClientSession<S>,
    request: TaskCoreRequest,
    absence: &str,
) -> Result<TaskCoreResponse, ClientError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut transport = session.into_inner();
    transport
        .write_frame(business_request_frame(request))
        .await
        .map_err(|error| {
            ClientError::BusinessAmbiguous(format!(
                "TaskCoreRequest send failed ({error}); endpoint {absence}; not replaying"
            ))
        })?;
    let frame = tokio::time::timeout(BUSINESS_RESPONSE_TIMEOUT, transport.read_frame())
        .await
        .map_err(|_| {
            ClientError::BusinessAmbiguous(format!(
                "business response timeout; endpoint {absence}; not replaying"
            ))
        })?;
    let frame = match frame {
        Ok(frame) => frame,
        Err(error) => {
            return Err(ClientError::BusinessAmbiguous(format!(
                "business response lost ({error}); endpoint {absence}; not replaying"
            )));
        }
    };
    let response = ControlResponse::decode(&frame[..]).map_err(|error| {
        ClientError::BusinessAmbiguous(format!(
            "business response undecodable ({error}); endpoint {absence}; not replaying"
        ))
    })?;
    match response.payload {
        Some(control_response::Payload::TaskCore(business)) => Ok(business),
        other => Err(ClientError::BusinessAmbiguous(format!(
            "unexpected business reply ({other:?}); endpoint {absence}; not replaying"
        ))),
    }
}

/// Runs one business request against the daemon, starting it first when
/// genuinely missing (connect-first plus bounded retry, exactly like the
/// G1 lifecycle). The request executes at most once per call.
#[cfg(unix)]
pub async fn business_request(
    endpoint: &UnixEndpoint,
    spawn: impl FnOnce() -> io::Result<Child>,
    request: TaskCoreRequest,
) -> Result<TaskCoreResponse, ClientError> {
    let session = connect_or_start_with(spawn, || connect_and_handshake_v1(endpoint)).await?;
    business_session(session, request, absence_note(endpoint)).await
}

/// Runs one business request against the daemon, starting it first when
/// genuinely missing.
#[cfg(windows)]
pub async fn business_request(
    pipe_name: &str,
    spawn: impl FnOnce() -> io::Result<Child>,
    request: TaskCoreRequest,
) -> Result<TaskCoreResponse, ClientError> {
    let session = connect_or_start_with(spawn, || connect_and_handshake_v1(pipe_name)).await?;
    business_session(session, request, absence_note(pipe_name)).await
}

/// Performs a version-2 handshake over a connected stream with a fresh
/// session. Execution (`execution_core`) authority requires the
/// negotiated version to be exactly 2; anything lower is a typed error
/// here (before any execution byte is written), never a silent fallback.
async fn handshake_stream_v2<S>(stream: S) -> Result<ClientSession<S>, ClientError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut session = ClientSession::new(
        FramedTransport::new(stream),
        ClientConfig::execution_default(CLIENT_KIND_AGENTCTL),
    );
    session
        .perform_handshake()
        .await
        .map_err(classify_control)?;
    if !session.execution_allowed() {
        return Err(ClientError::Control(ControlError::UnsupportedProtocol {
            negotiated: session.negotiated_version().unwrap_or(0),
            required: 2,
        }));
    }
    Ok(session)
}

/// Connects to an existing daemon and handshakes at protocol v2. No
/// spawning, no retries.
#[cfg(unix)]
pub async fn connect_and_handshake_v2(
    endpoint: &UnixEndpoint,
) -> Result<ClientSession<UnixStream>, ClientError> {
    let stream = transport_connect(endpoint)
        .await
        .map_err(|error| match error {
            TransportError::EndpointUnavailable { endpoint } => {
                ClientError::DaemonUnavailable(endpoint)
            }
            other => classify_transport(other),
        })?;
    handshake_stream_v2(stream).await
}

/// Connects to an existing daemon and handshakes at protocol v2. No
/// spawning, no retries.
#[cfg(windows)]
pub async fn connect_and_handshake_v2(
    pipe_name: &str,
) -> Result<ClientSession<NamedPipeClient>, ClientError> {
    let stream = connect_client(pipe_name).map_err(|error| match error {
        TransportError::EndpointUnavailable { endpoint } => {
            ClientError::DaemonUnavailable(endpoint)
        }
        other => classify_transport(other),
    })?;
    handshake_stream_v2(stream).await
}

fn execution_request_frame(request: ExecutionCoreRequest) -> Bytes {
    Bytes::from(
        ControlRequest {
            payload: Some(control_request::Payload::ExecutionCore(request)),
        }
        .encode_to_vec(),
    )
}

/// Sends one execution request and validates the execution reply. Shared
/// across platforms over any connected v2 session.
///
/// After the request is written, any failure (lost response, undecodable
/// reply, wrong variant, timeout) is [`ClientError::ExecutionAmbiguous`]:
/// `run` is never replayed — the caller re-probes with `status` on the
/// client-minted execution id. A well-formed reply — including a typed
/// execution-error payload — is a successful exchange, not an ambiguity.
async fn execution_session<S>(
    session: ClientSession<S>,
    request: ExecutionCoreRequest,
    absence: &str,
) -> Result<ExecutionCoreResponse, ClientError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut transport = session.into_inner();
    transport
        .write_frame(execution_request_frame(request))
        .await
        .map_err(|error| {
            ClientError::ExecutionAmbiguous(format!(
                "ExecutionCoreRequest send failed ({error}); endpoint {absence}; run not replayed — re-probe status"
            ))
        })?;
    let frame = tokio::time::timeout(EXECUTION_RESPONSE_TIMEOUT, transport.read_frame())
        .await
        .map_err(|_| {
            ClientError::ExecutionAmbiguous(format!(
                "execution response timeout; endpoint {absence}; run not replayed — re-probe status"
            ))
        })?;
    let frame = match frame {
        Ok(frame) => frame,
        Err(error) => {
            return Err(ClientError::ExecutionAmbiguous(format!(
                "execution response lost ({error}); endpoint {absence}; run not replayed — re-probe status"
            )));
        }
    };
    let response = ControlResponse::decode(&frame[..]).map_err(|error| {
        ClientError::ExecutionAmbiguous(format!(
            "execution response undecodable ({error}); endpoint {absence}; run not replayed — re-probe status"
        ))
    })?;
    match response.payload {
        Some(control_response::Payload::ExecutionCore(business)) => Ok(business),
        other => Err(ClientError::ExecutionAmbiguous(format!(
            "unexpected execution reply ({other:?}); endpoint {absence}; run not replayed — re-probe status"
        ))),
    }
}

/// Runs one execution request against the daemon, starting it first when
/// genuinely missing (connect-first plus bounded retry, exactly like the
/// G1 lifecycle). `run` executes at most once per call.
#[cfg(unix)]
pub async fn execution_request(
    endpoint: &UnixEndpoint,
    spawn: impl FnOnce() -> io::Result<Child>,
    request: ExecutionCoreRequest,
) -> Result<ExecutionCoreResponse, ClientError> {
    let session = connect_or_start_with(spawn, || connect_and_handshake_v2(endpoint)).await?;
    execution_session(session, request, absence_note(endpoint)).await
}

/// Runs one execution request against the daemon, starting it first when
/// genuinely missing.
#[cfg(windows)]
pub async fn execution_request(
    pipe_name: &str,
    spawn: impl FnOnce() -> io::Result<Child>,
    request: ExecutionCoreRequest,
) -> Result<ExecutionCoreResponse, ClientError> {
    let session = connect_or_start_with(spawn, || connect_and_handshake_v2(pipe_name)).await?;
    execution_session(session, request, absence_note(pipe_name)).await
}

/// Performs a protocol-v3 handshake for the bounded G6 approval surface.
async fn handshake_stream_v3<S>(stream: S) -> Result<ClientSession<S>, ClientError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut session = ClientSession::new(
        FramedTransport::new(stream),
        ClientConfig::policy_default(CLIENT_KIND_AGENTCTL),
    );
    session
        .perform_handshake()
        .await
        .map_err(classify_control)?;
    if !session.policy_allowed() {
        return Err(ClientError::Control(ControlError::UnsupportedProtocol {
            negotiated: session.negotiated_version().unwrap_or(0),
            required: ipc_contracts::IPC_PROTOCOL_VERSION_V3,
        }));
    }
    Ok(session)
}

#[cfg(unix)]
pub async fn connect_and_handshake_v3(
    endpoint: &UnixEndpoint,
) -> Result<ClientSession<UnixStream>, ClientError> {
    let stream = transport_connect(endpoint)
        .await
        .map_err(|error| match error {
            TransportError::EndpointUnavailable { endpoint } => {
                ClientError::DaemonUnavailable(endpoint)
            }
            other => classify_transport(other),
        })?;
    handshake_stream_v3(stream).await
}

#[cfg(windows)]
pub async fn connect_and_handshake_v3(
    pipe_name: &str,
) -> Result<ClientSession<NamedPipeClient>, ClientError> {
    let stream = connect_client(pipe_name).map_err(|error| match error {
        TransportError::EndpointUnavailable { endpoint } => {
            ClientError::DaemonUnavailable(endpoint)
        }
        other => classify_transport(other),
    })?;
    handshake_stream_v3(stream).await
}

fn policy_request_frame(request: PolicyCoreRequest) -> Bytes {
    Bytes::from(
        ControlRequest {
            payload: Some(control_request::Payload::PolicyCore(request)),
        }
        .encode_to_vec(),
    )
}

async fn policy_session<S>(
    session: ClientSession<S>,
    request: PolicyCoreRequest,
    absence: &str,
) -> Result<PolicyCoreResponse, ClientError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut transport = session.into_inner();
    transport
        .write_frame(policy_request_frame(request))
        .await
        .map_err(|error| {
            ClientError::PolicyAmbiguous(format!(
                "PolicyCoreRequest send failed ({error}); endpoint {absence}; not replaying"
            ))
        })?;
    let frame = tokio::time::timeout(POLICY_RESPONSE_TIMEOUT, transport.read_frame())
        .await
        .map_err(|_| {
            ClientError::PolicyAmbiguous(format!(
                "approval response timeout; endpoint {absence}; not replaying"
            ))
        })?;
    let frame = frame.map_err(|error| {
        ClientError::PolicyAmbiguous(format!(
            "approval response lost ({error}); endpoint {absence}; not replaying"
        ))
    })?;
    let response = ControlResponse::decode(&frame[..]).map_err(|error| {
        ClientError::PolicyAmbiguous(format!(
            "approval response undecodable ({error}); endpoint {absence}; not replaying"
        ))
    })?;
    match response.payload {
        Some(control_response::Payload::PolicyCore(policy)) => Ok(policy),
        other => Err(ClientError::PolicyAmbiguous(format!(
            "unexpected approval reply ({other:?}); endpoint {absence}; not replaying"
        ))),
    }
}

#[cfg(unix)]
pub async fn policy_request(
    endpoint: &UnixEndpoint,
    spawn: impl FnOnce() -> io::Result<Child>,
    request: PolicyCoreRequest,
) -> Result<PolicyCoreResponse, ClientError> {
    let session = connect_or_start_with(spawn, || connect_and_handshake_v3(endpoint)).await?;
    policy_session(session, request, absence_note(endpoint)).await
}

#[cfg(windows)]
pub async fn policy_request(
    pipe_name: &str,
    spawn: impl FnOnce() -> io::Result<Child>,
    request: PolicyCoreRequest,
) -> Result<PolicyCoreResponse, ClientError> {
    let session = connect_or_start_with(spawn, || connect_and_handshake_v3(pipe_name)).await?;
    policy_session(session, request, absence_note(pipe_name)).await
}

/// Performs a protocol-v4 handshake for the bounded G7 Human Decision
/// Queue surface.
async fn handshake_stream_v4<S>(stream: S) -> Result<ClientSession<S>, ClientError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut session = ClientSession::new(
        FramedTransport::new(stream),
        ClientConfig::human_decision_default(CLIENT_KIND_AGENTCTL),
    );
    session
        .perform_handshake()
        .await
        .map_err(classify_control)?;
    if !session.human_decision_allowed() {
        return Err(ClientError::Control(ControlError::UnsupportedProtocol {
            negotiated: session.negotiated_version().unwrap_or(0),
            required: ipc_contracts::IPC_PROTOCOL_VERSION_V4,
        }));
    }
    Ok(session)
}

#[cfg(unix)]
pub async fn connect_and_handshake_v4(
    endpoint: &UnixEndpoint,
) -> Result<ClientSession<UnixStream>, ClientError> {
    let stream = transport_connect(endpoint)
        .await
        .map_err(|error| match error {
            TransportError::EndpointUnavailable { endpoint } => {
                ClientError::DaemonUnavailable(endpoint)
            }
            other => classify_transport(other),
        })?;
    handshake_stream_v4(stream).await
}

#[cfg(windows)]
pub async fn connect_and_handshake_v4(
    pipe_name: &str,
) -> Result<ClientSession<NamedPipeClient>, ClientError> {
    let stream = connect_client(pipe_name).map_err(|error| match error {
        TransportError::EndpointUnavailable { endpoint } => {
            ClientError::DaemonUnavailable(endpoint)
        }
        other => classify_transport(other),
    })?;
    handshake_stream_v4(stream).await
}

fn human_decision_request_frame(request: HumanDecisionCoreRequest) -> Bytes {
    Bytes::from(
        ControlRequest {
            payload: Some(control_request::Payload::HumanDecisionCore(request)),
        }
        .encode_to_vec(),
    )
}

async fn human_decision_session<S>(
    session: ClientSession<S>,
    request: HumanDecisionCoreRequest,
    absence: &str,
) -> Result<HumanDecisionCoreResponse, ClientError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut transport = session.into_inner();
    transport
        .write_frame(human_decision_request_frame(request))
        .await
        .map_err(|error| {
            ClientError::HumanDecisionAmbiguous(format!(
                "HumanDecisionCoreRequest send failed ({error}); endpoint {absence}; not replaying"
            ))
        })?;
    let frame = tokio::time::timeout(HUMAN_DECISION_RESPONSE_TIMEOUT, transport.read_frame())
        .await
        .map_err(|_| {
            ClientError::HumanDecisionAmbiguous(format!(
                "human-decision response timeout; endpoint {absence}; not replaying"
            ))
        })?;
    let frame = frame.map_err(|error| {
        ClientError::HumanDecisionAmbiguous(format!(
            "human-decision response lost ({error}); endpoint {absence}; not replaying"
        ))
    })?;
    let response = ControlResponse::decode(&frame[..]).map_err(|error| {
        ClientError::HumanDecisionAmbiguous(format!(
            "human-decision response undecodable ({error}); endpoint {absence}; not replaying"
        ))
    })?;
    match response.payload {
        Some(control_response::Payload::HumanDecisionCore(response)) => Ok(response),
        other => Err(ClientError::HumanDecisionAmbiguous(format!(
            "unexpected human-decision reply ({other:?}); endpoint {absence}; not replaying"
        ))),
    }
}

#[cfg(unix)]
pub async fn human_decision_request(
    endpoint: &UnixEndpoint,
    spawn: impl FnOnce() -> io::Result<Child>,
    request: HumanDecisionCoreRequest,
) -> Result<HumanDecisionCoreResponse, ClientError> {
    let session = connect_or_start_with(spawn, || connect_and_handshake_v4(endpoint)).await?;
    human_decision_session(session, request, absence_note(endpoint)).await
}

/// Performs a protocol-v5 handshake for trusted planning operations.
async fn handshake_stream_v5<S>(stream: S) -> Result<ClientSession<S>, ClientError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut session = ClientSession::new(
        FramedTransport::new(stream),
        ClientConfig::human_decision_default(CLIENT_KIND_AGENTCTL),
    );
    session
        .perform_handshake()
        .await
        .map_err(classify_control)?;
    if !session.planning_allowed() {
        return Err(ClientError::Control(ControlError::UnsupportedProtocol {
            negotiated: session.negotiated_version().unwrap_or(0),
            required: ipc_contracts::IPC_PROTOCOL_VERSION_V5,
        }));
    }
    Ok(session)
}

#[cfg(unix)]
pub async fn planning_request(
    endpoint: &UnixEndpoint,
    spawn: impl FnOnce() -> io::Result<Child>,
    request: PlanningCoreRequest,
) -> Result<PlanningCoreResponse, ClientError> {
    let session = connect_or_start_with(spawn, || async {
        let stream = transport_connect(endpoint)
            .await
            .map_err(classify_transport)?;
        handshake_stream_v5(stream).await
    })
    .await?;
    planning_session(session, request, absence_note(endpoint)).await
}

fn planning_request_frame(request: PlanningCoreRequest) -> Bytes {
    Bytes::from(
        ControlRequest {
            payload: Some(control_request::Payload::PlanningCore(request)),
        }
        .encode_to_vec(),
    )
}

async fn planning_session<S>(
    session: ClientSession<S>,
    request: PlanningCoreRequest,
    absence: &str,
) -> Result<PlanningCoreResponse, ClientError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut transport = session.into_inner();
    transport
        .write_frame(planning_request_frame(request))
        .await
        .map_err(|error| {
            ClientError::HumanDecisionAmbiguous(format!(
                "PlanningCoreRequest send failed ({error}); endpoint {absence}; not replaying"
            ))
        })?;
    let frame = tokio::time::timeout(PLANNING_RESPONSE_TIMEOUT, transport.read_frame())
        .await
        .map_err(|_| {
            ClientError::HumanDecisionAmbiguous(format!(
                "planning response timeout; endpoint {absence}; not replaying"
            ))
        })?
        .map_err(|error| {
            ClientError::HumanDecisionAmbiguous(format!(
                "planning response lost ({error}); endpoint {absence}; not replaying"
            ))
        })?;
    let response = ControlResponse::decode(&frame[..]).map_err(|error| {
        ClientError::HumanDecisionAmbiguous(format!(
            "planning response undecodable ({error}); endpoint {absence}; not replaying"
        ))
    })?;
    match response.payload {
        Some(control_response::Payload::PlanningCore(response)) => Ok(response),
        other => Err(ClientError::HumanDecisionAmbiguous(format!(
            "unexpected planning reply ({other:?}); endpoint {absence}; not replaying"
        ))),
    }
}

/// Performs a protocol-v6 handshake for Task planning-metadata authoring.
#[cfg(unix)]
async fn handshake_stream_v6<S>(stream: S) -> Result<ClientSession<S>, ClientError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut session = ClientSession::new(
        FramedTransport::new(stream),
        ClientConfig::task_authoring_default(CLIENT_KIND_AGENTCTL),
    );
    session
        .perform_handshake()
        .await
        .map_err(classify_control)?;
    if !session.task_authoring_allowed() {
        return Err(ClientError::Control(ControlError::UnsupportedProtocol {
            negotiated: session.negotiated_version().unwrap_or(0),
            required: ipc_contracts::IPC_PROTOCOL_VERSION_V6,
        }));
    }
    Ok(session)
}

/// Sends one protocol-v6 Task authoring mutation. A lost or ambiguous reply
/// is [`ClientError::BusinessAmbiguous`]; creation is never replayed, and the
/// caller re-probes with `task get`.
#[cfg(unix)]
pub async fn task_authoring_request(
    endpoint: &UnixEndpoint,
    spawn: impl FnOnce() -> io::Result<Child>,
    request: TaskAuthoringCoreRequest,
) -> Result<TaskAuthoringCoreResponse, ClientError> {
    let session = connect_or_start_with(spawn, || async {
        let stream = transport_connect(endpoint)
            .await
            .map_err(classify_transport)?;
        handshake_stream_v6(stream).await
    })
    .await?;
    let absence = absence_note(endpoint);
    let mut transport = session.into_inner();
    let frame = Bytes::from(
        ControlRequest {
            payload: Some(control_request::Payload::TaskAuthoringCore(request)),
        }
        .encode_to_vec(),
    );
    transport.write_frame(frame).await.map_err(|error| {
        ClientError::BusinessAmbiguous(format!(
            "TaskAuthoringCoreRequest send failed ({error}); endpoint {absence}; not replaying"
        ))
    })?;
    let frame = tokio::time::timeout(BUSINESS_RESPONSE_TIMEOUT, transport.read_frame())
        .await
        .map_err(|_| {
            ClientError::BusinessAmbiguous(format!(
                "task authoring response timeout; endpoint {absence}; not replaying"
            ))
        })?
        .map_err(|error| {
            ClientError::BusinessAmbiguous(format!(
                "task authoring response lost ({error}); endpoint {absence}; not replaying"
            ))
        })?;
    let response = ControlResponse::decode(&frame[..]).map_err(|error| {
        ClientError::BusinessAmbiguous(format!(
            "task authoring response undecodable ({error}); endpoint {absence}; not replaying"
        ))
    })?;
    match response.payload {
        Some(control_response::Payload::TaskAuthoringCore(response)) => Ok(response),
        other => Err(ClientError::BusinessAmbiguous(format!(
            "unexpected task authoring reply ({other:?}); endpoint {absence}; not replaying"
        ))),
    }
}

#[cfg(windows)]
pub async fn human_decision_request(
    pipe_name: &str,
    spawn: impl FnOnce() -> io::Result<Child>,
    request: HumanDecisionCoreRequest,
) -> Result<HumanDecisionCoreResponse, ClientError> {
    let session = connect_or_start_with(spawn, || connect_and_handshake_v4(pipe_name)).await?;
    human_decision_session(session, request, absence_note(pipe_name)).await
}

/// Shared connect-first + spawn-once + bounded-retry lifecycle.
/// `spawn` runs at most once and only after a retryable connect failure;
/// non-retryable failures (security, incompatibility, malformed) return
/// immediately without spawning.
async fn connect_or_start_with<S, F, Fut>(
    spawn: impl FnOnce() -> io::Result<Child>,
    mut connect: F,
) -> Result<ClientSession<S>, ClientError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<ClientSession<S>, ClientError>>,
{
    match connect().await {
        Ok(session) => return Ok(session),
        Err(error) if !is_retryable(&error) => return Err(error),
        Err(_) => {}
    }
    spawn()
        .map_err(|error| ClientError::SpawnFailed(format!("failed to spawn agentd: {error}")))?;
    let deadline = Instant::now() + STARTUP_BUDGET;
    for delay in backoff_sequence() {
        if Instant::now() >= deadline {
            break;
        }
        let remaining = deadline - Instant::now();
        tokio::time::sleep(delay.min(remaining)).await;
        match connect().await {
            Ok(session) => return Ok(session),
            Err(error) if !is_retryable(&error) => return Err(error),
            Err(_) => continue,
        }
    }
    match connect().await {
        Ok(session) => Ok(session),
        Err(error) if !is_retryable(&error) => Err(error),
        Err(_) => Err(ClientError::StartupTimeout),
    }
}

/// Connects to the daemon, starting it first when genuinely missing.
/// Idempotent: an already-running daemon is reused, never duplicated.
#[cfg(unix)]
pub async fn connect_or_start(
    endpoint: &UnixEndpoint,
    spawn: impl FnOnce() -> io::Result<Child>,
) -> Result<ClientSession<UnixStream>, ClientError> {
    connect_or_start_with(spawn, || connect_and_handshake(endpoint)).await
}

/// Connects to the daemon, starting it first when genuinely missing.
#[cfg(windows)]
pub async fn connect_or_start(
    pipe_name: &str,
    spawn: impl FnOnce() -> io::Result<Child>,
) -> Result<ClientSession<NamedPipeClient>, ClientError> {
    connect_or_start_with(spawn, || connect_and_handshake(pipe_name)).await
}

/// Pings the daemon, starting it first when genuinely missing.
#[cfg(unix)]
pub async fn ping(
    endpoint: &UnixEndpoint,
    spawn: impl FnOnce() -> io::Result<Child>,
) -> Result<(), ClientError> {
    let mut session = connect_or_start(endpoint, spawn).await?;
    session.ping().await.map_err(classify_control)
}

/// Pings the daemon, starting it first when genuinely missing.
#[cfg(windows)]
pub async fn ping(
    pipe_name: &str,
    spawn: impl FnOnce() -> io::Result<Child>,
) -> Result<(), ClientError> {
    let mut session = connect_or_start(pipe_name, spawn).await?;
    session.ping().await.map_err(classify_control)
}

fn shutdown_request_frame() -> Bytes {
    Bytes::from(
        ControlRequest {
            payload: Some(control_request::Payload::Shutdown(ShutdownRequest {})),
        }
        .encode_to_vec(),
    )
}

/// Human-readable endpoint presence for ambiguity diagnostics.
#[cfg(unix)]
fn absence_note(endpoint: &UnixEndpoint) -> &'static str {
    if endpoint_absent(endpoint) {
        "endpoint absent"
    } else {
        "endpoint still present"
    }
}

/// Human-readable endpoint presence for ambiguity diagnostics.
#[cfg(windows)]
fn absence_note(pipe_name: &str) -> &'static str {
    if endpoint_absent(pipe_name) {
        "endpoint absent"
    } else {
        "endpoint still present"
    }
}

/// Reports whether the daemon endpoint is gone (Unix).
#[cfg(unix)]
fn endpoint_absent(endpoint: &UnixEndpoint) -> bool {
    inspect_endpoint(&endpoint.socket_path) == EndpointState::Absent
}

/// Reports whether the daemon endpoint is gone (Windows: unopenable as
/// missing means no listener owns the name).
#[cfg(windows)]
fn endpoint_absent(pipe_name: &str) -> bool {
    matches!(
        connect_client(pipe_name),
        Err(TransportError::EndpointUnavailable { .. })
    )
}

/// Waits for endpoint disappearance after an acknowledged shutdown.
#[cfg(unix)]
async fn wait_for_absence(endpoint: &UnixEndpoint) -> Result<(), ClientError> {
    let deadline = Instant::now() + DISAPPEAR_TIMEOUT;
    while Instant::now() < deadline {
        if endpoint_absent(endpoint) {
            return Ok(());
        }
        tokio::time::sleep(ABSENCE_POLL).await;
    }
    if endpoint_absent(endpoint) {
        Ok(())
    } else {
        Err(ClientError::ShutdownTimeout {
            stage: "disappearance",
        })
    }
}

/// Waits for endpoint disappearance after an acknowledged shutdown.
#[cfg(windows)]
async fn wait_for_absence(pipe_name: &str) -> Result<(), ClientError> {
    let deadline = Instant::now() + DISAPPEAR_TIMEOUT;
    while Instant::now() < deadline {
        if endpoint_absent(pipe_name) {
            return Ok(());
        }
        tokio::time::sleep(ABSENCE_POLL).await;
    }
    if endpoint_absent(pipe_name) {
        Ok(())
    } else {
        Err(ClientError::ShutdownTimeout {
            stage: "disappearance",
        })
    }
}

/// Requests graceful shutdown of an existing daemon.
///
/// Never spawns a missing daemon: absence maps to [`ClientError::NotRunning`].
/// After the request is written, any failure (lost response, undecodable
/// reply, wrong variant) is [`ClientError::ShutdownAmbiguous`] and the
/// command is never replayed — callers re-probe daemon state instead.
#[cfg(unix)]
pub async fn shutdown(endpoint: &UnixEndpoint) -> Result<(), ClientError> {
    let session = match connect_and_handshake(endpoint).await {
        Ok(session) => session,
        Err(ClientError::DaemonUnavailable(_)) => return Err(ClientError::NotRunning),
        Err(other) => return Err(other),
    };
    shutdown_session(session, absence_note(endpoint)).await?;
    wait_for_absence(endpoint).await
}

/// Requests graceful shutdown of an existing daemon. Never spawns.
#[cfg(windows)]
pub async fn shutdown(pipe_name: &str) -> Result<(), ClientError> {
    let session = match connect_and_handshake(pipe_name).await {
        Ok(session) => session,
        Err(ClientError::DaemonUnavailable(_)) => return Err(ClientError::NotRunning),
        Err(other) => return Err(other),
    };
    shutdown_session(session, absence_note(pipe_name)).await?;
    wait_for_absence(pipe_name).await
}

/// Sends `ShutdownRequest` and validates the acknowledgement. Shared across
/// platforms over any connected ready session.
async fn shutdown_session<S>(session: ClientSession<S>, absence: &str) -> Result<(), ClientError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut transport = session.into_inner();
    transport
        .write_frame(shutdown_request_frame())
        .await
        .map_err(|error| {
            ClientError::ShutdownAmbiguous(format!(
                "ShutdownRequest send failed ({error}); endpoint {absence}; not replaying"
            ))
        })?;
    let frame = tokio::time::timeout(SHUTDOWN_RESPONSE_TIMEOUT, transport.read_frame())
        .await
        .map_err(|_| ClientError::ShutdownTimeout { stage: "response" })?;
    let frame = match frame {
        Ok(frame) => frame,
        Err(error) => {
            return Err(ClientError::ShutdownAmbiguous(format!(
                "ShutdownResponse lost ({error}); endpoint {absence}; not replaying ShutdownRequest"
            )));
        }
    };
    let response = ControlResponse::decode(&frame[..]).map_err(|error| {
        ClientError::ShutdownAmbiguous(format!(
            "ShutdownResponse undecodable ({error}); endpoint {absence}; not replaying"
        ))
    })?;
    match response.payload {
        Some(control_response::Payload::Shutdown(_)) => Ok(()),
        other => Err(ClientError::ShutdownAmbiguous(format!(
            "unexpected shutdown reply ({other:?}); endpoint {absence}; not replaying"
        ))),
    }
}

/// Resolves the sibling `agentd` executable next to the running `agentctl`.
/// Pure and unit-tested; tests inject explicit paths instead.
pub fn daemon_executable_next_to(current_exe: &Path) -> io::Result<PathBuf> {
    let name = if cfg!(windows) {
        "agentd.exe"
    } else {
        "agentd"
    };
    let dir = current_exe
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "agentctl executable has no parent directory for sibling agentd",
            )
        })?;
    Ok(dir.join(name))
}

/// Spawns the daemon executable directly: no shell, no command strings,
/// no `sh -c`. Stdio is detached so the launcher can exit.
pub fn spawn_daemon(exe: &Path) -> io::Result<Child> {
    Command::new(exe)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

/// Spawns the installed sibling `agentd` next to this `agentctl`.
pub fn spawn_sibling_daemon() -> io::Result<Child> {
    let current = std::env::current_exe()?;
    spawn_daemon(&daemon_executable_next_to(&current)?)
}

/// Resolves the canonical production endpoint (Unix).
#[cfg(unix)]
pub fn resolve_production_endpoint() -> Result<UnixEndpoint, ClientError> {
    resolve_endpoint().map_err(classify_transport)
}

/// Resolves the canonical production endpoint (Windows).
#[cfg(windows)]
pub fn resolve_production_endpoint() -> Result<String, ClientError> {
    pipe_name().map_err(classify_transport)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use ipc_contracts::proto::{handshake_response, HandshakeAccepted, HandshakeResponse};
    use ipc_transport::platform::unix::{bind_server as transport_bind, SOCKET_FILE_NAME};
    use std::sync::{
        atomic::{AtomicU32, AtomicU64, Ordering},
        Arc,
    };

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    static TEST_COUNTER: AtomicU32 = AtomicU32::new(0);

    struct Sandbox {
        root: PathBuf,
        endpoint: UnixEndpoint,
    }

    impl Sandbox {
        fn new(suite: &str) -> Self {
            let id = COUNTER.fetch_add(1, Ordering::SeqCst);
            let root =
                PathBuf::from("/tmp").join(format!("acl-{}-{}-{}", std::process::id(), suite, id));
            std::fs::create_dir(&root).expect("sandbox root");
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
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn spawn_counter() -> Arc<AtomicU32> {
        Arc::new(AtomicU32::new(0))
    }

    /// Fake spawn: counts invocations and spawns a benign process.
    /// Starts no daemon; the retry loop must time out.
    fn inert_spawn(counter: &Arc<AtomicU32>) -> impl FnOnce() -> io::Result<Child> {
        let counter = counter.clone();
        move || {
            counter.fetch_add(1, Ordering::SeqCst);
            Command::new("true").spawn()
        }
    }

    /// Fake spawn that starts a real in-process daemon on `endpoint`
    /// (plus a benign process handle to satisfy the spawn contract).
    fn daemon_spawn(
        counter: &Arc<AtomicU32>,
        endpoint: &UnixEndpoint,
    ) -> impl FnOnce() -> io::Result<Child> {
        let counter = counter.clone();
        let endpoint = endpoint.clone();
        move || {
            counter.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async move {
                let _ =
                    agentd::run_on_endpoint(&endpoint, agentd::DaemonConfig::g1_default()).await;
            });
            Command::new("true").spawn()
        }
    }

    async fn wait_ready(endpoint: &UnixEndpoint) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if connect_and_handshake(endpoint).await.is_ok() {
                return;
            }
            if Instant::now() >= deadline {
                panic!("daemon never became ready");
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    fn accept_frame() -> Bytes {
        Bytes::from(
            ControlResponse {
                payload: Some(control_response::Payload::Handshake(HandshakeResponse {
                    outcome: Some(handshake_response::Outcome::Accepted(HandshakeAccepted {
                        selected_protocol_version: 0,
                        daemon_build_version: "test-daemon".to_owned(),
                        accepted_capabilities: Vec::new(),
                    })),
                })),
            }
            .encode_to_vec(),
        )
    }

    #[test]
    fn backoff_sequence_matches_accepted_profile() {
        let first: Vec<Duration> = backoff_sequence().take(8).collect();
        assert_eq!(
            first,
            vec![
                Duration::from_millis(50),
                Duration::from_millis(100),
                Duration::from_millis(200),
                Duration::from_millis(400),
                Duration::from_millis(800),
                Duration::from_millis(1000),
                Duration::from_millis(1000),
                Duration::from_millis(1000),
            ]
        );
    }

    #[test]
    fn planned_delays_fit_budget_without_weakening_constants() {
        assert_eq!(STARTUP_BUDGET, Duration::from_secs(10));
        let delays = planned_delays(STARTUP_BUDGET);
        assert_eq!(
            &delays[..6],
            &[
                Duration::from_millis(50),
                Duration::from_millis(100),
                Duration::from_millis(200),
                Duration::from_millis(400),
                Duration::from_millis(800),
                Duration::from_millis(1000),
            ]
        );
        let total: Duration = delays.iter().sum();
        assert!(total <= STARTUP_BUDGET);
        let last = *delays.last().expect("non-empty schedule");
        let next = (last * 2).min(Duration::from_millis(1000));
        assert!(
            total + next > STARTUP_BUDGET,
            "schedule must fill the budget"
        );
    }

    #[test]
    fn retry_classification_fails_closed() {
        assert!(is_retryable(&ClientError::DaemonUnavailable(
            "x".to_owned()
        )));
        assert!(is_retryable(&ClientError::Transport(
            TransportError::EndpointUnavailable {
                endpoint: "x".to_owned()
            }
        )));
        assert!(is_retryable(&ClientError::Control(ControlError::Timeout {
            op: "handshake",
            after: Duration::from_secs(5),
        })));
        assert!(!is_retryable(&ClientError::Security("x".to_owned())));
        assert!(!is_retryable(&ClientError::Transport(
            TransportError::EndpointSecurity("x".to_owned())
        )));
        assert!(!is_retryable(&ClientError::Control(
            ControlError::HandshakeRejected {
                reason: "x".to_owned(),
                daemon_versions: vec![0],
            }
        )));
        assert!(!is_retryable(&ClientError::Control(
            ControlError::UnknownClientKind("x".to_owned())
        )));
        assert!(!is_retryable(&ClientError::StartupTimeout));
        assert!(!is_retryable(&ClientError::NotRunning));
        assert!(!is_retryable(&ClientError::SpawnFailed("x".to_owned())));
        // A possibly-committed business mutation is never retried: the
        // caller re-probes with a read instead. G2 has no generic
        // automatic mutation retry.
        assert!(!is_retryable(&ClientError::BusinessAmbiguous(
            "x".to_owned()
        )));
        // Same rule for execution exchanges: `run` is never replayed
        // (re-probe `status` on the client-minted execution id instead).
        assert!(!is_retryable(&ClientError::ExecutionAmbiguous(
            "x".to_owned()
        )));
    }

    #[test]
    fn daemon_executable_resolution_is_explicit_and_testable() {
        assert_eq!(
            daemon_executable_next_to(Path::new("/foo/bar/agentctl")).unwrap(),
            PathBuf::from("/foo/bar/agentd")
        );
        assert!(daemon_executable_next_to(Path::new("agentctl")).is_err());
        let _ = TEST_COUNTER.fetch_add(0, Ordering::SeqCst);
    }

    #[tokio::test]
    async fn connect_first_reuses_running_daemon_without_spawning() {
        let sandbox = Sandbox::new("connectfirst");
        let endpoint = sandbox.endpoint.clone();
        tokio::spawn(async move {
            let _ = agentd::run_on_endpoint(&endpoint, agentd::DaemonConfig::g1_default()).await;
        });
        wait_ready(&sandbox.endpoint).await;
        let counter = spawn_counter();
        let mut session = connect_or_start(&sandbox.endpoint, inert_spawn(&counter))
            .await
            .expect("connect");
        session.ping().await.expect("ping");
        assert_eq!(
            counter.load(Ordering::SeqCst),
            0,
            "no spawn when daemon runs"
        );
        shutdown(&sandbox.endpoint).await.expect("shutdown");
    }

    #[tokio::test]
    async fn missing_daemon_is_spawned_once_then_converges() {
        let sandbox = Sandbox::new("spawnonce");
        let counter = spawn_counter();
        let mut session =
            connect_or_start(&sandbox.endpoint, daemon_spawn(&counter, &sandbox.endpoint))
                .await
                .expect("converge");
        session.ping().await.expect("ping");
        assert_eq!(counter.load(Ordering::SeqCst), 1, "exactly one spawn");
        shutdown(&sandbox.endpoint).await.expect("shutdown");
    }

    #[tokio::test]
    async fn concurrent_racers_converge_on_one_owner() {
        let sandbox = Sandbox::new("racers");
        let counter = spawn_counter();
        let (first, second) = tokio::join!(
            connect_or_start(&sandbox.endpoint, daemon_spawn(&counter, &sandbox.endpoint)),
            connect_or_start(&sandbox.endpoint, daemon_spawn(&counter, &sandbox.endpoint)),
        );
        let mut first = first.expect("racer one converges");
        let mut second = second.expect("racer two converges");
        first.ping().await.expect("ping one");
        second.ping().await.expect("ping two");
        drop((first, second));
        shutdown(&sandbox.endpoint).await.expect("shutdown");
        assert!(endpoint_absent(&sandbox.endpoint));
    }

    #[tokio::test]
    async fn shutdown_without_daemon_never_spawns() {
        let sandbox = Sandbox::new("notrunning");
        let counter = spawn_counter();
        // No daemon started; shutdown must report absence, not spawn.
        match shutdown(&sandbox.endpoint).await {
            Err(ClientError::NotRunning) => {}
            other => panic!("expected NotRunning, got {other:?}"),
        }
        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn shutdown_happy_path_ends_with_absent_endpoint() {
        let sandbox = Sandbox::new("shutdownok");
        let endpoint = sandbox.endpoint.clone();
        let daemon = tokio::spawn(async move {
            agentd::run_on_endpoint(&endpoint, agentd::DaemonConfig::g1_default()).await
        });
        wait_ready(&sandbox.endpoint).await;
        shutdown(&sandbox.endpoint).await.expect("shutdown");
        assert!(endpoint_absent(&sandbox.endpoint));
        let outcome = tokio::time::timeout(Duration::from_secs(10), daemon)
            .await
            .expect("daemon exits")
            .expect("join")
            .expect("clean shutdown");
        assert!(matches!(
            outcome,
            agentd::RunOutcome::ShutdownCompleted { .. }
        ));
    }

    #[tokio::test]
    async fn ambiguous_shutdown_is_never_replayed() {
        let sandbox = Sandbox::new("ambig");
        let listener = transport_bind(&sandbox.endpoint).expect("bind");
        let server_task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept");
            let mut transport = FramedTransport::new(stream);
            let frame = transport.read_frame().await.expect("read hello");
            let request = ControlRequest::decode(&frame[..]).expect("decode");
            assert!(matches!(
                request.payload,
                Some(control_request::Payload::Handshake(_))
            ));
            transport.write_frame(accept_frame()).await.expect("accept");
            // Read exactly one post-handshake frame, then go silent forever.
            let frame = transport.read_frame().await.expect("read shutdown");
            let request = ControlRequest::decode(&frame[..]).expect("decode");
            assert!(matches!(
                request.payload,
                Some(control_request::Payload::Shutdown(_))
            ));
            let frames_seen = 1u32;
            // Catch any (forbidden) replay within a bounded window.
            let replayed = tokio::time::timeout(Duration::from_millis(300), transport.read_frame())
                .await
                .is_ok();
            (frames_seen, replayed)
        });
        match shutdown(&sandbox.endpoint).await {
            Err(ClientError::ShutdownAmbiguous(_)) => {}
            other => panic!("expected ShutdownAmbiguous, got {other:?}"),
        }
        let (frames_seen, replayed) = server_task.await.expect("join");
        assert_eq!(frames_seen, 1, "exactly one ShutdownRequest may be sent");
        assert!(!replayed, "ShutdownRequest must not be replayed");
    }
}
