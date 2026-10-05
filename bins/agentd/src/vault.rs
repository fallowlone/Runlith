//! ADR-0039 operator credential vault service behind protocol v7
//! `vault_core`.
//!
//! The daemon owns the single vault file next to its database. Requests are
//! handled one at a time (the caller holds the service lock), so at most one
//! Argon2id derivation runs at once. Failed secret checks are rate limited
//! in memory; an idle deadline locks the vault (see [`VaultService::expire_if_idle`]).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, TryLockError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ipc_contracts::proto::{
    vault_core_request, vault_core_response, VaultCoreRequest, VaultCoreResponse, VaultEntryDto,
    VaultError as WireError, VaultErrorCode, VaultPhraseResponse, VaultState, VaultStatusResponse,
};
use secrets::{
    SecretBackend, SecretBackendError, SecretBinding, SecretBindingId, SecretDelivery,
    SecretDeliveryError, SecretDeliveryTarget, SecretMetadataError, SecretMetadataStore, SecretRef,
};
use secrets_vault::{Password, RecoveryPhrase, VaultBackend, VaultError};
use zeroize::Zeroizing;

/// Vault file, in the daemon data directory beside the database.
pub const VAULT_FILE_NAME: &str = "vault.rlv";

use ipc_contracts::PROVIDER_CREDENTIAL_LIFETIME_DAYS;
pub use ipc_contracts::VAULT_PROVIDER_CREDENTIAL_ENTRY as PROVIDER_CREDENTIAL_ENTRY;

const ALLOWED_ENTRIES: [&str; 1] = [PROVIDER_CREDENTIAL_ENTRY];
const DEFAULT_IDLE_MINUTES: u32 = 30;
const MAX_IDLE_MINUTES: u32 = 720;
/// Failed secret checks allowed before backoff starts.
const FREE_FAILURES: u32 = 3;
const MAX_BACKOFF: Duration = Duration::from_secs(300);

/// How often the idle watch checks the deadline; the vault locks at most
/// this long after its deadline.
const IDLE_CHECK_INTERVAL: Duration = Duration::from_secs(5);

const SECONDS_PER_DAY: u64 = 86_400;

pub type VaultHandle = Arc<Mutex<VaultService>>;

/// Vault path for a database-backed daemon; `None` in control-only mode.
pub fn vault_path(database_path: Option<&Path>) -> Option<PathBuf> {
    database_path.map(|db| db.with_file_name(VAULT_FILE_NAME))
}

pub fn open_handle(path: PathBuf) -> VaultHandle {
    Arc::new(Mutex::new(VaultService::new(path)))
}

/// Handles one request off the async runtime: key derivation and file I/O
/// block, and the service lock serializes them across connections.
pub async fn dispatch(handle: VaultHandle, request: VaultCoreRequest) -> VaultCoreResponse {
    tokio::task::spawn_blocking(move || service(&handle).handle(request, Instant::now()))
        .await
        .unwrap_or_else(|_| crate::dispatch::vault_unavailable())
}

/// Locks the vault after its idle deadline. A request in progress keeps the
/// service busy; the next tick checks again.
pub async fn idle_watch(handle: VaultHandle) {
    let mut ticks = tokio::time::interval(IDLE_CHECK_INTERVAL);
    loop {
        ticks.tick().await;
        let mut service = match handle.try_lock() {
            Ok(service) => service,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => continue,
        };
        service.expire_if_idle(Instant::now());
    }
}

pub fn lock_handle(handle: &VaultHandle) {
    service(handle).lock();
}

/// The service stays usable after a panic elsewhere: locking must never
/// be refused because of a poisoned mutex.
fn service(handle: &VaultHandle) -> MutexGuard<'_, VaultService> {
    handle.lock().unwrap_or_else(PoisonError::into_inner)
}

pub struct VaultService {
    path: PathBuf,
    vault: Option<VaultBackend>,
    idle: Duration,
    idle_deadline: Option<Instant>,
    failures: u32,
    retry_at: Option<Instant>,
    /// `created_at` of the provider credential last released to a launch.
    released: Option<u64>,
    /// `created_at` of a provider credential the provider refused; it stays
    /// refused until a new credential is stored (or the daemon restarts).
    rejected: Option<u64>,
}

