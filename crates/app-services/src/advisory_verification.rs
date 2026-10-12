//! Advisory verification stage (TASK-0165, G9): the orchestration
//! cycle's consumer of [`VerificationPlan`].
//!
//! [`run_advisory_verification`] derives the plan for one concluded
//! attempt with the existing TASK-0134 selection (no new selection
//! logic), executes V1..V3 plans through the TASK-0162 runner, and
//! maps everything into caller-visible [`AdvisoryVerification`]
//! evidence. It never blocks the cycle: every failure mode — refused
//! plan, unresolvable executable, timeout, store failure — becomes a
//! `Failed` report, and the TASK-0125 admission path proceeds
//! unchanged.

use std::path::Path;
use std::time::Duration;

use evaluation::ObservedEffect;
use scheduler::{AdvisorySkipReason, AdvisoryVerification, WorkItem};

use crate::verification_execution::{VerificationExecutionRequest, VerificationRunner};
use crate::verification_levels::{plan_verification, RiskTier, VerificationLevel};
use crate::Clock;

/// Explicit policy knobs for the advisory verification stage. Owned
/// by the cycle caller (the daemon); nothing is defaulted inside the
/// stage.
#[derive(Debug, Clone, Copy)]
pub struct AdvisoryVerificationPolicy {
    /// Wall-clock budget per verification command; the runner kills and
    /// records timed-out commands, then fails fast.
    pub timeout: Duration,
    /// Retained bytes per captured output stream.
    pub per_stream_cap: u64,
}

/// How the stage obtains its runner. `Ready` borrows a pre-built runner;
/// `Build` constructs one on demand — and only when a plan will actually
/// execute, so skipped cycles (and failure paths that never submit)
/// never pay construction.
pub enum AdvisoryRunner<'r, C> {
    Ready(&'r mut VerificationRunner<C>),
    Build(&'r mut dyn FnMut() -> Result<VerificationRunner<C>, String>),
}

/// Injected context for the advisory verification stage: how to obtain
/// the runner plus the caller's explicit policy knobs. `None` at the
/// cycle boundary means the stage does not exist — no workspace to
/// execute against — and the cycle behaves exactly as before.
pub struct AdvisoryVerificationRequest<'r, C> {
    /// Runner source; resolved only when a plan will execute.
    pub runner: AdvisoryRunner<'r, C>,
    /// Caller policy knobs.
    pub policy: AdvisoryVerificationPolicy,
}

/// Everything the stage needs besides the runner: selection inputs plus
/// the attempt the plan executes for.
pub(crate) struct AdvisoryStageInputs<'a> {
    pub policy: &'a AdvisoryVerificationPolicy,
    pub work: &'a WorkItem,
    pub workspace: &'a Path,
    pub changed_paths: &'a [String],
    pub attempt_effects: &'a [ObservedEffect],
    pub captured_at: &'a str,
}

/// Maximum bytes kept from an advisory-stage failure detail.
/// Underlying errors are typed and output-free by contract; the bound is
/// defense in depth against paths embedded in backend diagnostics.
const MAX_ADVISORY_REASON_LEN: usize = 512;

/// Bounds an advisory failure detail on a char boundary with explicit
/// truncation (never panics on multibyte input).
pub(crate) fn bound_advisory_reason(mut reason: String) -> String {
    if reason.len() > MAX_ADVISORY_REASON_LEN {
        let mut end = MAX_ADVISORY_REASON_LEN;
        while !reason.is_char_boundary(end) {
            end -= 1;
        }
        reason.truncate(end);
        reason.push('…');
    }
    reason
}

