//! ADR-0046 worker vocabulary: device identity (ed25519), signed
//! challenge authentication, job lifecycle. Pure; private keys are
//! inputs to signing only and are never stored in any record.
use std::fmt;

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};

pub const MAX_TEXT_CHARS: usize = 1024;
pub const MAX_OBJECTIVE_CHARS: usize = 16_384;

const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

fn valid_prefixed(raw: &str, prefix: &str) -> bool {
    raw.strip_prefix(prefix)
        .is_some_and(|rest| rest.len() == 26 && rest.bytes().all(|b| CROCKFORD.contains(&b)))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerError {
    InvalidInput(String),
    UnknownKind(String),
    Revoked(String),
    Unauthorized(String),
    AlreadyClaimed(String),
    BadSignature(String),
    ChallengeUsed(String),
    Conflict(String),
    NotFound(String),
    BackendFailure { detail: String },
}

impl fmt::Display for WorkerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(d) => write!(f, "invalid input: {d}"),
            Self::UnknownKind(d) => write!(f, "unknown kind: {d}"),
            Self::Revoked(d) => write!(f, "device revoked: {d}"),
            Self::Unauthorized(d) => write!(f, "unauthorized: {d}"),
            Self::AlreadyClaimed(d) => write!(f, "already claimed: {d}"),
            Self::BadSignature(d) => write!(f, "bad signature: {d}"),
            Self::ChallengeUsed(d) => write!(f, "challenge already used: {d}"),
            Self::Conflict(d) => write!(f, "conflict: {d}"),
            Self::NotFound(d) => write!(f, "not found: {d}"),
            Self::BackendFailure { detail } => write!(f, "backend failure: {detail}"),
        }
    }
}

impl std::error::Error for WorkerError {}

macro_rules! wid {
    ($name:ident, $prefix:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(String);
        impl $name {
            pub fn new(raw: impl Into<String>) -> Result<Self, WorkerError> {
                let raw = raw.into();
                if !valid_prefixed(&raw, $prefix) {
                    return Err(WorkerError::InvalidInput(format!(
                        "id must be {} + 26 Crockford chars",
                        $prefix
                    )));
                }
                Ok(Self(raw))
            }
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

wid!(DeviceId, "dev_");
wid!(JobId, "job_");
wid!(ChallengeId, "chl_");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CapabilityScope {
    ReadOnly,
    WorkspaceWrite,
    Execution,
}

impl CapabilityScope {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "READ_ONLY",
            Self::WorkspaceWrite => "WORKSPACE_WRITE",
            Self::Execution => "EXECUTION",
        }
    }
    pub fn parse(raw: &str) -> Result<Self, WorkerError> {
        match raw {
            "READ_ONLY" => Ok(Self::ReadOnly),
            "WORKSPACE_WRITE" => Ok(Self::WorkspaceWrite),
            "EXECUTION" => Ok(Self::Execution),
            _ => Err(WorkerError::UnknownKind(raw.to_owned())),
        }
    }
}

macro_rules! wire_enum {
    ($name:ident, [$($variant:ident => $wire:literal),+ $(,)?]) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum $name { $($variant,)+ }
        impl $name {
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $wire,)+ }
            }
            pub fn parse(raw: &str) -> Result<Self, WorkerError> {
                match raw {
                    $($wire => Ok(Self::$variant),)+
                    _ => Err(WorkerError::UnknownKind(raw.to_owned())),
                }
            }
        }
    };
}

wire_enum!(DeviceState, [Enrolled => "ENROLLED", Revoked => "REVOKED"]);
wire_enum!(JobStatus, [
    Offered => "OFFERED",
    Claimed => "CLAIMED",
    Completed => "COMPLETED",
    Failed => "FAILED",
    Blocked => "BLOCKED",
]);

fn bounded(value: &str, field: &str, max: usize) -> Result<String, WorkerError> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.chars().count() > max {
        return Err(WorkerError::InvalidInput(format!(
            "{field} must be 1..={max} chars"
        )));
    }
    Ok(trimmed.to_owned())
}

/// sha256 fingerprint of a device public key (W-1).
#[must_use]
pub fn fingerprint(public_key: &[u8; 32]) -> String {
    format!("sha256:{:x}", Sha256::digest(public_key))
}

/// Derives (secret, public) from a 32-byte secret seed. The secret is
/// returned to the caller (vault) and never stored in domain records.
pub fn keypair_from_secret(secret: &[u8; 32]) -> Result<([u8; 32], [u8; 32]), WorkerError> {
    let signing = SigningKey::from_bytes(secret);
    Ok((signing.to_bytes(), signing.verifying_key().to_bytes()))
}

fn challenge_message(nonce: &[u8; 32], job_id: &JobId) -> Vec<u8> {
    let mut msg = nonce.to_vec();
    msg.extend_from_slice(job_id.as_str().as_bytes());
    msg
}

/// Signs `nonce || job_id` with the device secret (W-2).
pub fn sign_challenge(
    secret: &[u8; 32],
    nonce: &[u8; 32],
    job_id: &JobId,
) -> Result<[u8; 64], WorkerError> {
    let signing = SigningKey::from_bytes(secret);
    Ok(signing.sign(&challenge_message(nonce, job_id)).to_bytes())
}

/// Verifies a challenge signature against the enrolled device key.
#[must_use]
pub fn verify_challenge(
    device: &DeviceRecord,
    nonce: &[u8; 32],
    job_id: &JobId,
    signature: &[u8; 64],
) -> bool {
    if !device.is_enrolled() {
        return false;
    }
    let Ok(verifying) = VerifyingKey::from_bytes(&device.public_key) else {
        return false;
    };
    verifying
        .verify(
            &challenge_message(nonce, job_id),
            &Signature::from_bytes(signature),
        )
        .is_ok()
}