impl std::fmt::Debug for VaultService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VaultService")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl VaultService {
    /// Opening is lazy: a missing or unreadable file is reported per request,
    /// never as a daemon startup failure.
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            vault: None,
            idle: minutes(DEFAULT_IDLE_MINUTES),
            idle_deadline: None,
            failures: 0,
            retry_at: None,
            released: None,
            rejected: None,
        }
    }

    pub fn handle(&mut self, request: VaultCoreRequest, now: Instant) -> VaultCoreResponse {
        use vault_core_request::Payload as Request;

        self.expire_if_idle(now);
        let outcome = match request.payload {
            None => Err(invalid("empty vault request")),
            Some(Request::Status(_)) => self.status(now).map(Response::Status),
            Some(Request::Create(create)) => self.create(create, now),
            Some(Request::Unlock(unlock)) => self.unlock(unlock, now),
            Some(Request::Lock(_)) => {
                self.lock();
                self.status(now).map(Response::Status)
            }
            Some(Request::Store(store)) => self.store(store, now),
            Some(Request::ChangePassword(change)) => self.change_password(change, now),
            Some(Request::Recover(recover)) => self.recover(recover, now),
            Some(Request::Rotate(rotate)) => self.rotate(rotate, now),
        };
        VaultCoreResponse {
            payload: Some(outcome.unwrap_or_else(Response::Error)),
        }
    }

    /// Locks the vault once its idle deadline has passed.
    pub fn expire_if_idle(&mut self, now: Instant) {
        if self.idle_deadline.is_some_and(|deadline| now >= deadline) {
            self.lock();
        }
    }

    /// Drops the data key (idle expiry, explicit lock, daemon shutdown).
    pub fn lock(&mut self) {
        if let Some(vault) = &self.vault {
            vault.lock();
        }
        self.idle_deadline = None;
    }

    /// Brokered launch preflight (ADR-0039 decision 7): the vault exists and
    /// is unlocked, the provider credential is stored and younger than its
    /// lifetime. Reads names and creation times only, never the value.
    pub fn check_provider_credential(
        &mut self,
        now: Instant,
        now_s: u64,
    ) -> Result<(), CredentialRefusal> {
        self.expire_if_idle(now);
        let vault = self
            .backend()
            .map_err(|_| CredentialRefusal::VaultUnavailable)?
            .ok_or(CredentialRefusal::VaultAbsent)?;
        if !vault.is_unlocked() {
            return Err(CredentialRefusal::VaultLocked);
        }
        let entries = vault.entries().map_err(|failure| match failure {
            VaultError::Locked => CredentialRefusal::VaultLocked,
            _ => CredentialRefusal::VaultUnavailable,
        })?;
        let credential = entries
            .iter()
            .find(|entry| entry.name == PROVIDER_CREDENTIAL_ENTRY)
            .ok_or(CredentialRefusal::Missing)?;
        if self.rejected == Some(credential.created_at) {
            return Err(CredentialRefusal::Rejected);
        }
        // Same day count as `agentctl vault status`: expired from day 365.
        let age_days = now_s.saturating_sub(credential.created_at) / SECONDS_PER_DAY;
        if age_days >= PROVIDER_CREDENTIAL_LIFETIME_DAYS {
            return Err(CredentialRefusal::Expired);
        }
        Ok(())
    }

    /// Loads the provider credential for one brokered launch and extends the
    /// idle deadline: a release counts as vault use.
    pub fn release_provider_credential(
        &mut self,
        now: Instant,
    ) -> Result<Vec<u8>, SecretBackendError> {
        self.expire_if_idle(now);
        let credential =
            SecretRef::new(PROVIDER_CREDENTIAL_ENTRY).map_err(|_| SecretBackendError::Failure)?;
        let vault = self
            .backend()
            .map_err(|_| SecretBackendError::Unavailable)?
            .ok_or(SecretBackendError::NotFound)?;
        let value = vault.load(&credential)?;
        self.released = vault.entries().ok().and_then(|entries| {
            entries
                .into_iter()
                .find(|entry| entry.name == PROVIDER_CREDENTIAL_ENTRY)
                .map(|entry| entry.created_at)
        });
        self.idle_deadline = Some(now + self.idle);
        Ok(value)
    }

    /// The provider refused the credential last released (ADR-0039
    /// decision 7): later launches refuse until a new one is stored. Never
    /// retried automatically.
    pub fn mark_provider_credential_rejected(&mut self) {
        if let Some(created_at) = self.released {
            self.rejected = Some(created_at);
        }
    }

    fn status(&mut self, now: Instant) -> Result<VaultStatusResponse, WireError> {
        let Some(vault) = self.backend()? else {
            return Ok(VaultStatusResponse {
                state: VaultState::VaultAbsent as i32,
                entries: Vec::new(),
                unlocked_until_ms: None,
            });
        };
        if !vault.is_unlocked() {
            return Ok(VaultStatusResponse {
                state: VaultState::VaultLocked as i32,
                entries: Vec::new(),
                unlocked_until_ms: None,
            });
        }
        let entries = vault
            .entries()
            .map_err(wire)?
            .into_iter()
            .map(|entry| VaultEntryDto {
                rejected: entry.name == PROVIDER_CREDENTIAL_ENTRY
                    && self.rejected == Some(entry.created_at),
                name: entry.name,
                created_at_s: entry.created_at,
            })
            .collect();
        let unlocked_until_ms = self.idle_deadline.map(|deadline| {
            unix_millis(SystemTime::now() + deadline.saturating_duration_since(now))
        });
        Ok(VaultStatusResponse {
            state: VaultState::VaultUnlocked as i32,
            entries,
            unlocked_until_ms,
        })
    }

    fn create(
        &mut self,
        request: ipc_contracts::proto::VaultCreateRequest,
        now: Instant,
    ) -> Outcome {
        let idle = idle_from(request.idle_minutes)?;
        let password = password(request.password)?;
        let (vault, phrase) = create_backend(&self.path, &password).map_err(wire)?;
        self.vault = Some(vault);
        self.idle = idle;
        self.idle_deadline = Some(now + idle);
        self.phrase_response(&phrase, now)
    }

    fn unlock(
        &mut self,
        request: ipc_contracts::proto::VaultUnlockRequest,
        now: Instant,
    ) -> Outcome {
        let idle = idle_from(request.idle_minutes)?;
        let password = password(request.password)?;
        self.checked(now, |vault| vault.unlock(&password))?;
        self.idle = idle;
        self.status_after_unlock(now)
    }

    /// Re-checks the password even though the vault is unlocked, so a
    /// same-user IPC peer cannot replace the credential without it.
    fn store(&mut self, request: ipc_contracts::proto::VaultStoreRequest, now: Instant) -> Outcome {
        let value = Zeroizing::new(request.value);
        let password = password(request.password)?;
        if !ALLOWED_ENTRIES.contains(&request.name.as_str()) {
            return Err(error(
                VaultErrorCode::VaultErrorEntryNotAllowed,
                "entry name is not allowed",
            ));
        }
        if value.is_empty() {
            return Err(invalid("entry value is empty"));
        }
        let name = SecretRef::new(request.name).map_err(|_| invalid("entry name is invalid"))?;
        if !self.backend()?.is_some_and(VaultBackend::is_unlocked) {
            return Err(wire(VaultError::Locked));
        }
        self.checked(now, |vault| vault.unlock(&password))?;
        self.backend()?
            .ok_or_else(|| wire(VaultError::NotFound))?
            .store(&name, &value)
            .map_err(backend_wire)?;
        self.rejected = None;
        self.status_after_unlock(now)
    }

    fn change_password(
        &mut self,
        request: ipc_contracts::proto::VaultChangePasswordRequest,
        now: Instant,
    ) -> Outcome {
        let current = password(request.current_password)?;
        let new = password(request.new_password)?;
        self.checked(now, |vault| vault.change_password(&current, &new))?;
        self.status_after_unlock(now)
    }

    fn recover(
        &mut self,
        request: ipc_contracts::proto::VaultRecoverRequest,
        now: Instant,
    ) -> Outcome {
        let idle = idle_from(request.idle_minutes)?;
        let phrase = phrase(request.recovery_phrase)?;
        let new = password(request.new_password)?;
        self.checked(now, |vault| vault.recover(&phrase, &new))?;
        self.idle = idle;
        self.status_after_unlock(now)
    }

    fn rotate(
        &mut self,
        request: ipc_contracts::proto::VaultRotateRequest,
        now: Instant,
    ) -> Outcome {
        let password = password(request.password)?;
        let phrase = self.checked(now, |vault| {
            vault.rotate(&password, secrets_vault::DEFAULT_PHRASE_WORDS)
        })?;
        self.idle_deadline = Some(now + self.idle);
        self.phrase_response(&phrase, now)
    }

    /// Runs one secret check on an existing vault under the rate limit: a
    /// limited attempt is refused before any derivation; a wrong secret
    /// counts as a failure; success resets the counter.
    fn checked<T>(
        &mut self,
        now: Instant,
        check: impl FnOnce(&VaultBackend) -> Result<T, VaultError>,
    ) -> Result<T, WireError> {
        if let Some(retry_at) = self.retry_at.filter(|retry_at| now < *retry_at) {
            return Err(WireError {
                code: VaultErrorCode::VaultErrorRateLimited as i32,
                message: "too many failed attempts; retry later".to_owned(),
                retry_after_ms: Some(
                    u64::try_from((retry_at - now).as_millis()).unwrap_or(u64::MAX),
                ),
            });
        }
        let vault = self.backend()?.ok_or_else(|| wire(VaultError::NotFound))?;
        match check(vault) {
            Ok(value) => {
                self.failures = 0;
                self.retry_at = None;
                Ok(value)
            }
            Err(VaultError::UnlockFailed) => {
                self.failures = self.failures.saturating_add(1);
                if let Some(exponent) = self.failures.checked_sub(FREE_FAILURES) {
                    let delay = 1_u64.checked_shl(exponent).unwrap_or(u64::MAX);
                    self.retry_at = Some(now + Duration::from_secs(delay).min(MAX_BACKOFF));
                }
                Err(wire(VaultError::UnlockFailed))
            }
            Err(other) => Err(wire(other)),
        }
    }

    fn status_after_unlock(&mut self, now: Instant) -> Outcome {
        self.idle_deadline = Some(now + self.idle);
        self.status(now).map(Response::Status)
    }

    fn phrase_response(&mut self, phrase: &RecoveryPhrase, now: Instant) -> Outcome {
        Ok(Response::Phrase(VaultPhraseResponse {
            recovery_phrase: phrase.expose().as_bytes().to_vec(),
            status: Some(self.status(now)?),
        }))
    }

    /// The vault, opened on first use; `None` while no file exists.
    fn backend(&mut self) -> Result<Option<&VaultBackend>, WireError> {
        if self.vault.is_none() {
            match VaultBackend::open(&self.path) {
                Ok(vault) => self.vault = Some(vault),
                Err(VaultError::NotFound) => return Ok(None),
                Err(other) => return Err(wire(other)),
            }
        }
        Ok(self.vault.as_ref())
    }
}

