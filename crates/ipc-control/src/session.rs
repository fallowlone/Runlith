//! Bootstrap control-protocol sessions: handshake-first ordering, version
//! negotiation wiring, ping, and the shutdown-request boundary.
//!
//! Transport-generic over [`FramedTransport`]: tests run on in-memory
//! duplex streams, production runs on UDS/Named Pipe streams. Sequential
//! one-request-in-flight model only: no request IDs, no multiplexing, no
//! parallel in-flight requests.
//!
//! `ShutdownRequest` is parsed and exposed as [`ReadyRequest::Shutdown`]
//! with zero lifecycle semantics: no process termination, no daemon state
//! change, no authorization. TASK-0020 owns its meaning.

use std::future::Future;
use std::time::Duration;

use bytes::Bytes;
use ipc_contracts::proto::{
    control_request, control_response, handshake_response, ControlRequest, ControlResponse,
    HandshakeAccepted, HandshakeRequest, HandshakeResponse, PingRequest, PingResponse,
    ShutdownResponse,
};
use ipc_transport::FramedTransport;
use prost::Message;
use tokio::io::{AsyncRead, AsyncWrite};

use crate::error::{ControlError, ControlResult};
use crate::negotiation::{
    default_client_build_version, evaluate_handshake, rejection_for, validate_business_allowed,
    validate_execution_allowed, validate_human_decision_allowed, validate_policy_allowed,
    ServerConfig,
};

/// Deadline for a full client/server handshake exchange.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

/// Deadline for a ping exchange after a successful handshake. A valid
/// reply proves control-channel responsiveness only — never Task, DB,
/// provider, or process health.
pub const PING_TIMEOUT: Duration = Duration::from_secs(2);

/// Protocol-local connection state. Implementation detail of this layer:
/// NOT a domain Task lifecycle and never project business state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionState {
    /// Fresh connection: only `HandshakeRequest` is valid.
    AwaitingHandshake,
    /// Handshake accepted: ping/shutdown requests may flow.
    Ready,
    /// Fatal violation, rejection, timeout, or transport failure: the
    /// session cannot continue. Reconnects start over as new sessions.
    Terminated,
}

/// Post-handshake request observed by the server side. Ping replies are
/// already sent when [`ReadyRequest::Ping`] is returned; shutdown carries
/// no semantics here and no reply is sent; business requests are decoded
/// and surfaced without executing anything — dispatch belongs to `agentd`
/// (TASK-0028), which enforces the same version gate before handling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadyRequest {
    /// Liveness probe; `PingResponse` already sent.
    Ping,
    /// Shutdown asked; exposed upward with no lifecycle effect.
    /// TASK-0020 owns authorization, reply, and process behavior.
    Shutdown,
    /// Decoded G2 business request on a version-1+ session. The version
    /// gate already passed; nothing has executed.
    Business(ipc_contracts::proto::TaskCoreRequest),
    /// Decoded G5 execution request on a version-2 session. The version
    /// gate already passed; nothing has executed.
    Execution(ipc_contracts::proto::ExecutionCoreRequest),
    /// Decoded G6 approval control-plane request on a version-3 session.
    Policy(ipc_contracts::proto::PolicyCoreRequest),
    /// Decoded G7 Human Decision Queue request on a version-4 session.
    HumanDecision(ipc_contracts::proto::HumanDecisionCoreRequest),
}

/// Client-side handshake outcome.
#[derive(Debug, Clone)]
pub struct HandshakeAcceptedInfo {
    /// Version both sides will use (always advertised by the client).
    pub selected_version: u32,
    /// Daemon build version: diagnostic metadata, never compatibility.
    pub daemon_build_version: String,
    /// Negotiated capabilities (G1: always empty).
    pub accepted_capabilities: Vec<String>,
}

/// Server-side handshake outcome.
#[derive(Debug, Clone)]
pub struct ServerHandshakeInfo {
    /// Descriptive client identity. Never authentication/authorization.
    pub client_kind: String,
    /// Client build version: diagnostic metadata only.
    pub client_build_version: String,
    /// Version both sides will use.
    pub selected_version: u32,
    /// Negotiated capabilities (G1: always empty).
    pub accepted_capabilities: Vec<String>,
}

/// Client-side protocol configuration.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// Protocol versions offered. G1 default: exactly `[0]`.
    pub supported_versions: Vec<u32>,
    /// Descriptive identity (`tauri-host` or `agentctl`); validated
    /// server-side, never a credential.
    pub client_kind: String,
    /// Diagnostic build metadata; mismatch never rejects.
    pub client_build_version: String,
    /// Requested optional capabilities (G1: empty).
    pub requested_capabilities: Vec<String>,
}

impl ClientConfig {
    /// G1 bootstrap client configuration for an explicit client kind:
    /// offers protocol `[0]` only. Retained so old-client behavior stays
    /// simulatable; new business-capable callers use `business_default`.
    pub fn new(client_kind: impl Into<String>) -> Self {
        Self {
            supported_versions: vec![ipc_contracts::IPC_PROTOCOL_VERSION_V0],
            client_kind: client_kind.into(),
            client_build_version: default_client_build_version(),
            requested_capabilities: Vec::new(),
        }
    }

    /// G2 business-capable client configuration (ADR-0021): offers
    /// protocols `[0, 1]` so an old daemon negotiates 0 (G1 controls
    /// keep working) while a new daemon negotiates 1 (business IPC).
    /// TASK-0028 adopts this for `agentctl`; G1 callers keep `new`.
    pub fn business_default(client_kind: impl Into<String>) -> Self {
        Self {
            supported_versions: vec![
                ipc_contracts::IPC_PROTOCOL_VERSION_V0,
                ipc_contracts::IPC_PROTOCOL_VERSION_V1,
            ],
            client_kind: client_kind.into(),
            client_build_version: default_client_build_version(),
            requested_capabilities: Vec::new(),
        }
    }

    /// G5 execution-capable client configuration (TASK-0047): offers
    /// protocols `[0, 1, 2]` so an old daemon negotiates its best
    /// version while a new daemon negotiates 2 (execution IPC). The
    /// caller must check `execution_allowed` — a negotiated 0/1 session
    /// carries no execution authority.
    pub fn execution_default(client_kind: impl Into<String>) -> Self {
        Self {
            supported_versions: vec![
                ipc_contracts::IPC_PROTOCOL_VERSION_V0,
                ipc_contracts::IPC_PROTOCOL_VERSION_V1,
                ipc_contracts::IPC_PROTOCOL_VERSION_V2,
            ],
            client_kind: client_kind.into(),
            client_build_version: default_client_build_version(),
            requested_capabilities: Vec::new(),
        }
    }