/// One enrolled (or revoked) worker device (ADR-0046 §1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceRecord {
    pub id: DeviceId,
    pub name: String,
    pub public_key: [u8; 32],
    pub fingerprint: String,
    pub state: DeviceState,
    pub enrolled_by: String,
    pub enrolled_at: u64,
}

impl DeviceRecord {
    pub fn enroll(
        id: DeviceId,
        name: impl Into<String>,
        public_key: &[u8; 32],
        enrolled_by: impl Into<String>,
        now: u64,
    ) -> Result<Self, WorkerError> {
        // Reject non-points early: ed25519 verifying key must parse.
        VerifyingKey::from_bytes(public_key)
            .map_err(|_| WorkerError::InvalidInput("invalid ed25519 public key".to_owned()))?;
        Ok(Self {
            fingerprint: fingerprint(public_key),
            id,
            name: bounded(&name.into(), "name", MAX_TEXT_CHARS)?,
            public_key: *public_key,
            state: DeviceState::Enrolled,
            enrolled_by: bounded(&enrolled_by.into(), "enrolled_by", MAX_TEXT_CHARS)?,
            enrolled_at: now,
        })
    }

    pub fn revalidate(&self) -> Result<(), WorkerError> {
        if self.fingerprint != fingerprint(&self.public_key) {
            return Err(WorkerError::Conflict(format!(
                "fingerprint mismatch for {}",
                self.id
            )));
        }
        Ok(())
    }

    #[must_use]
    pub fn is_enrolled(&self) -> bool {
        self.state == DeviceState::Enrolled
    }

    pub fn revoke(&mut self) -> Result<(), WorkerError> {
        if self.state == DeviceState::Revoked {
            return Err(WorkerError::InvalidInput(
                "device already revoked".to_owned(),
            ));
        }
        self.state = DeviceState::Revoked;
        Ok(())
    }
}

/// One authorized remote job (ADR-0046 §3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerJob {
    pub id: JobId,
    pub objective: String,
    pub capability_scope: CapabilityScope,
    pub authorized_by: String,
    pub status: JobStatus,
    pub claimed_by: Option<DeviceId>,
    pub created_at: u64,
    pub finished_at: Option<u64>,
    pub evidence_ref: Option<String>,
}

impl WorkerJob {
    pub fn offer(
        id: JobId,
        objective: impl Into<String>,
        capability_scope: CapabilityScope,
        authorized_by: impl Into<String>,
        now: u64,
    ) -> Result<Self, WorkerError> {
        Ok(Self {
            id,
            objective: bounded(&objective.into(), "objective", MAX_OBJECTIVE_CHARS)?,
            capability_scope,
            authorized_by: bounded(&authorized_by.into(), "authorized_by", MAX_TEXT_CHARS)?,
            status: JobStatus::Offered,
            claimed_by: None,
            created_at: now,
            finished_at: None,
            evidence_ref: None,
        })
    }

    pub fn revalidate(&self) -> Result<(), WorkerError> {
        if self.status == JobStatus::Claimed && self.claimed_by.is_none() {
            return Err(WorkerError::Conflict(format!(
                "claimed job {} lacks claimant",
                self.id
            )));
        }
        Ok(())
    }

    /// CLAIM: only an ENROLLED device, only an OFFERED job (W-5, W-7).
    pub fn claim(&mut self, device: &DeviceRecord) -> Result<(), WorkerError> {
        if !device.is_enrolled() {
            return Err(WorkerError::Revoked(device.id.to_string()));
        }
        if self.status != JobStatus::Offered {
            return Err(WorkerError::AlreadyClaimed(self.id.to_string()));
        }
        self.status = JobStatus::Claimed;
        self.claimed_by = Some(device.id.clone());
        Ok(())
    }

    fn finish(&mut self, status: JobStatus, now: u64) -> Result<(), WorkerError> {
        if self.status != JobStatus::Claimed {
            return Err(WorkerError::InvalidInput(format!(
                "cannot finish job in state {}",
                self.status.as_str()
            )));
        }
        if now < self.created_at {
            return Err(WorkerError::InvalidInput(
                "finish before created_at".to_owned(),
            ));
        }
        self.status = status;
        self.finished_at = Some(now);
        Ok(())
    }

    pub fn complete(&mut self, evidence: &str, now: u64) -> Result<(), WorkerError> {
        self.evidence_ref = Some(bounded(evidence, "evidence_ref", MAX_TEXT_CHARS)?);
        self.finish(JobStatus::Completed, now)
    }

    pub fn fail(&mut self, now: u64) -> Result<(), WorkerError> {
        self.finish(JobStatus::Failed, now)
    }

    pub fn block(&mut self, now: u64) -> Result<(), WorkerError> {
        self.finish(JobStatus::Blocked, now)
    }
}

/// Single-use signed challenge (W-4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Challenge {
    pub id: ChallengeId,
    pub device_id: DeviceId,
    pub job_id: JobId,
    pub nonce: [u8; 32],
    pub used: bool,
    pub created_at: u64,
}

impl Challenge {
    pub fn issue(
        id: ChallengeId,
        device_id: DeviceId,
        job_id: JobId,
        nonce: [u8; 32],
        now: u64,
    ) -> Self {
        Self {
            id,
            device_id,
            job_id,
            nonce,
            used: false,
            created_at: now,
        }
    }

    pub fn consume(&mut self) -> Result<(), WorkerError> {
        if self.used {
            return Err(WorkerError::ChallengeUsed(self.id.to_string()));
        }
        self.used = true;
        Ok(())
    }
}