/// Why a brokered provider launch is refused before any provider work. Each
/// cause names the operator action; there is no fallback to the host login.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CredentialRefusal {
    VaultAbsent,
    VaultUnavailable,
    VaultLocked,
    Missing,
    Expired,
    Rejected,
}

impl CredentialRefusal {
    pub fn message(self) -> &'static str {
        match self {
            Self::VaultAbsent => {
                "provider_credential_vault_absent: no operator vault exists; run \
                 `agentctl vault init`, then `agentctl vault store`"
            }
            Self::VaultUnavailable => {
                "provider_credential_vault_unavailable: the operator vault cannot be read; \
                 run `agentctl vault status`"
            }
            Self::VaultLocked => {
                "provider_credential_vault_locked: the operator vault is locked; run \
                 `agentctl vault unlock`"
            }
            Self::Missing => {
                "provider_credential_missing: no provider credential is stored; create one \
                 with `claude setup-token` and run `agentctl vault store`"
            }
            Self::Expired => {
                "provider_credential_expired: the stored provider credential is past its \
                 one-year lifetime; create a new one with `claude setup-token` and run \
                 `agentctl vault store`"
            }
            Self::Rejected => {
                "provider_credential_rejected: the provider refused the stored credential; \
                 create a new one with `claude setup-token` and run `agentctl vault store`"
            }
        }
    }
}

