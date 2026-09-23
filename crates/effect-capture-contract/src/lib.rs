//! Shared provider-neutral observed-effect capture contract (TASK-0124/G8).
//!
//! Implementations receive only bounded capture intent plus durable
//! Execution/Attempt identity. Process evidence must already have been
//! observed from the governed runtime; the capture call itself must never
//! spawn a substitute process. Filesystem evidence must bind each path to its
//! content so permutations cannot preserve the same state fingerprint.

use evaluation::{
    AttemptId, CaptureRequest, EffectCapturePort, EffectKind, EffectPayload, ExecutionId,
    ProcessExitObservation, ProcessExitObservationSink,
};
use std::path::Path;

pub fn run_effect_capture_contract<P>(
    capture: &P,
    root: &Path,
    execution_id: &ExecutionId,
    attempt_id: &AttemptId,
) where
    P: EffectCapturePort + ProcessExitObservationSink,
{
    process_exit_requires_governed_observation(capture, execution_id, attempt_id);
    artifact_capture_is_content_bound(capture, root, execution_id, attempt_id);
    filesystem_capture_binds_paths_to_content(capture, root, execution_id, attempt_id);
    test_suite_without_trusted_source_fails_closed(capture, execution_id, attempt_id);
}

fn process_exit_requires_governed_observation<P>(
    capture: &P,
    execution_id: &ExecutionId,
    attempt_id: &AttemptId,
) where
    P: EffectCapturePort + ProcessExitObservationSink,
{
    let request = CaptureRequest::ProcessExit {
        subject_ref: "contract-process".to_owned(),
    };
    assert!(
        capture.capture(&request, execution_id, attempt_id).is_err(),
        "PROCESS_EXIT must not run a substitute process when no governed observation exists"
    );
    capture.observe_process_exit(ProcessExitObservation::from_captured_streams(
        execution_id.clone(),
        attempt_id.clone(),
        Some(7),
        b"contract stdout",
        b"contract stderr",
    ));
    let observed = capture
        .capture(&request, execution_id, attempt_id)
        .expect("governed process observation must be capturable");
    assert_eq!(observed.kind, EffectKind::ProcessExit);
    assert!(matches!(
        observed.payload,
        EffectPayload::ProcessExit {
            exit_code: Some(7),
            stdout_bytes: 15,
            stderr_bytes: 15,
            ..
        }
    ));
}

fn artifact_capture_is_content_bound(
    capture: &dyn EffectCapturePort,
    root: &Path,
    execution_id: &ExecutionId,
    attempt_id: &AttemptId,
) {
    let path = root.join("artifact.txt");
    std::fs::write(&path, b"first").expect("write first artifact");
    let request = CaptureRequest::ArtifactHash {
        path: path.to_string_lossy().into_owned(),
        subject_ref: "contract-artifact".to_owned(),
    };
    let first = capture
        .capture(&request, execution_id, attempt_id)
        .expect("capture first artifact");
    std::fs::write(&path, b"second").expect("write second artifact");
    let second = capture
        .capture(&request, execution_id, attempt_id)
        .expect("capture second artifact");
    assert_ne!(first.subject_material, second.subject_material);
    assert!(matches!(first.payload, EffectPayload::ArtifactHash { .. }));
}

fn filesystem_capture_binds_paths_to_content(
    capture: &dyn EffectCapturePort,
    root: &Path,
    execution_id: &ExecutionId,
    attempt_id: &AttemptId,
) {
    let tree = root.join("tree");
    std::fs::create_dir_all(&tree).expect("create tree");
    let left = tree.join("left");
    let right = tree.join("right");
    std::fs::write(&left, b"A").expect("left A");
    std::fs::write(&right, b"B").expect("right B");
    let request = CaptureRequest::FsState {
        root: tree.to_string_lossy().into_owned(),
        subject_ref: "contract-tree".to_owned(),
    };
    let first = capture
        .capture(&request, execution_id, attempt_id)
        .expect("capture first tree");
    std::fs::write(&left, b"B").expect("left B");
    std::fs::write(&right, b"A").expect("right A");
    let second = capture
        .capture(&request, execution_id, attempt_id)
        .expect("capture swapped tree");
    assert_ne!(
        first.subject_material, second.subject_material,
        "swapping equal-size contents between paths must change FS_STATE"
    );
}

fn test_suite_without_trusted_source_fails_closed(
    capture: &dyn EffectCapturePort,
    execution_id: &ExecutionId,
    attempt_id: &AttemptId,
) {
    let request = CaptureRequest::TestSuiteResult {
        subject_ref: "contract-suite".to_owned(),
    };
    assert!(capture.capture(&request, execution_id, attempt_id).is_err());
}
