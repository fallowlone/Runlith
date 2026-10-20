//! `agentd` G2 control server: secure endpoint ownership, OS peer
//! authentication (ADR-0020), accept loop, handshake/ping via
//! `ipc-control`, version-gated business dispatch (ADR-0021), and graceful
//! shutdown.
//!
//! Daemon process lifecycle + control-server integration plus the G2
//! composition root (database-path resolver → `storage-sqlite` open and
//! migrations → trusted `TaskService` → bounded business dispatcher).
//! No providers, Runner, Memory, Context, Policy Engine (G6),
//! Execution/Attempt/Session runtimes, or AI of any kind. Internal
//! process-lifecycle state below is NOT Task lifecycle state.
//!
//! Connection pipeline: transport → OS-authenticated peer → handshake →
//! READY → (v1 only) typed business dispatch. Peer authentication is
//! connection-local and never durable. Business dispatch never bypasses
//! the trusted application service: `agentd` owns composition, never
//! business rules.

use std::fmt;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use app_services::{
    background_service::BackgroundService, org_service::OrgService, worker_service::WorkerService,
    ApprovalService, FinderService, HumanDecisionService, MemoryService, SystemClock, TaskService,
};
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

pub mod approval;
pub mod data_dir;
pub mod dispatch;
pub mod execution;
pub mod finder;
pub mod human_decision;
pub mod memory;
pub mod org;
pub mod planning;
mod provider_credential;
pub mod routine;
pub mod vault;
pub mod worker;

pub use data_dir::{resolve_data_dir, resolve_database_path};

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
    /// Durable storage failure: open, migration, or data-directory fault.
    /// The daemon fails clearly rather than serving without its database
    /// or silently creating an alternate one elsewhere.
    Storage(storage_sqlite::StorageError),
    /// Canonical data path unusable (missing home, unsafe directory,
    /// relative override, non-directory object). Fail closed, no fallback
    /// to /tmp, the working directory, or an in-memory database.
    DataDir(String),
    /// OS/runtime I/O failure.
    Io(io::Error),
}

/// Convenience alias for daemon operations.
pub type DaemonResult<T> = Result<T, DaemonError>;

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
            Self::Storage(error) => write!(f, "daemon storage failure: {error}"),
            Self::DataDir(detail) => write!(f, "daemon data directory failure: {detail}"),
            Self::Io(error) => write!(f, "daemon I/O failure: {error}"),
        }
    }
}