/// Launch-scoped secret metadata for one brokered release: the provider
/// credential reference and its single binding, never persisted.
#[derive(Default)]
pub struct LaunchSecretMetadata {
    refs: Vec<SecretRef>,
    bindings: Vec<SecretBinding>,
}

impl SecretMetadataStore for LaunchSecretMetadata {
    fn ensure_ref(&mut self, secret: &SecretRef) -> Result<(), SecretMetadataError> {
        if !self.refs.contains(secret) {
            self.refs.push(secret.clone());
        }
        Ok(())
    }

    fn has_ref(&self, secret: &SecretRef) -> Result<bool, SecretMetadataError> {
        Ok(self.refs.contains(secret))
    }

    fn put_binding(&mut self, binding: &SecretBinding) -> Result<(), SecretMetadataError> {
        if self.bindings.iter().any(|known| known.id() == binding.id()) {
            return Err(SecretMetadataError::Conflict);
        }
        self.bindings.push(binding.clone());
        Ok(())
    }

    fn get_binding(
        &self,
        id: &SecretBindingId,
    ) -> Result<Option<SecretBinding>, SecretMetadataError> {
        Ok(self.bindings.iter().find(|known| known.id() == id).cloned())
    }
}

/// Read-only view of the daemon vault for the Secrets Broker. Only the
/// provider credential can be loaded; writes go through `vault_core`.
pub struct VaultCredentialBackend(pub VaultHandle);

impl SecretBackend for VaultCredentialBackend {
    fn store(&self, _secret: &SecretRef, _value: &[u8]) -> Result<(), SecretBackendError> {
        Err(SecretBackendError::Rejected)
    }

    fn load(&self, secret: &SecretRef) -> Result<Vec<u8>, SecretBackendError> {
        if secret.as_str() != PROVIDER_CREDENTIAL_ENTRY {
            return Err(SecretBackendError::NotFound);
        }
        service(&self.0).release_provider_credential(Instant::now())
    }

    fn delete(&self, _secret: &SecretRef) -> Result<(), SecretBackendError> {
        Err(SecretBackendError::Rejected)
    }
}

/// Delivers the released credential into the late-bound child secret slot.
#[derive(Default)]
pub struct ProviderCredentialDelivery(pub runner::ChildSecretEnvironment);

impl SecretDelivery for ProviderCredentialDelivery {
    fn deliver(
        &mut self,
        _target: &SecretDeliveryTarget,
        value: &[u8],
    ) -> Result<(), SecretDeliveryError> {
        self.0
            .insert(provider_claude::OAUTH_TOKEN_ENV, value.to_vec())
            .map_err(|_| SecretDeliveryError::Rejected)
    }
}

/// Records a provider refusal of the released credential, if a vault exists.
pub fn mark_provider_credential_rejected(handle: Option<&VaultHandle>) {
    if let Some(handle) = handle {
        service(handle).mark_provider_credential_rejected();
    }
}

/// Preflight for a brokered launch against the daemon vault, if any.
pub fn check_provider_credential(
    handle: Option<&VaultHandle>,
    now_s: u64,
) -> Result<(), CredentialRefusal> {
    let handle = handle.ok_or(CredentialRefusal::VaultAbsent)?;
    service(handle).check_provider_credential(Instant::now(), now_s)
}

pub fn unix_now_s() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

type Response = vault_core_response::Payload;
type Outcome = Result<Response, WireError>;

fn idle_from(idle_minutes: u32) -> Result<Duration, WireError> {
    match idle_minutes {
        0 => Ok(minutes(DEFAULT_IDLE_MINUTES)),
        1..=MAX_IDLE_MINUTES => Ok(minutes(idle_minutes)),
        _ => Err(invalid("idle_minutes is out of range (1-720)")),
    }
}

/// Takes ownership of the wire bytes so they are zeroized with the
/// `Password` (or here, when they are not UTF-8).
fn password(bytes: Vec<u8>) -> Result<Password, WireError> {
    let text = String::from_utf8(bytes).map_err(|error| {
        drop(Zeroizing::new(error.into_bytes()));
        invalid("password is not UTF-8")
    })?;
    Password::new(text).map_err(wire)
}

fn phrase(bytes: Vec<u8>) -> Result<RecoveryPhrase, WireError> {
    let bytes = Zeroizing::new(bytes);
    let text = std::str::from_utf8(&bytes).map_err(|_| invalid("recovery phrase is not UTF-8"))?;
    RecoveryPhrase::parse(text).map_err(wire)
}

