//! Trusted composition for observed effects. Callers provide capture intent;
//! this boundary resolves durable lineage/workspace authority, asks local
//! observers for bounded facts, fingerprints them, and persists the result.

use evaluation::{
    subject_fingerprint, CaptureRequest, CapturedObservation, EffectCapturePort, EffectKind,
    EffectPayload, EffectProvenance, EffectSubjectRef, ObservedEffect, Sha256Digest,
    SubjectMaterial,
};
use storage_sqlite::SqliteStore;
use workspace::{GitPort, TrustedCommonGit, WorkspaceLease, WorkspaceStore};

pub struct ObservedEffectService<C, P, G> {
    store: SqliteStore,
    clock: C,
    port: P,
    git: G,
}

impl<C: crate::Clock, P: EffectCapturePort, G: GitPort> ObservedEffectService<C, P, G> {
    pub fn new(store: SqliteStore, clock: C, port: P, git: G) -> Self {
        Self {
            store,
            clock,
            port,
            git,
        }
    }

    pub fn capture(
        &mut self,
        request: &CaptureRequest,
        execution_id: agent_runtime::ExecutionId,
        attempt_id: agent_runtime::AttemptId,
    ) -> Result<ObservedEffect, ObservedEffectServiceError> {
        request
            .validate()
            .map_err(ObservedEffectServiceError::Capture)?;
        self.validate_attempt(&execution_id, &attempt_id)?;
        let resolved = self.resolve_request(request, &execution_id)?;
        let observed = if request.kind() == EffectKind::GitState {
            self.capture_git_state(&resolved, &execution_id, &attempt_id)?
        } else {
            self.port
                .capture(&resolved.request, &execution_id, &attempt_id)
                .map_err(ObservedEffectServiceError::Capture)?
        };
        if observed.kind != request.kind() || observed.payload.kind() != observed.kind {
            return Err(ObservedEffectServiceError::Capture(
                evaluation::EffectError::CaptureFailed("adapter returned the wrong effect kind"),
            ));
        }
        let candidate_assessment = if observed.kind == EffectKind::TestSuiteResult {
            let assessment = self
                .port
                .candidate_assessment(&execution_id, &attempt_id)
                .map_err(ObservedEffectServiceError::Capture)?
                .ok_or_else(|| {
                    ObservedEffectServiceError::Lineage(
                        "trusted candidate integrity assessment is unavailable".to_owned(),
                    )
                })?;
            assessment
                .validate()
                .map_err(|error| ObservedEffectServiceError::Lineage(error.to_string()))?;
            if assessment.execution_id != execution_id.as_str()
                || assessment.attempt_id != attempt_id.as_str()
            {
                return Err(ObservedEffectServiceError::Lineage(
                    "candidate integrity assessment lineage mismatch".to_owned(),
                ));
            }
            Some(assessment)
        } else {
            None
        };
        self.revalidate_resolved_target(&resolved)?;

        let subject_ref = EffectSubjectRef::new(observed.subject_ref.clone())
            .map_err(ObservedEffectServiceError::Effect)?;
        let fields = material_fields(&observed.subject_material);
        let fingerprint = subject_fingerprint(observed.kind, subject_ref.as_str(), &fields)
            .map_err(ObservedEffectServiceError::Effect)?;
        let runtime_ref = if let Some(assessment) = &candidate_assessment {
            assessment.runtime_ref.clone()
        } else if observed.kind == EffectKind::GitState {
            format!("{}+{}", self.git.backend_ref(), self.port.backend_ref())
        } else {
            self.port.backend_ref().to_owned()
        };
        let provenance = EffectProvenance::new(execution_id, attempt_id, runtime_ref)
            .and_then(|provenance| {
                provenance.with_lineage(
                    candidate_assessment
                        .as_ref()
                        .map(|item| item.evaluator_ref.clone()),
                    candidate_assessment
                        .as_ref()
                        .map(|item| item.invocation_id.clone()),
                )
            })
            .map_err(ObservedEffectServiceError::Effect)?;
        let captured_at = crate::format_rfc3339(self.clock.now_millis());
        match candidate_assessment {
            Some(assessment) => self
                .store
                .append_candidate_result(
                    subject_ref,
                    fingerprint,
                    observed.payload,
                    captured_at,
                    provenance,
                    &assessment,
                )
                .map_err(ObservedEffectServiceError::Storage),
            None => self
                .store
                .append_observed_effect(
                    subject_ref,
                    fingerprint,
                    observed.payload,
                    captured_at,
                    provenance,
                )
                .map_err(ObservedEffectServiceError::Storage),
        }
    }