    /// G6 approval-capable client profile. Offers every historical version
    /// and requires callers to check `policy_allowed` before use.
    pub fn policy_default(client_kind: impl Into<String>) -> Self {
        Self {
            supported_versions: vec![
                ipc_contracts::IPC_PROTOCOL_VERSION_V0,
                ipc_contracts::IPC_PROTOCOL_VERSION_V1,
                ipc_contracts::IPC_PROTOCOL_VERSION_V2,
                ipc_contracts::IPC_PROTOCOL_VERSION_V3,
            ],
            client_kind: client_kind.into(),
            client_build_version: default_client_build_version(),
            requested_capabilities: Vec::new(),
        }
    }

    /// G7 Human Decision Queue client profile. Offers all historical
    /// versions and requires `human_decision_allowed` before use.
    pub fn human_decision_default(client_kind: impl Into<String>) -> Self {
        Self {
            supported_versions: vec![
                ipc_contracts::IPC_PROTOCOL_VERSION_V0,
                ipc_contracts::IPC_PROTOCOL_VERSION_V1,
                ipc_contracts::IPC_PROTOCOL_VERSION_V2,
                ipc_contracts::IPC_PROTOCOL_VERSION_V3,
                ipc_contracts::IPC_PROTOCOL_VERSION_V4,
            ],
            client_kind: client_kind.into(),
            client_build_version: default_client_build_version(),
            requested_capabilities: Vec::new(),
        }
    }
}

/// Client side of the bootstrap control protocol.
pub struct ClientSession<T> {
    transport: FramedTransport<T>,
    config: ClientConfig,
    state: SessionState,
    negotiated_version: Option<u32>,
}

/// Server side of the bootstrap control protocol.
pub struct ServerSession<T> {
    transport: FramedTransport<T>,
    config: ServerConfig,
    state: SessionState,
    /// Version accepted by the handshake, once READY. Gates business
    /// (`task_core`) authority: only 1 qualifies. Stored so the version
    /// check lives with the session instead of being re-derived.
    negotiated_version: Option<u32>,
}

impl<T> ClientSession<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    /// Wraps a connected framed stream. Starts `AwaitingHandshake`.
    pub fn new(transport: FramedTransport<T>, config: ClientConfig) -> Self {
        Self {
            transport,
            config,
            state: SessionState::AwaitingHandshake,
            negotiated_version: None,
        }
    }

    /// Reports whether the session completed handshake negotiation.
    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.state == SessionState::Ready
    }

    /// Negotiated version after a successful handshake, else `None`.
    #[must_use]
    pub fn negotiated_version(&self) -> Option<u32> {
        self.negotiated_version
    }

    /// Reports whether this session negotiated business (G2 `task_core`)
    /// authority: versions 1 and 2 qualify. Descriptive gate for future
    /// client-side checks; the server enforces the same rule.
    #[must_use]
    pub fn business_allowed(&self) -> bool {
        self.negotiated_version
            .is_some_and(crate::negotiation::is_business_allowed)
    }

    /// Reports whether this session negotiated execution (G5
    /// `execution_core`) authority: only version 2 qualifies.
    #[must_use]
    pub fn execution_allowed(&self) -> bool {
        self.negotiated_version
            .is_some_and(crate::negotiation::is_execution_allowed)
    }

    /// Reports whether this session negotiated the G6 approval surface.
    #[must_use]
    pub fn policy_allowed(&self) -> bool {
        self.negotiated_version
            .is_some_and(crate::negotiation::is_policy_allowed)
    }

    #[must_use]
    pub fn human_decision_allowed(&self) -> bool {
        self.negotiated_version
            .is_some_and(crate::negotiation::is_human_decision_allowed)
    }

    /// Unwraps into the framed transport for lifecycle-owned exchanges
    /// (e.g. the `agentctl` shutdown sequence). Protocol use of this
    /// session ends here; the transport carries no handshake state.
    #[must_use]
    pub fn into_inner(self) -> FramedTransport<T> {
        self.transport
    }

    /// Sends `HandshakeRequest`, waits within [`HANDSHAKE_TIMEOUT`],
    /// validates acceptance, and transitions to READY only on a valid
    /// acceptance of a client-offered version. Rejection, timeout,
    /// malformed, transport, and unexpected-variant failures stay
    /// distinguishable and terminate protocol use of the connection.
    pub async fn perform_handshake(&mut self) -> ControlResult<HandshakeAcceptedInfo> {
        match self.state {
            SessionState::AwaitingHandshake => {}
            SessionState::Ready => return Err(ControlError::DuplicateHandshake),
            SessionState::Terminated => return Err(ControlError::SessionTerminated),
        }
        let request = HandshakeRequest {
            supported_protocol_versions: self.config.supported_versions.clone(),
            client_kind: self.config.client_kind.clone(),
            client_build_version: self.config.client_build_version.clone(),
            requested_capabilities: self.config.requested_capabilities.clone(),
        };
        let outcome = with_deadline("handshake", HANDSHAKE_TIMEOUT, async {
            self.transport
                .write_frame(handshake_request_frame(request))
                .await?;
            let frame = self.transport.read_frame().await?;
            decode_response(&frame)
        })
        .await;
        let response = match outcome {
            Ok(response) => response,
            Err(error) => return self.fail(error),
        };
        match response.payload {
            Some(control_response::Payload::Handshake(HandshakeResponse {
                outcome: Some(handshake_response::Outcome::Accepted(accepted)),
            })) => {
                if !self
                    .config
                    .supported_versions
                    .contains(&accepted.selected_protocol_version)
                {
                    return self.fail(ControlError::InvalidHandshake(format!(
                        "daemon selected version {} not advertised by this client",
                        accepted.selected_protocol_version
                    )));
                }
                self.state = SessionState::Ready;
                self.negotiated_version = Some(accepted.selected_protocol_version);
                Ok(HandshakeAcceptedInfo {
                    selected_version: accepted.selected_protocol_version,
                    daemon_build_version: accepted.daemon_build_version,
                    accepted_capabilities: accepted.accepted_capabilities,
                })
            }
            Some(control_response::Payload::Handshake(HandshakeResponse {
                outcome: Some(handshake_response::Outcome::Rejected(rejected)),
            })) => self.fail(ControlError::HandshakeRejected {
                reason: rejected.reason,
                daemon_versions: rejected.daemon_supported_protocol_versions,
            }),
            other => self.fail(ControlError::UnexpectedMessage(format!(
                "expected HandshakeResponse, got {other:?}"
            ))),
        }
    }

    /// Sends `PingRequest` and waits for `PingResponse` within
    /// [`PING_TIMEOUT`]. Valid only after a successful handshake.
    pub async fn ping(&mut self) -> ControlResult<()> {
        self.require_ready()?;
        let outcome = with_deadline("ping", PING_TIMEOUT, async {
            self.transport.write_frame(ping_request_frame()).await?;
            let frame = self.transport.read_frame().await?;
            decode_response(&frame)
        })
        .await;
        let response = match outcome {
            Ok(response) => response,
            Err(error) => return self.fail(error),
        };
        match response.payload {
            Some(control_response::Payload::Ping(_)) => Ok(()),
            other => self.fail(ControlError::UnexpectedMessage(format!(
                "expected PingResponse, got {other:?}"
            ))),
        }
    }

    fn require_ready(&self) -> ControlResult<()> {
        match self.state {
            SessionState::Ready => Ok(()),
            SessionState::AwaitingHandshake => Err(ControlError::HandshakeRequired),
            SessionState::Terminated => Err(ControlError::SessionTerminated),
        }
    }

    fn fail<U>(&mut self, error: ControlError) -> ControlResult<U> {
        self.state = SessionState::Terminated;
        Err(error)
    }
}