fn wire(failure: VaultError) -> WireError {
    let code = match failure {
        VaultError::WeakPassword => VaultErrorCode::VaultErrorWeakPassword,
        VaultError::InvalidPhrase => VaultErrorCode::VaultErrorInvalidPhrase,
        VaultError::InvalidParams | VaultError::Corrupt => VaultErrorCode::VaultErrorCorrupt,
        VaultError::UnlockFailed => VaultErrorCode::VaultErrorUnlockFailed,
        VaultError::EntryRejected => VaultErrorCode::VaultErrorEntryRejected,
        VaultError::Unavailable | VaultError::Io => VaultErrorCode::VaultErrorUnavailable,
        VaultError::Locked => VaultErrorCode::VaultErrorLocked,
        VaultError::AlreadyExists => VaultErrorCode::VaultErrorAlreadyExists,
        VaultError::NotFound => VaultErrorCode::VaultErrorAbsent,
    };
    error(code, &failure.to_string())
}

fn backend_wire(failure: SecretBackendError) -> WireError {
    let code = match failure {
        SecretBackendError::Locked => VaultErrorCode::VaultErrorLocked,
        SecretBackendError::Rejected => VaultErrorCode::VaultErrorEntryRejected,
        SecretBackendError::Corrupt => VaultErrorCode::VaultErrorCorrupt,
        SecretBackendError::NotFound
        | SecretBackendError::Unavailable
        | SecretBackendError::Failure => VaultErrorCode::VaultErrorUnavailable,
    };
    error(code, &failure.to_string())
}

fn invalid(message: &str) -> WireError {
    error(VaultErrorCode::VaultErrorInvalidRequest, message)
}

fn error(code: VaultErrorCode, message: &str) -> WireError {
    WireError {
        code: code as i32,
        message: message.to_owned(),
        retry_after_ms: None,
    }
}

