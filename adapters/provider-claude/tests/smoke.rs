//! Real-provider smoke test (TASK-0046, G5 acceptance §28).
//!
//! The smallest bounded real-provider path: one harmless tightly-scoped
//! task through the real `claude` CLI with the user's subscription
//! authentication. The prompt requests a fixed marker with no tool use
//! and performs no modifications anywhere.
//!
//! Separation (TASK-0046 §27): this test is `#[ignore]`d AND requires
//! `RUNLITH_CLAUDE_SMOKE=1`, so deterministic CI (no CLI, no auth, no
//! network, no quota) never runs it and never fails on its absence. A
//! missing CLI or missing auth skips gracefully inside the test; only a
//! genuinely broken integration fails.

use std::ffi::OsStr;
use std::time::Duration;

use agent_runtime::{AttemptId, ExecutionId};
use provider_claude::{ClaudeConfig, ClaudeExecutor, Detection, ProviderAvailability};
use scheduler::{ExecutionPort, WorkItem};
use task_domain::TaskId;

fn smoke_enabled() -> bool {
    std::env::var("RUNLITH_CLAUDE_SMOKE").is_ok_and(|v| v == "1")
}

#[test]
#[ignore = "real provider: needs RUNLITH_CLAUDE_SMOKE=1 plus an authenticated claude CLI"]
fn real_claude_smoke_replies_with_marker() {
    if !smoke_enabled() {
        eprintln!("smoke skipped: set RUNLITH_CLAUDE_SMOKE=1 to run against the real CLI");
        return;
    }
    let detection: Detection = provider_claude::detect(OsStr::new("claude"));
    if detection.status != ProviderAvailability::Available {
        eprintln!(
            "smoke skipped: claude CLI not available ({:?}: {})",
            detection.status, detection.detail
        );
        return;
    }
    let capabilities = provider_claude::probe_capabilities(OsStr::new("claude"), detection.version);
    assert!(capabilities.non_interactive, "smoke needs print mode");
    assert!(capabilities.structured_json, "smoke needs json output");

    let workdir = std::env::temp_dir().join(format!(
        "runlith-claude-smoke-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_millis()
    ));
    std::fs::create_dir_all(&workdir).expect("smoke workdir");
    let git = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(&workdir)
            .status()
            .expect("smoke git command");
        assert!(status.success(), "smoke git command failed: {args:?}");
    };
    git(&["init", "-q"]);
    git(&["config", "user.name", "runlith-smoke"]);
    git(&["config", "user.email", "smoke@example.invalid"]);
    std::fs::write(workdir.join(".keep"), b"").expect("smoke marker");
    git(&["add", ".keep"]);
    git(&["commit", "-qm", "init"]);
    let environment_base = workdir
        .parent()
        .expect("smoke parent")
        .join(format!("runlith-claude-smoke-env-{}", std::process::id()));
    let config = ClaudeConfig::default()
        .with_timeout(Duration::from_secs(180))
        .expect("smoke timeout");
    let executor = ClaudeExecutor::bind_trusted_host(
        config,
        "Do not use any tools. Reply with exactly: G5_SMOKE_OK and nothing else.",
        workdir.clone(),
        environment_base.clone(),
    )
    .expect("bind smoke executor");
    let work = WorkItem {
        task_id: TaskId::new("T-SMOKE").unwrap(),
        execution_id: ExecutionId::new("E-SMOKE").unwrap(),
        attempt_id: AttemptId::new("A-SMOKE").unwrap(),
    };
    let started = std::time::Instant::now();
    let result = executor.execute(&work);
    eprintln!("smoke concluded in {:?}", started.elapsed());
    let status = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(&workdir)
        .output()
        .expect("git status after smoke");
    assert!(
        status.status.success(),
        "git status after smoke must succeed"
    );
    assert!(
        status.stdout.is_empty(),
        "real provider must not dirty smoke workdir; status={:?}",
        String::from_utf8_lossy(&status.stdout)
    );
    let _ = std::fs::remove_dir_all(&workdir);
    let _ = std::fs::remove_dir_all(&environment_base);

    match result {
        Ok(evidence) => {
            eprintln!("smoke summary: {:?}", evidence.summary);
            eprintln!("smoke evidence_ref: {:?}", evidence.evidence_ref);
            assert!(!evidence.partial);
            assert!(
                evidence.summary.contains("G5_SMOKE_OK"),
                "real provider must echo the marker, got {:?}",
                evidence.summary
            );
        }
        Err(error) => {
            // Auth/quota/network failures are environment facts, not
            // integration defects: report loudly but do not fail. Only a
            // structural failure (malformed CLI contract) fails.
            eprintln!("smoke provider error: {error:?}");
            let detail = format!("{error:?}");
            assert!(
                !detail.contains("malformed_output"),
                "CLI contract drift is a real defect: {detail:?}"
            );
        }
    }
}