impl<T> ServerSession<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    /// Wraps a connected framed stream. Starts `AwaitingHandshake`.
    pub fn new(transport: FramedTransport<T>, config: ServerConfig) -> Self {
        Self {
            transport,
            config,
            state: SessionState::AwaitingHandshake,
            negotiated_version: None,
        }
    }

    /// Sends one business response for a previously observed
    /// [`ReadyRequest::Business`].
    ///
    /// Protocol framing only: encodes the `TaskCoreResponse` inside the
    /// `ControlResponse.task_core` envelope and writes the frame. Executes
    /// nothing — dispatch belongs to `agentd`. Valid only after a
    /// successful handshake; the caller guarantees the session negotiated
    /// version 1+ (only v1+ sessions can observe `Business`).
    pub async fn respond_business(
        &mut self,
        response: ipc_contracts::proto::TaskCoreResponse,
    ) -> ControlResult<()> {
        self.require_ready()?;
        let frame = bytes::Bytes::from(
            ControlResponse {
                payload: Some(control_response::Payload::TaskCore(response)),
            }
            .encode_to_vec(),
        );
        match self.transport.write_frame(frame).await {
            Ok(()) => Ok(()),
            Err(error) => self.fail(error.into()),
        }
    }

    /// Protocol framing only: encodes the `ExecutionCoreResponse` inside
    /// the `ControlResponse.execution_core` envelope and writes the frame.
    /// Executes nothing — dispatch belongs to `agentd`. Valid only after
    /// a successful handshake; the caller guarantees the session
    /// negotiated version 2 (only v2 sessions can observe `Execution`).
    pub async fn respond_execution(
        &mut self,
        response: ipc_contracts::proto::ExecutionCoreResponse,
    ) -> ControlResult<()> {
        self.require_ready()?;
        let frame = bytes::Bytes::from(
            ControlResponse {
                payload: Some(control_response::Payload::ExecutionCore(response)),
            }
            .encode_to_vec(),
        );
        match self.transport.write_frame(frame).await {
            Ok(()) => Ok(()),
            Err(error) => self.fail(error.into()),
        }
    }

    /// Protocol framing only: sends one G6 approval control-plane response.
    pub async fn respond_policy(
        &mut self,
        response: ipc_contracts::proto::PolicyCoreResponse,
    ) -> ControlResult<()> {
        self.require_ready()?;
        let frame = bytes::Bytes::from(
            ControlResponse {
                payload: Some(control_response::Payload::PolicyCore(response)),
            }
            .encode_to_vec(),
        );
        match self.transport.write_frame(frame).await {
            Ok(()) => Ok(()),
            Err(error) => self.fail(error.into()),
        }
    }

    /// Protocol framing only: sends one G7 Human Decision Queue response.
    pub async fn respond_human_decision(
        &mut self,
        response: ipc_contracts::proto::HumanDecisionCoreResponse,
    ) -> ControlResult<()> {
        self.require_ready()?;
        let frame = bytes::Bytes::from(
            ControlResponse {
                payload: Some(control_response::Payload::HumanDecisionCore(response)),
            }
            .encode_to_vec(),
        );
        match self.transport.write_frame(frame).await {
            Ok(()) => Ok(()),
            Err(error) => self.fail(error.into()),
        }
    }

    /// Sends the existing `ShutdownResponse` acknowledgement for a
    /// previously observed [`ReadyRequest::Shutdown`].
    ///
    /// Protocol response only: encodes/sends the acknowledgement and ends
    /// protocol use of this connection. It never stops any process, never
    /// changes daemon lifecycle state, and never authorizes anything —
    /// lifecycle remains entirely in `agentd` (TASK-0020).
    pub async fn respond_shutdown(&mut self) -> ControlResult<()> {
        self.require_ready()?;
        match self.transport.write_frame(shutdown_response_frame()).await {
            Ok(()) => {}
            Err(error) => return self.fail(error.into()),
        }
        self.state = SessionState::Terminated;
        Ok(())
    }

    /// Reports whether the session completed handshake negotiation.
    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.state == SessionState::Ready
    }

    /// Negotiated version after a successful handshake, else `None`.
    /// Gates business (`task_core`) authority: versions 1 and 2 qualify.
    #[must_use]
    pub fn negotiated_version(&self) -> Option<u32> {
        self.negotiated_version
    }

    /// Reports whether this session negotiated business (G2 `task_core`)
    /// authority: versions 1 and 2 qualify.
    #[must_use]
    pub fn business_allowed(&self) -> bool {
        self.negotiated_version
            .is_some_and(crate::negotiation::is_business_allowed)
    }

    /// Reports whether this session negotiated execution (G5
    /// `execution_core`) authority: only version 2 qualifies.
    #[must_use]
    pub fn execution_allowed(&self) -> bool {
        self.negotiated_version
            .is_some_and(crate::negotiation::is_execution_allowed)
    }

    /// Reports whether this server session negotiated G6 approval authority.
    #[must_use]
    pub fn policy_allowed(&self) -> bool {
        self.negotiated_version
            .is_some_and(crate::negotiation::is_policy_allowed)
    }

    #[must_use]
    pub fn human_decision_allowed(&self) -> bool {
        self.negotiated_version
            .is_some_and(crate::negotiation::is_human_decision_allowed)
    }

    /// Reads the first request (within [`HANDSHAKE_TIMEOUT`]), requires it
    /// to be a handshake, validates fields, selects the highest common
    /// version, validates the G1 client kind, intersects capabilities, and
    /// sends the accepted/rejected response. READY only after an accepted
    /// response is successfully sent. `client_kind` never authorizes
    /// anything: it only selects accept/reject of the handshake itself.
    pub async fn run_handshake(&mut self) -> ControlResult<ServerHandshakeInfo> {
        match self.state {
            SessionState::AwaitingHandshake => {}
            SessionState::Ready => return Err(ControlError::DuplicateHandshake),
            SessionState::Terminated => return Err(ControlError::SessionTerminated),
        }
        let outcome = with_deadline("handshake", HANDSHAKE_TIMEOUT, async {
            let frame = self.transport.read_frame().await?;
            decode_request(&frame)
        })
        .await;
        let request = match outcome {
            Ok(request) => request,
            Err(error) => return self.fail(error),
        };
        self.apply_handshake_request(request).await
    }

    /// Completes a handshake from one already-read frame, using the same
    /// validator, state machine, and negotiation semantics as
    /// [`ServerSession::run_handshake`].
    ///
    /// Exists for transports that must stage bytes before protocol
    /// authority (ADR-0020 Windows first frame: the frame is read as
    /// opaque bytes, peer authentication succeeds, and only then is the
    /// prefetched frame interpreted here). Callers must guarantee the
    /// frame was obtained under the framing rules (bounded, complete)
    /// and that any required pre-authentication already succeeded:
    /// this function decodes and may grant READY.
    pub async fn run_handshake_from_frame(
        &mut self,
        frame: bytes::BytesMut,
    ) -> ControlResult<ServerHandshakeInfo> {
        match self.state {
            SessionState::AwaitingHandshake => {}
            SessionState::Ready => return Err(ControlError::DuplicateHandshake),
            SessionState::Terminated => return Err(ControlError::SessionTerminated),
        }
        let request = match decode_request(&frame) {
            Ok(request) => request,
            Err(error) => return self.fail(error),
        };
        self.apply_handshake_request(request).await
    }

    /// Validates one decoded handshake request and sends the
    /// accepted/rejected response. Shared by both handshake entries so
    /// exactly one validator and one state machine exist.
    async fn apply_handshake_request(
        &mut self,
        request: ControlRequest,
    ) -> ControlResult<ServerHandshakeInfo> {
        let handshake = match request.payload {
            Some(control_request::Payload::Handshake(handshake)) => handshake,
            Some(_) => return self.fail(ControlError::HandshakeRequired),
            None => {
                return self.fail(ControlError::UnexpectedMessage(
                    "empty control request envelope".to_owned(),
                ));
            }
        };
        match evaluate_handshake(&handshake, &self.config) {
            Ok(params) => {
                let frame = accept_response_frame(&params, &self.config);
                if let Err(error) = self.transport.write_frame(frame).await {
                    return self.fail(error.into());
                }
                self.state = SessionState::Ready;
                self.negotiated_version = Some(params.selected_version);
                Ok(ServerHandshakeInfo {
                    client_kind: handshake.client_kind,
                    client_build_version: handshake.client_build_version,
                    selected_version: params.selected_version,
                    accepted_capabilities: params.accepted_capabilities,
                })
            }
            Err(fault) => {
                let rejected = rejection_for(&fault, &self.config.supported_versions);
                let frame = rejected_response_frame(rejected);
                if let Err(error) = self.transport.write_frame(frame).await {
                    return self.fail(error.into());
                }
                self.fail(fault)
            }
        }
    }

    /// Reads one post-handshake request. Ping is answered internally and
    /// reported as [`ReadyRequest::Ping`]; shutdown is reported as
    /// [`ReadyRequest::Shutdown`] with no reply and no lifecycle effect; a
    /// second handshake is a fatal protocol violation.
    pub async fn next_request(&mut self) -> ControlResult<ReadyRequest> {
        self.require_ready()?;
        let frame = match self.transport.read_frame().await {
            Ok(frame) => frame,
            Err(error) => return self.fail(error.into()),
        };
        let request = match decode_request(&frame) {
            Ok(request) => request,
            Err(error) => return self.fail(error),
        };
        match request.payload {
            Some(control_request::Payload::Ping(_)) => {
                if let Err(error) = self.transport.write_frame(ping_response_frame()).await {
                    return self.fail(error.into());
                }
                Ok(ReadyRequest::Ping)
            }
            Some(control_request::Payload::Shutdown(_)) => Ok(ReadyRequest::Shutdown),
            Some(control_request::Payload::TaskCore(request)) => {
                // Version gate, not dispatch: a v0 session rejects with
                // UnsupportedProtocol and the request never executes; a
                // v1+ session surfaces the decoded request for the
                // `agentd` dispatcher, which owns handling (TASK-0028).
                let negotiated = self
                    .negotiated_version
                    .unwrap_or(ipc_contracts::IPC_PROTOCOL_VERSION_V0);
                match validate_business_allowed(negotiated) {
                    Ok(()) => Ok(ReadyRequest::Business(request)),
                    Err(error) => self.fail(error),
                }
            }
            Some(control_request::Payload::ExecutionCore(request)) => {
                // Same gate discipline one version higher: only a v2
                // session surfaces the decoded request; v0/v1 reject
                // with UnsupportedProtocol and the request never
                // executes (TASK-0047).
                let negotiated = self
                    .negotiated_version
                    .unwrap_or(ipc_contracts::IPC_PROTOCOL_VERSION_V0);
                match validate_execution_allowed(negotiated) {
                    Ok(()) => Ok(ReadyRequest::Execution(request)),
                    Err(error) => self.fail(error),
                }
            }
            Some(control_request::Payload::PolicyCore(request)) => {
                let negotiated = self
                    .negotiated_version
                    .unwrap_or(ipc_contracts::IPC_PROTOCOL_VERSION_V0);
                match validate_policy_allowed(negotiated) {
                    Ok(()) => Ok(ReadyRequest::Policy(request)),
                    Err(error) => self.fail(error),
                }
            }
            Some(control_request::Payload::HumanDecisionCore(request)) => {
                let negotiated = self
                    .negotiated_version
                    .unwrap_or(ipc_contracts::IPC_PROTOCOL_VERSION_V0);
                match validate_human_decision_allowed(negotiated) {
                    Ok(()) => Ok(ReadyRequest::HumanDecision(request)),
                    Err(error) => self.fail(error),
                }
            }
            Some(control_request::Payload::Handshake(_)) => {
                self.fail(ControlError::DuplicateHandshake)
            }
            None => self.fail(ControlError::UnexpectedMessage(
                "empty control request envelope".to_owned(),
            )),
        }
    }

    fn require_ready(&self) -> ControlResult<()> {
        match self.state {
            SessionState::Ready => Ok(()),
            SessionState::AwaitingHandshake => Err(ControlError::HandshakeRequired),
            SessionState::Terminated => Err(ControlError::SessionTerminated),
        }
    }

    fn fail<U>(&mut self, error: ControlError) -> ControlResult<U> {
        self.state = SessionState::Terminated;
        Err(error)
    }
}

