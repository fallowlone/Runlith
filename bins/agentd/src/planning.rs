//! G7 production planning composition (TASK-0078).
//!
//! `agentd` is the only composition root that instantiates the concrete
//! Claude `PlannerPort`. Every real planner call is authorized by the G6
//! Policy boundary and receives a Runner-owned `SandboxedLaunch` before the
//! provider adapter can spawn. ADR-0027 makes the typed PlannerPort request
//! the complete planner-readable product context, so the provider receives no
//! repository filesystem authority. Its cwd is an empty app-owned isolation
//! anchor; provider support paths stay adapter/backend-owned.

use std::cell::Cell;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use plan_domain::{
    AtomicityAssessment, AtomicityRequest, DecompositionRequest, PlanChangeSet, PlannerCallSpend,
    PlannerFault, PlannerPort, ReplanRequest,
};
use task_domain::TaskId;

use crate::execution::{
    build_provider_launch_security_for_context, prepare_provider_launch, ExecutionComposition,
    ProviderLaunchContext, ProviderWorkspaceAccess,
};

const PLANNER_LAUNCH_REQUESTER: &str = "planning-service";
static PLANNER_OPERATION_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Production Claude planner wrapper. The inner provider adapter remains
/// proposal-only; this wrapper owns only the G6/Runner launch boundary.
pub struct SecuredClaudePlanner {
    inner: provider_claude::ClaudePlanner,
    db_path: PathBuf,
    authority_task_ref: String,
}

impl SecuredClaudePlanner {
    fn call_secured<T>(
        &self,
        kind: &str,
        request_task_ref: &str,
        invoke: impl FnOnce(&provider_claude::ClaudePlanner) -> Result<T, PlannerFault>,
    ) -> Result<T, PlannerFault> {
        if request_task_ref != self.authority_task_ref {
            return Err(not_started(
                "planner request task does not match the composition-bound task",
            ));
        }

        let call_id = PLANNER_OPERATION_COUNTER.fetch_add(1, Ordering::SeqCst);
        let launch = build_provider_launch_security_for_context(ProviderLaunchContext {
            task_id: &self.authority_task_ref,
            execution_id: None,
            attempt_id: None,
            operation_id: format!("planner.launch:{kind}:{call_id}"),
            requester: PLANNER_LAUNCH_REQUESTER,
            workspace: self.inner.workdir(),
            executable: self.inner.executable(),
            workspace_access: ProviderWorkspaceAccess::ReadOnly,
        })
        .map_err(not_started)?;
        let support_read = self
            .inner
            .isolation_support_read_roots()
            .map_err(not_started)?;
        let support_write = self
            .inner
            .isolation_support_write_roots()
            .map_err(not_started)?;
        let environment = self.inner.child_environment().map_err(not_started)?;
        let prepared = prepare_provider_launch(
            &self.db_path,
            &launch,
            support_read,
            support_write,
            environment,
        )
        .map_err(not_started)?;

        // Inside the G6 boundary the inner adapter reports its own spend. A
        // boundary error before the adapter ran (isolation enforcement) is
        // zero spend; one after it ran (post-call audit) discarded that
        // evidence, so it is charged as unmeasured spend.
        let adapter_invoked = Cell::new(false);
        prepared
            .invoke(|sandboxed_launch| {
                adapter_invoked.set(true);
                let planner = self.inner.clone().with_sandboxed_launch(sandboxed_launch);
                invoke(&planner)
            })
            .map_err(|detail| PlannerFault::ProviderFailure {
                typed_cause: detail,
                spend: if adapter_invoked.get() {
                    PlannerCallSpend::Unmeasured
                } else {
                    PlannerCallSpend::NotStarted
                },
            })?
    }
}

impl PlannerPort for SecuredClaudePlanner {
    fn assess_atomicity(
        &self,
        input: AtomicityRequest,
    ) -> Result<AtomicityAssessment, PlannerFault> {
        let task_ref = input.task_ref.clone();
        self.call_secured("assess-atomicity", &task_ref, |planner| {
            planner.assess_atomicity(input)
        })
    }

