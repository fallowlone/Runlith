use agent_runtime::{AttemptId, ExecutionId};

use crate::{
    CandidateIntegrityAssessment, CandidateTestResult, EffectError, EffectKind, EffectPayload,
    Sha256Digest,
};

pub const MAX_CAPTURE_PATH: usize = 4096;
pub const MAX_CAPTURE_SUBJECT: usize = 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CaptureRequest {
    ProcessExit {
        subject_ref: String,
    },
    TestSuiteResult {
        subject_ref: String,
    },
    ArtifactHash {
        path: String,
        subject_ref: String,
    },
    FsState {
        root: String,
        subject_ref: String,
    },
    GitState {
        workspace: String,
        subject_ref: String,
    },
}

impl CaptureRequest {
    pub fn kind(&self) -> EffectKind {
        match self {
            Self::ProcessExit { .. } => EffectKind::ProcessExit,
            Self::TestSuiteResult { .. } => EffectKind::TestSuiteResult,
            Self::ArtifactHash { .. } => EffectKind::ArtifactHash,
            Self::FsState { .. } => EffectKind::FsState,
            Self::GitState { .. } => EffectKind::GitState,
        }
    }

    pub fn subject_ref(&self) -> &str {
        match self {
            Self::ProcessExit { subject_ref }
            | Self::TestSuiteResult { subject_ref }
            | Self::ArtifactHash { subject_ref, .. }
            | Self::FsState { subject_ref, .. }
            | Self::GitState { subject_ref, .. } => subject_ref,
        }
    }

    pub fn validate(&self) -> Result<(), EffectError> {
        bounded(
            self.subject_ref(),
            MAX_CAPTURE_SUBJECT,
            "invalid capture subject",
        )?;
        match self {
            Self::ArtifactHash { path, .. } => {
                bounded(path, MAX_CAPTURE_PATH, "invalid artifact path")
            }
            Self::FsState { root, .. } => {
                bounded(root, MAX_CAPTURE_PATH, "invalid filesystem root")
            }
            Self::GitState { workspace, .. } => {
                bounded(workspace, MAX_CAPTURE_PATH, "invalid git workspace")
            }
            Self::ProcessExit { .. } | Self::TestSuiteResult { .. } => Ok(()),
        }
    }

    pub fn with_resolved_path(&self, resolved: String) -> Self {
        match self {
            Self::ArtifactHash { subject_ref, .. } => Self::ArtifactHash {
                path: resolved,
                subject_ref: subject_ref.clone(),
            },
            Self::FsState { subject_ref, .. } => Self::FsState {
                root: resolved,
                subject_ref: subject_ref.clone(),
            },
            Self::GitState { subject_ref, .. } => Self::GitState {
                workspace: resolved,
                subject_ref: subject_ref.clone(),
            },
            other => other.clone(),
        }
    }
}

fn bounded(value: &str, max: usize, error: &'static str) -> Result<(), EffectError> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        Err(EffectError::Invalid(error))
    } else {
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapturedObservation {
    pub kind: EffectKind,
    pub subject_ref: String,
    pub subject_material: SubjectMaterial,
    pub payload: EffectPayload,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SubjectMaterial {
    ProcessAttempt {
        execution_id: String,
        attempt_id: String,
    },
    Content {
        digest: String,
    },
    ContentAndToolchain {
        digest: String,
        toolchain: String,
    },
    ContentToolchainAndConfig {
        digest: String,
        toolchain: String,
        config: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessExitObservation {
    pub execution_id: ExecutionId,
    pub attempt_id: AttemptId,
    pub exit_code: Option<i32>,
    pub stdout_bytes: u64,
    pub stdout_sha256: Sha256Digest,
    pub stderr_bytes: u64,
    pub stderr_sha256: Sha256Digest,
}

impl ProcessExitObservation {
    pub fn from_captured_streams(
        execution_id: ExecutionId,
        attempt_id: AttemptId,
        exit_code: Option<i32>,
        stdout: &[u8],
        stderr: &[u8],
    ) -> Self {
        Self {
            execution_id,
            attempt_id,
            exit_code,
            stdout_bytes: stdout.len() as u64,
            stdout_sha256: Sha256Digest::of(stdout),
            stderr_bytes: stderr.len() as u64,
            stderr_sha256: Sha256Digest::of(stderr),
        }
    }

    pub fn payload(&self) -> EffectPayload {
        EffectPayload::ProcessExit {
            exit_code: self.exit_code,
            stdout_bytes: self.stdout_bytes,
            stdout_sha256: self.stdout_sha256.clone(),
            stderr_bytes: self.stderr_bytes,
            stderr_sha256: self.stderr_sha256.clone(),
        }
    }
}

pub trait ProcessExitObservationSink: Send + Sync {
    fn observe_process_exit(&self, observation: ProcessExitObservation);
}

pub trait EffectCapturePort: Send + Sync {
    fn backend_ref(&self) -> &'static str;
    fn capture(
        &self,
        request: &CaptureRequest,
        execution_id: &ExecutionId,
        attempt_id: &AttemptId,
    ) -> Result<CapturedObservation, EffectError>;

    /// Returns product-minted candidate evidence only when a governed
    /// evaluator placed a matching result and integrity assessment at the
    /// capture boundary. Implementations default to unavailable.
    fn candidate_assessment(
        &self,
        _execution_id: &ExecutionId,
        _attempt_id: &AttemptId,
    ) -> Result<Option<CandidateIntegrityAssessment>, EffectError> {
        Ok(None)
    }

    fn observe_candidate_result(
        &self,
        _execution_id: ExecutionId,
        _attempt_id: AttemptId,
        _result: CandidateTestResult,
        _assessment: CandidateIntegrityAssessment,
    ) -> Result<(), EffectError> {
        Err(EffectError::Unsupported(
            "trusted candidate result source unavailable",
        ))
    }
}