async fn with_deadline<O>(
    op: &'static str,
    after: Duration,
    future: impl Future<Output = ControlResult<O>>,
) -> ControlResult<O> {
    tokio::time::timeout(after, future)
        .await
        .map_err(|_| ControlError::Timeout { op, after })?
}

fn handshake_request_frame(request: HandshakeRequest) -> Bytes {
    Bytes::from(
        ControlRequest {
            payload: Some(control_request::Payload::Handshake(request)),
        }
        .encode_to_vec(),
    )
}

fn ping_request_frame() -> Bytes {
    Bytes::from(
        ControlRequest {
            payload: Some(control_request::Payload::Ping(PingRequest {})),
        }
        .encode_to_vec(),
    )
}

fn ping_response_frame() -> Bytes {
    Bytes::from(
        ControlResponse {
            payload: Some(control_response::Payload::Ping(PingResponse {})),
        }
        .encode_to_vec(),
    )
}

fn shutdown_response_frame() -> Bytes {
    Bytes::from(
        ControlResponse {
            payload: Some(control_response::Payload::Shutdown(ShutdownResponse {})),
        }
        .encode_to_vec(),
    )
}

fn accept_response_frame(
    params: &crate::negotiation::AcceptedParameters,
    config: &ServerConfig,
) -> Bytes {
    Bytes::from(
        ControlResponse {
            payload: Some(control_response::Payload::Handshake(HandshakeResponse {
                outcome: Some(handshake_response::Outcome::Accepted(HandshakeAccepted {
                    selected_protocol_version: params.selected_version,
                    daemon_build_version: config.daemon_build_version.clone(),
                    accepted_capabilities: params.accepted_capabilities.clone(),
                })),
            })),
        }
        .encode_to_vec(),
    )
}

