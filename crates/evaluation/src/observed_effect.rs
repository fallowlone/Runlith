//! Trusted, provider-neutral observed runtime evidence (ADR-0030/G8).

use agent_runtime::{AttemptId, ExecutionId};
use sha2::{Digest, Sha256};
use std::fmt;

pub const MAX_EFFECT_TEXT: usize = 1024;
pub const MAX_EFFECT_PAYLOAD: usize = 4096;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum EffectKind {
    ProcessExit,
    TestSuiteResult,
    ArtifactHash,
    FsState,
    GitState,
}

impl EffectKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ProcessExit => "PROCESS_EXIT",
            Self::TestSuiteResult => "TEST_SUITE_RESULT",
            Self::ArtifactHash => "ARTIFACT_HASH",
            Self::FsState => "FS_STATE",
            Self::GitState => "GIT_STATE",
        }
    }

    pub fn parse(value: &str) -> Result<Self, EffectError> {
        match value {
            "PROCESS_EXIT" => Ok(Self::ProcessExit),
            "TEST_SUITE_RESULT" => Ok(Self::TestSuiteResult),
            "ARTIFACT_HASH" => Ok(Self::ArtifactHash),
            "FS_STATE" => Ok(Self::FsState),
            "GIT_STATE" => Ok(Self::GitState),
            _ => Err(EffectError::Invalid("unknown effect kind")),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum CaptureAuthority {
    LocalHarness,
}

impl CaptureAuthority {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LocalHarness => "runlith.local-harness.v1",
        }
    }

    pub fn parse(value: &str) -> Result<Self, EffectError> {
        match value {
            "runlith.local-harness.v1" => Ok(Self::LocalHarness),
            _ => Err(EffectError::Invalid("unknown capture authority")),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffectId(String);
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffectSubjectRef(String);
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubjectFingerprint(String);
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Sha256Digest(String);

macro_rules! bounded {
    ($name:ident, $label:literal) => {
        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, EffectError> {
                let value = value.into();
                let trimmed = value.trim();
                if trimmed.is_empty()
                    || trimmed.len() > MAX_EFFECT_TEXT
                    || trimmed.chars().any(char::is_control)
                {
                    return Err(EffectError::Invalid($label));
                }
                Ok(Self(trimmed.to_owned()))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}
bounded!(EffectId, "invalid effect id");
bounded!(EffectSubjectRef, "invalid subject reference");

impl SubjectFingerprint {
    pub fn new(value: impl Into<String>) -> Result<Self, EffectError> {
        let value = value.into();
        validate_sha256(&value, "invalid subject fingerprint")?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Sha256Digest {
    pub fn new(value: impl Into<String>) -> Result<Self, EffectError> {
        let value = value.into();
        validate_sha256(&value, "invalid sha256 digest")?;
        Ok(Self(value))
    }

    pub fn of(bytes: &[u8]) -> Self {
        Self(format!("sha256:{:x}", Sha256::digest(bytes)))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn validate_sha256(value: &str, error: &'static str) -> Result<(), EffectError> {
    let valid = value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    });
    if valid {
        Ok(())
    } else {
        Err(EffectError::Invalid(error))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EffectPayload {
    ProcessExit {
        exit_code: Option<i32>,
        stdout_bytes: u64,
        stdout_sha256: Sha256Digest,
        stderr_bytes: u64,
        stderr_sha256: Sha256Digest,
    },
    TestSuiteResult {
        passed: u32,
        failed: u32,
        skipped: u32,
        result_sha256: Sha256Digest,
    },
    ArtifactHash {
        bytes: u64,
        sha256: Sha256Digest,
    },
    FsState {
        entries: u32,
        bytes: u64,
        manifest_sha256: Sha256Digest,
    },
    GitState {
        head_oid: String,
        tree_oid: String,
        index_sha256: Sha256Digest,
        filesystem_sha256: Sha256Digest,
    },
}

impl EffectPayload {
    pub const fn kind(&self) -> EffectKind {
        match self {
            Self::ProcessExit { .. } => EffectKind::ProcessExit,
            Self::TestSuiteResult { .. } => EffectKind::TestSuiteResult,
            Self::ArtifactHash { .. } => EffectKind::ArtifactHash,
            Self::FsState { .. } => EffectKind::FsState,
            Self::GitState { .. } => EffectKind::GitState,
        }
    }

    pub fn encode(&self) -> String {
        match self {
            Self::ProcessExit {
                exit_code,
                stdout_bytes,
                stdout_sha256,
                stderr_bytes,
                stderr_sha256,
            } => format!(
                "exit={};stdout_bytes={stdout_bytes};stdout_sha256={};stderr_bytes={stderr_bytes};stderr_sha256={}",
                exit_code.map_or_else(|| "signal".to_owned(), |value| value.to_string()),
                stdout_sha256.as_str(),
                stderr_sha256.as_str(),
            ),
            Self::TestSuiteResult {
                passed,
                failed,
                skipped,
                result_sha256,
            } => format!(
                "passed={passed};failed={failed};skipped={skipped};result_sha256={}",
                result_sha256.as_str()
            ),
            Self::ArtifactHash { bytes, sha256 } => {
                format!("bytes={bytes};sha256={}", sha256.as_str())
            }
            Self::FsState {
                entries,
                bytes,
                manifest_sha256,
            } => format!(
                "entries={entries};bytes={bytes};manifest_sha256={}",
                manifest_sha256.as_str()
            ),
            Self::GitState {
                head_oid,
                tree_oid,
                index_sha256,
                filesystem_sha256,
            } => format!(
                "head={head_oid};tree={tree_oid};index_sha256={};filesystem_sha256={}",
                index_sha256.as_str(),
                filesystem_sha256.as_str()
            ),
        }
    }

    pub fn decode(kind: EffectKind, value: &str) -> Result<Self, EffectError> {
        if value.is_empty()
            || value.len() > MAX_EFFECT_PAYLOAD
            || value.chars().any(char::is_control)
        {
            return Err(EffectError::Invalid("invalid effect payload"));
        }
        let fields = value
            .split(';')
            .map(|field| {
                field
                    .split_once('=')
                    .ok_or(EffectError::Invalid("invalid effect payload"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        match kind {
            EffectKind::ProcessExit => {
                require_keys(
                    &fields,
                    &[
                        "exit",
                        "stdout_bytes",
                        "stdout_sha256",
                        "stderr_bytes",
                        "stderr_sha256",
                    ],
                )?;
                let exit_code = match fields[0].1 {
                    "signal" => None,
                    value => Some(parse_num(value)?),
                };
                Ok(Self::ProcessExit {
                    exit_code,
                    stdout_bytes: parse_num(fields[1].1)?,
                    stdout_sha256: Sha256Digest::new(fields[2].1)?,
                    stderr_bytes: parse_num(fields[3].1)?,
                    stderr_sha256: Sha256Digest::new(fields[4].1)?,
                })
            }
            EffectKind::TestSuiteResult => {
                require_keys(&fields, &["passed", "failed", "skipped", "result_sha256"])?;
                Ok(Self::TestSuiteResult {
                    passed: parse_num(fields[0].1)?,
                    failed: parse_num(fields[1].1)?,
                    skipped: parse_num(fields[2].1)?,
                    result_sha256: Sha256Digest::new(fields[3].1)?,
                })
            }
            EffectKind::ArtifactHash => {
                require_keys(&fields, &["bytes", "sha256"])?;
                Ok(Self::ArtifactHash {
                    bytes: parse_num(fields[0].1)?,
                    sha256: Sha256Digest::new(fields[1].1)?,
                })
            }
            EffectKind::FsState => {
                require_keys(&fields, &["entries", "bytes", "manifest_sha256"])?;
                Ok(Self::FsState {
                    entries: parse_num(fields[0].1)?,
                    bytes: parse_num(fields[1].1)?,
                    manifest_sha256: Sha256Digest::new(fields[2].1)?,
                })
            }
            EffectKind::GitState => {
                require_keys(
                    &fields,
                    &["head", "tree", "index_sha256", "filesystem_sha256"],
                )?;
                validate_git_oid(fields[0].1)?;
                validate_git_oid(fields[1].1)?;
                Ok(Self::GitState {
                    head_oid: fields[0].1.to_owned(),
                    tree_oid: fields[1].1.to_owned(),
                    index_sha256: Sha256Digest::new(fields[2].1)?,
                    filesystem_sha256: Sha256Digest::new(fields[3].1)?,
                })
            }
        }
    }
}

fn require_keys(fields: &[(&str, &str)], expected: &[&str]) -> Result<(), EffectError> {
    if fields.len() == expected.len()
        && fields
            .iter()
            .zip(expected)
            .all(|((actual, _), expected)| actual == expected)
    {
        Ok(())
    } else {
        Err(EffectError::Invalid(
            "effect payload does not match effect kind",
        ))
    }
}

fn parse_num<T: std::str::FromStr>(value: &str) -> Result<T, EffectError> {
    value
        .parse()
        .map_err(|_| EffectError::Invalid("invalid numeric effect payload"))
}

pub fn validate_git_oid(value: &str) -> Result<(), EffectError> {
    if matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        Ok(())
    } else {
        Err(EffectError::Invalid("invalid git object id"))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffectProvenance {
    pub execution_id: ExecutionId,
    pub attempt_id: AttemptId,
    pub runtime_ref: String,
    pub evaluator_ref: Option<String>,
    pub receipt_ref: Option<String>,
}

impl EffectProvenance {
    pub fn new(
        execution_id: ExecutionId,
        attempt_id: AttemptId,
        runtime_ref: impl Into<String>,
    ) -> Result<Self, EffectError> {
        let runtime_ref = bounded_text(runtime_ref.into(), "invalid runtime identity")?;
        Ok(Self {
            execution_id,
            attempt_id,
            runtime_ref,
            evaluator_ref: None,
            receipt_ref: None,
        })
    }

    pub fn with_lineage(
        mut self,
        evaluator_ref: Option<String>,
        receipt_ref: Option<String>,
    ) -> Result<Self, EffectError> {
        self.evaluator_ref = evaluator_ref
            .map(|value| bounded_text(value, "invalid evaluator identity"))
            .transpose()?;
        self.receipt_ref = receipt_ref
            .map(|value| bounded_text(value, "invalid receipt reference"))
            .transpose()?;
        Ok(self)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservedEffect {
    effect_id: EffectId,
    subject_ref: EffectSubjectRef,
    subject_fingerprint: SubjectFingerprint,
    effect_kind: EffectKind,
    captured_by: CaptureAuthority,
    captured_at: String,
    payload: EffectPayload,
    provenance: EffectProvenance,
}

impl ObservedEffect {
    /// Rebuilds a validated durable row. Persistence is the authority boundary:
    /// callers cannot select an arbitrary capture-authority string or open
    /// payload; both vocabularies are closed and kind-consistent here.
    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        effect_id: EffectId,
        subject_ref: EffectSubjectRef,
        subject_fingerprint: SubjectFingerprint,
        effect_kind: EffectKind,
        captured_by: CaptureAuthority,
        captured_at: impl Into<String>,
        payload: EffectPayload,
        provenance: EffectProvenance,
    ) -> Result<Self, EffectError> {
        if payload.kind() != effect_kind {
            return Err(EffectError::Invalid(
                "effect payload does not match effect kind",
            ));
        }
        let captured_at = canonical_timestamp(captured_at.into())?;
        Ok(Self {
            effect_id,
            subject_ref,
            subject_fingerprint,
            effect_kind,
            captured_by,
            captured_at,
            payload,
            provenance,
        })
    }

    /// F16: SHA-256 over every persisted field (kind, authority, timestamp,
    /// encoded payload, provenance). `subject_fingerprint` binds only the
    /// subject, so a type-valid edit to the payload or provenance would
    /// otherwise survive `restore`.
    #[must_use]
    pub fn record_fingerprint(&self) -> String {
        let mut hash = Sha256::new();
        push(&mut hash, b"runlith:observed-effect-record:v1");
        for value in [
            self.effect_id.as_str(),
            self.subject_ref.as_str(),
            self.subject_fingerprint.as_str(),
            self.effect_kind.as_str(),
            self.captured_by.as_str(),
            self.captured_at.as_str(),
            self.payload.encode().as_str(),
            self.provenance.execution_id.as_str(),
            self.provenance.attempt_id.as_str(),
            self.provenance.runtime_ref.as_str(),
        ] {
            push(&mut hash, value.as_bytes());
        }
        for optional in [&self.provenance.evaluator_ref, &self.provenance.receipt_ref] {
            match optional {
                Some(value) => {
                    hash.update([1]);
                    push(&mut hash, value.as_bytes());
                }
                None => hash.update([0]),
            }
        }
        format!("sha256:{:x}", hash.finalize())
    }

    pub fn effect_id(&self) -> &EffectId {
        &self.effect_id
    }
    pub fn subject_ref(&self) -> &EffectSubjectRef {
        &self.subject_ref
    }
    pub fn subject_fingerprint(&self) -> &SubjectFingerprint {
        &self.subject_fingerprint
    }
    pub fn effect_kind(&self) -> EffectKind {
        self.effect_kind
    }
    pub fn captured_by(&self) -> CaptureAuthority {
        self.captured_by
    }
    pub fn captured_at(&self) -> &str {
        &self.captured_at
    }
    pub fn payload(&self) -> &EffectPayload {
        &self.payload
    }
    pub fn provenance(&self) -> &EffectProvenance {
        &self.provenance
    }
}

pub fn subject_fingerprint(
    kind: EffectKind,
    subject_ref: &str,
    binding_material: &[(&str, &str)],
) -> Result<SubjectFingerprint, EffectError> {
    let mut fields = binding_material.to_vec();
    fields.sort_unstable_by(|left, right| left.0.cmp(right.0));
    let mut hash = Sha256::new();
    push(&mut hash, b"runlith:observed-effect-subject:v1");
    push(&mut hash, kind.as_str().as_bytes());
    push(&mut hash, subject_ref.as_bytes());
    for (key, value) in fields {
        push(&mut hash, key.as_bytes());
        push(&mut hash, value.as_bytes());
    }
    SubjectFingerprint::new(format!("sha256:{:x}", hash.finalize()))
}

fn push(hash: &mut Sha256, bytes: &[u8]) {
    hash.update((bytes.len() as u64).to_be_bytes());
    hash.update(bytes);
}

fn bounded_text(value: String, what: &'static str) -> Result<String, EffectError> {
    let value = value.trim().to_owned();
    if value.is_empty() || value.len() > MAX_EFFECT_TEXT || value.chars().any(char::is_control) {
        return Err(EffectError::Invalid(what));
    }
    Ok(value)
}

fn canonical_timestamp(value: String) -> Result<String, EffectError> {
    let value = value.trim().to_owned();
    let bytes = value.as_bytes();
    let shape = bytes.len() == 20
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes[13] == b':'
        && bytes[16] == b':'
        && bytes[19] == b'Z'
        && [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18]
            .iter()
            .all(|&index| bytes[index].is_ascii_digit());
    if !shape {
        return Err(EffectError::Invalid("invalid capture timestamp"));
    }
    let year = value[0..4].parse::<u16>().unwrap_or(0);
    let month = value[5..7].parse::<u8>().unwrap_or(0);
    let day = value[8..10].parse::<u8>().unwrap_or(0);
    let hour = value[11..13].parse::<u8>().unwrap_or(24);
    let minute = value[14..16].parse::<u8>().unwrap_or(60);
    let second = value[17..19].parse::<u8>().unwrap_or(60);
    let max_day = days_in_month(year, month).unwrap_or(0);
    if day == 0 || day > max_day || hour > 23 || minute > 59 || second > 59 {
        return Err(EffectError::Invalid("invalid capture timestamp"));
    }
    Ok(value)
}

fn days_in_month(year: u16, month: u8) -> Option<u8> {
    Some(match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => return None,
    })
}

fn is_leap_year(year: u16) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EffectError {
    Invalid(&'static str),
    Unsupported(&'static str),
    CaptureFailed(&'static str),
}

impl fmt::Display for EffectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "observed effect error: {:?}", self)
    }
}
impl std::error::Error for EffectError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_effect(timestamp: &str) -> Result<ObservedEffect, EffectError> {
        ObservedEffect::restore(
            EffectId::new("effect-1")?,
            EffectSubjectRef::new("subject")?,
            SubjectFingerprint::new(format!("sha256:{}", "0".repeat(64)))?,
            EffectKind::ArtifactHash,
            CaptureAuthority::LocalHarness,
            timestamp,
            EffectPayload::ArtifactHash {
                bytes: 0,
                sha256: Sha256Digest::of(&[]),
            },
            EffectProvenance::new(
                ExecutionId::new("execution-1").unwrap(),
                AttemptId::new("attempt-1").unwrap(),
                "local",
            )?,
        )
    }

    #[test]
    fn closed_kinds_payloads_and_fingerprints_fail_closed() {
        assert_eq!(
            EffectKind::parse("GIT_STATE").unwrap(),
            EffectKind::GitState
        );
        assert!(EffectKind::parse("provider-result").is_err());
        assert!(EffectPayload::decode(EffectKind::ArtifactHash, "anything").is_err());
        let left = subject_fingerprint(EffectKind::FsState, "workspace", &[("b", "2"), ("a", "1")])
            .unwrap();
        let right =
            subject_fingerprint(EffectKind::FsState, "workspace", &[("a", "1"), ("b", "2")])
                .unwrap();
        assert_eq!(left, right);
    }

    #[test]
    fn calendar_timestamp_validation_rejects_impossible_dates() {
        assert!(sample_effect("2024-02-29T12:00:00Z").is_ok());
        for timestamp in [
            "2026-02-31T12:00:00Z",
            "2026-04-31T12:00:00Z",
            "2023-02-29T12:00:00Z",
        ] {
            assert!(sample_effect(timestamp).is_err(), "accepted {timestamp}");
        }
    }
}