fn unix_millis(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

fn minutes(count: u32) -> Duration {
    Duration::from_secs(u64::from(count) * 60)
}

#[cfg(not(test))]
fn create_backend(
    path: &Path,
    password: &Password,
) -> Result<(VaultBackend, RecoveryPhrase), VaultError> {
    VaultBackend::create(path, password, secrets_vault::DEFAULT_PHRASE_WORDS)
}

/// Unit tests use cheap KDF parameters; the stored file still says so, so
/// every later unlock of it is cheap too.
#[cfg(test)]
fn create_backend(
    path: &Path,
    password: &Password,
) -> Result<(VaultBackend, RecoveryPhrase), VaultError> {
    VaultBackend::create_with_test_params(path, password, secrets_vault::DEFAULT_PHRASE_WORDS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost::Message;
    use vault_core_request::Payload as Request;
    use vault_core_response::Payload as Response;

    const PASSWORD: &str = "correct-horse-battery";
    const TOKEN: &[u8] = b"sk-ant-oat01-TOKEN-MARKER-5c1e";

    fn service() -> (tempfile::TempDir, VaultService) {
        let dir = tempfile::tempdir().unwrap();
        let service = VaultService::new(dir.path().join(VAULT_FILE_NAME));
        (dir, service)
    }

    fn send(service: &mut VaultService, request: Request, now: Instant) -> Response {
        service
            .handle(
                VaultCoreRequest {
                    payload: Some(request),
                },
                now,
            )
            .payload
            .expect("response payload")
    }

    fn create(password: &str) -> Request {
        Request::Create(ipc_contracts::proto::VaultCreateRequest {
            password: password.as_bytes().to_vec(),
            idle_minutes: 0,
        })
    }

    fn unlock(password: &str, idle_minutes: u32) -> Request {
        Request::Unlock(ipc_contracts::proto::VaultUnlockRequest {
            password: password.as_bytes().to_vec(),
            idle_minutes,
        })
    }

    fn store(password: &str, name: &str, value: &[u8]) -> Request {
        Request::Store(ipc_contracts::proto::VaultStoreRequest {
            password: password.as_bytes().to_vec(),
            name: name.to_owned(),
            value: value.to_vec(),
        })
    }

    fn status() -> Request {
        Request::Status(ipc_contracts::proto::VaultStatusRequest {})
    }

    fn lock() -> Request {
        Request::Lock(ipc_contracts::proto::VaultLockRequest {})
    }

    fn state_of(response: Response) -> VaultStatusResponse {
        match response {
            Response::Status(status) => status,
            other => panic!("expected status, got {other:?}"),
        }
    }

    fn error_of(response: Response) -> WireError {
        match response {
            Response::Error(error) => error,
            other => panic!("expected error, got {other:?}"),
        }
    }

    fn code(response: Response) -> VaultErrorCode {
        VaultErrorCode::try_from(error_of(response).code).unwrap()
    }

    /// A created vault, then locked, as the daemon finds it after a restart.
    fn locked_service(now: Instant) -> (tempfile::TempDir, VaultService) {
        let (dir, mut service) = service();
        send(&mut service, create(PASSWORD), now);
        send(&mut service, lock(), now);
        (dir, service)
    }

    #[test]
    fn absent_vault_reports_absent_and_refuses_unlock() {
        let (_dir, mut service) = service();
        let now = Instant::now();

        let status = state_of(send(&mut service, status(), now));

        assert_eq!(status.state(), VaultState::VaultAbsent);
        assert_eq!(
            code(send(&mut service, unlock(PASSWORD, 0), now)),
            VaultErrorCode::VaultErrorAbsent
        );
    }

    #[test]
    fn create_returns_the_phrase_once_and_leaves_the_vault_unlocked() {
        let (_dir, mut service) = service();
        let now = Instant::now();

        let Response::Phrase(created) = send(&mut service, create(PASSWORD), now) else {
            panic!("create answers with the phrase");
        };

        let words = String::from_utf8(created.recovery_phrase).unwrap();
        assert_eq!(
            words.split(' ').count(),
            secrets_vault::DEFAULT_PHRASE_WORDS
        );
        let status = created.status.expect("status");
        assert_eq!(status.state(), VaultState::VaultUnlocked);
        assert!(status.entries.is_empty());
        assert!(status.unlocked_until_ms.is_some());
        assert_eq!(
            code(send(&mut service, create(PASSWORD), now)),
            VaultErrorCode::VaultErrorAlreadyExists
        );
    }

    #[test]
    fn create_refuses_a_weak_password_and_bad_idle_minutes() {
        let (_dir, mut service) = service();
        let now = Instant::now();

        assert_eq!(
            code(send(&mut service, create("short"), now)),
            VaultErrorCode::VaultErrorWeakPassword
        );
        let too_long = Request::Create(ipc_contracts::proto::VaultCreateRequest {
            password: PASSWORD.as_bytes().to_vec(),
            idle_minutes: MAX_IDLE_MINUTES + 1,
        });
        assert_eq!(
            code(send(&mut service, too_long, now)),
            VaultErrorCode::VaultErrorInvalidRequest
        );
        assert_eq!(
            state_of(send(&mut service, status(), now)).state(),
            VaultState::VaultAbsent
        );
    }

    #[test]
    fn store_needs_an_allowlisted_name_the_password_and_an_unlocked_vault() {
        let (_dir, mut service) = locked_service(Instant::now());
        let now = Instant::now();

        assert_eq!(
            code(send(
                &mut service,
                store(PASSWORD, PROVIDER_CREDENTIAL_ENTRY, TOKEN),
                now
            )),
            VaultErrorCode::VaultErrorLocked
        );
        send(&mut service, unlock(PASSWORD, 0), now);
        assert_eq!(
            code(send(
                &mut service,
                store(PASSWORD, "other.secret", TOKEN),
                now
            )),
            VaultErrorCode::VaultErrorEntryNotAllowed
        );
        assert_eq!(
            code(send(
                &mut service,
                store("incorrect-horse", PROVIDER_CREDENTIAL_ENTRY, TOKEN),
                now
            )),
            VaultErrorCode::VaultErrorUnlockFailed
        );
        assert_eq!(
            code(send(
                &mut service,
                store(PASSWORD, PROVIDER_CREDENTIAL_ENTRY, b""),
                now
            )),
            VaultErrorCode::VaultErrorInvalidRequest
        );

        let stored = send(
            &mut service,
            store(PASSWORD, PROVIDER_CREDENTIAL_ENTRY, TOKEN),
            now,
        );

        let wire = VaultCoreResponse {
            payload: Some(stored.clone()),
        }
        .encode_to_vec();
        assert!(!wire.windows(TOKEN.len()).any(|window| window == TOKEN));
        let entries = state_of(stored).entries;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, PROVIDER_CREDENTIAL_ENTRY);
        assert!(entries[0].created_at_s > 0);
    }

    /// Three free failures, then doubling backoff. A limited attempt is
    /// refused before any derivation, even with the right password.
    #[test]
    fn failed_secret_checks_are_rate_limited() {
        let start = Instant::now();
        let (_dir, mut service) = locked_service(start);

        for _ in 0..FREE_FAILURES {
            assert_eq!(
                code(send(&mut service, unlock("incorrect-horse", 0), start)),
                VaultErrorCode::VaultErrorUnlockFailed
            );
        }
        let limited = error_of(send(&mut service, unlock(PASSWORD, 0), start));
        assert_eq!(limited.code(), VaultErrorCode::VaultErrorRateLimited);
        assert_eq!(limited.retry_after_ms, Some(1_000));

        let later = start + Duration::from_secs(1);
        assert_eq!(
            code(send(&mut service, unlock("incorrect-horse", 0), later)),
            VaultErrorCode::VaultErrorUnlockFailed
        );
        let limited = error_of(send(&mut service, unlock(PASSWORD, 0), later));
        assert_eq!(limited.retry_after_ms, Some(2_000));

        let after = later + Duration::from_secs(2);
        let status = state_of(send(&mut service, unlock(PASSWORD, 0), after));
        assert_eq!(status.state(), VaultState::VaultUnlocked);
        assert_eq!(
            code(send(&mut service, unlock("incorrect-horse", 0), after)),
            VaultErrorCode::VaultErrorUnlockFailed,
            "success resets the counter"
        );
    }

    #[test]
    fn backoff_is_capped() {
        let start = Instant::now();
        let (_dir, mut service) = locked_service(start);
        service.failures = 40;
        service.retry_at = None;

        send(&mut service, unlock("incorrect-horse", 0), start);
        let limited = error_of(send(&mut service, unlock(PASSWORD, 0), start));

        assert_eq!(limited.retry_after_ms, Some(MAX_BACKOFF.as_millis() as u64));
    }

    #[test]
    fn idle_deadline_locks_the_vault() {
        let start = Instant::now();
        let (_dir, mut service) = locked_service(start);
        send(&mut service, unlock(PASSWORD, 1), start);

        service.expire_if_idle(start + Duration::from_secs(59));
        assert_eq!(
            state_of(send(&mut service, status(), start)).state(),
            VaultState::VaultUnlocked
        );
        service.expire_if_idle(start + Duration::from_secs(60));

        assert_eq!(
            state_of(send(&mut service, status(), start)).state(),
            VaultState::VaultLocked
        );
    }

    #[test]
    fn lock_drops_the_key_and_status_hides_entries() {
        let now = Instant::now();
        let (_dir, mut service) = service();
        send(&mut service, create(PASSWORD), now);
        send(
            &mut service,
            store(PASSWORD, PROVIDER_CREDENTIAL_ENTRY, TOKEN),
            now,
        );

        let status = state_of(send(&mut service, lock(), now));

        assert_eq!(status.state(), VaultState::VaultLocked);
        assert!(status.entries.is_empty());
        assert_eq!(status.unlocked_until_ms, None);
    }

    #[test]
    fn rotate_and_recover_issue_and_accept_phrases() {
        let now = Instant::now();
        let (_dir, mut service) = locked_service(now);

        let Response::Phrase(rotated) = send(
            &mut service,
            Request::Rotate(ipc_contracts::proto::VaultRotateRequest {
                password: PASSWORD.as_bytes().to_vec(),
            }),
            now,
        ) else {
            panic!("rotate answers with the new phrase");
        };
        assert_eq!(
            rotated.status.expect("status").state(),
            VaultState::VaultUnlocked
        );
        send(&mut service, lock(), now);

        let recovered = send(
            &mut service,
            Request::Recover(ipc_contracts::proto::VaultRecoverRequest {
                recovery_phrase: rotated.recovery_phrase,
                new_password: b"a-new-long-password".to_vec(),
                idle_minutes: 0,
            }),
            now,
        );

        assert_eq!(state_of(recovered).state(), VaultState::VaultUnlocked);
        send(&mut service, lock(), now);
        assert_eq!(
            code(send(&mut service, unlock(PASSWORD, 0), now)),
            VaultErrorCode::VaultErrorUnlockFailed
        );
    }

    #[test]
    fn change_password_needs_the_current_one() {
        let now = Instant::now();
        let (_dir, mut service) = locked_service(now);
        let change = |current: &str| {
            Request::ChangePassword(ipc_contracts::proto::VaultChangePasswordRequest {
                current_password: current.as_bytes().to_vec(),
                new_password: b"a-new-long-password".to_vec(),
            })
        };

        assert_eq!(
            code(send(&mut service, change("incorrect-horse"), now)),
            VaultErrorCode::VaultErrorUnlockFailed
        );
        let changed = state_of(send(&mut service, change(PASSWORD), now));

        assert_eq!(changed.state(), VaultState::VaultUnlocked);
        send(&mut service, lock(), now);
        assert_eq!(
            state_of(send(&mut service, unlock("a-new-long-password", 0), now)).state(),
            VaultState::VaultUnlocked
        );
    }

    #[test]
    fn empty_request_is_invalid() {
        let (_dir, mut service) = service();

        let response = service.handle(VaultCoreRequest { payload: None }, Instant::now());

        assert_eq!(
            code(response.payload.unwrap()),
            VaultErrorCode::VaultErrorInvalidRequest
        );
    }

    fn stored_service(now: Instant) -> (tempfile::TempDir, VaultService) {
        let (dir, mut service) = service();
        send(&mut service, create(PASSWORD), now);
        send(
            &mut service,
            store(PASSWORD, PROVIDER_CREDENTIAL_ENTRY, TOKEN),
            now,
        );
        (dir, service)
    }

    #[test]
    fn provider_credential_preflight_names_each_refusal() {
        let now = Instant::now();
        let now_s = unix_now_s();
        let (_absent_dir, mut absent) = service();
        assert_eq!(
            absent.check_provider_credential(now, now_s),
            Err(CredentialRefusal::VaultAbsent)
        );
        assert_eq!(
            check_provider_credential(None, now_s),
            Err(CredentialRefusal::VaultAbsent)
        );

        let (_locked_dir, mut locked) = locked_service(now);
        assert_eq!(
            locked.check_provider_credential(now, now_s),
            Err(CredentialRefusal::VaultLocked)
        );

        let (_empty_dir, mut empty) = service();
        send(&mut empty, create(PASSWORD), now);
        assert_eq!(
            empty.check_provider_credential(now, now_s),
            Err(CredentialRefusal::Missing)
        );

        let (_dir, mut stored) = stored_service(now);
        let created = stored.backend().unwrap().unwrap().entries().unwrap()[0].created_at;
        assert_eq!(stored.check_provider_credential(now, created), Ok(()));
        let lifetime = PROVIDER_CREDENTIAL_LIFETIME_DAYS * SECONDS_PER_DAY;
        assert_eq!(
            stored.check_provider_credential(now, created + lifetime - 1),
            Ok(())
        );
        assert_eq!(
            stored.check_provider_credential(now, created + lifetime),
            Err(CredentialRefusal::Expired)
        );

        std::fs::write(&stored.path, b"not a vault").unwrap();
        let mut reopened = VaultService::new(stored.path.clone());
        assert_eq!(
            reopened.check_provider_credential(now, now_s),
            Err(CredentialRefusal::VaultUnavailable)
        );
    }

    #[test]
    fn preflight_applies_the_idle_deadline_first() {
        let start = Instant::now();
        let (_dir, mut service) = stored_service(start);

        assert_eq!(
            service.check_provider_credential(start + minutes(DEFAULT_IDLE_MINUTES), unix_now_s()),
            Err(CredentialRefusal::VaultLocked)
        );
    }

    #[test]
    fn release_returns_the_value_and_extends_the_idle_deadline() {
        let start = Instant::now();
        let (_dir, mut service) = stored_service(start);
        let later = start + minutes(20);

        let value = service.release_provider_credential(later).unwrap();

        assert_eq!(value, TOKEN);
        service.expire_if_idle(start + minutes(DEFAULT_IDLE_MINUTES));
        assert_eq!(
            service.check_provider_credential(start + minutes(DEFAULT_IDLE_MINUTES), unix_now_s()),
            Ok(())
        );
        service.expire_if_idle(later + minutes(DEFAULT_IDLE_MINUTES));
        assert_eq!(
            service.release_provider_credential(later + minutes(DEFAULT_IDLE_MINUTES)),
            Err(SecretBackendError::Locked)
        );
    }

    #[test]
    fn release_refuses_an_absent_or_locked_vault() {
        let now = Instant::now();
        let (_absent_dir, mut absent) = service();
        assert_eq!(
            absent.release_provider_credential(now),
            Err(SecretBackendError::NotFound)
        );
        let (_dir, mut locked) = locked_service(now);
        assert_eq!(
            locked.release_provider_credential(now),
            Err(SecretBackendError::Locked)
        );
    }

    #[test]
    fn credential_backend_is_read_only_and_serves_one_name() {
        let now = Instant::now();
        let (_dir, stored) = stored_service(now);
        let backend = VaultCredentialBackend(Arc::new(Mutex::new(stored)));
        let credential = SecretRef::new(PROVIDER_CREDENTIAL_ENTRY).unwrap();
        let other = SecretRef::new("provider.other.token").unwrap();

        assert_eq!(backend.load(&credential).unwrap(), TOKEN);
        assert_eq!(backend.load(&other), Err(SecretBackendError::NotFound));
        assert_eq!(
            backend.store(&credential, b"replacement"),
            Err(SecretBackendError::Rejected)
        );
        assert_eq!(
            backend.delete(&credential),
            Err(SecretBackendError::Rejected)
        );
        assert_eq!(backend.load(&credential).unwrap(), TOKEN);
    }

    #[test]
    fn rejected_credential_refuses_until_a_new_one_is_stored() {
        let now = Instant::now();
        let (_dir, mut service) = stored_service(now);
        service.mark_provider_credential_rejected();
        assert_eq!(service.check_provider_credential(now, unix_now_s()), Ok(()));

        service.release_provider_credential(now).unwrap();
        service.mark_provider_credential_rejected();
        assert_eq!(
            service.check_provider_credential(now, unix_now_s()),
            Err(CredentialRefusal::Rejected)
        );

        let rotated = send(
            &mut service,
            Request::Rotate(ipc_contracts::proto::VaultRotateRequest {
                password: PASSWORD.as_bytes().to_vec(),
            }),
            now,
        );
        assert!(matches!(rotated, Response::Phrase(_)));
        assert_eq!(
            service.check_provider_credential(now, unix_now_s()),
            Err(CredentialRefusal::Rejected),
            "rotation keeps the same token, so it stays refused"
        );

        send(
            &mut service,
            store(
                PASSWORD,
                PROVIDER_CREDENTIAL_ENTRY,
                b"sk-ant-oat01-REPLACEMENT",
            ),
            now,
        );
        assert_eq!(service.check_provider_credential(now, unix_now_s()), Ok(()));
    }

    #[test]
    fn status_marks_the_rejected_provider_credential() {
        let now = Instant::now();
        let (_dir, mut service) = stored_service(now);

        let fresh = state_of(send(&mut service, status(), now));
        assert_eq!(fresh.state(), VaultState::VaultUnlocked);
        let fresh_entry = fresh
            .entries
            .iter()
            .find(|entry| entry.name == PROVIDER_CREDENTIAL_ENTRY)
            .expect("provider entry");
        assert!(!fresh_entry.rejected);

        service.release_provider_credential(now).unwrap();
        service.mark_provider_credential_rejected();

        let marked = state_of(send(&mut service, status(), now));
        let marked_entry = marked
            .entries
            .iter()
            .find(|entry| entry.name == PROVIDER_CREDENTIAL_ENTRY)
            .expect("provider entry");
        assert!(marked_entry.rejected);
    }

    #[test]
    fn status_clears_the_rejected_mark_after_a_new_credential_is_stored() {
        let now = Instant::now();
        let (_dir, mut service) = stored_service(now);
        service.release_provider_credential(now).unwrap();
        service.mark_provider_credential_rejected();

        send(
            &mut service,
            store(PASSWORD, PROVIDER_CREDENTIAL_ENTRY, b"[REDACTED]"),
            now,
        );

        let status = state_of(send(&mut service, status(), now));
        assert!(!status.entries.iter().any(|entry| entry.rejected));
    }

    #[test]
    fn status_keeps_the_rejected_mark_across_rotation() {
        let now = Instant::now();
        let (_dir, mut service) = stored_service(now);
        service.release_provider_credential(now).unwrap();
        service.mark_provider_credential_rejected();

        let rotated = send(
            &mut service,
            Request::Rotate(ipc_contracts::proto::VaultRotateRequest {
                password: PASSWORD.as_bytes().to_vec(),
            }),
            now,
        );
        assert!(matches!(rotated, Response::Phrase(_)));

        let status = state_of(send(&mut service, status(), now));
        let entry = status
            .entries
            .iter()
            .find(|entry| entry.name == PROVIDER_CREDENTIAL_ENTRY)
            .expect("provider entry");
        assert!(
            entry.rejected,
            "rotation keeps the same token, so it stays refused"
        );
    }
}
