//! Bounded local observers. They return raw observations only; durable
//! authority is minted by the trusted application/storage boundary.

mod candidate_snapshot;

pub use candidate_snapshot::{
    materialize_candidate_snapshot, read_candidate_snapshot, read_linked_metadata_file,
    read_worktree_git_pointer, remove_candidate_snapshot, scan_live_worktree, CandidateSnapshot,
    DescriptorReadOperation,
};

use evaluation::{
    AttemptId, CandidateIntegrityAssessment, CandidateTestResult, CaptureRequest,
    CapturedObservation, EffectCapturePort, EffectError, EffectKind, EffectPayload, ExecutionId,
    ProcessExitObservation, ProcessExitObservationSink, Sha256Digest, SubjectMaterial,
};
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    fs,
    io::Read,
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub const BACKEND_REF: &str = "runlith.local-observer.v1";
const MAX_CAPTURE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_FS_ENTRIES: usize = 4096;
const MAX_FS_DEPTH: usize = 32;
const MAX_PENDING_PROCESS_OBSERVATIONS: usize = 1024;
const TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, Default)]
pub struct LocalEffectCapture {
    process_exits: Arc<Mutex<VecDeque<ProcessExitObservation>>>,
    candidate_results: Arc<Mutex<VecDeque<StoredCandidateResult>>>,
}

#[derive(Clone, Debug)]
struct StoredCandidateResult {
    execution_id: ExecutionId,
    attempt_id: AttemptId,
    result: CandidateTestResult,
    assessment: CandidateIntegrityAssessment,
}

impl ProcessExitObservationSink for LocalEffectCapture {
    fn observe_process_exit(&self, observation: ProcessExitObservation) {
        let Ok(mut observations) = self.process_exits.lock() else {
            return;
        };
        if observations.len() == MAX_PENDING_PROCESS_OBSERVATIONS {
            observations.pop_front();
        }
        observations.push_back(observation);
    }
}

impl EffectCapturePort for LocalEffectCapture {
    fn backend_ref(&self) -> &'static str {
        BACKEND_REF
    }

    fn capture(
        &self,
        request: &CaptureRequest,
        execution_id: &ExecutionId,
        attempt_id: &AttemptId,
    ) -> Result<CapturedObservation, EffectError> {
        request.validate()?;
        match request {
            CaptureRequest::ProcessExit { subject_ref } => {
                self.process_exit(execution_id, attempt_id, subject_ref)
            }
            CaptureRequest::ArtifactHash { path, subject_ref } => artifact(path, subject_ref),
            CaptureRequest::FsState { root, subject_ref } => filesystem(root, subject_ref),
            CaptureRequest::GitState { .. } => Err(EffectError::Unsupported(
                "git state requires trusted GitPort snapshot",
            )),
            CaptureRequest::TestSuiteResult { subject_ref } => {
                let results = self.candidate_results.lock().map_err(|_| {
                    EffectError::CaptureFailed("candidate result store unavailable")
                })?;
                let result = results
                    .iter()
                    .rev()
                    .find(|result| {
                        result.execution_id == *execution_id && result.attempt_id == *attempt_id
                    })
                    .ok_or(EffectError::CaptureFailed(
                        "trusted structured source unavailable",
                    ))?;
                result.assessment.validate().map_err(|_| {
                    EffectError::CaptureFailed("candidate integrity assessment is invalid")
                })?;
                if result.assessment.execution_id != execution_id.as_str()
                    || result.assessment.attempt_id != attempt_id.as_str()
                    || result.result.selector != result.assessment.selector
                {
                    return Err(EffectError::CaptureFailed(
                        "candidate result lineage mismatch",
                    ));
                }
                Ok(CapturedObservation {
                    kind: EffectKind::TestSuiteResult,
                    subject_ref: subject_ref.clone(),
                    subject_material: SubjectMaterial::ContentToolchainAndConfig {
                        digest: result.assessment.candidate_manifest_sha256.clone(),
                        toolchain: result.assessment.runtime_ref.clone(),
                        config: evaluation::candidate_sha256(
                            format!(
                                "{}\0{}\0{}\0{}\0{}",
                                result.assessment.requirement_revision,
                                result.assessment.policy_fingerprint,
                                result.assessment.selector,
                                result.assessment.selector_path,
                                result.assessment.selector_literal,
                            )
                            .as_bytes(),
                        ),
                    },
                    payload: EffectPayload::TestSuiteResult {
                        passed: result.result.passed,
                        failed: result.result.failed,
                        skipped: result.result.skipped,
                        result_sha256: Sha256Digest::new(result.result.result_sha256.clone())?,
                    },
                })
            }
        }
    }

    fn candidate_assessment(
        &self,
        execution_id: &ExecutionId,
        attempt_id: &AttemptId,
    ) -> Result<Option<CandidateIntegrityAssessment>, EffectError> {
        let results = self
            .candidate_results
            .lock()
            .map_err(|_| EffectError::CaptureFailed("candidate result store unavailable"))?;
        Ok(results
            .iter()
            .rev()
            .find(|result| result.execution_id == *execution_id && result.attempt_id == *attempt_id)
            .map(|result| result.assessment.clone()))
    }

    fn observe_candidate_result(
        &self,
        execution_id: ExecutionId,
        attempt_id: AttemptId,
        result: CandidateTestResult,
        assessment: CandidateIntegrityAssessment,
    ) -> Result<(), EffectError> {
        assessment
            .validate()
            .map_err(|_| EffectError::CaptureFailed("candidate integrity assessment is invalid"))?;
        if assessment.execution_id != execution_id.as_str()
            || assessment.attempt_id != attempt_id.as_str()
            || result.selector != assessment.selector
        {
            return Err(EffectError::CaptureFailed(
                "candidate result lineage mismatch",
            ));
        }
        let mut results = self
            .candidate_results
            .lock()
            .map_err(|_| EffectError::CaptureFailed("candidate result store unavailable"))?;
        if results.iter().any(|previous| {
            previous.execution_id == execution_id && previous.attempt_id == attempt_id
        }) {
            return Err(EffectError::CaptureFailed(
                "candidate result already exists for this attempt",
            ));
        }
        if results.len() >= MAX_PENDING_PROCESS_OBSERVATIONS {
            return Err(EffectError::CaptureFailed(
                "candidate result store exceeded its bound",
            ));
        }
        results.push_back(StoredCandidateResult {
            execution_id,
            attempt_id,
            result,
            assessment,
        });
        Ok(())
    }
}