fn rejected_response_frame(rejected: ipc_contracts::proto::HandshakeRejected) -> Bytes {
    Bytes::from(
        ControlResponse {
            payload: Some(control_response::Payload::Handshake(HandshakeResponse {
                outcome: Some(handshake_response::Outcome::Rejected(rejected)),
            })),
        }
        .encode_to_vec(),
    )
}

fn decode_request(frame: &[u8]) -> ControlResult<ControlRequest> {
    ControlRequest::decode(frame)
        .map_err(|error| ControlError::MalformedProtobuf(format!("ControlRequest: {error}")))
}

fn decode_response(frame: &[u8]) -> ControlResult<ControlResponse> {
    ControlResponse::decode(frame)
        .map_err(|error| ControlError::MalformedProtobuf(format!("ControlResponse: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::negotiation::{CLIENT_KIND_AGENTCTL, CLIENT_KIND_TAURI_HOST};
    use ipc_contracts::proto::ShutdownRequest;

    type Duplex = tokio::io::DuplexStream;

    struct Pair {
        client: ClientSession<Duplex>,
        server: ServerSession<Duplex>,
    }

    fn agentctl_config() -> ClientConfig {
        let mut config = ClientConfig::new(CLIENT_KIND_AGENTCTL);
        // A deliberately different build version: mismatch must not reject.
        config.client_build_version = "9.9.9-test".to_owned();
        config
    }

    async fn run_handshake_pair(
        client_config: ClientConfig,
        server_config: ServerConfig,
    ) -> (
        Pair,
        ControlResult<HandshakeAcceptedInfo>,
        ControlResult<ServerHandshakeInfo>,
    ) {
        let (left, right) = tokio::io::duplex(1024);
        let mut client = ClientSession::new(FramedTransport::new(left), client_config);
        let mut server = ServerSession::new(FramedTransport::new(right), server_config);
        let server_task = tokio::spawn(async move {
            let outcome = server.run_handshake().await;
            (server, outcome)
        });
        let client_outcome = client.perform_handshake().await;
        let (server, server_outcome) = server_task.await.expect("server task joins");
        (Pair { client, server }, client_outcome, server_outcome)
    }

    fn raw_pair() -> (FramedTransport<Duplex>, FramedTransport<Duplex>) {
        let (left, right) = tokio::io::duplex(1024);
        (FramedTransport::new(left), FramedTransport::new(right))
    }

    fn valid_handshake_request() -> HandshakeRequest {
        HandshakeRequest {
            supported_protocol_versions: vec![ipc_contracts::IPC_PROTOCOL_VERSION_V0],
            client_kind: CLIENT_KIND_AGENTCTL.to_owned(),
            client_build_version: "test".to_owned(),
            requested_capabilities: Vec::new(),
        }
    }

    #[tokio::test]
    async fn successful_handshake_marks_both_sides_ready() {
        let (pair, client_outcome, server_outcome) =
            run_handshake_pair(agentctl_config(), ServerConfig::g1_default()).await;
        let accepted = client_outcome.expect("client handshake");
        let server_info = server_outcome.expect("server handshake");
        assert_eq!(accepted.selected_version, 0);
        assert!(!accepted.daemon_build_version.is_empty());
        assert!(accepted.accepted_capabilities.is_empty());
        assert_eq!(server_info.selected_version, 0);
        assert_eq!(server_info.client_kind, CLIENT_KIND_AGENTCTL);
        assert_eq!(server_info.client_build_version, "9.9.9-test");
        assert!(pair.client.is_ready());
        assert!(pair.server.is_ready());
        assert_eq!(pair.client.negotiated_version(), Some(0));
    }

    #[tokio::test]
    async fn prefetched_frame_handshake_matches_live_path() {
        // Staged prefetch: the frame is read as opaque bytes first (as the
        // Windows server does before peer authentication), then interpreted
        // here — after auth — with the single shared validator.
        let (mut client_transport, server_transport) = raw_pair();
        client_transport
            .write_frame(handshake_request_frame(valid_handshake_request()))
            .await
            .expect("client write");
        let mut server_transport = server_transport;
        let prefetched = server_transport.read_frame().await.expect("prefetch");
        let mut server = ServerSession::new(server_transport, ServerConfig::g1_default());
        assert!(!server.is_ready());
        let info = server
            .run_handshake_from_frame(prefetched)
            .await
            .expect("prefetched handshake");
        assert!(server.is_ready());
        assert_eq!(info.selected_version, 0);
        assert_eq!(info.client_kind, CLIENT_KIND_AGENTCTL);
        // The accept response went back on the wire for the client to read.
        let reply = client_transport.read_frame().await.expect("reply");
        assert!(!reply.is_empty());
    }

    #[tokio::test]
    async fn prefetched_garbage_is_rejected_without_ready() {
        use bytes::BytesMut;

        let (_, server_transport) = raw_pair();
        let mut server = ServerSession::new(server_transport, ServerConfig::g1_default());
        let garbage = BytesMut::from(&b"not-a-handshake"[..]);
        assert!(server.run_handshake_from_frame(garbage).await.is_err());
        assert!(!server.is_ready());
    }

    #[tokio::test]
    async fn prefetched_second_handshake_is_rejected() {
        let (mut client_transport, server_transport) = raw_pair();
        client_transport
            .write_frame(handshake_request_frame(valid_handshake_request()))
            .await
            .expect("client write");
        let mut server_transport = server_transport;
        let first = server_transport.read_frame().await.expect("prefetch");
        let mut server = ServerSession::new(server_transport, ServerConfig::g1_default());
        server
            .run_handshake_from_frame(first)
            .await
            .expect("first handshake");
        // Client must still drain the accept response before sending more.
        let _ = client_transport.read_frame().await.expect("reply");
        client_transport
            .write_frame(handshake_request_frame(valid_handshake_request()))
            .await
            .expect("client write");
        let second = server.transport.read_frame().await.expect("prefetch");
        match server.run_handshake_from_frame(second).await {
            Err(ControlError::DuplicateHandshake) => {}
            other => panic!("expected DuplicateHandshake, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn tauri_host_handshake_is_accepted() {
        let (pair, client_outcome, _) = run_handshake_pair(
            ClientConfig::new(CLIENT_KIND_TAURI_HOST),
            ServerConfig::g1_default(),
        )
        .await;
        assert_eq!(client_outcome.expect("handshake").selected_version, 0);
        assert!(pair.client.is_ready());
    }

    #[tokio::test]
    async fn wrong_protocol_version_is_rejected_with_daemon_versions() {
        let mut config = agentctl_config();
        config.supported_versions = vec![99];
        let (pair, client_outcome, server_outcome) =
            run_handshake_pair(config, ServerConfig::g1_default()).await;
        match client_outcome {
            Err(ControlError::HandshakeRejected {
                reason,
                daemon_versions,
            }) => {
                assert!(!reason.is_empty());
                assert_eq!(daemon_versions, vec![0]);
            }
            other => panic!("expected HandshakeRejected, got {other:?}"),
        }
        assert!(!pair.client.is_ready());
        assert!(!pair.server.is_ready());
        assert!(
            matches!(
                server_outcome,
                Err(ControlError::IncompatibleProtocol { .. })
            ),
            "server fault stays typed, got {server_outcome:?}"
        );
    }

    #[tokio::test]
    async fn unknown_client_kind_is_rejected() {
        let (pair, client_outcome, server_outcome) = run_handshake_pair(
            ClientConfig::new("evil-test-client"),
            ServerConfig::g1_default(),
        )
        .await;
        assert!(
            matches!(client_outcome, Err(ControlError::HandshakeRejected { .. })),
            "got {client_outcome:?}"
        );
        assert!(!pair.client.is_ready());
        assert!(
            matches!(server_outcome, Err(ControlError::UnknownClientKind(kind)) if kind == "evil-test-client")
        );
    }

    #[tokio::test]
    async fn unknown_capabilities_are_accepted_but_empty() {
        let mut config = agentctl_config();
        config.requested_capabilities = vec!["unknown-capability".to_owned()];
        let (pair, client_outcome, _) =
            run_handshake_pair(config, ServerConfig::g1_default()).await;
        let accepted = client_outcome.expect("unknown caps must not reject");
        assert!(accepted.accepted_capabilities.is_empty());
        assert!(pair.client.is_ready());
    }

    #[tokio::test]
    async fn ping_before_handshake_is_violation() {
        let (mut raw_client, raw_server) = raw_pair();
        let mut server = ServerSession::new(raw_server, ServerConfig::g1_default());
        raw_client
            .write_frame(ping_request_frame())
            .await
            .expect("write");
        match server.run_handshake().await {
            Err(ControlError::HandshakeRequired) => {}
            other => panic!("expected HandshakeRequired, got {other:?}"),
        }
        assert!(!server.is_ready());
        match server.run_handshake().await {
            Err(ControlError::SessionTerminated) => {}
            other => panic!("expected SessionTerminated, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn shutdown_before_handshake_is_violation() {
        let (mut raw_client, raw_server) = raw_pair();
        let mut server = ServerSession::new(raw_server, ServerConfig::g1_default());
        let shutdown = Bytes::from(
            ControlRequest {
                payload: Some(control_request::Payload::Shutdown(ShutdownRequest {})),
            }
            .encode_to_vec(),
        );
        raw_client.write_frame(shutdown).await.expect("write");
        match server.run_handshake().await {
            Err(ControlError::HandshakeRequired) => {}
            other => panic!("expected HandshakeRequired, got {other:?}"),
        }
        assert!(!server.is_ready());
    }

    #[tokio::test]
    async fn client_ping_before_handshake_is_rejected_locally() {
        let (left, _right) = tokio::io::duplex(1024);
        let mut client = ClientSession::new(FramedTransport::new(left), agentctl_config());
        match client.ping().await {
            Err(ControlError::HandshakeRequired) => {}
            other => panic!("expected HandshakeRequired, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn duplicate_handshake_is_rejected_on_both_sides() {
        let (mut pair, client_outcome, _) =
            run_handshake_pair(agentctl_config(), ServerConfig::g1_default()).await;
        client_outcome.expect("first handshake");
        match pair.client.perform_handshake().await {
            Err(ControlError::DuplicateHandshake) => {}
            other => panic!("expected DuplicateHandshake, got {other:?}"),
        }

        let (mut raw_client, raw_server) = raw_pair();
        let mut server = ServerSession::new(raw_server, ServerConfig::g1_default());
        let hello = valid_handshake_request();
        raw_client
            .write_frame(handshake_request_frame(hello.clone()))
            .await
            .expect("write");
        server.run_handshake().await.expect("ready");
        raw_client
            .write_frame(handshake_request_frame(hello))
            .await
            .expect("write");
        match server.next_request().await {
            Err(ControlError::DuplicateHandshake) => {}
            other => panic!("expected DuplicateHandshake, got {other:?}"),
        }
        assert!(!server.is_ready());
    }

    #[tokio::test]
    async fn ping_round_trip_after_handshake() {
        let (mut pair, client_outcome, _) =
            run_handshake_pair(agentctl_config(), ServerConfig::g1_default()).await;
        client_outcome.expect("handshake");
        let server_task = tokio::spawn(async move {
            let request = pair.server.next_request().await;
            (pair.server, request)
        });
        pair.client.ping().await.expect("ping");
        let (_server, request) = server_task.await.expect("join");
        assert_eq!(request.expect("server observes ping"), ReadyRequest::Ping);
    }

    #[tokio::test]
    async fn shutdown_request_is_exposed_without_lifecycle_effect() {
        let (mut raw_client, raw_server) = raw_pair();
        let mut server = ServerSession::new(raw_server, ServerConfig::g1_default());
        raw_client
            .write_frame(handshake_request_frame(valid_handshake_request()))
            .await
            .expect("write");
        server.run_handshake().await.expect("ready");
        let shutdown = Bytes::from(
            ControlRequest {
                payload: Some(control_request::Payload::Shutdown(ShutdownRequest {})),
            }
            .encode_to_vec(),
        );
        raw_client.write_frame(shutdown).await.expect("write");
        // Exposed upward as a typed event; nothing terminated, nothing
        // authorized, no reply sent by this layer.
        assert_eq!(
            server.next_request().await.expect("shutdown event"),
            ReadyRequest::Shutdown
        );
        assert!(
            server.is_ready(),
            "shutdown request changes no session state here"
        );
    }

    #[tokio::test]
    async fn malformed_protobuf_terminates_session() {
        let (mut raw_client, raw_server) = raw_pair();
        let mut server = ServerSession::new(raw_server, ServerConfig::g1_default());
        raw_client
            .write_frame(Bytes::from_static(b"definitely-not-protobuf"))
            .await
            .expect("write");
        match server.run_handshake().await {
            Err(ControlError::MalformedProtobuf(_)) => {}
            other => panic!("expected MalformedProtobuf, got {other:?}"),
        }
        assert!(!server.is_ready());
        match server.next_request().await {
            Err(ControlError::SessionTerminated) => {}
            other => panic!("expected SessionTerminated, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn unexpected_response_variant_is_protocol_error() {
        let (mut raw_server, client_end) = raw_pair();
        let mut client = ClientSession::new(client_end, agentctl_config());
        let client_task = tokio::spawn(async move {
            let handshake = client.perform_handshake().await;
            (client, handshake)
        });
        let frame = raw_server.read_frame().await.expect("read handshake");
        let request = ControlRequest::decode(&frame[..]).expect("decode");
        assert!(matches!(
            request.payload,
            Some(control_request::Payload::Handshake(_))
        ));
        let accept = accept_frame_for_test();
        raw_server
            .write_frame(accept.clone())
            .await
            .expect("write accept");
        let (mut client, handshake) = client_task.await.expect("join");
        handshake.expect("client ready");
        assert!(client.is_ready());

        let ping_task = tokio::spawn(async move {
            let outcome = client.ping().await;
            (client, outcome)
        });
        let ping_frame = raw_server.read_frame().await.expect("read ping");
        let ping_request = ControlRequest::decode(&ping_frame[..]).expect("decode");
        assert!(matches!(
            ping_request.payload,
            Some(control_request::Payload::Ping(_))
        ));
        // Wrong but well-formed variant: a handshake where ping was expected.
        raw_server
            .write_frame(accept)
            .await
            .expect("write wrong variant");
        let (_client, outcome) = ping_task.await.expect("join");
        match outcome {
            Err(ControlError::UnexpectedMessage(_)) => {}
            other => panic!("expected UnexpectedMessage, got {other:?}"),
        }
    }

    fn accept_frame_for_test() -> Bytes {
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

    #[tokio::test]
    async fn ping_to_silent_peer_times_out() {
        let (mut raw_server, client_end) = raw_pair();
        let mut client = ClientSession::new(client_end, agentctl_config());
        let client_task = tokio::spawn(async move {
            let handshake = client.perform_handshake().await;
            (client, handshake)
        });
        let frame = raw_server.read_frame().await.expect("read handshake");
        ControlRequest::decode(&frame[..]).expect("decode");
        raw_server
            .write_frame(accept_frame_for_test())
            .await
            .expect("write accept");
        let (mut client, handshake) = client_task.await.expect("join");
        handshake.expect("client ready");

        // Peer stays alive but silent: the 2-second ping deadline must fire.
        let _silent_peer = raw_server;
        let start = std::time::Instant::now();
        match client.ping().await {
            Err(ControlError::Timeout { op: "ping", after }) => {
                assert_eq!(after, PING_TIMEOUT);
                assert!(
                    start.elapsed() >= PING_TIMEOUT,
                    "deadline must actually elapse"
                );
            }
            other => panic!("expected ping Timeout, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn respond_shutdown_sends_acknowledgement_only() {
        let (mut raw_client, raw_server) = raw_pair();
        let mut server = ServerSession::new(raw_server, ServerConfig::g1_default());
        raw_client
            .write_frame(handshake_request_frame(valid_handshake_request()))
            .await
            .expect("write");
        server.run_handshake().await.expect("ready");
        // Drain the handshake acceptance already queued for the client.
        let accept_frame = raw_client.read_frame().await.expect("read accept");
        let accept = decode_response(&accept_frame).expect("decode");
        assert!(matches!(
            accept.payload,
            Some(control_response::Payload::Handshake(_))
        ));
        server.respond_shutdown().await.expect("ack");
        let frame = raw_client.read_frame().await.expect("read ack");
        let response = decode_response(&frame).expect("decode");
        assert!(
            matches!(
                response.payload,
                Some(control_response::Payload::Shutdown(_))
            ),
            "must be ShutdownResponse, got {response:?}"
        );
        assert!(
            !server.is_ready(),
            "protocol use of the connection ends here"
        );
    }

    #[test]
    fn business_default_client_offers_both_versions() {
        let config = ClientConfig::business_default(CLIENT_KIND_AGENTCTL);
        assert_eq!(config.supported_versions, vec![0, 1]);
        let legacy = ClientConfig::new(CLIENT_KIND_AGENTCTL);
        assert_eq!(legacy.supported_versions, vec![0]);
    }

    #[test]
    fn execution_default_client_offers_all_versions() {
        let config = ClientConfig::execution_default(CLIENT_KIND_AGENTCTL);
        assert_eq!(config.supported_versions, vec![0, 1, 2]);
    }

    fn task_core_frame() -> Bytes {
        use ipc_contracts::proto::{task_core_request, CreateTaskRequest, TaskCoreRequest};
        Bytes::from(
            ControlRequest {
                payload: Some(control_request::Payload::TaskCore(TaskCoreRequest {
                    payload: Some(task_core_request::Payload::CreateTask(CreateTaskRequest {
                        id: "T-1".to_owned(),
                        project_id: "P-1".to_owned(),
                        objective: "ship G2".to_owned(),
                    })),
                })),
            }
            .encode_to_vec(),
        )
    }

    #[tokio::test]
    async fn v0_session_rejects_business_as_unsupported() {
        let (mut raw_client, raw_server) = raw_pair();
        let mut server = ServerSession::new(raw_server, ServerConfig::g1_default());
        raw_client
            .write_frame(handshake_request_frame(valid_handshake_request()))
            .await
            .expect("write");
        let info = server.run_handshake().await.expect("ready");
        assert_eq!(info.selected_version, 0);
        assert_eq!(server.negotiated_version(), Some(0));
        assert!(!server.business_allowed());
        raw_client
            .write_frame(task_core_frame())
            .await
            .expect("write");
        match server.next_request().await {
            Err(ControlError::UnsupportedProtocol {
                negotiated,
                required,
            }) => {
                assert_eq!(negotiated, 0);
                assert_eq!(required, 1);
            }
            other => panic!("expected UnsupportedProtocol, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn v1_session_surfaces_business_without_executing() {
        // New client [0,1] against the G2 daemon profile negotiates 1 on
        // both sides of a live pair.
        let (pair, client_outcome, server_outcome) = run_handshake_pair(
            ClientConfig::business_default(CLIENT_KIND_AGENTCTL),
            ServerConfig::g2_default(),
        )
        .await;
        assert_eq!(
            client_outcome.expect("client handshake").selected_version,
            1
        );
        assert_eq!(
            server_outcome.expect("server handshake").selected_version,
            1
        );
        assert!(pair.client.business_allowed());
        assert!(pair.server.business_allowed());
        assert_eq!(pair.client.negotiated_version(), Some(1));
        assert_eq!(pair.server.negotiated_version(), Some(1));
        drop(pair);

        // The wire shape is valid on a v1 session: the decoded request is
        // surfaced for the dispatcher with nothing executed here.
        let (mut raw_client, raw_server) = raw_pair();
        let mut server = ServerSession::new(raw_server, ServerConfig::g2_default());
        raw_client
            .write_frame(handshake_request_frame(HandshakeRequest {
                supported_protocol_versions: vec![0, 1],
                client_kind: CLIENT_KIND_AGENTCTL.to_owned(),
                client_build_version: "test".to_owned(),
                requested_capabilities: Vec::new(),
            }))
            .await
            .expect("write");
        let info = server.run_handshake().await.expect("ready");
        assert_eq!(info.selected_version, 1);
        assert!(server.business_allowed());
        raw_client
            .write_frame(task_core_frame())
            .await
            .expect("write");
        match server.next_request().await {
            Ok(ReadyRequest::Business(request)) => match request.payload {
                Some(ipc_contracts::proto::task_core_request::Payload::CreateTask(create)) => {
                    assert_eq!(create.id, "T-1");
                    assert_eq!(create.objective, "ship G2");
                }
                other => panic!("expected create_task payload, got {other:?}"),
            },
            other => panic!("expected Business request, got {other:?}"),
        }
        // The session survives a business request: version-gated dispatch
        // never terminates the connection on its own.
        assert!(server.is_ready());
    }

    fn execution_core_frame() -> Bytes {
        use ipc_contracts::proto::{
            execution_core_request, ExecutionCoreRequest, GetExecutionRequest,
        };
        Bytes::from(
            ControlRequest {
                payload: Some(control_request::Payload::ExecutionCore(
                    ExecutionCoreRequest {
                        payload: Some(execution_core_request::Payload::Get(GetExecutionRequest {
                            execution_id: "E-1".to_owned(),
                        })),
                    },
                )),
            }
            .encode_to_vec(),
        )
    }

    #[tokio::test]
    async fn v1_session_rejects_execution_as_unsupported() {
        // G2 daemon profile negotiates 1 with a [0,1] client: business is
        // allowed, execution is not — the request never executes.
        let (mut raw_client, raw_server) = raw_pair();
        let mut server = ServerSession::new(raw_server, ServerConfig::g2_default());
        raw_client
            .write_frame(handshake_request_frame(HandshakeRequest {
                supported_protocol_versions: vec![0, 1],
                client_kind: CLIENT_KIND_AGENTCTL.to_owned(),
                client_build_version: "test".to_owned(),
                requested_capabilities: Vec::new(),
            }))
            .await
            .expect("write");
        let info = server.run_handshake().await.expect("ready");
        assert_eq!(info.selected_version, 1);
        assert!(server.business_allowed());
        assert!(!server.execution_allowed());
        raw_client
            .write_frame(execution_core_frame())
            .await
            .expect("write");
        match server.next_request().await {
            Err(ControlError::UnsupportedProtocol {
                negotiated,
                required,
            }) => {
                assert_eq!(negotiated, 1);
                assert_eq!(required, 2);
            }
            other => panic!("expected UnsupportedProtocol, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn v2_session_surfaces_execution_without_executing() {
        // New client [0,1,2] against the G5 daemon profile negotiates 2
        // on both sides of a live pair.
        let (pair, client_outcome, server_outcome) = run_handshake_pair(
            ClientConfig::execution_default(CLIENT_KIND_AGENTCTL),
            ServerConfig::g5_default(),
        )
        .await;
        assert_eq!(
            client_outcome.expect("client handshake").selected_version,
            2
        );
        assert_eq!(
            server_outcome.expect("server handshake").selected_version,
            2
        );
        assert!(pair.client.business_allowed());
        assert!(pair.client.execution_allowed());
        assert!(pair.server.business_allowed());
        assert!(pair.server.execution_allowed());
        assert_eq!(pair.client.negotiated_version(), Some(2));
        assert_eq!(pair.server.negotiated_version(), Some(2));
        drop(pair);

        // The wire shape is valid on a v2 session: the decoded request is
        // surfaced for the dispatcher with nothing executed here.
        let (mut raw_client, raw_server) = raw_pair();
        let mut server = ServerSession::new(raw_server, ServerConfig::g5_default());
        raw_client
            .write_frame(handshake_request_frame(HandshakeRequest {
                supported_protocol_versions: vec![0, 1, 2],
                client_kind: CLIENT_KIND_AGENTCTL.to_owned(),
                client_build_version: "test".to_owned(),
                requested_capabilities: Vec::new(),
            }))
            .await
            .expect("write");
        let info = server.run_handshake().await.expect("ready");
        assert_eq!(info.selected_version, 2);
        assert!(server.execution_allowed());
        raw_client
            .write_frame(execution_core_frame())
            .await
            .expect("write");
        match server.next_request().await {
            Ok(ReadyRequest::Execution(request)) => match request.payload {
                Some(ipc_contracts::proto::execution_core_request::Payload::Get(get)) => {
                    assert_eq!(get.execution_id, "E-1");
                }
                other => panic!("expected get payload, got {other:?}"),
            },
            other => panic!("expected Execution request, got {other:?}"),
        }
        assert!(server.is_ready());
    }
}