/// Derives the verification plan for one concluded attempt and executes
/// it through the resolved runner, mapping every outcome into
/// caller-visible advisory evidence. See the module docs for the contract.
///
/// Selection runs before runner resolution: skipped plans never build
/// the runner.
pub(crate) fn run_advisory_verification<C: Clock + Clone>(
    runner: AdvisoryRunner<'_, C>,
    inputs: &AdvisoryStageInputs<'_>,
) -> AdvisoryVerification {
    // Existing TASK-0134 selection, reused verbatim: the cycle carries
    // no risk signal, so the neutral `Standard` tier applies. Anything
    // else would be new selection logic.
    let plan = plan_verification(
        inputs.changed_paths,
        inputs.attempt_effects,
        RiskTier::Standard,
    );
    if plan.level == VerificationLevel::V0 {
        return AdvisoryVerification::Skipped {
            reason: AdvisorySkipReason::LevelV0,
        };
    }
    if plan.commands.is_empty() {
        return AdvisoryVerification::Skipped {
            reason: AdvisorySkipReason::EmptyPlan,
        };
    }
    let mut built;
    let runner = match runner {
        AdvisoryRunner::Ready(runner) => runner,
        AdvisoryRunner::Build(build) => match build() {
            Ok(runner) => {
                built = runner;
                &mut built
            }
            Err(reason) => {
                return AdvisoryVerification::Failed {
                    reason: bound_advisory_reason(reason),
                };
            }
        },
    };
    let request = VerificationExecutionRequest {
        plan,
        task_id: inputs.work.task_id.as_str().to_owned(),
        execution_id: inputs.work.execution_id.clone(),
        attempt_id: inputs.work.attempt_id.clone(),
        workspace: inputs.workspace.to_path_buf(),
        captured_at: inputs.captured_at.to_owned(),
        timeout: inputs.policy.timeout,
        per_stream_cap: inputs.policy.per_stream_cap,
    };
    match runner.execute(request) {
        // The runner is fail-fast: `Ok` means every derived command ran
        // and recorded its `ProcessExit` effect.
        Ok(verified) => AdvisoryVerification::Completed {
            commands_executed: verified.len() as u32,
        },
        // Typed runner errors carry positions and static reasons only —
        // never command output bytes — so the display form is safe to
        // surface as opaque failure evidence.
        Err(error) => AdvisoryVerification::Failed {
            reason: error.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use agent_runtime::{AttemptId, Execution, ExecutionId, ProviderRef};
    use scheduler::AdvisoryVerification;
    use storage_sqlite::{EventType, NewJournalEvent, SqliteStore};
    use task_domain::{Project, ProjectId, Task, TaskId};

    use super::{
        run_advisory_verification, AdvisoryRunner, AdvisoryStageInputs, AdvisoryVerificationPolicy,
    };
    use crate::verification_execution::VerificationRunner;
    use crate::{FixedClock, G6SecurityService, TaskService};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    const FIXED_MILLIS: u64 = 1_704_067_200_000;
    const FIXED_STAMP: &str = "2024-01-01T00:00:00Z";

    struct Fixture {
        dir: PathBuf,
        db: PathBuf,
        workspace: PathBuf,
        task_id: TaskId,
        execution_id: ExecutionId,
        attempt_id: AttemptId,
    }

    impl Fixture {
        fn new(suite: &str) -> Self {
            let id = COUNTER.fetch_add(1, Ordering::SeqCst);
            let dir = std::env::temp_dir().join(format!(
                "runlith-advisory-verify-test-{suite}-{id}-{}",
                std::process::id()
            ));
            let workspace = dir.join("workspace");
            std::fs::create_dir_all(&workspace).expect("workspace dir");
            let db = dir.join("test.db");
            let mut fixture = Self {
                dir,
                db,
                workspace,
                task_id: TaskId::new("T-1").unwrap(),
                execution_id: ExecutionId::new("E-1").unwrap(),
                attempt_id: AttemptId::new("A-1").unwrap(),
            };
            fixture.seed_lineage();
            fixture
        }

        fn seed_lineage(&mut self) {
            let mut service =
                TaskService::open(&self.db, FixedClock(FIXED_MILLIS)).expect("service");
            let project_id = ProjectId::new("P-1").unwrap();
            service
                .create_project(&Project::new(project_id.clone(), "project P-1").unwrap())
                .unwrap();
            let task = Task::draft(self.task_id.clone(), project_id, "verify me").unwrap();
            service.create_task(&task).unwrap();
            let task = service.store().get_task(&self.task_id).unwrap().unwrap();
            let mut execution = Execution::begin(self.execution_id.clone(), self.task_id.clone());
            service
                .store_mut()
                .create_execution_with_event(
                    &execution,
                    &journal(
                        EventType::ExecutionCreated,
                        &task,
                        Some(self.execution_id.clone()),
                        None,
                    ),
                )
                .unwrap();
            execution
                .start_attempt(
                    self.attempt_id.clone(),
                    ProviderRef::new("advisory-test").unwrap(),
                )
                .unwrap();
            let attempt = execution.get_attempt(&self.attempt_id).unwrap().clone();
            service
                .store_mut()
                .create_attempt_with_event(
                    &attempt,
                    &journal(
                        EventType::AttemptCreated,
                        &task,
                        Some(self.execution_id.clone()),
                        Some(self.attempt_id.clone()),
                    ),
                )
                .unwrap();
        }

        fn script(&self, name: &str, body: &str) -> PathBuf {
            let path = self.workspace.join(name);
            std::fs::write(&path, body).expect("fixture script");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut permissions = std::fs::metadata(&path).unwrap().permissions();
                permissions.set_mode(0o755);
                std::fs::set_permissions(&path, permissions).unwrap();
            }
            path
        }

        fn runner(
            &self,
            resolve: crate::verification_execution::ExecutableResolver,
        ) -> VerificationRunner<FixedClock> {
            let store = SqliteStore::open(&self.db).expect("store");
            let security =
                G6SecurityService::open(&self.db, FixedClock(FIXED_MILLIS)).expect("security");
            VerificationRunner::new(
                store,
                security,
                Arc::new(runner_native::NativeRunner),
                runner::IsolationRequirements::trusted_host(),
                resolve,
            )
        }

        fn effect_count(&self) -> i64 {
            let connection = rusqlite::Connection::open(&self.db).unwrap();
            connection
                .query_row("SELECT COUNT(*) FROM observed_effects", [], |row| {
                    row.get(0)
                })
                .unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn journal(
        kind: EventType,
        task: &Task,
        execution_id: Option<ExecutionId>,
        attempt_id: Option<AttemptId>,
    ) -> NewJournalEvent {
        NewJournalEvent {
            event_type: kind,
            task_id: Some(task.id().clone()),
            project_id: Some(task.project_id().clone()),
            from_state: None,
            to_state: None,
            reason: None,
            correlation_id: "advisory-verification-test".to_owned(),
            causation_id: "advisory-verification-test".to_owned(),
            occurred_at: FIXED_STAMP.to_owned(),
            provenance: "app-services/test".to_owned(),
            execution_id,
            attempt_id,
            attempt_outcome: None,
            workspace_id: None,
            from_workspace_state: None,
            to_workspace_state: None,
            plan_revision: None,
        }
    }

    fn work_for(fixture: &Fixture) -> scheduler::WorkItem {
        scheduler::WorkItem {
            task_id: fixture.task_id.clone(),
            execution_id: fixture.execution_id.clone(),
            attempt_id: fixture.attempt_id.clone(),
        }
    }

    fn run_stage(
        runner: &mut VerificationRunner<FixedClock>,
        work: &scheduler::WorkItem,
        workspace: &std::path::Path,
        changed_paths: &[String],
        timeout: Duration,
    ) -> AdvisoryVerification {
        let policy = AdvisoryVerificationPolicy {
            timeout,
            per_stream_cap: 65536,
        };
        let inputs = AdvisoryStageInputs {
            policy: &policy,
            work,
            workspace,
            changed_paths,
            attempt_effects: &[],
            captured_at: FIXED_STAMP,
        };
        run_advisory_verification(AdvisoryRunner::Ready(runner), &inputs)
    }

    #[test]
    fn cargo_manifest_change_executes_v3_and_records_effects() {
        let fixture = Fixture::new("v3-executes");
        let script = fixture.script("ok.sh", "#!/bin/sh\nexit 0\n");
        let mut runner = fixture.runner(Arc::new(move |_| Ok(script.clone())));
        let work = work_for(&fixture);

        let report = run_stage(
            &mut runner,
            &work,
            &fixture.workspace,
            &["Cargo.toml".to_owned(), "src/main.rs".to_owned()],
            Duration::from_secs(60),
        );

        assert!(
            matches!(
                report,
                AdvisoryVerification::Completed {
                    commands_executed: 3
                }
            ),
            "manifest change must execute the V3 plan, got {report:?}"
        );
        assert_eq!(fixture.effect_count(), 3);
    }

    #[test]
    fn docs_only_change_skips_v0_without_effects_or_processes() {
        let fixture = Fixture::new("v0-skips");
        let mut runner = fixture.runner(Arc::new(|program| {
            panic!("V0 must never resolve an executable, got {program}")
        }));
        let work = work_for(&fixture);

        let report = run_stage(
            &mut runner,
            &work,
            &fixture.workspace,
            &["README.md".to_owned()],
            Duration::from_secs(60),
        );

        assert!(
            matches!(
                report,
                AdvisoryVerification::Skipped {
                    reason: scheduler::AdvisorySkipReason::LevelV0
                }
            ),
            "docs-only change must skip by construction, got {report:?}"
        );
        assert_eq!(fixture.effect_count(), 0);
    }

    #[test]
    fn code_without_manifest_skips_empty_plan() {
        let fixture = Fixture::new("empty-plan-skips");
        let mut runner = fixture.runner(Arc::new(|program| {
            panic!("empty plan must never resolve an executable, got {program}")
        }));
        let work = work_for(&fixture);

        let report = run_stage(
            &mut runner,
            &work,
            &fixture.workspace,
            &["src/main.rs".to_owned()],
            Duration::from_secs(60),
        );

        assert!(
            matches!(
                report,
                AdvisoryVerification::Skipped {
                    reason: scheduler::AdvisorySkipReason::EmptyPlan
                }
            ),
            "V2 without an ecosystem signal derives no commands, got {report:?}"
        );
        assert_eq!(fixture.effect_count(), 0);
    }

    #[test]
    fn unresolvable_executable_reports_failure_without_effects() {
        let fixture = Fixture::new("infra-fails");
        let mut runner = fixture.runner(Arc::new(|_| Err("no executable in test".to_owned())));
        let work = work_for(&fixture);

        let report = run_stage(
            &mut runner,
            &work,
            &fixture.workspace,
            &["Cargo.toml".to_owned()],
            Duration::from_secs(60),
        );

        match report {
            AdvisoryVerification::Failed { reason } => {
                assert!(
                    reason.contains("unresolved"),
                    "infra failure must name itself, got {reason:?}"
                );
            }
            other => panic!("unresolvable executable must report failure, got {other:?}"),
        }
        assert_eq!(
            fixture.effect_count(),
            0,
            "pre-execution failure records nothing durable"
        );
    }

    #[test]
    fn slow_command_times_out_with_signal_effect_recorded() {
        let fixture = Fixture::new("timeout-fails");
        let script = fixture.script("slow.sh", "#!/bin/sh\nsleep 5\n");
        let mut runner = fixture.runner(Arc::new(move |_| Ok(script.clone())));
        let work = work_for(&fixture);

        let report = run_stage(
            &mut runner,
            &work,
            &fixture.workspace,
            &["Cargo.toml".to_owned()],
            Duration::from_millis(100),
        );

        match report {
            AdvisoryVerification::Failed { reason } => {
                assert!(
                    reason.contains("timed out"),
                    "timeout must name itself, got {reason:?}"
                );
            }
            other => panic!("slow command must time out, got {other:?}"),
        }
        assert_eq!(
            fixture.effect_count(),
            1,
            "timed-out command records its signal exit, fail-fast stops the rest"
        );
    }

    #[test]
    fn skipped_plan_never_builds_the_runner() {
        // Laziness guarantee: selection runs before runner resolution, so
        // skipped cycles never pay construction (daemon store opens,
        // backend build) — including failure paths that never submit.
        let fixture = Fixture::new("skip-never-builds");
        let work = work_for(&fixture);
        let policy = AdvisoryVerificationPolicy {
            timeout: Duration::from_secs(60),
            per_stream_cap: 65536,
        };
        let inputs = AdvisoryStageInputs {
            policy: &policy,
            work: &work,
            workspace: &fixture.workspace,
            changed_paths: &["README.md".to_owned()],
            attempt_effects: &[],
            captured_at: FIXED_STAMP,
        };
        let mut factory = || -> Result<VerificationRunner<FixedClock>, String> {
            panic!("skipped plan must never build the runner")
        };

        let report = run_advisory_verification(AdvisoryRunner::Build(&mut factory), &inputs);

        assert!(
            matches!(
                report,
                AdvisoryVerification::Skipped {
                    reason: scheduler::AdvisorySkipReason::LevelV0
                }
            ),
            "V0 must skip without building, got {report:?}"
        );
        assert_eq!(fixture.effect_count(), 0);
    }

    #[test]
    fn factory_error_maps_to_failed_report() {
        let fixture = Fixture::new("factory-fails");
        let work = work_for(&fixture);
        let policy = AdvisoryVerificationPolicy {
            timeout: Duration::from_secs(60),
            per_stream_cap: 65536,
        };
        let inputs = AdvisoryStageInputs {
            policy: &policy,
            work: &work,
            workspace: &fixture.workspace,
            changed_paths: &["Cargo.toml".to_owned()],
            attempt_effects: &[],
            captured_at: FIXED_STAMP,
        };
        let mut factory = || -> Result<VerificationRunner<FixedClock>, String> {
            Err("runner backend unavailable".to_owned())
        };

        let report = run_advisory_verification(AdvisoryRunner::Build(&mut factory), &inputs);

        match report {
            AdvisoryVerification::Failed { reason } => {
                assert!(
                    reason.contains("unavailable"),
                    "factory failure must surface, got {reason:?}"
                );
            }
            other => panic!("factory error must report failure, got {other:?}"),
        }
        assert_eq!(fixture.effect_count(), 0);
    }

    #[test]
    fn built_runner_executes_like_ready_runner() {
        let fixture = Fixture::new("build-executes");
        let script = fixture.script("ok.sh", "#!/bin/sh\nexit 0\n");
        let runner = fixture.runner(Arc::new(move |_| Ok(script.clone())));
        let work = work_for(&fixture);
        let policy = AdvisoryVerificationPolicy {
            timeout: Duration::from_secs(60),
            per_stream_cap: 65536,
        };
        let inputs = AdvisoryStageInputs {
            policy: &policy,
            work: &work,
            workspace: &fixture.workspace,
            changed_paths: &["Cargo.toml".to_owned()],
            attempt_effects: &[],
            captured_at: FIXED_STAMP,
        };
        let mut slot = Some(runner);
        let mut factory = move || slot.take().ok_or_else(|| "runner consumed".to_owned());

        let report = run_advisory_verification(AdvisoryRunner::Build(&mut factory), &inputs);

        assert!(
            matches!(
                report,
                AdvisoryVerification::Completed {
                    commands_executed: 3
                }
            ),
            "built runner must execute, got {report:?}"
        );
        assert_eq!(fixture.effect_count(), 3);
    }

    #[test]
    fn empty_inputs_skip_v0() {
        let fixture = Fixture::new("empty-skips");
        let mut runner = fixture.runner(Arc::new(|program| {
            panic!("empty inputs must never resolve an executable, got {program}")
        }));
        let work = work_for(&fixture);

        let report = run_stage(
            &mut runner,
            &work,
            &fixture.workspace,
            &[],
            Duration::from_secs(60),
        );

        assert!(
            matches!(
                report,
                AdvisoryVerification::Skipped {
                    reason: scheduler::AdvisorySkipReason::LevelV0
                }
            ),
            "empty inputs are a no-op, got {report:?}"
        );
        assert_eq!(fixture.effect_count(), 0);
    }
}
