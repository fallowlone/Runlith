//! Required scenario matrix (TASK-0036): every canonical scenario is
//! deterministic, side-effect free, and distinguishable through actual
//! adapter behavior — never mere enum parsing.
//!
//! Timing discipline: only `hang` tests use short bounded waits, and only
//! to prove blocking-then-decision semantics. Every blocking receive has
//! a finite timeout so a regression fails instead of hanging the suite.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use agent_runtime::{AttemptId, ExecutionId};
use fake_agent::{ExecutionControl, ExecutionOutput, ExecutionRequest, FakeAgent, FakeScenario};
use task_domain::TaskId;

fn request(task: &str, execution: &str, attempt: &str) -> ExecutionRequest {
    ExecutionRequest::new(
        TaskId::new(task).unwrap(),
        ExecutionId::new(execution).unwrap(),
        AttemptId::new(attempt).unwrap(),
    )
}

fn agent(scenario: FakeScenario) -> FakeAgent {
    FakeAgent::for_scenario(scenario)
}

fn fresh_control() -> ExecutionControl {
    ExecutionControl::fresh()
}

/// Executes twice with equivalent inputs; both results must be identical.
fn assert_deterministic(
    scenario: FakeScenario,
    first: &Result<ExecutionOutput, fake_agent::FakeAgentError>,
    second: &Result<ExecutionOutput, fake_agent::FakeAgentError>,
) {
    assert_eq!(first, second, "{scenario:?} must be deterministic");
}

#[test]
fn success_produces_valid_complete_evidence() {
    let req = request("T-1", "E-1", "A-1");
    let first = agent(FakeScenario::Success).execute(&req, &fresh_control());
    let second = agent(FakeScenario::Success).execute(&req, &fresh_control());
    assert_deterministic(FakeScenario::Success, &first, &second);
    let output = first.unwrap();
    assert!(!output.is_partial(), "success must be complete");
    assert!(!output.is_rejectable_fixture());
    assert!(!output.summary().trim().is_empty());
    assert!(output.evidence_ref().contains("success"));
    assert!(output.evidence_ref().contains("E-1"));
    assert!(output.evidence_ref().contains("A-1"));
}

#[test]
fn crash_is_deterministic_and_distinct() {
    let req = request("T-1", "E-1", "A-1");
    let first = agent(FakeScenario::Crash).execute(&req, &fresh_control());
    let second = agent(FakeScenario::Crash).execute(&req, &fresh_control());
    assert_deterministic(FakeScenario::Crash, &first, &second);
    let error = first.unwrap_err();
    assert_eq!(error.kind(), "terminated");
    assert!(matches!(
        error,
        fake_agent::FakeAgentError::Terminated { .. }
    ));
}

#[test]
fn timeout_returns_promptly_and_differs_from_hang() {
    let req = request("T-1", "E-1", "A-1");
    // Synchronous in the calling thread: no release, no second thread, no
    // sleep required. A `hang` can never do this — it blocks for a
    // decision — which is exactly the specified distinction.
    let started = Instant::now();
    let first = agent(FakeScenario::Timeout).execute(&req, &fresh_control());
    let second = agent(FakeScenario::Timeout).execute(&req, &fresh_control());
    assert_deterministic(FakeScenario::Timeout, &first, &second);
    let error = first.unwrap_err();
    assert_eq!(error.kind(), "timed_out");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "timeout must not sleep for wall-clock durations"
    );
}

#[test]
fn malformed_result_can_never_be_trusted_success() {
    let req = request("T-1", "E-1", "A-1");
    for _ in 0..3 {
        let result = agent(FakeScenario::MalformedResult).execute(&req, &fresh_control());
        assert!(
            matches!(
                result,
                Err(fake_agent::FakeAgentError::MalformedOutput { .. })
            ),
            "malformed output must surface as an error, never Ok: {result:?}"
        );
    }
}