impl std::error::Error for DaemonError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Transport(error) => Some(error),
            Self::Control(error) => Some(error),
            Self::Storage(error) => Some(error),
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<storage_sqlite::StorageError> for DaemonError {
    fn from(error: storage_sqlite::StorageError) -> Self {
        Self::Storage(error)
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

/// Daemon configuration: protocol behavior plus the durable database
/// composition. `database_path = None` is the control-only mode (G1
/// behavior and existing tests): v1 business and v2 execution requests
/// answer with a typed unavailable error and never touch storage.
/// Production always sets a path via [`DaemonConfig::with_database`].
#[derive(Debug, Clone)]
pub struct DaemonConfig {
    /// Handshake negotiation parameters.
    pub server_config: ServerConfig,
    /// Durable database file. The daemon owns the store lifecycle built
    /// on it; `agentctl` never opens SQLite directly.
    pub database_path: Option<PathBuf>,
    /// Composition-owned provider executable override. `None` resolves
    /// `claude` via `PATH` at `run` time; `Some` pins an explicit binary
    /// (tests inject fixture doubles). Never client-supplied.
    pub provider_executable: Option<PathBuf>,
}

impl DaemonConfig {
    /// Accepted G1 daemon configuration: protocol 0, no database.
    pub fn g1_default() -> Self {
        Self {
            server_config: ServerConfig::g1_default(),
            database_path: None,
            provider_executable: None,
        }
    }

    /// G2 daemon configuration (ADR-0021): protocols `[0, 1]` with the
    /// durable database at `path`. Old `[0]` clients negotiate 0 and keep
    /// the G1 surface; new `[0, 1]` clients negotiate 1 for business IPC.
    pub fn with_database(path: impl Into<PathBuf>) -> Self {
        Self {
            server_config: ServerConfig::g2_default(),
            database_path: Some(path.into()),
            provider_executable: None,
        }
    }

    /// G5 daemon configuration (TASK-0047): protocols `[0, 1, 2]` with
    /// the durable database at `path`, enabling the execution surface
    /// (`run`/`status`/`cancel`) through the real-provider composition.
    /// Production default from TASK-0047 on; G11 negotiates through v10
    /// (TASK-0193 routine rollout) while preserving every older surface.
    pub fn with_execution(path: impl Into<PathBuf>) -> Self {
        Self {
            server_config: ServerConfig::g13_default(),
            database_path: Some(path.into()),
            provider_executable: None,
        }
    }

    /// Pins the provider executable for this daemon (test seam for
    /// fixture doubles; production leaves `None` for `PATH` resolution).
    #[must_use]
    pub fn with_provider_executable(mut self, executable: impl Into<PathBuf>) -> Self {
        self.provider_executable = Some(executable.into());
        self
    }
}

/// Shared trusted business service for the serve loop. One store, one
/// connection, guarded by an async mutex: connections stay sequential
/// per session (one in flight) while concurrent sessions serialize
/// through the single SQLite connection.
pub type BusinessHandle = Arc<tokio::sync::Mutex<TaskService>>;

/// Shared trusted G6 approval service. It owns a separate short-lived SQLite
/// connection over the same canonical database file as the other contexts.
pub type ApprovalHandle = Arc<tokio::sync::Mutex<ApprovalService<SystemClock>>>;
pub type HumanDecisionHandle = Arc<tokio::sync::Mutex<HumanDecisionService<SystemClock>>>;

/// Shared trusted G9 finder service. Stateless and `Sync`: no mutex,
/// no database — every handle and token is self-describing, so pages
/// and reopens serve from any connection concurrently.
pub type FinderHandle = Arc<FinderService>;

/// Shared trusted G10 memory service over its own SQLite connection.
pub type MemoryHandle = Arc<tokio::sync::Mutex<MemoryService<storage_sqlite::SqliteStore>>>;

/// Shared trusted G11 background service over its own SQLite connection.
pub type RoutineHandle = Arc<tokio::sync::Mutex<BackgroundService<storage_sqlite::SqliteStore>>>;

/// Shared trusted G12 worker service over its own SQLite connection.
pub type WorkerHandle = Arc<tokio::sync::Mutex<WorkerService<storage_sqlite::SqliteStore>>>;

/// Shared trusted G13 org service over its own SQLite connection.
pub type OrgHandle = Arc<tokio::sync::Mutex<OrgService<storage_sqlite::SqliteStore>>>;

#[derive(Clone)]
struct ConnectionServices {
    business: Option<BusinessHandle>,
    execution: Option<execution::ExecutionHandle>,
    approval: Option<ApprovalHandle>,
    human_decision: Option<HumanDecisionHandle>,
    vault: Option<vault::VaultHandle>,
    finder: FinderHandle,
    memory: Option<MemoryHandle>,
    routine: Option<RoutineHandle>,
    worker: Option<WorkerHandle>,
    org: Option<OrgHandle>,
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

/// Opens the durable business service for a database-backed daemon:
/// validates (creating, 0700) the data directory, opens and migrates the
/// SQLite store, and wraps it in the trusted application service. Any
/// failure — missing home, unsafe directory, permission denied, future
/// schema, corrupt migration state — fails the daemon startup clearly:
/// no silent alternate database, no /tmp fallback, no in-memory
/// substitute, no serving business IPC without durable state.
fn open_business_service(config: &DaemonConfig) -> Result<Option<BusinessHandle>, DaemonError> {
    let Some(path) = config.database_path.as_ref() else {
        return Ok(None);
    };
    let parent = path.parent().ok_or_else(|| {
        DaemonError::DataDir(format!(
            "database path {} has no parent directory",
            path.display()
        ))
    })?;
    data_dir::ensure_data_dir(parent)?;
    let store = storage_sqlite::SqliteStore::open(path)?;
    let mut service = TaskService::attach(store, app_services::SystemClock);
    // F1: recover tasks stranded mid-chain by a prior crash before serving.
    let report = service
        .reconcile_stranded()
        .map_err(|error| DaemonError::DataDir(format!("stranded-task sweep failed: {error}")))?;
    for id in &report.routed {
        eprintln!("agentd: routed stranded task {id} SUBMITTED -> AWAITING_REVIEW");
    }
    for (id, state) in &report.needs_operator {
        eprintln!("agentd: task {id} is stranded in {state}; operator action required");
    }
    Ok(Some(Arc::new(tokio::sync::Mutex::new(service))))
}

/// Opens the execution composition for a database-backed daemon: shares
/// the database path (services open their own short-lived connections
/// per operation) plus a fresh cancel registry, process tracker, and the
/// configured provider executable override. No I/O: the managed workspace
/// root is ensured per `run`. Control-only daemons (`None` database)
/// get no execution composition — execution requests answer typed
/// unavailable, exactly like business requests.
fn open_execution(
    config: &DaemonConfig,
    vault: Option<&vault::VaultHandle>,
) -> Option<execution::ExecutionHandle> {
    config.database_path.as_ref().map(|db| {
        let composition =
            execution::ExecutionComposition::new(db.clone(), config.provider_executable.clone());
        Arc::new(match vault {
            Some(vault) => composition.with_vault(vault.clone()),
            None => composition,
        })
    })
}

/// ADR-0039: the operator vault lives beside the database and is shared by
/// `vault_core` and brokered provider launches; control-only daemons have
/// none.
fn open_vault(config: &DaemonConfig) -> Option<vault::VaultHandle> {
    vault::vault_path(config.database_path.as_deref()).map(vault::open_handle)
}

fn open_approval_service(config: &DaemonConfig) -> Result<Option<ApprovalHandle>, DaemonError> {
    let Some(path) = config.database_path.as_ref() else {
        return Ok(None);
    };
    Ok(Some(Arc::new(tokio::sync::Mutex::new(
        ApprovalService::open(path, SystemClock).map_err(|error| match error {
            app_services::ServiceError::Storage(storage) => DaemonError::Storage(storage),
            other => DaemonError::DataDir(format!("approval service startup failed: {other}")),
        })?,
    ))))
}

/// Opens the finder composition: snapshot-indexed engine plus Git
/// revision backend behind the trusted service, with INV-008
/// redaction on all display bytes. No database, no provider — always
/// present, including on control-only daemons.
fn open_finder() -> FinderHandle {
    let revision: Arc<dyn repo_finder::RepoRevisionPort> = Arc::new(git_cli::GitCli::new());
    let engine = Arc::new(finder_local::IndexedFinder::new(revision.clone()));
    let search: Arc<dyn repo_finder::RepoSearchPort> = engine.clone();
    let read: Arc<dyn repo_finder::RepoReadPort> = engine;
    Arc::new(
        FinderService::new(search, read, revision)
            .with_filter(Arc::new(app_services::RedactingFilter)),
    )
}

fn open_routine_service(config: &DaemonConfig) -> Result<Option<RoutineHandle>, DaemonError> {
    let Some(path) = config.database_path.as_ref() else {
        return Ok(None);
    };
    let store = storage_sqlite::SqliteStore::open(path)?;
    let mut service = BackgroundService::new(store);
    // Restart reconciliation (ADR-0045 B-9): runs left RUNNING by a
    // dead daemon become INTERRUPTED, never silently re-executed.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0);
    let _ = service.reconcile(now);
    Ok(Some(Arc::new(tokio::sync::Mutex::new(service))))
}

fn open_worker_service(config: &DaemonConfig) -> Result<Option<WorkerHandle>, DaemonError> {
    let Some(path) = config.database_path.as_ref() else {
        return Ok(None);
    };
    let store = storage_sqlite::SqliteStore::open(path)?;
    Ok(Some(Arc::new(tokio::sync::Mutex::new(WorkerService::new(
        store,
    )))))
}

fn open_org_service(config: &DaemonConfig) -> Result<Option<OrgHandle>, DaemonError> {
    let Some(path) = config.database_path.as_ref() else {
        return Ok(None);
    };
    let store = storage_sqlite::SqliteStore::open(path)?;
    Ok(Some(Arc::new(tokio::sync::Mutex::new(OrgService::new(
        store,
    )))))
}

fn open_memory_service(config: &DaemonConfig) -> Result<Option<MemoryHandle>, DaemonError> {
    let Some(path) = config.database_path.as_ref() else {
        return Ok(None);
    };
    let store = storage_sqlite::SqliteStore::open(path)?;
    Ok(Some(Arc::new(tokio::sync::Mutex::new(MemoryService::new(
        store,
    )))))
}

fn open_human_decision_service(
    config: &DaemonConfig,
) -> Result<Option<HumanDecisionHandle>, DaemonError> {
    let Some(path) = config.database_path.as_ref() else {
        return Ok(None);
    };
    Ok(Some(Arc::new(tokio::sync::Mutex::new(
        HumanDecisionService::open(path).map_err(|error| match error {
            app_services::ServiceError::Storage(storage) => DaemonError::Storage(storage),
            other => {
                DaemonError::DataDir(format!("human decision service startup failed: {other}"))
            }
        })?,
    ))))
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
/// surfaced; version-1 business requests dispatch through the shared
/// trusted service with typed responses; version-2 execution requests
/// dispatch through the shared execution composition (`run` commits its
/// durable entry synchronously and continues in a detached background
/// thread — the connection never carries the provider session).
///
/// `prefetched` carries a frame read before authentication (Windows
/// first-frame staging): it is interpreted here only because the caller
/// already authenticated the peer. `None` reads the handshake live.
async fn serve_connection<S>(
    stream: S,
    config: &ServerConfig,
    services: ConnectionServices,
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
            Ok(ReadyRequest::Business(request)) => {
                let response = match &services.business {
                    Some(handle) => {
                        let mut service = handle.lock().await;
                        dispatch::dispatch(&mut service, request, services.execution.as_deref())
                    }
                    None => dispatch::unavailable(),
                };
                if session.respond_business(response).await.is_err() {
                    return ConnectionOutcome::Ended;
                }
            }
            Ok(ReadyRequest::Execution(request)) => {
                let (response, job) = match &services.execution {
                    Some(composition) => execution::dispatch_execution(composition, request),
                    None => (execution::execution_unavailable(), None),
                };
                // Retained by design (F17, see `execution::run_background`):
                // the serve loop never blocks on provider time, but the
                // handle is kept in the composition registry; on daemon
                // shutdown wind-down cancels in-flight runs first and
                // then joins them, so runs conclude their bookkeeping
                // through the cancel path instead of dying mid-write.
                // A spawn failure is reported honestly with the
                // committed execution named.
                let response = match job {
                    Some(job) => {
                        let id = job.work_execution_id();
                        let owner = job.owner_token();
                        let composition = services
                            .execution
                            .clone()
                            .expect("job exists only with execution composition");
                        match composition.spawn_background(job) {
                            Ok(()) => response,
                            Err(_) => execution::abort_start(&composition, &id, owner),
                        }
                    }
                    None => response,
                };
                if session.respond_execution(response).await.is_err() {
                    return ConnectionOutcome::Ended;
                }
            }
            Ok(ReadyRequest::Policy(request)) => {
                let response = match &services.approval {
                    Some(handle) => {
                        let mut service = handle.lock().await;
                        approval::dispatch(&mut service, request)
                    }
                    None => approval::unavailable(),
                };
                if session.respond_policy(response).await.is_err() {
                    return ConnectionOutcome::Ended;
                }
            }
            Ok(ReadyRequest::HumanDecision(request)) => {
                let negotiated = session.negotiated_version().unwrap_or(0);
                let response = match &services.human_decision {
                    Some(handle) => match &services.business {
                        Some(business) => {
                            let tasks = business.lock().await;
                            let mut service = handle.lock().await;
                            human_decision::dispatch(
                                &mut service,
                                Some(&tasks),
                                request,
                                negotiated,
                            )
                        }
                        None => {
                            let mut service = handle.lock().await;
                            human_decision::dispatch(&mut service, None, request, negotiated)
                        }
                    },
                    None => human_decision::unavailable(),
                };
                if session.respond_human_decision(response).await.is_err() {
                    return ConnectionOutcome::Ended;
                }
            }
            Ok(ReadyRequest::Planning(request)) => {
                let response = match (&services.execution, &services.human_decision) {
                    (Some(composition), Some(handle)) => {
                        let mut decisions = handle.lock().await;
                        planning::dispatch_control(composition, &mut decisions, request)
                    }
                    _ => planning::unavailable(),
                };
                if session.respond_planning(response).await.is_err() {
                    return ConnectionOutcome::Ended;
                }
            }
            Ok(ReadyRequest::TaskAuthoring(request)) => {
                let response = match &services.business {
                    Some(handle) => {
                        let mut service = handle.lock().await;
                        dispatch::dispatch_task_authoring(&mut service, request)
                    }
                    None => dispatch::task_authoring_unavailable(),
                };
                if session.respond_task_authoring(response).await.is_err() {
                    return ConnectionOutcome::Ended;
                }
            }
            Ok(ReadyRequest::Finder(request)) => {
                // Scans are seconds-long filesystem work: off the serve
                // loop onto the blocking pool (safe on either runtime
                // flavor), so other connections never stall behind one.
                let service = services.finder.clone();
                let response =
                    match tokio::task::spawn_blocking(move || finder::dispatch(&service, request))
                        .await
                    {
                        Ok(response) => response,
                        Err(_) => finder::failed(),
                    };
                if session.respond_finder(response).await.is_err() {
                    return ConnectionOutcome::Ended;
                }
            }
            Ok(ReadyRequest::Memory(request)) => {
                let response = match &services.memory {
                    Some(handle) => {
                        let mut service = handle.lock().await;
                        memory::dispatch(&mut service, request)
                    }
                    None => memory::unavailable(),
                };
                if session.respond_memory(response).await.is_err() {
                    return ConnectionOutcome::Ended;
                }
            }
            Ok(ReadyRequest::Routine(request)) => {
                let response = match &services.routine {
                    Some(handle) => {
                        let mut service = handle.lock().await;
                        routine::dispatch(&mut service, request)
                    }
                    None => routine::unavailable(),
                };
                if session.respond_routine(response).await.is_err() {
                    return ConnectionOutcome::Ended;
                }
            }
            Ok(ReadyRequest::Worker(request)) => {
                let response = match &services.worker {
                    Some(handle) => {
                        let mut service = handle.lock().await;
                        worker::dispatch(&mut service, request)
                    }
                    None => worker::unavailable(),
                };
                if session.respond_worker(response).await.is_err() {
                    return ConnectionOutcome::Ended;
                }
            }
            Ok(ReadyRequest::Org(request)) => {
                let response = match &services.org {
                    Some(handle) => {
                        let mut service = handle.lock().await;
                        org::dispatch(&mut service, request)
                    }
                    None => org::unavailable(),
                };
                if session.respond_org(response).await.is_err() {
                    return ConnectionOutcome::Ended;
                }
            }
            Ok(ReadyRequest::Vault(request)) => {
                let response = match &services.vault {
                    Some(handle) => vault::dispatch(handle.clone(), request).await,
                    None => dispatch::vault_unavailable(),
                };
                if session.respond_vault(response).await.is_err() {
                    return ConnectionOutcome::Ended;
                }
            }
            Err(_) => return ConnectionOutcome::Ended,
        }
    }
}

/// G11 routine tick: due authorized routines run without any client
/// connected (ADR-0045). Shutdown-safe via the same watch channel.
async fn routine_tick_loop(
    handle: RoutineHandle,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
) {
    loop {
        tokio::select! {
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() { break; }
            }
            _ = tokio::time::sleep(Duration::from_secs(1)) => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
                    .unwrap_or(0);
                let mut service = handle.lock().await;
                match service.tick(now, "evidence:daemon-tick") {
                    Ok(runs) if !runs.is_empty() => {
                        eprintln!("agentd: routine tick completed {} run(s)", runs.len());
                    }
                    Ok(_) => {}
                    Err(error) => eprintln!("agentd: routine tick failed: {error}"),
                }
            }
        }
    }
}