impl LocalEffectCapture {
    #[must_use]
    pub fn has_process_exit(&self, execution_id: &ExecutionId, attempt_id: &AttemptId) -> bool {
        self.process_exits.lock().is_ok_and(|observations| {
            observations.iter().any(|observation| {
                observation.execution_id == *execution_id && observation.attempt_id == *attempt_id
            })
        })
    }

    fn process_exit(
        &self,
        execution_id: &ExecutionId,
        attempt_id: &AttemptId,
        subject_ref: &str,
    ) -> Result<CapturedObservation, EffectError> {
        let observations = self
            .process_exits
            .lock()
            .map_err(|_| EffectError::CaptureFailed("process observation store unavailable"))?;
        let observation = observations
            .iter()
            .rev()
            .find(|observation| {
                observation.execution_id == *execution_id && observation.attempt_id == *attempt_id
            })
            .ok_or(EffectError::CaptureFailed(
                "governed process exit observation unavailable",
            ))?;
        Ok(CapturedObservation {
            kind: EffectKind::ProcessExit,
            subject_ref: subject_ref.to_owned(),
            subject_material: SubjectMaterial::ProcessAttempt {
                execution_id: execution_id.as_str().to_owned(),
                attempt_id: attempt_id.as_str().to_owned(),
            },
            payload: observation.payload(),
        })
    }
}

fn artifact(path: &str, subject_ref: &str) -> Result<CapturedObservation, EffectError> {
    let canonical =
        fs::canonicalize(path).map_err(|_| EffectError::CaptureFailed("artifact unavailable"))?;
    let pre = fs::symlink_metadata(&canonical)
        .map_err(|_| EffectError::CaptureFailed("artifact metadata unavailable"))?;
    if !pre.is_file() || pre.file_type().is_symlink() {
        return Err(EffectError::CaptureFailed("artifact is not a regular file"));
    }
    let mut file = fs::File::open(&canonical)
        .map_err(|_| EffectError::CaptureFailed("artifact unavailable"))?;
    let opened = file
        .metadata()
        .map_err(|_| EffectError::CaptureFailed("artifact metadata unavailable"))?;
    if !same_file(&pre, &opened) {
        return Err(EffectError::CaptureFailed("artifact identity changed"));
    }
    let deadline = Instant::now() + TIMEOUT;
    let (digest, bytes) = hash_reader(&mut file, deadline, MAX_CAPTURE_BYTES)?;
    verify_same_path_identity(&canonical, &opened)?;
    Ok(CapturedObservation {
        kind: EffectKind::ArtifactHash,
        subject_ref: subject_ref.to_owned(),
        subject_material: SubjectMaterial::Content {
            digest: digest.as_str().to_owned(),
        },
        payload: EffectPayload::ArtifactHash {
            bytes,
            sha256: digest,
        },
    })
}