    fn propose_decomposition(
        &self,
        input: DecompositionRequest,
    ) -> Result<PlanChangeSet, PlannerFault> {
        let task_ref = input.parent_task_ref.clone();
        self.call_secured("decomposition", &task_ref, |planner| {
            planner.propose_decomposition(input)
        })
    }

    fn propose_replan(&self, input: ReplanRequest) -> Result<PlanChangeSet, PlannerFault> {
        let task_ref = input.branch_root_ref.clone();
        self.call_secured("replan", &task_ref, |planner| planner.propose_replan(input))
    }
}

impl ExecutionComposition {
    /// Opens the trusted Planning application service over the daemon-owned
    /// canonical database.
    pub fn planning_service(
        &self,
    ) -> Result<
        app_services::PlanningService<app_services::SystemClock>,
        app_services::PlanningServiceError,
    > {
        app_services::PlanningService::open(self.db_path(), app_services::SystemClock)
    }

    /// Instantiates the concrete production Claude planner for one durable
    /// Task. ADR-0027 makes the typed PlannerPort request the complete set of
    /// product inputs the planner may read, so this path deliberately does
    /// not grant repository filesystem access. The provider runs from an
    /// empty app-owned per-task directory used only as a sandbox/cwd anchor.
    /// The returned planner is still incapable of spawning until each call
    /// crosses Policy + Runner inside [`SecuredClaudePlanner`].
    pub fn secured_claude_planner(
        &self,
        authority_task_ref: impl Into<String>,
    ) -> Result<SecuredClaudePlanner, String> {
        let authority_task_ref = authority_task_ref.into();
        let task_id = TaskId::new(authority_task_ref.clone())
            .map_err(|error| format!("planner authority task ref is invalid: {error}"))?;
        let store = storage_sqlite::SqliteStore::open(self.db_path())
            .map_err(|error| format!("planner authority store unavailable: {error}"))?;
        if store
            .get_task(&task_id)
            .map_err(|error| format!("planner authority task lookup failed: {error}"))?
            .is_none()
        {
            return Err(format!(
                "planner authority task {} does not exist",
                task_id.as_str()
            ));
        }

        let workdir = planner_workdir(self.db_path(), &task_id)?;
        let executable = self.resolved_provider_executable()?;
        let config = provider_claude::ClaudeConfig::default()
            .with_executable(executable)
            .map_err(|error| format!("planner provider configuration refused: {error}"))?;
        let inner = provider_claude::ClaudePlanner::bind(
            config,
            workdir,
            self.provider_environment_base()?,
        )?;
        Ok(SecuredClaudePlanner {
            inner,
            db_path: self.db_path().to_path_buf(),
            authority_task_ref,
        })
    }
}

fn planner_workdir(db_path: &std::path::Path, task_id: &TaskId) -> Result<PathBuf, String> {
    let parent = db_path.parent().ok_or_else(|| {
        format!(
            "planner workspace unavailable: database path {} has no parent",
            db_path.display()
        )
    })?;
    let root = parent.join("planner-workdirs");
    crate::data_dir::ensure_data_dir(&root)
        .map_err(|error| format!("planner workspace root unavailable: {error}"))?;
    let workdir = root.join(format!(
        "task-{}",
        app_services::managed_slug(task_id.as_str())
    ));
    crate::data_dir::ensure_data_dir(&workdir)
        .map_err(|error| format!("planner workspace unavailable: {error}"))?;
    Ok(workdir)
}

/// Refusal before the G6 boundary let any provider process start.
fn not_started(detail: impl Into<String>) -> PlannerFault {
    PlannerFault::ProviderFailure {
        typed_cause: detail.into(),
        spend: PlannerCallSpend::NotStarted,
    }
}