#[test]
fn forbidden_path_write_performs_no_write() {
    let sentinel = std::path::Path::new("/forbidden/fake-agent-write");
    let req = request("T-1", "E-1", "A-1");
    let result = agent(FakeScenario::ForbiddenPathWrite).execute(&req, &fresh_control());
    let error = result.unwrap_err();
    match &error {
        fake_agent::FakeAgentError::CapabilityDenied { capability, .. } => {
            assert_eq!(capability, "fs.write");
        }
        other => panic!("expected capability denial, got {other:?}"),
    }
    assert_eq!(error.kind(), "capability_denied");
    assert!(!sentinel.exists(), "no forbidden write may occur");
}

#[test]
fn forbidden_command_launches_no_shell() {
    let req = request("T-1", "E-1", "A-1");
    let result = agent(FakeScenario::ForbiddenCommand).execute(&req, &fresh_control());
    let error = result.unwrap_err();
    match &error {
        fake_agent::FakeAgentError::CapabilityDenied {
            capability,
            attempted,
        } => {
            assert_eq!(capability, "exec");
            assert!(attempted.contains("not executed"));
        }
        other => panic!("expected capability denial, got {other:?}"),
    }
}

#[test]
fn quota_reached_neither_retries_nor_leaks_global_state() {
    let req = request("T-1", "E-1", "A-1");
    let quota = agent(FakeScenario::QuotaReached);
    // One deterministic result per call: the adapter never retries.
    let first = quota.execute(&req, &fresh_control());
    let second = quota.execute(&req, &fresh_control());
    assert_deterministic(FakeScenario::QuotaReached, &first, &second);
    assert_eq!(first.unwrap_err().kind(), "quota_exhausted");
    // Instance isolation: quota exhaustion on one execution leaves an
    // independent execution untouched.
    let success = agent(FakeScenario::Success).execute(&req, &fresh_control());
    assert!(
        success.is_ok(),
        "quota state must not leak across executions"
    );
}

#[test]
fn network_failure_uses_no_network() {
    let req = request("T-1", "E-1", "A-1");
    let first = agent(FakeScenario::NetworkFailure).execute(&req, &fresh_control());
    let second = agent(FakeScenario::NetworkFailure).execute(&req, &fresh_control());
    assert_deterministic(FakeScenario::NetworkFailure, &first, &second);
    assert_eq!(first.unwrap_err().kind(), "network_failure");
}

#[test]
fn partial_result_is_typed_distinct_from_complete_success() {
    let req = request("T-1", "E-1", "A-1");
    let partial = agent(FakeScenario::PartialResult)
        .execute(&req, &fresh_control())
        .unwrap();
    let complete = agent(FakeScenario::Success)
        .execute(&req, &fresh_control())
        .unwrap();
    assert!(partial.is_partial(), "partial flag carries the distinction");
    assert!(!complete.is_partial());
    assert_ne!(partial, complete, "never silently equal to full success");
    assert_ne!(partial.evidence_ref(), complete.evidence_ref());
    // Deterministic replay preserves the flag.
    let replay = agent(FakeScenario::PartialResult)
        .execute(&req, &fresh_control())
        .unwrap();
    assert_eq!(partial, replay);
}

#[test]
fn review_rejection_is_fixture_not_verdict() {
    let req = request("T-1", "E-1", "A-1");
    let output = agent(FakeScenario::ReviewRejection)
        .execute(&req, &fresh_control())
        .unwrap();
    // Structurally valid work (complete, like success) …
    assert!(!output.is_partial());
    assert!(!output.summary().trim().is_empty());
    // … plus the deterministic marker the TASK-0037 evaluation fixture
    // reads. FakeAgent exposes no verify/reject/complete API: this flag
    // is data for the future evaluator, never a verdict.
    assert!(output.is_rejectable_fixture());
    let success = agent(FakeScenario::Success)
        .execute(&req, &fresh_control())
        .unwrap();
    assert!(!success.is_rejectable_fixture());
    assert_ne!(output, success);
}