/// Bounded cadence for the daemon TAKEOVER expiry tick (TASK-0177).
const TAKEOVER_EXPIRY_TICK: Duration = Duration::from_secs(1);

/// Reconciles expired ACTIVE takeovers without any service call.
/// Shutdown-safe: exits when the shutdown watch flips and is aborted
/// during wind-down; a reconcile failure is logged, never fatal.
async fn takeover_expiry_loop(
    handle: HumanDecisionHandle,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
) {
    loop {
        tokio::select! {
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    break;
                }
            }
            _ = tokio::time::sleep(TAKEOVER_EXPIRY_TICK) => {
                let mut service = handle.lock().await;
                match service.reconcile_takeover_expiry(None) {
                    Ok(abandoned) if !abandoned.is_empty() => {
                        eprintln!(
                            "agentd: takeover expiry abandoned {} record(s)",
                            abandoned.len()
                        );
                    }
                    Ok(_) => {}
                    Err(error) => {
                        eprintln!("agentd: takeover expiry reconcile failed: {error}");
                    }
                }
            }
        }
    }
}

/// Shared serve loop: admits authenticated streams from the accept pump
/// until a shutdown is initiated, then winds down. Each item carries an
/// optional prefetched first frame (Windows staging; `None` on Unix).
/// Admission is bounded: past [`MAX_CONTROL_CLIENTS`] the extra
/// connection is closed, never a daemon failure. Only peer-authenticated
/// streams ever arrive here — the pumps authenticate before sending.
///
/// Background provider runs are retained threads, not tracked tasks:
/// wind-down aborts connection tasks promptly while in-flight provider
/// runs are joined through the composition registry (F17) so their
/// terminal bookkeeping lands before the endpoint is released.
/// Pre-attempt strands are reported at startup for explicit resume
/// (TASK-0054).
async fn serve_streams<S>(
    mut accepted: tokio::sync::mpsc::Receiver<(S, Option<BytesMut>)>,
    acceptor: tokio::task::JoinHandle<()>,
    config: &DaemonConfig,
    services: ConnectionServices,
) -> RunOutcome
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);
    let vault_idle = services
        .vault
        .clone()
        .map(|handle| tokio::spawn(vault::idle_watch(handle)));
    let takeover_tick = services
        .human_decision
        .clone()
        .map(|handle| tokio::spawn(takeover_expiry_loop(handle, shutdown_rx.clone())));
    let routine_tick = services
        .routine
        .clone()
        .map(|handle| tokio::spawn(routine_tick_loop(handle, shutdown_rx.clone())));
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
                let services = services.clone();
                joins.spawn(async move {
                    let _permit = permit;
                    serve_connection(stream, &server_config, services, sender, prefetched).await
                });
            }
        }
    }

    // Wind down: acceptor output is ignored from here on, stragglers are
    // aborted, tasks reaped. In-flight provider runs are cancelled first
    // through the composition registry, then joined: the cancel path
    // concludes their bookkeeping instead of dying mid-write, and the
    // join cannot stall behind provider time. The owned endpoint is
    // released by the caller.
    // The accept task (F19) is stopped first so the listener closes
    // before the endpoint is released.
    acceptor.abort();
    let _ = acceptor.await;
    joins.abort_all();
    while joins.join_next().await.is_some() {}
    if let Some(task) = vault_idle {
        task.abort();
        let _ = task.await;
    }
    if let Some(task) = takeover_tick {
        task.abort();
        let _ = task.await;
    }
    if let Some(task) = routine_tick {
        task.abort();
        let _ = task.await;
    }
    if let Some(handle) = services.vault.as_ref() {
        vault::lock_handle(handle);
    }
    if let Some(composition) = services.execution.as_ref() {
        let composition = composition.clone();
        let _ = tokio::task::spawn_blocking(move || {
            composition.cancel_inflight_runs();
            composition.join_background_runs();
        })
        .await;
    }
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
        let business = open_business_service(&config)?;
        // Startup reconciliation surface (TASK-0054, G5 B-RS): report
        // stranded pre-attempt entries for explicit operator resume.
        // Read-only and best-effort — never a boot gate.
        if let Some(db) = config.database_path.as_ref() {
            execution::log_pre_attempt_strands(db);
        }
        // Daemon principal once, from the OS — never from configuration.
        let daemon = daemon_identity().map_err(DaemonError::from)?;
        // Snapshot the node we own so shutdown cleanup removes only it.
        let owned = snapshot_endpoint(&endpoint.socket_path).ok();
        let (accepted_tx, accepted_rx) =
            tokio::sync::mpsc::channel::<(UnixStream, Option<BytesMut>)>(MAX_CONTROL_CLIENTS + 2);
        let acceptor = tokio::spawn(async move {
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
        let approval = open_approval_service(&config)?;
        let human_decision = open_human_decision_service(&config)?;
        let vault = open_vault(&config);
        let outcome = serve_streams(
            accepted_rx,
            acceptor,
            &config,
            ConnectionServices {
                business,
                execution: open_execution(&config, vault.as_ref()),
                approval,
                human_decision,
                vault,
                finder: open_finder(),
                memory: open_memory_service(&config)?,
                routine: open_routine_service(&config)?,
                worker: open_worker_service(&config)?,
                org: open_org_service(&config)?,
            },
        )
        .await;
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
    /// The durable business service opens before binding (fail clearly on
    /// data-directory or migration faults); v0 negotiation semantics never
    /// depend on storage availability.
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
        let business = super::open_business_service(&config)?;
        // Startup reconciliation surface (TASK-0054, G5 B-RS): report
        // stranded pre-attempt entries for explicit operator resume.
        // Read-only and best-effort — never a boot gate.
        if let Some(db) = config.database_path.as_ref() {
            super::execution::log_pre_attempt_strands(db);
        }
        // Daemon principal once, from the OS — never from configuration.
        let daemon = daemon_identity().map_err(DaemonError::from)?;
        let (accepted_tx, accepted_rx) = tokio::sync::mpsc::channel::<(
            NamedPipeServer,
            Option<BytesMut>,
        )>(MAX_CONTROL_CLIENTS + 2);
        let acceptor = tokio::spawn(async move {
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
        let approval = super::open_approval_service(&config)?;
        let human_decision = super::open_human_decision_service(&config)?;
        let vault = super::open_vault(&config);
        Ok(serve_streams(
            accepted_rx,
            acceptor,
            &config,
            ConnectionServices {
                business,
                execution: open_execution(&config, vault.as_ref()),
                approval,
                human_decision,
                vault,
                finder: super::open_finder(),
                memory: super::open_memory_service(&config)?,
                routine: super::open_routine_service(&config)?,
                worker: super::open_worker_service(&config)?,
                org: super::open_org_service(&config)?,
            },
        )
        .await)
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

    /// One admission attempt that reports refusal instead of panicking.
    ///
    /// A client is refused either by the kernel (the accept backlog is full)
    /// or by the daemon (it accepts, then closes because no admission permit
    /// is free). Both are the same observable fact — the client does not
    /// become ready — and a capacity test must not care which one happened.
    async fn try_admin_session(endpoint: &UnixEndpoint) -> Option<ClientSession<UnixStream>> {
        let stream = transport_connect(endpoint).await.ok()?;
        let mut session = ClientSession::new(
            FramedTransport::new(stream),
            ClientConfig::new(ipc_control::CLIENT_KIND_AGENTCTL),
        );
        session.perform_handshake().await.ok()?;
        session.is_ready().then_some(session)
    }

    /// Readiness that keeps the session it proved readiness with.
    ///
    /// A control slot is released when the *daemon* observes the close, which
    /// is asynchronous with the client-side drop. A test that probes readiness
    /// and then saturates `MAX_CONTROL_CLIENTS` can therefore race its own
    /// probe: under load the probe's slot is still held when the last holder
    /// connects, and that holder is refused instead of the test's intended
    /// extra client. Keeping the probe as the first holder makes occupancy
    /// deterministic — the slot is never released, so it cannot be released
    /// late.
    async fn wait_ready_session(endpoint: &UnixEndpoint) -> ClientSession<UnixStream> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            if let Ok(stream) = transport_connect(endpoint).await {
                let mut session = ClientSession::new(
                    FramedTransport::new(stream),
                    ClientConfig::new(ipc_control::CLIENT_KIND_AGENTCTL),
                );
                if session.perform_handshake().await.is_ok() {
                    return session;
                }
            }
            if tokio::time::Instant::now() >= deadline {
                panic!("daemon never became ready");
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
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

    fn fixture_git(repo: &std::path::Path) {
        let output = std::process::Command::new("git")
            .args([
                "-c",
                "user.name=fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "init",
                "-q",
            ])
            .current_dir(repo)
            .stdin(std::process::Stdio::null())
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .expect("fixture git init");
        assert!(output.status.success(), "git init failed");
        let output = std::process::Command::new("git")
            .args([
                "-c",
                "user.name=fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "init",
            ])
            .current_dir(repo)
            .stdin(std::process::Stdio::null())
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .expect("fixture git commit");
        assert!(output.status.success(), "git commit failed");
    }

    fn fixture_script(dir: &std::path::Path, name: &str, body: &str) -> PathBuf {
        use std::io::Write;
        let path = dir.join(name);
        let mut file = std::fs::File::create(&path).expect("fixture file");
        file.write_all(b"#!/bin/sh\n").expect("shebang");
        file.write_all(body.as_bytes()).expect("fixture body");
        drop(file);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("fixture chmod");
        path
    }

    /// Seeds a READY task through an accepted fixture plan, mirroring the
    /// execution-composition fixtures: a planning parent carrying the
    /// objective, published with this task as its only child.
    fn seed_ready_task(db: &std::path::Path, project: &str, task: &str) {
        let mut tasks = app_services::TaskService::open(db, app_services::SystemClock)
            .expect("open task service");
        let project_id = task_domain::ProjectId::new(project).unwrap();
        tasks
            .create_project(
                &task_domain::Project::new(project_id.clone(), format!("project {project}"))
                    .unwrap(),
            )
            .unwrap();
        let parent_id = task_domain::TaskId::new(format!("{task}-PARENT")).unwrap();
        tasks
            .create_task(
                &task_domain::Task::draft(
                    parent_id.clone(),
                    project_id,
                    format!("objective {task}"),
                )
                .unwrap(),
            )
            .unwrap();
        tasks.mark_ready(&parent_id).unwrap();
        let mut planning = app_services::PlanningService::open(db, app_services::SystemClock)
            .expect("open planning service");
        let scope = plan_domain::ScopeBound::new(vec!["repo:src".to_owned()]).unwrap();
        let evidence = plan_domain::EvidenceRef::new(format!("fixture-plan:{task}")).unwrap();
        let branch = plan_domain::PlanBranchId::new(format!("fixture-plan-{task}")).unwrap();
        let planner =
            fake_planner::FakePlanner::for_single_leaf(plan_domain::PlanNodeId::new(task).unwrap());
        let request = plan_domain::DecompositionRequest {
            parent_task_ref: format!("{task}-PARENT"),
            base_plan_revision: 0,
            acceptance_criteria_refs: vec![],
            scope_bound: scope.clone(),
            evidence_refs: vec![evidence],
            resolved_evidence: Vec::new(),
            depth_remaining: plan_domain::MAX_DECOMPOSITION_DEPTH,
            baseline_risk: plan_domain::RiskClass::Low,
            budget_remaining: plan_domain::PlanningBudget {
                calls_remaining: plan_domain::MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH,
                input_tokens_remaining: plan_domain::MAX_PLANNER_CUMULATIVE_INPUT_TOKENS,
            },
        };
        let policy = app_services::PlanningPolicyContext {
            scope_bound: scope,
            baseline_risk: plan_domain::RiskClass::Low,
            affected_parent_criteria: vec![],
            parent_task_ref: format!("{task}-PARENT"),
            canonical_task_authority: false,
            lineage_fingerprints: vec![],
            risk_grant: None,
            risk_decision_id: None,
            policy_fingerprint: format!("policy:fixture:{task}"),
        };
        let outcome = planning
            .propose_decomposition(&planner, &branch, request, &policy)
            .expect("seed accepted plan");
        assert!(
            matches!(outcome, app_services::PlanningProposalOutcome::Accepted(_)),
            "fixture plan must be accepted"
        );
    }

    fn wait_for_attempt(db: &std::path::Path, execution_id: &agent_runtime::ExecutionId) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            let store = storage_sqlite::SqliteStore::open(db).expect("open test db");
            if let Ok(Some(execution)) = store.get_execution(execution_id) {
                if !execution.attempts().is_empty() {
                    return;
                }
            }
            if std::time::Instant::now() >= deadline {
                panic!("provider run never went in flight");
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    /// Shutdown with an in-flight provider run must release the endpoint
    /// promptly instead of pinning the daemon until the provider finishes.
    /// The fixture provider never exits on its own, so without a
    /// wind-down cancel this hangs past the shutdown budget below.
    #[tokio::test]
    async fn shutdown_with_inflight_provider_run_exits_promptly() {
        use ipc_contracts::proto::{
            control_request, control_response, execution_core_request, execution_core_response,
            ControlRequest, ControlResponse, ExecutionCoreRequest, RunExecutionRequest,
        };

        let sandbox = Sandbox::new("shutdown-inflight");
        let db = test_db_path(&sandbox.root);
        let db_parent = db.parent().expect("db parent");
        std::fs::create_dir_all(db_parent).expect("db parent dir");
        std::fs::set_permissions(db_parent, std::fs::Permissions::from_mode(0o700))
            .expect("db parent mode");
        let repo = sandbox.root.join("repo");
        std::fs::create_dir_all(&repo).expect("repo dir");
        fixture_git(&repo);
        let fixture = fixture_script(&sandbox.root, "slow-provider.sh", "while :; do :; done\n");
        seed_ready_task(&db, "P-SHUT", "T-SHUT");
        let endpoint = sandbox.endpoint.clone();
        let config = DaemonConfig::with_execution(db.clone()).with_provider_executable(fixture);
        let handle =
            tokio::spawn(
                async move { run_on_endpoint_with(&endpoint, config, OsPeerSource).await },
            );
        wait_ready(&sandbox.endpoint).await;

        let stream = transport_connect(&sandbox.endpoint).await.expect("connect");
        let mut session = ClientSession::new(
            FramedTransport::new(stream),
            ClientConfig::execution_default(ipc_control::CLIENT_KIND_AGENTCTL),
        );
        session.perform_handshake().await.expect("v2 handshake");
        assert!(session.execution_allowed());
        let mut transport = session.into_inner();
        let frame = Bytes::from(
            ControlRequest {
                payload: Some(control_request::Payload::ExecutionCore(
                    ExecutionCoreRequest {
                        payload: Some(execution_core_request::Payload::Run(RunExecutionRequest {
                            task_id: "T-SHUT".to_owned(),
                            execution_id: "E-SHUT".to_owned(),
                            attempt_id: "A-SHUT".to_owned(),
                            repository_ref: "repo-local:shutdown-test".to_owned(),
                            repository_path: repo.to_string_lossy().into_owned(),
                            base_revision: "HEAD".to_owned(),
                        })),
                    },
                )),
            }
            .encode_to_vec(),
        );
        transport.write_frame(frame).await.expect("write run");
        let reply = transport.read_frame().await.expect("run reply");
        let response = ControlResponse::decode(&reply[..]).expect("decode");
        let run = match response.payload {
            Some(control_response::Payload::ExecutionCore(body)) => match body.payload {
                Some(execution_core_response::Payload::Run(run)) => run,
                other => panic!("expected run payload, got {other:?}"),
            },
            other => panic!("expected execution envelope, got {other:?}"),
        };
        assert_eq!(run.task_state, "RUNNING");
        drop(transport);

        let execution_id =
            agent_runtime::ExecutionId::new(run.execution_id.clone()).expect("execution id");
        wait_for_attempt(&db, &execution_id);
        shutdown_and_await(handle, &sandbox.endpoint).await;
        assert!(
            !sandbox.endpoint.socket_path.exists(),
            "owned endpoint must be released"
        );
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

    /// Admission is bounded and a refused client is not fatal.
    ///
    /// Formerly `ninth_concurrent_client_is_closed_not_fatal`, which held
    /// exactly `MAX_CONTROL_CLIENTS` sessions and asserted that the next
    /// handshake fails. That assumed the test controls occupancy, and it does
    /// not: admission is a semaphore whose permit is released when the
    /// daemon's connection task ends, which is asynchronous with the client
    /// dropping its side. Under full-run load a saturating client could
    /// therefore be refused instead of the intended extra one — the panic
    /// landed in the saturating loop, not on the assertion — and the earlier
    /// hardening in `wait_ready_session` only removed one instance of that
    /// race, not the assumption behind it (G5 TEST-H2).
    ///
    /// The bound itself is the contract, so this saturates by observation:
    /// admit until one client is refused. With capacity `MAX_CONTROL_CLIENTS`
    /// and `MAX_CONTROL_CLIENTS + 1` attempts a refusal is guaranteed, so the
    /// loop terminates without a timeout, and nothing here depends on when a
    /// permit is returned.
    #[tokio::test]
    async fn admission_is_bounded_and_a_refused_client_is_not_fatal() {
        let sandbox = Sandbox::new("ninth");
        let handle = spawn_daemon(&sandbox.endpoint);
        let mut holders = vec![wait_ready_session(&sandbox.endpoint).await];

        let mut refused = false;
        for _ in 0..MAX_CONTROL_CLIENTS {
            match try_admin_session(&sandbox.endpoint).await {
                Some(session) => holders.push(session),
                None => {
                    refused = true;
                    break;
                }
            }
        }

        assert!(
            refused,
            "admission must refuse a client within {} attempts against a bound of {MAX_CONTROL_CLIENTS}",
            MAX_CONTROL_CLIENTS + 1
        );
        assert!(
            holders.len() <= MAX_CONTROL_CLIENTS,
            "more ready sessions ({}) than the admission bound allows",
            holders.len()
        );
        // The refusal harmed neither an existing session nor the daemon.
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

    /// Production source that carries a liveness token: the token drops
    /// only when the acceptor task (the sole owner of the source) ends.
    struct TokenOsSource(#[allow(dead_code)] Arc<()>);

    impl PeerSource for TokenOsSource {
        fn peer_of(&self, stream: &UnixStream) -> TransportResult<PeerIdentity> {
            OsPeerSource.peer_of(stream)
        }
    }

    /// F19: shutdown must stop and reap the accept task, not leave it
    /// detached holding the listener after the daemon reports completion.
    #[tokio::test]
    async fn shutdown_reaps_acceptor_task() {
        let sandbox = Sandbox::new("f19-acceptor");
        let token = Arc::new(());
        let handle = spawn_daemon_with(&sandbox.endpoint, TokenOsSource(token.clone()));
        wait_ready(&sandbox.endpoint).await;
        shutdown_and_await(handle, &sandbox.endpoint).await;
        assert_eq!(Arc::strong_count(&token), 1, "acceptor task still alive");
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

    fn test_db_path(root: &std::path::Path) -> PathBuf {
        // Parent absent on purpose: the daemon admits it by creating it
        // at 0700, exactly like the canonical application directory.
        root.join("db").join("state.sqlite3")
    }

    fn spawn_daemon_with_db(
        endpoint: &UnixEndpoint,
        db: PathBuf,
        source: impl PeerSource + 'static,
    ) -> tokio::task::JoinHandle<Result<RunOutcome, DaemonError>> {
        let endpoint = endpoint.clone();
        tokio::spawn(async move {
            run_on_endpoint_with(&endpoint, DaemonConfig::with_database(db), source).await
        })
    }

    #[tokio::test]
    async fn denied_peer_cannot_reach_business_dispatch_or_storage() {
        let sandbox = Sandbox::new("denybiz");
        let db = test_db_path(&sandbox.root);
        let handle = spawn_daemon_with_db(
            &sandbox.endpoint,
            db.clone(),
            FixedPeerSource {
                peer: from_uid(mismatched_uid()),
                calls: Arc::new(AtomicUsize::new(0)),
            },
        );
        // Give the daemon a moment to bind (no handshake can succeed).
        tokio::time::sleep(Duration::from_millis(300)).await;
        // A valid-looking v1 handshake is still rejected before READY:
        // kind and version never substitute for authentication.
        assert!(
            !handshake_as(&sandbox.endpoint, ipc_control::CLIENT_KIND_AGENTCTL).await,
            "mismatched peer must never reach READY"
        );
        // The database the daemon composed stays empty: no project, no
        // task, no journal record was minted for the denied peer.
        let store = storage_sqlite::SqliteStore::open(&db).expect("open test db");
        assert!(store.list_events_ordered().expect("journal").is_empty());
        drop(store);
        assert!(
            !handle.is_finished(),
            "denied peers must not kill the daemon"
        );
        handle.abort();
    }

    #[tokio::test]
    async fn v1_business_round_trip_through_daemon_dispatch() {
        use ipc_contracts::proto::{
            control_request, control_response, task_core_request, task_core_response,
            ControlRequest, ControlResponse, CreateProjectRequest, TaskCoreRequest,
        };
        use prost::Message;

        let sandbox = Sandbox::new("dispatch");
        let db = test_db_path(&sandbox.root);
        let handle = spawn_daemon_with_db(&sandbox.endpoint, db.clone(), OsPeerSource);
        wait_ready(&sandbox.endpoint).await;
        // Drive one business request over a live v1 session: the daemon
        // dispatches through the trusted service and answers typed.
        let stream = transport_connect(&sandbox.endpoint).await.expect("connect");
        let mut session = ClientSession::new(
            FramedTransport::new(stream),
            ClientConfig::business_default(ipc_control::CLIENT_KIND_AGENTCTL),
        );
        let accepted = session.perform_handshake().await.expect("v1 handshake");
        assert_eq!(accepted.selected_version, 1);
        let mut transport = session.into_inner();
        let frame = bytes::Bytes::from(
            ControlRequest {
                payload: Some(control_request::Payload::TaskCore(TaskCoreRequest {
                    payload: Some(task_core_request::Payload::CreateProject(
                        CreateProjectRequest {
                            id: "P-1".to_owned(),
                            name: "Dispatched".to_owned(),
                        },
                    )),
                })),
            }
            .encode_to_vec(),
        );
        transport.write_frame(frame).await.expect("write");
        let reply = transport.read_frame().await.expect("reply");
        let response = ControlResponse::decode(&reply[..]).expect("decode");
        match response.payload {
            Some(control_response::Payload::TaskCore(business)) => match business.payload {
                Some(task_core_response::Payload::CreateProject(created)) => {
                    assert_eq!(created.project.expect("dto").id, "P-1");
                }
                other => panic!("expected create_project, got {other:?}"),
            },
            other => panic!("expected task_core envelope, got {other:?}"),
        }
        // The session survives a business exchange: ping still works.
        drop(transport);
        admin_session(&sandbox.endpoint)
            .await
            .ping()
            .await
            .expect("ping");
        // And the mutation is journaled exactly once.
        let store = storage_sqlite::SqliteStore::open(&db).expect("open test db");
        let history = store.list_events_ordered().expect("journal");
        assert_eq!(history.len(), 1);
        assert_eq!(
            history[0].fact.event_type,
            storage_sqlite::EventType::ProjectCreated
        );
        drop(store);
        shutdown_and_await(handle, &sandbox.endpoint).await;
    }

    #[tokio::test]
    async fn control_only_daemon_reports_business_unavailable() {
        use ipc_contracts::proto::{
            control_request, control_response, task_core_request, task_core_response,
            ControlRequest, ControlResponse, ListProjectsRequest, TaskCoreRequest,
        };
        use prost::Message;

        // Bespoke composition: v1 offered, no database. The version gate
        // passes but handling answers typed-unavailable, never touching
        // storage (which does not exist here).
        let sandbox = Sandbox::new("nounavailable");
        let endpoint = sandbox.endpoint.clone();
        let handle = tokio::spawn(async move {
            run_on_endpoint_with(
                &endpoint,
                DaemonConfig {
                    server_config: ServerConfig::g2_default(),
                    database_path: None,
                    provider_executable: None,
                },
                OsPeerSource,
            )
            .await
        });
        wait_ready(&sandbox.endpoint).await;
        let stream = transport_connect(&sandbox.endpoint).await.expect("connect");
        let mut session = ClientSession::new(
            FramedTransport::new(stream),
            ClientConfig::business_default(ipc_control::CLIENT_KIND_AGENTCTL),
        );
        session.perform_handshake().await.expect("v1 handshake");
        let mut transport = session.into_inner();
        let frame = bytes::Bytes::from(
            ControlRequest {
                payload: Some(control_request::Payload::TaskCore(TaskCoreRequest {
                    payload: Some(task_core_request::Payload::ListProjects(
                        ListProjectsRequest {},
                    )),
                })),
            }
            .encode_to_vec(),
        );
        transport.write_frame(frame).await.expect("write");
        let reply = transport.read_frame().await.expect("reply");
        let response = ControlResponse::decode(&reply[..]).expect("decode");
        match response.payload {
            Some(control_response::Payload::TaskCore(business)) => match business.payload {
                Some(task_core_response::Payload::Error(error)) => {
                    assert_eq!(
                        error.code,
                        ipc_contracts::proto::BusinessErrorCode::Internal as i32
                    );
                }
                other => panic!("expected typed unavailable error, got {other:?}"),
            },
            other => panic!("expected task_core envelope, got {other:?}"),
        }
        drop(transport);
        shutdown_and_await(handle, &sandbox.endpoint).await;
    }

    /// ADR-0039 P5c: the vault round trip over a negotiated v7 session.
    /// No response frame ever carries the stored value, and the file sits
    /// beside the database, owner-only.
    #[tokio::test]
    async fn vault_round_trip_over_v7_never_returns_the_value() {
        use ipc_contracts::proto::{
            vault_core_request::Payload as Request, vault_core_response::Payload as Response,
            VaultCoreRequest, VaultCreateRequest, VaultLockRequest, VaultState, VaultStatusRequest,
            VaultStoreRequest, VaultUnlockRequest,
        };

        const PASSWORD: &[u8] = b"correct-horse-battery";
        const TOKEN: &[u8] = b"sk-ant-oat01-TOKEN-MARKER-91b2";
        let sandbox = Sandbox::new("vault");
        let db = test_db_path(&sandbox.root);
        let endpoint = sandbox.endpoint.clone();
        let config = DaemonConfig {
            server_config: ServerConfig::g7_default(),
            database_path: Some(db.clone()),
            provider_executable: None,
        };
        let handle =
            tokio::spawn(
                async move { run_on_endpoint_with(&endpoint, config, OsPeerSource).await },
            );
        wait_ready(&sandbox.endpoint).await;
        let stream = transport_connect(&sandbox.endpoint).await.expect("connect");
        let mut session = ClientSession::new(
            FramedTransport::new(stream),
            ClientConfig::vault_default(ipc_control::CLIENT_KIND_AGENTCTL),
        );
        session.perform_handshake().await.expect("v7 handshake");
        assert!(session.vault_allowed());
        let mut transport = session.into_inner();
        async fn exchange(
            transport: &mut FramedTransport<UnixStream>,
            request: Request,
        ) -> Response {
            let frame = Bytes::from(
                ControlRequest {
                    payload: Some(control_request::Payload::VaultCore(VaultCoreRequest {
                        payload: Some(request),
                    })),
                }
                .encode_to_vec(),
            );
            transport.write_frame(frame).await.expect("write");
            let reply = transport.read_frame().await.expect("reply");
            assert!(
                !reply.windows(TOKEN.len()).any(|window| window == TOKEN),
                "a response frame carried the stored value"
            );
            match ControlResponse::decode(&reply[..]).expect("decode").payload {
                Some(control_response::Payload::VaultCore(vault)) => {
                    vault.payload.expect("vault payload")
                }
                other => panic!("expected vault_core envelope, got {other:?}"),
            }
        }
        let state = |response: Response| match response {
            Response::Status(status) => status,
            Response::Phrase(phrase) => phrase.status.expect("status"),
            Response::Error(error) => panic!("unexpected vault error: {}", error.message),
        };

        let absent = state(exchange(&mut transport, Request::Status(VaultStatusRequest {})).await);
        assert_eq!(absent.state(), VaultState::VaultAbsent);
        let created = exchange(
            &mut transport,
            Request::Create(VaultCreateRequest {
                password: PASSWORD.to_vec(),
                idle_minutes: 0,
            }),
        )
        .await;
        assert!(matches!(&created, Response::Phrase(phrase) if !phrase.recovery_phrase.is_empty()));
        exchange(
            &mut transport,
            Request::Store(VaultStoreRequest {
                password: PASSWORD.to_vec(),
                name: vault::PROVIDER_CREDENTIAL_ENTRY.to_owned(),
                value: TOKEN.to_vec(),
            }),
        )
        .await;
        let locked = state(exchange(&mut transport, Request::Lock(VaultLockRequest {})).await);
        assert_eq!(locked.state(), VaultState::VaultLocked);
        let unlocked = state(
            exchange(
                &mut transport,
                Request::Unlock(VaultUnlockRequest {
                    password: PASSWORD.to_vec(),
                    idle_minutes: 5,
                }),
            )
            .await,
        );

        assert_eq!(unlocked.state(), VaultState::VaultUnlocked);
        assert_eq!(unlocked.entries.len(), 1);
        assert_eq!(unlocked.entries[0].name, vault::PROVIDER_CREDENTIAL_ENTRY);
        let file = db.with_file_name(vault::VAULT_FILE_NAME);
        let mode = std::fs::metadata(&file)
            .expect("vault file")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        let bytes = std::fs::read(&file).expect("read vault");
        assert!(!bytes.windows(TOKEN.len()).any(|window| window == TOKEN));
        drop(transport);
        shutdown_and_await(handle, &sandbox.endpoint).await;
    }

    #[tokio::test]
    async fn database_failure_fails_startup_clearly() {
        // A regular file where the data directory must be: fail closed,
        // never repaired, never bypassed.
        let sandbox = Sandbox::new("dbfail");
        let blocker = sandbox.root.join("blocker");
        std::fs::write(&blocker, b"not a dir").expect("setup");
        let db = blocker.join("state.sqlite3");
        match run_on_endpoint(&sandbox.endpoint, DaemonConfig::with_database(&db)).await {
            Err(DaemonError::DataDir(_)) => {}
            other => panic!("expected DataDir failure, got {other:?}"),
        }
        // Corrupt database content fails as storage failure, never as a
        // silently created alternate database.
        let dir = sandbox.root.join("data");
        std::fs::create_dir(&dir).expect("setup");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).expect("setup");
        let db = dir.join("state.sqlite3");
        std::fs::write(&db, b"definitely-not-sqlite").expect("setup");
        match run_on_endpoint(&sandbox.endpoint, DaemonConfig::with_database(&db)).await {
            Err(DaemonError::Storage(_)) => {}
            other => panic!("expected Storage failure, got {other:?}"),
        }
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