    pub fn store(&self) -> &SqliteStore {
        &self.store
    }

    fn validate_attempt(
        &self,
        execution_id: &agent_runtime::ExecutionId,
        attempt_id: &agent_runtime::AttemptId,
    ) -> Result<(), ObservedEffectServiceError> {
        let attempt = self
            .store
            .get_attempt(attempt_id)
            .map_err(ObservedEffectServiceError::Storage)?
            .ok_or_else(|| {
                ObservedEffectServiceError::Lineage("capture attempt does not exist".to_owned())
            })?;
        if attempt.execution_id() != execution_id {
            return Err(ObservedEffectServiceError::Lineage(
                "capture attempt belongs to a different execution".to_owned(),
            ));
        }
        Ok(())
    }

    fn resolve_request(
        &self,
        request: &CaptureRequest,
        execution_id: &agent_runtime::ExecutionId,
    ) -> Result<ResolvedCapture, ObservedEffectServiceError> {
        let raw_path = match request {
            CaptureRequest::ArtifactHash { path, .. } => Some(path.as_str()),
            CaptureRequest::FsState { root, .. } => Some(root.as_str()),
            CaptureRequest::GitState { workspace, .. } => Some(workspace.as_str()),
            CaptureRequest::ProcessExit { .. } | CaptureRequest::TestSuiteResult { .. } => None,
        };
        let Some(raw_path) = raw_path else {
            return Ok(ResolvedCapture {
                request: request.clone(),
                root: None,
                target: None,
                lease: None,
                anchor: None,
            });
        };
        let lease = self
            .store
            .get_workspace_for_execution(execution_id)
            .map_err(|error| ObservedEffectServiceError::Lineage(error.to_string()))?
            .ok_or_else(|| {
                ObservedEffectServiceError::Lineage(
                    "workspace-scoped capture has no execution-owned workspace".to_owned(),
                )
            })?;
        let root = std::fs::canonicalize(lease.worktree_ref()).map_err(|_| {
            ObservedEffectServiceError::Lineage("leased workspace is unavailable".to_owned())
        })?;
        let target = std::fs::canonicalize(raw_path).map_err(|_| {
            ObservedEffectServiceError::Lineage("capture target is unavailable".to_owned())
        })?;
        if !target.starts_with(&root) {
            return Err(ObservedEffectServiceError::Lineage(
                "capture path is outside the execution-owned workspace".to_owned(),
            ));
        }
        if matches!(request, CaptureRequest::GitState { .. }) && target != root {
            return Err(ObservedEffectServiceError::Lineage(
                "git-state capture must target the execution workspace root".to_owned(),
            ));
        }
        let target_text = target.to_str().ok_or_else(|| {
            ObservedEffectServiceError::Lineage("capture target is not valid UTF-8".to_owned())
        })?;
        let anchor = if matches!(request, CaptureRequest::GitState { .. }) {
            Some(
                self.store
                    .get_trusted_anchor(lease.id())
                    .map_err(|error| ObservedEffectServiceError::Lineage(error.to_string()))?
                    .ok_or_else(|| {
                        ObservedEffectServiceError::Lineage(
                            "git-state capture has no trusted repository anchor".to_owned(),
                        )
                    })?,
            )
        } else {
            None
        };
        Ok(ResolvedCapture {
            request: request.with_resolved_path(target_text.to_owned()),
            root: Some(root),
            target: Some(target),
            lease: Some(lease),
            anchor,
        })
    }

    fn revalidate_resolved_target(
        &self,
        resolved: &ResolvedCapture,
    ) -> Result<(), ObservedEffectServiceError> {
        let (Some(root), Some(target), Some(lease)) =
            (&resolved.root, &resolved.target, &resolved.lease)
        else {
            return Ok(());
        };
        let current_root = std::fs::canonicalize(lease.worktree_ref()).map_err(|_| {
            ObservedEffectServiceError::Lineage(
                "leased workspace changed during capture".to_owned(),
            )
        })?;
        let current_target = std::fs::canonicalize(target).map_err(|_| {
            ObservedEffectServiceError::Lineage("capture target changed during capture".to_owned())
        })?;
        if current_root != *root || current_target != *target || !current_target.starts_with(root) {
            return Err(ObservedEffectServiceError::Lineage(
                "capture target identity changed during observation".to_owned(),
            ));
        }
        Ok(())
    }