fn filesystem(root: &str, subject_ref: &str) -> Result<CapturedObservation, EffectError> {
    let root = fs::canonicalize(root)
        .map_err(|_| EffectError::CaptureFailed("filesystem root unavailable"))?;
    let root_meta = fs::symlink_metadata(&root)
        .map_err(|_| EffectError::CaptureFailed("filesystem root unavailable"))?;
    if !root_meta.is_dir() || root_meta.file_type().is_symlink() {
        return Err(EffectError::CaptureFailed(
            "filesystem root is not a directory",
        ));
    }
    let deadline = Instant::now() + TIMEOUT;
    let mut entries = Vec::new();
    let mut total_bytes = 0u64;
    walk(&root, &root, &mut entries, 0, &mut total_bytes, deadline)?;
    verify_same_path_identity(&root, &root_meta)?;
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    let mut manifest = Sha256::new();
    for entry in &entries {
        push(&mut manifest, &entry.path);
        push(&mut manifest, &[entry.kind]);
        match &entry.digest {
            Some(digest) => push(&mut manifest, digest.as_str().as_bytes()),
            None => push(&mut manifest, b"-"),
        }
    }
    let digest = Sha256Digest::new(format!("sha256:{:x}", manifest.finalize()))
        .map_err(|_| EffectError::CaptureFailed("filesystem manifest hash failed"))?;
    let entry_count = u32::try_from(entries.len())
        .map_err(|_| EffectError::CaptureFailed("filesystem state exceeded bound"))?;
    Ok(CapturedObservation {
        kind: EffectKind::FsState,
        subject_ref: subject_ref.to_owned(),
        subject_material: SubjectMaterial::Content {
            digest: digest.as_str().to_owned(),
        },
        payload: EffectPayload::FsState {
            entries: entry_count,
            bytes: total_bytes,
            manifest_sha256: digest,
        },
    })
}

#[derive(Debug)]
struct FsEntry {
    path: Vec<u8>,
    kind: u8,
    digest: Option<Sha256Digest>,
}

fn walk(
    root: &Path,
    path: &Path,
    entries: &mut Vec<FsEntry>,
    depth: usize,
    total_bytes: &mut u64,
    deadline: Instant,
) -> Result<(), EffectError> {
    check_deadline(deadline)?;
    if depth > MAX_FS_DEPTH || entries.len() >= MAX_FS_ENTRIES {
        return Err(EffectError::CaptureFailed(
            "filesystem state exceeded bound",
        ));
    }
    for item in
        fs::read_dir(path).map_err(|_| EffectError::CaptureFailed("filesystem read failed"))?
    {
        check_deadline(deadline)?;
        let item = item.map_err(|_| EffectError::CaptureFailed("filesystem entry unavailable"))?;
        let item_path = item.path();
        let relative = item_path
            .strip_prefix(root)
            .map_err(|_| EffectError::CaptureFailed("filesystem path escaped root"))?;
        let metadata = fs::symlink_metadata(&item_path)
            .map_err(|_| EffectError::CaptureFailed("filesystem metadata unavailable"))?;
        if metadata.file_type().is_symlink() {
            return Err(EffectError::CaptureFailed(
                "filesystem state contains symlink",
            ));
        }
        if entries.len() >= MAX_FS_ENTRIES {
            return Err(EffectError::CaptureFailed(
                "filesystem state exceeded bound",
            ));
        }
        let path_bytes = path_bytes(relative);
        if metadata.is_file() {
            let remaining = MAX_CAPTURE_BYTES.saturating_sub(*total_bytes);
            if remaining == 0 {
                return Err(EffectError::CaptureFailed(
                    "filesystem content exceeded bound",
                ));
            }
            let mut file = fs::File::open(&item_path)
                .map_err(|_| EffectError::CaptureFailed("filesystem file unavailable"))?;
            let opened = file
                .metadata()
                .map_err(|_| EffectError::CaptureFailed("filesystem metadata unavailable"))?;
            if !same_file(&metadata, &opened) {
                return Err(EffectError::CaptureFailed(
                    "filesystem file identity changed",
                ));
            }
            let (digest, bytes) = hash_reader(&mut file, deadline, remaining)?;
            *total_bytes = total_bytes
                .checked_add(bytes)
                .ok_or(EffectError::CaptureFailed(
                    "filesystem content exceeded bound",
                ))?;
            verify_same_path_identity(&item_path, &opened)?;
            entries.push(FsEntry {
                path: path_bytes,
                kind: b'f',
                digest: Some(digest),
            });
        } else if metadata.is_dir() {
            entries.push(FsEntry {
                path: path_bytes,
                kind: b'd',
                digest: None,
            });
            walk(root, &item_path, entries, depth + 1, total_bytes, deadline)?;
        } else {
            return Err(EffectError::CaptureFailed(
                "filesystem state contains unsupported entry",
            ));
        }
    }
    Ok(())
}