#[test]
fn failure_kinds_stay_mutually_distinguishable() {
    let req = request("T-1", "E-1", "A-1");
    let kinds = [
        (FakeScenario::Crash, "terminated"),
        (FakeScenario::Timeout, "timed_out"),
        (FakeScenario::MalformedResult, "malformed_output"),
        (FakeScenario::ForbiddenPathWrite, "capability_denied"),
        (FakeScenario::ForbiddenCommand, "capability_denied"),
        (FakeScenario::QuotaReached, "quota_exhausted"),
        (FakeScenario::NetworkFailure, "network_failure"),
    ];
    for (scenario, expected) in kinds {
        let error = agent(scenario).execute(&req, &fresh_control()).unwrap_err();
        assert_eq!(error.kind(), expected, "{scenario:?}");
    }
    // Crash, timeout, network failure, and malformed output are four
    // different variants — never one generic failure.
    let crash = agent(FakeScenario::Crash).execute(&req, &fresh_control());
    let timeout = agent(FakeScenario::Timeout).execute(&req, &fresh_control());
    let network = agent(FakeScenario::NetworkFailure).execute(&req, &fresh_control());
    let malformed = agent(FakeScenario::MalformedResult).execute(&req, &fresh_control());
    assert_ne!(crash, timeout);
    assert_ne!(crash, network);
    assert_ne!(crash, malformed);
    assert_ne!(timeout, network);
    // The two forbidden scenarios share the denial variant but differ in
    // the capability they attempted.
    let path = agent(FakeScenario::ForbiddenPathWrite).execute(&req, &fresh_control());
    let command = agent(FakeScenario::ForbiddenCommand).execute(&req, &fresh_control());
    assert_ne!(path, command);
}

#[test]
fn hang_blocks_until_cancelled() {
    let req = request("T-1", "E-1", "A-1");
    let control = fresh_control();
    let worker_control = control.clone();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result = agent(FakeScenario::Hang).execute(&req, &worker_control);
        let _ = tx.send(result);
    });
    // Still blocked after a short bounded wait: no terminal result yet.
    std::thread::sleep(Duration::from_millis(50));
    assert!(
        rx.try_recv().is_err(),
        "hang must produce no terminal result before a decision"
    );
    control.cancel();
    // Idempotent: repeated cancellation is accepted.
    control.cancel();
    let result = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("cancelled hang must exit promptly");
    assert_eq!(result.unwrap_err().kind(), "cancelled");
}

#[test]
fn hang_blocks_until_test_controlled_release() {
    let req = request("T-1", "E-1", "A-1");
    let control = fresh_control();
    let worker_control = control.clone();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result = agent(FakeScenario::Hang).execute(&req, &worker_control);
        let _ = tx.send(result);
    });
    std::thread::sleep(Duration::from_millis(50));
    assert!(rx.try_recv().is_err(), "hang must wait for the decision");
    control.release();
    let result = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("released hang must exit promptly");
    let output = result.unwrap();
    assert!(!output.is_partial());
}

#[test]
fn concurrent_executions_stay_isolated() {
    // E1 hangs; E2 succeeds independently; cancelling E1 never touches E2.
    let hang_control = fresh_control();
    let worker_control = hang_control.clone();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result =
            agent(FakeScenario::Hang).execute(&request("T-1", "E-1", "A-1"), &worker_control);
        let _ = tx.send(result);
    });
    let independent = agent(FakeScenario::Success)
        .execute(&request("T-1", "E-2", "A-1"), &fresh_control())
        .expect("E2 must complete while E1 hangs");
    assert!(!independent.is_partial());
    hang_control.cancel();
    let hang_result = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("cancelled E1 must exit");
    assert_eq!(hang_result.unwrap_err().kind(), "cancelled");
    // E2's evidence was already complete and is unaffected.
    assert!(independent.evidence_ref().contains("E-2"));
}