    fn capture_git_state(
        &self,
        resolved: &ResolvedCapture,
        execution_id: &agent_runtime::ExecutionId,
        attempt_id: &agent_runtime::AttemptId,
    ) -> Result<CapturedObservation, ObservedEffectServiceError> {
        let target = resolved.target.as_ref().ok_or_else(|| {
            ObservedEffectServiceError::Lineage("git-state target is missing".to_owned())
        })?;
        let anchor = resolved.anchor.as_ref().ok_or_else(|| {
            ObservedEffectServiceError::Lineage("git-state trusted anchor is missing".to_owned())
        })?;
        let target_text = target.to_str().ok_or_else(|| {
            ObservedEffectServiceError::Lineage("git-state target is not valid UTF-8".to_owned())
        })?;
        let git = self
            .git
            .git_state_snapshot(target_text, anchor)
            .map_err(|error| ObservedEffectServiceError::Lineage(error.to_string()))?;
        let fs_request = CaptureRequest::FsState {
            root: target_text.to_owned(),
            subject_ref: resolved.request.subject_ref().to_owned(),
        };
        let fs = self
            .port
            .capture(&fs_request, execution_id, attempt_id)
            .map_err(ObservedEffectServiceError::Capture)?;
        let filesystem_sha256 = match &fs.payload {
            EffectPayload::FsState {
                manifest_sha256, ..
            } => manifest_sha256.clone(),
            _ => {
                return Err(ObservedEffectServiceError::Capture(
                    evaluation::EffectError::CaptureFailed(
                        "filesystem observer returned the wrong payload kind",
                    ),
                ))
            }
        };
        let index_sha256 = Sha256Digest::of(&git.index_manifest);
        let toolchain = format!(
            "head={};tree={};index={}",
            git.head_revision.as_str(),
            git.head_tree_oid,
            index_sha256.as_str()
        );
        Ok(CapturedObservation {
            kind: EffectKind::GitState,
            subject_ref: resolved.request.subject_ref().to_owned(),
            subject_material: SubjectMaterial::ContentAndToolchain {
                digest: filesystem_sha256.as_str().to_owned(),
                toolchain,
            },
            payload: EffectPayload::GitState {
                head_oid: git.head_revision.as_str().to_owned(),
                tree_oid: git.head_tree_oid,
                index_sha256,
                filesystem_sha256,
            },
        })
    }
}

fn material_fields(material: &SubjectMaterial) -> Vec<(&'static str, &str)> {
    match material {
        SubjectMaterial::ProcessAttempt {
            execution_id,
            attempt_id,
        } => vec![
            ("execution", execution_id.as_str()),
            ("attempt", attempt_id.as_str()),
        ],
        SubjectMaterial::Content { digest } => vec![("content", digest.as_str())],
        SubjectMaterial::ContentAndToolchain { digest, toolchain } => vec![
            ("content", digest.as_str()),
            ("toolchain", toolchain.as_str()),
        ],
        SubjectMaterial::ContentToolchainAndConfig {
            digest,
            toolchain,
            config,
        } => vec![
            ("content", digest.as_str()),
            ("toolchain", toolchain.as_str()),
            ("config", config.as_str()),
        ],
    }
}

struct ResolvedCapture {
    request: CaptureRequest,
    root: Option<std::path::PathBuf>,
    target: Option<std::path::PathBuf>,
    lease: Option<WorkspaceLease>,
    anchor: Option<TrustedCommonGit>,
}

#[derive(Debug)]
pub enum ObservedEffectServiceError {
    Capture(evaluation::EffectError),
    Effect(evaluation::EffectError),
    Storage(storage_sqlite::StorageError),
    Lineage(String),
}

impl std::fmt::Display for ObservedEffectServiceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "observed effect capture failed")
    }
}
impl std::error::Error for ObservedEffectServiceError {}