fn hash_reader(
    reader: &mut impl Read,
    deadline: Instant,
    max_bytes: u64,
) -> Result<(Sha256Digest, u64), EffectError> {
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 8192];
    let mut count = 0u64;
    loop {
        check_deadline(deadline)?;
        let read = reader
            .read(&mut buffer)
            .map_err(|_| EffectError::CaptureFailed("capture read failed"))?;
        if read == 0 {
            break;
        }
        count = count
            .checked_add(read as u64)
            .ok_or(EffectError::CaptureFailed("capture exceeded byte bound"))?;
        if count > max_bytes {
            return Err(EffectError::CaptureFailed("capture exceeded byte bound"));
        }
        hash.update(&buffer[..read]);
    }
    Sha256Digest::new(format!("sha256:{:x}", hash.finalize()))
        .map(|digest| (digest, count))
        .map_err(|_| EffectError::CaptureFailed("capture hash failed"))
}

fn check_deadline(deadline: Instant) -> Result<(), EffectError> {
    if Instant::now() > deadline {
        Err(EffectError::CaptureFailed("capture timeout"))
    } else {
        Ok(())
    }
}

fn verify_same_path_identity(path: &Path, expected: &fs::Metadata) -> Result<(), EffectError> {
    let canonical = fs::canonicalize(path)
        .map_err(|_| EffectError::CaptureFailed("capture target disappeared"))?;
    if canonical != path {
        return Err(EffectError::CaptureFailed("capture target changed"));
    }
    let current = fs::symlink_metadata(path)
        .map_err(|_| EffectError::CaptureFailed("capture target disappeared"))?;
    if current.file_type().is_symlink() || !same_file(&current, expected) {
        return Err(EffectError::CaptureFailed(
            "capture target identity changed",
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn same_file(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    left.dev() == right.dev() && left.ino() == right.ino()
}

#[cfg(not(unix))]
fn same_file(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    left.len() == right.len()
        && left.is_file() == right.is_file()
        && left.is_dir() == right.is_dir()
}

#[cfg(unix)]
fn path_bytes(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(not(unix))]
fn path_bytes(path: &Path) -> Vec<u8> {
    path.to_string_lossy().as_bytes().to_vec()
}

fn push(hash: &mut Sha256, bytes: &[u8]) {
    hash.update((bytes.len() as u64).to_be_bytes());
    hash.update(bytes);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn ids() -> (ExecutionId, AttemptId) {
        (
            ExecutionId::new("execution-1").unwrap(),
            AttemptId::new("attempt-1").unwrap(),
        )
    }

    #[test]
    fn cv_2_process_exit_uses_only_governed_observation() {
        let capture = LocalEffectCapture::default();
        let (execution, attempt) = ids();
        let request = CaptureRequest::ProcessExit {
            subject_ref: "attempt-output".to_owned(),
        };
        assert!(capture.capture(&request, &execution, &attempt).is_err());
        capture.observe_process_exit(ProcessExitObservation::from_captured_streams(
            execution.clone(),
            attempt.clone(),
            Some(0),
            b"stdout",
            b"stderr",
        ));
        let observed = capture.capture(&request, &execution, &attempt).unwrap();
        assert_eq!(observed.kind, EffectKind::ProcessExit);
        assert!(matches!(
            observed.payload,
            EffectPayload::ProcessExit {
                exit_code: Some(0),
                ..
            }
        ));
    }

    #[test]
    fn cv_3_filesystem_manifest_binds_each_path_to_its_content() {
        let root = std::env::temp_dir().join(format!(
            "runlith-effect-fs-{}-{}",
            std::process::id(),
            Instant::now().elapsed().as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let a = root.join("a");
        let b = root.join("b");
        fs::File::create(&a).unwrap().write_all(b"left").unwrap();
        fs::File::create(&b).unwrap().write_all(b"right").unwrap();
        let first = filesystem(root.to_str().unwrap(), "fs").unwrap();
        fs::write(&a, b"right").unwrap();
        fs::write(&b, b"left").unwrap();
        let second = filesystem(root.to_str().unwrap(), "fs").unwrap();
        assert_ne!(first.subject_material, second.subject_material);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn candidate_result_capture_requires_one_product_minted_bound_assessment() {
        let capture = LocalEffectCapture::default();
        let (execution, attempt) = ids();
        let request = CaptureRequest::TestSuiteResult {
            subject_ref: format!("attempt:{}", attempt.as_str()),
        };
        assert!(capture.capture(&request, &execution, &attempt).is_err());
        let phases = [
            evaluation::CandidateCheckpointPhase::Selection,
            evaluation::CandidateCheckpointPhase::BeforeEvaluation,
            evaluation::CandidateCheckpointPhase::AfterEvaluation,
            evaluation::CandidateCheckpointPhase::ResultAdmission,
        ];
        let mut assessment = CandidateIntegrityAssessment {
            task_id: "task-1".to_owned(),
            execution_id: execution.as_str().to_owned(),
            attempt_id: attempt.as_str().to_owned(),
            subject_ref: request.subject_ref().to_owned(),
            requirement_revision: format!("sha256:{}", "a".repeat(64)),
            policy_revision: 1,
            policy_fingerprint: format!("sha256:{}", "b".repeat(64)),
            criterion_id: "criterion-1".to_owned(),
            selector: "candidate.file-contains.v1".to_owned(),
            selector_path: "src/main.rs".to_owned(),
            selector_literal: "accepted".to_owned(),
            invocation_id: "invocation-1".to_owned(),
            evaluator_ref: "runlith.agentd.candidate-evaluator.v1".to_owned(),
            runtime_ref: "agentd/1".to_owned(),
            enforcement_ref: "G6+Runner".to_owned(),
            invocation_descriptor_version: 1,
            invocation_descriptor_sha256: String::new(),
            evaluator_executable: "/trusted/agentd".to_owned(),
            toolchain_ref: "git version 2.54.0".to_owned(),
            runner_backend: "native".to_owned(),
            runner_plan_sha256: format!("sha256:{}", "8".repeat(64)),
            runner_enforced_axes: 3,
            runner_env_isolated: true,
            base_commit_oid: "1".repeat(40),
            base_tree_oid: "2".repeat(40),
            base_manifest_sha256: format!("sha256:{}", "c".repeat(64)),
            candidate_commit_oid: "3".repeat(40),
            candidate_tree_oid: "4".repeat(40),
            candidate_manifest_sha256: format!("sha256:{}", "d".repeat(64)),
            deletions: vec![],
            checkpoints: phases
                .into_iter()
                .enumerate()
                .map(|(index, phase)| evaluation::CandidateFreshnessCheckpoint {
                    phase,
                    observed_at_ms: 11 + index as u64,
                    git_metadata_fingerprint: format!("sha256:{}", "e".repeat(64)),
                    live_worktree_manifest_sha256: format!("sha256:{}", "f".repeat(64)),
                    matched: true,
                })
                .collect(),
            outcome: evaluation::CandidateIntegrityOutcome::Intact,
            started_at_ms: 10,
            completed_at_ms: 20,
        };
        assessment.refresh_invocation_descriptor_digest();
        let result = CandidateTestResult {
            selector: assessment.selector.clone(),
            passed: 1,
            failed: 0,
            skipped: 0,
            result_sha256: format!("sha256:{}", "9".repeat(64)),
        };
        capture
            .observe_candidate_result(
                execution.clone(),
                attempt.clone(),
                result,
                assessment.clone(),
            )
            .unwrap();
        assert_eq!(
            capture.candidate_assessment(&execution, &attempt).unwrap(),
            Some(assessment.clone())
        );
        assert!(matches!(
            capture
                .capture(&request, &execution, &attempt)
                .unwrap()
                .payload,
            EffectPayload::TestSuiteResult {
                passed: 1,
                failed: 0,
                skipped: 0,
                ..
            }
        ));
        assert!(capture
            .observe_candidate_result(
                execution,
                attempt,
                CandidateTestResult {
                    selector: "candidate.file-contains.v1".to_owned(),
                    passed: 1,
                    failed: 0,
                    skipped: 0,
                    result_sha256: format!("sha256:{}", "8".repeat(64)),
                },
                assessment,
            )
            .is_err());
    }
}
