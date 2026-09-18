//! Pure Task domain entity: durable business intent plus lifecycle state.
//!
//! Provider-neutral by construction: there are no provider, model, session,
//! process, or runner fields here (`claude_session_id`-style fields are an
//! explicit domain-model violation per ADR-0005). Execution, Attempt, and
//! Session do not exist in this crate at all.
//!
//! State changes happen only through validated domain operations. There is
//! no `set_state` and no `complete_task`: the agent/runtime-facing
//! operation is [`Task::submit_result`] (`RUNNING → SUBMITTED`), while
//! [`Task::verification_succeeded`] (`VERIFYING → COMPLETED`) is shaped as
//! the trusted Evaluation-path transition.

use crate::error::{DomainError, DomainResult};
use crate::id::{ProjectId, TaskId};
use crate::state::{can_transition, TaskState};

/// Intent to move a task along its lifecycle. Trusted application services
/// will later persist one journal record per applied command; the command
/// names here (including [`TransitionCommand::Fail`] and
/// [`TransitionCommand::Cancel`]) let those services distinguish intent even
/// where the transition itself is not yet decided by architecture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransitionCommand {
    /// Authoring finished; work declared eligible (`DRAFT → READY`).
    MarkReady,
    /// Orchestration reserves the work (`READY → CLAIMED`).
    Claim,
    /// Execution begins (`CLAIMED → RUNNING`, `BLOCKED → RUNNING`,
    /// `RETRY_READY → RUNNING`).
    Start,
    /// Explicit impediment encountered (`RUNNING → BLOCKED`).
    Block { reason: String },
    /// Attempt claims readiness for review (`RUNNING → SUBMITTED`).
    /// This is the agent-facing `submit_result()`; it never completes.
    SubmitResult { summary: String },
    /// Routed to required review (`SUBMITTED → AWAITING_REVIEW`).
    RouteToReview,
    /// Admitted to evaluation (`AWAITING_REVIEW → VERIFYING`).
    BeginVerification,
    /// Trusted VERIFIED verdict (`VERIFYING → COMPLETED`). Evaluation path
    /// only — never agent-callable authority.
    VerificationSucceeded { verdict: String },
    /// Trusted REJECTED verdict (`VERIFYING → RETRY_READY`). Evaluation
    /// path only.
    VerificationFailed { verdict: String },
    /// Cancel the work. Incoming `CANCELLED` edges are unresolved
    /// (`task-engine.md` §16), so this is rejected until decided.
    Cancel { reason: String },
    /// Declare the work unviable. Incoming `FAILED` edges are unresolved
    /// (`task-engine.md` §16), so this is rejected until decided.
    Fail { reason: String },
}

impl TransitionCommand {
    /// The lifecycle edge this command intends, if the architecture has
    /// decided it. `None` means "not yet decided" and always fails closed.
    fn intended_edge(&self, from: TaskState) -> Option<(TaskState, TaskState)> {
        let to = match self {
            Self::MarkReady => TaskState::Ready,
            Self::Claim => TaskState::Claimed,
            Self::Start => {
                return match from {
                    TaskState::Claimed | TaskState::Blocked | TaskState::RetryReady => {
                        Some((from, TaskState::Running))
                    }
                    _ => None,
                };
            }
            Self::Block { .. } => TaskState::Blocked,
            Self::SubmitResult { .. } => TaskState::Submitted,
            Self::RouteToReview => TaskState::AwaitingReview,
            Self::BeginVerification => TaskState::Verifying,
            Self::VerificationSucceeded { .. } => TaskState::Completed,
            Self::VerificationFailed { .. } => TaskState::RetryReady,
            Self::Cancel { .. } => TaskState::Cancelled,
            Self::Fail { .. } => TaskState::Failed,
        };
        Some((from, to))
    }
}

/// Durable unit of intended work and acceptance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    id: TaskId,
    project_id: ProjectId,
    objective: String,
    parent: Option<TaskId>,
    depends_on: Vec<TaskId>,
    acceptance: Vec<String>,
    allowed_paths: Vec<String>,
    forbidden_paths: Vec<String>,
    /// Reserved for the G8 typed requirement-to-evidence model
    /// (ADR-0027 TASK-0093 amendment). Persisted and round-tripped, but the
    /// domain exposes no setter and protocol v6 authoring does not carry it,
    /// so on the supported control plane it is always empty. Do not start
    /// writing it without a contract that also names its reader.
    verification: Vec<String>,
    /// Reserved for the G8 typed requirement-to-evidence model
    /// (ADR-0027 TASK-0093 amendment). Same standing as `verification`.
    required_review: Vec<String>,
    risk: Option<String>,
    artifacts: Vec<String>,
    kind: Option<String>,
    state: TaskState,
}

impl Task {
    /// Creates a `DRAFT` task. `objective` is required durable intent and
    /// must be non-blank; everything else starts empty and is attached
    /// through validated operations.
    pub fn draft(
        id: TaskId,
        project_id: ProjectId,
        objective: impl Into<String>,
    ) -> DomainResult<Self> {
        let objective = objective.into();
        if objective.trim().is_empty() {
            return Err(DomainError::input("task objective must not be blank"));
        }
        Ok(Self {
            id,
            project_id,
            objective: objective.trim().to_owned(),
            parent: None,
            depends_on: Vec::new(),
            acceptance: Vec::new(),
            allowed_paths: Vec::new(),
            forbidden_paths: Vec::new(),
            verification: Vec::new(),
            required_review: Vec::new(),
            risk: None,
            artifacts: Vec::new(),
            kind: None,
            state: TaskState::Draft,
        })
    }

    /// Applies one validated lifecycle command. The single choke point for
    /// state changes: the edge must be intended by the command *and* present
    /// in the authoritative table, otherwise nothing mutates.
    pub fn apply(&mut self, command: TransitionCommand) -> DomainResult<TaskState> {
        require_nonblank_payload(&command)?;
        let from = self.state;
        match command.intended_edge(from) {
            Some((_, to)) if can_transition(from, to) => {
                self.state = to;
                Ok(to)
            }
            _ => Err(DomainError::transition(format!(
                "{from} cannot accept {}",
                command_name(&command)
            ))),
        }
    }

    /// `DRAFT → READY`: authoring finished, work declared eligible.
    pub fn mark_ready(&mut self) -> DomainResult<TaskState> {
        self.apply(TransitionCommand::MarkReady)
    }

    /// `READY → CLAIMED`: orchestration reserves the work for execution.
    pub fn claim(&mut self) -> DomainResult<TaskState> {
        self.apply(TransitionCommand::Claim)
    }

    /// `CLAIMED → RUNNING` (also `BLOCKED → RUNNING` resume and
    /// `RETRY_READY → RUNNING` retry): execution begins under an assignment.
    pub fn start(&mut self) -> DomainResult<TaskState> {
        self.apply(TransitionCommand::Start)
    }

    /// `RUNNING → BLOCKED`: an explicit, observable impediment.
    pub fn block(&mut self, reason: impl Into<String>) -> DomainResult<TaskState> {
        self.apply(TransitionCommand::Block {
            reason: reason.into(),
        })
    }

    /// `RUNNING → SUBMITTED`: the agent-facing `submit_result()`. A claim
    /// of readiness for review — never completion.
    pub fn submit_result(&mut self, summary: impl Into<String>) -> DomainResult<TaskState> {
        self.apply(TransitionCommand::SubmitResult {
            summary: summary.into(),
        })
    }

    /// `SUBMITTED → AWAITING_REVIEW`: routed to required review.
    pub fn route_to_review(&mut self) -> DomainResult<TaskState> {
        self.apply(TransitionCommand::RouteToReview)
    }

    /// `AWAITING_REVIEW → VERIFYING`: admitted to evaluation.
    pub fn begin_verification(&mut self) -> DomainResult<TaskState> {
        self.apply(TransitionCommand::BeginVerification)
    }

    /// `VERIFYING → COMPLETED`: trusted VERIFIED verdict. This is the only
    /// path to `COMPLETED`; it belongs to the Evaluation path, never to the
    /// implementing agent.
    pub fn verification_succeeded(
        &mut self,
        verdict: impl Into<String>,
    ) -> DomainResult<TaskState> {
        self.apply(TransitionCommand::VerificationSucceeded {
            verdict: verdict.into(),
        })
    }

    /// `VERIFYING → RETRY_READY`: trusted REJECTED verdict. Evaluation path
    /// only.
    pub fn verification_failed(&mut self, verdict: impl Into<String>) -> DomainResult<TaskState> {
        self.apply(TransitionCommand::VerificationFailed {
            verdict: verdict.into(),
        })
    }

    /// Records `depends_on` as a dependency of this task. Rejects
    /// self-edges, duplicates, and cross-project edges. Cycle validation
    /// across the project graph is [`validate_acyclic`](crate::validate_acyclic).
    pub fn add_dependency(&mut self, depends_on: &Task) -> DomainResult<()> {
        if depends_on.id == self.id {
            return Err(DomainError::SelfDependency(format!(
                "task {} cannot depend on itself",
                self.id
            )));
        }
        if depends_on.project_id != self.project_id {
            return Err(DomainError::ProjectMismatch(format!(
                "task {} (project {}) cannot depend on task {} (project {})",
                self.id, self.project_id, depends_on.id, depends_on.project_id
            )));
        }
        if self.depends_on.contains(&depends_on.id) {
            return Err(DomainError::DuplicateDependency(format!(
                "task {} already depends on {}",
                self.id, depends_on.id
            )));
        }
        self.depends_on.push(depends_on.id.clone());
        Ok(())
    }

    /// Attaches the optional parent/epic reference. Creating children never
    /// satisfies parent acceptance — the parent still completes only via its
    /// own verification transition.
    pub fn set_parent(&mut self, parent: TaskId) -> DomainResult<()> {
        if parent == self.id {
            return Err(DomainError::SelfDependency(format!(
                "task {} cannot be its own parent",
                self.id
            )));
        }
        self.parent = Some(parent);
        Ok(())
    }

    /// Authors the Task-owned planning metadata: allowed scope paths,
    /// acceptance obligations and baseline risk. Authoring belongs to
    /// `DRAFT`; once work is declared READY this intent is frozen. Values are
    /// trimmed, bounded and de-duplicated; any prior values are replaced
    /// atomically, so a refused call leaves the Task unchanged.
    pub fn define_planning_metadata(
        &mut self,
        allowed_paths: Vec<String>,
        acceptance: Vec<String>,
        risk: impl Into<String>,
    ) -> DomainResult<()> {
        if self.state != TaskState::Draft {
            return Err(DomainError::transition(format!(
                "planning metadata can only be authored while DRAFT; task {} is {}",
                self.id, self.state
            )));
        }
        let allowed_paths =
            bounded_metadata_list("allowed path", allowed_paths, MAX_TASK_ALLOWED_PATHS)?;
        let acceptance =
            bounded_metadata_list("acceptance criterion", acceptance, MAX_TASK_ACCEPTANCE)?;
        let risk = bounded_metadata_item("risk", risk.into())?;
        self.allowed_paths = allowed_paths;
        self.acceptance = acceptance;
        self.risk = Some(risk);
        Ok(())
    }

    /// Appends one acceptance criterion (durable intent; tests are evidence,
    /// never the criteria themselves).
    pub fn add_acceptance(&mut self, criterion: impl Into<String>) -> DomainResult<()> {
        let criterion = criterion.into();
        if criterion.trim().is_empty() {
            return Err(DomainError::input("acceptance criterion must not be blank"));
        }
        self.acceptance.push(criterion.trim().to_owned());
        Ok(())
    }

    /// Task identity.
    #[must_use]
    pub fn id(&self) -> &TaskId {
        &self.id
    }

    /// Owning project.
    #[must_use]
    pub fn project_id(&self) -> &ProjectId {
        &self.project_id
    }

    /// Intended work.
    #[must_use]
    pub fn objective(&self) -> &str {
        &self.objective
    }

    /// Optional parent/epic reference.
    #[must_use]
    pub fn parent(&self) -> Option<&TaskId> {
        self.parent.as_ref()
    }

    /// Direct dependency references.
    #[must_use]
    pub fn depends_on(&self) -> &[TaskId] {
        &self.depends_on
    }

    /// Acceptance criteria (durable intent).
    #[must_use]
    pub fn acceptance(&self) -> &[String] {
        &self.acceptance
    }

    /// Current lifecycle state.
    #[must_use]
    pub fn state(&self) -> TaskState {
        self.state
    }

    /// Exports the full validated task content as a plain-data snapshot for
    /// the outbound storage port. The snapshot carries no behavior; state
    /// changes still go through [`Task::apply`] only.
    #[must_use]
    pub fn snapshot(&self) -> TaskSnapshot {
        TaskSnapshot {
            id: self.id.clone(),
            project_id: self.project_id.clone(),
            objective: self.objective.clone(),
            parent: self.parent.clone(),
            depends_on: self.depends_on.clone(),
            acceptance: self.acceptance.clone(),
            allowed_paths: self.allowed_paths.clone(),
            forbidden_paths: self.forbidden_paths.clone(),
            verification: self.verification.clone(),
            required_review: self.required_review.clone(),
            risk: self.risk.clone(),
            artifacts: self.artifacts.clone(),
            kind: self.kind.clone(),
            state: self.state,
        }
    }

    /// Rebuilds a task from a storage-port snapshot, re-validating every
    /// field. This is reconstruction, not a transition: any of the 12
    /// canonical states is acceptable here (persistence stores current
    /// state), but malformed content fails instead of being repaired.
    pub fn restore(snapshot: TaskSnapshot) -> DomainResult<Self> {
        if snapshot.objective.trim().is_empty() {
            return Err(DomainError::input("task objective must not be blank"));
        }
        if let Some(parent) = &snapshot.parent {
            if *parent == snapshot.id {
                return Err(DomainError::SelfDependency(format!(
                    "task {} cannot be its own parent",
                    snapshot.id
                )));
            }
        }
        let mut seen: Vec<&TaskId> = Vec::with_capacity(snapshot.depends_on.len());
        for dep in &snapshot.depends_on {
            if *dep == snapshot.id {
                return Err(DomainError::SelfDependency(format!(
                    "task {} cannot depend on itself",
                    snapshot.id
                )));
            }
            if seen.contains(&dep) {
                return Err(DomainError::DuplicateDependency(format!(
                    "task {} depends on {} twice",
                    snapshot.id, dep
                )));
            }
            seen.push(dep);
        }
        for list in [
            &snapshot.acceptance,
            &snapshot.allowed_paths,
            &snapshot.forbidden_paths,
            &snapshot.verification,
            &snapshot.required_review,
            &snapshot.artifacts,
        ] {
            for item in list {
                if item.trim().is_empty() {
                    return Err(DomainError::input("task metadata items must not be blank"));
                }
            }
        }
        Ok(Self {
            id: snapshot.id,
            project_id: snapshot.project_id,
            objective: snapshot.objective,
            parent: snapshot.parent,
            depends_on: snapshot.depends_on,
            acceptance: snapshot.acceptance,
            allowed_paths: snapshot.allowed_paths,
            forbidden_paths: snapshot.forbidden_paths,
            verification: snapshot.verification,
            required_review: snapshot.required_review,
            risk: snapshot.risk,
            artifacts: snapshot.artifacts,
            kind: snapshot.kind,
            state: snapshot.state,
        })
    }
}

/// Plain-data image of a [`Task`] for the outbound storage port.
///
/// All fields are public because this type is a boundary DTO, not an
/// entity: constructing one grants nothing until [`Task::restore`]
/// validates it. Storage adapters persist snapshots and rebuild tasks
/// exclusively through `restore`, so database content can never bypass
/// domain validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSnapshot {
    /// Task identity (re-validated as an opaque id by the adapter codec).
    pub id: TaskId,
    /// Owning project.
    pub project_id: ProjectId,
    /// Intended work; must be non-blank.
    pub objective: String,
    /// Optional parent/epic reference.
    pub parent: Option<TaskId>,
    /// Direct dependency references; no self-edges, no duplicates.
    pub depends_on: Vec<TaskId>,
    /// Acceptance criteria.
    pub acceptance: Vec<String>,
    /// Scope contract: allowed paths.
    pub allowed_paths: Vec<String>,
    /// Scope contract: forbidden paths.
    pub forbidden_paths: Vec<String>,
    /// Required verification references.
    pub verification: Vec<String>,
    /// Required review references.
    pub required_review: Vec<String>,
    /// Risk metadata.
    pub risk: Option<String>,
    /// Artifact references.
    pub artifacts: Vec<String>,
    /// Routing/planning kind metadata.
    pub kind: Option<String>,
    /// Current lifecycle state (any canonical state; restore is not a
    /// transition).
    pub state: TaskState,
}

/// Upper bound on authored allowed scope paths per Task.
pub const MAX_TASK_ALLOWED_PATHS: usize = 32;
/// Upper bound on authored acceptance obligations per Task.
pub const MAX_TASK_ACCEPTANCE: usize = 64;
/// Upper bound in bytes on one authored planning metadata value.
pub const MAX_TASK_METADATA_ITEM_LEN: usize = 256;

fn bounded_metadata_list(
    field: &str,
    values: Vec<String>,
    max_items: usize,
) -> DomainResult<Vec<String>> {
    if values.is_empty() {
        return Err(DomainError::input(format!(
            "at least one {field} is required"
        )));
    }
    if values.len() > max_items {
        return Err(DomainError::input(format!(
            "at most {max_items} {field} values are allowed"
        )));
    }
    let mut out: Vec<String> = Vec::with_capacity(values.len());
    for value in values {
        let value = bounded_metadata_item(field, value)?;
        if !out.contains(&value) {
            out.push(value);
        }
    }
    Ok(out)
}

fn bounded_metadata_item(field: &str, value: String) -> DomainResult<String> {
    let value = value.trim();
    if value.is_empty() || value.chars().any(char::is_control) {
        return Err(DomainError::input(format!(
            "{field} must be non-blank with no control characters"
        )));
    }
    if value.len() > MAX_TASK_METADATA_ITEM_LEN {
        return Err(DomainError::input(format!(
            "{field} exceeds {MAX_TASK_METADATA_ITEM_LEN} bytes"
        )));
    }
    Ok(value.to_owned())
}

/// Commands carrying a reason/summary/verdict must not be blank: every
/// eventual durable mutation is reason-bearing (INV-010).
fn require_nonblank_payload(command: &TransitionCommand) -> DomainResult<()> {
    let payload = match command {
        TransitionCommand::Block { reason }
        | TransitionCommand::Cancel { reason }
        | TransitionCommand::Fail { reason } => reason,
        TransitionCommand::SubmitResult { summary } => summary,
        TransitionCommand::VerificationSucceeded { verdict }
        | TransitionCommand::VerificationFailed { verdict } => verdict,
        _ => return Ok(()),
    };
    if payload.trim().is_empty() {
        return Err(DomainError::input(format!(
            "{} requires a non-blank reason",
            command_name(command)
        )));
    }
    Ok(())
}

fn command_name(command: &TransitionCommand) -> &'static str {
    match command {
        TransitionCommand::MarkReady => "MarkReady",
        TransitionCommand::Claim => "Claim",
        TransitionCommand::Start => "Start",
        TransitionCommand::Block { .. } => "Block",
        TransitionCommand::SubmitResult { .. } => "SubmitResult",
        TransitionCommand::RouteToReview => "RouteToReview",
        TransitionCommand::BeginVerification => "BeginVerification",
        TransitionCommand::VerificationSucceeded { .. } => "VerificationSucceeded",
        TransitionCommand::VerificationFailed { .. } => "VerificationFailed",
        TransitionCommand::Cancel { .. } => "Cancel",
        TransitionCommand::Fail { .. } => "Fail",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft_task(id: &str) -> Task {
        Task::draft(
            TaskId::new(id).unwrap(),
            ProjectId::new("P-1").unwrap(),
            format!("objective {id}"),
        )
        .unwrap()
    }

    fn ready_task(id: &str) -> Task {
        let mut task = draft_task(id);
        task.mark_ready().unwrap();
        task
    }

    #[test]
    fn draft_starts_in_draft_with_required_fields() {
        let task = draft_task("T-1");
        assert_eq!(task.state(), TaskState::Draft);
        assert_eq!(task.id().as_str(), "T-1");
        assert_eq!(task.project_id().as_str(), "P-1");
        assert_eq!(task.objective(), "objective T-1");
        assert!(task.depends_on().is_empty());
    }

    #[test]
    fn draft_rejects_blank_objective() {
        assert!(Task::draft(
            TaskId::new("T-1").unwrap(),
            ProjectId::new("P-1").unwrap(),
            "   "
        )
        .is_err());
    }

    #[test]
    fn full_lifecycle_reaches_completed_through_verification_only() {
        let mut task = draft_task("T-1");
        assert_eq!(task.mark_ready().unwrap(), TaskState::Ready);
        assert_eq!(task.claim().unwrap(), TaskState::Claimed);
        assert_eq!(task.start().unwrap(), TaskState::Running);
        assert_eq!(
            task.submit_result("diff + tests").unwrap(),
            TaskState::Submitted
        );
        assert_eq!(task.route_to_review().unwrap(), TaskState::AwaitingReview);
        assert_eq!(task.begin_verification().unwrap(), TaskState::Verifying);
        assert_eq!(
            task.verification_succeeded("VERIFIED: evidence refs")
                .unwrap(),
            TaskState::Completed
        );
        assert_eq!(task.state(), TaskState::Completed);
    }

    #[test]
    fn retry_path_returns_to_running() {
        let mut task = ready_task("T-1");
        task.claim().unwrap();
        task.start().unwrap();
        task.submit_result("attempt").unwrap();
        task.route_to_review().unwrap();
        task.begin_verification().unwrap();
        assert_eq!(
            task.verification_failed("REJECTED: coverage gap").unwrap(),
            TaskState::RetryReady
        );
        assert_eq!(task.start().unwrap(), TaskState::Running);
    }

    #[test]
    fn block_resume_loop() {
        let mut task = ready_task("T-1");
        task.claim().unwrap();
        task.start().unwrap();
        assert_eq!(task.block("waiting on input").unwrap(), TaskState::Blocked);
        assert_eq!(task.start().unwrap(), TaskState::Running);
    }

    #[test]
    fn direct_paths_to_completed_are_rejected() {
        // DRAFT → COMPLETED rejected.
        let mut task = draft_task("T-1");
        assert!(task.verification_succeeded("x").is_err());
        assert_eq!(task.state(), TaskState::Draft);
        // READY → COMPLETED rejected.
        let mut task = ready_task("T-2");
        assert!(task.verification_succeeded("x").is_err());
        // RUNNING → COMPLETED rejected (agent cannot complete its own work).
        task.claim().unwrap();
        task.start().unwrap();
        assert!(task.verification_succeeded("self-report").is_err());
        assert_eq!(task.state(), TaskState::Running);
        // SUBMITTED → COMPLETED rejected (submission is a claim, not proof).
        task.submit_result("done?").unwrap();
        assert!(task.verification_succeeded("x").is_err());
        assert_eq!(task.state(), TaskState::Submitted);
    }

    #[test]
    fn terminal_states_reject_everything() {
        let mut completed = draft_task("T-1");
        completed.mark_ready().unwrap();
        completed.claim().unwrap();
        completed.start().unwrap();
        completed.submit_result("s").unwrap();
        completed.route_to_review().unwrap();
        completed.begin_verification().unwrap();
        completed.verification_succeeded("v").unwrap();
        for command in [
            TransitionCommand::MarkReady,
            TransitionCommand::Claim,
            TransitionCommand::Start,
            TransitionCommand::Block { reason: "r".into() },
            TransitionCommand::SubmitResult {
                summary: "s".into(),
            },
            TransitionCommand::RouteToReview,
            TransitionCommand::BeginVerification,
            TransitionCommand::VerificationSucceeded {
                verdict: "v".into(),
            },
            TransitionCommand::VerificationFailed {
                verdict: "v".into(),
            },
        ] {
            assert!(completed.apply(command).is_err());
        }
    }

    #[test]
    fn failed_and_cancelled_are_unreachable_until_decided() {
        let mut task = ready_task("T-1");
        assert!(matches!(
            task.apply(TransitionCommand::Fail { reason: "r".into() }),
            Err(DomainError::InvalidTransition(_))
        ));
        assert!(matches!(
            task.apply(TransitionCommand::Cancel { reason: "r".into() }),
            Err(DomainError::InvalidTransition(_))
        ));
        assert_eq!(task.state(), TaskState::Ready);
    }

    #[test]
    fn submitted_cannot_bypass_review() {
        let mut task = ready_task("T-1");
        task.claim().unwrap();
        task.start().unwrap();
        task.submit_result("s").unwrap();
        assert!(task.begin_verification().is_err());
        assert_eq!(task.state(), TaskState::Submitted);
    }

    #[test]
    fn blank_reasons_are_rejected_without_mutation() {
        let mut task = ready_task("T-1");
        task.claim().unwrap();
        task.start().unwrap();
        assert!(task.block("   ").is_err());
        assert_eq!(task.state(), TaskState::Running);
        assert!(task.submit_result("").is_err());
        assert_eq!(task.state(), TaskState::Running);
    }

    #[test]
    fn failed_transition_leaves_state_unchanged() {
        let mut task = draft_task("T-1");
        assert!(task.claim().is_err());
        assert_eq!(task.state(), TaskState::Draft);
    }

    #[test]
    fn verification_succeeded_fails_outside_verifying() {
        // Every legally reachable non-VERIFYING state rejects the trusted
        // completion op; only VERIFYING accepts it (proven in
        // full_lifecycle_reaches_completed_through_verification_only).
        let mut task = draft_task("T-x");
        assert!(task.verification_succeeded("x").is_err());
        task.mark_ready().unwrap();
        assert!(task.verification_succeeded("x").is_err());
        task.claim().unwrap();
        assert!(task.verification_succeeded("x").is_err());
        task.start().unwrap();
        assert!(task.verification_succeeded("self-report").is_err());
        task.block("impediment").unwrap();
        assert!(task.verification_succeeded("x").is_err());
        task.start().unwrap();
        task.submit_result("s").unwrap();
        assert!(task.verification_succeeded("x").is_err());
        task.route_to_review().unwrap();
        assert!(task.verification_succeeded("x").is_err());
        task.begin_verification().unwrap();
        assert!(task.verification_succeeded("VERIFIED").is_ok());
    }

    #[test]
    fn parent_reference_never_completes_the_parent() {
        let mut parent = draft_task("P-TASK");
        let child = draft_task("C-TASK");
        parent.set_parent(child.id().clone()).unwrap();
        assert_eq!(parent.parent().unwrap().as_str(), "C-TASK");
        // A parent with children still cannot complete without verification.
        assert!(parent.verification_succeeded("children exist").is_err());
        assert_eq!(parent.parent().unwrap().as_str(), "C-TASK");
    }

    #[test]
    fn self_parent_is_rejected() {
        let mut task = draft_task("T-1");
        assert!(task.set_parent(TaskId::new("T-1").unwrap()).is_err());
    }

    #[test]
    fn acceptance_criteria_accumulate() {
        let mut task = draft_task("T-1");
        task.add_acceptance("compiles").unwrap();
        task.add_acceptance("tests pass").unwrap();
        assert_eq!(task.acceptance(), ["compiles", "tests pass"]);
        assert!(task.add_acceptance("  ").is_err());
    }

    #[test]
    fn planning_metadata_is_authored_only_while_draft_and_bounded() {
        let mut task = draft_task("T-1");
        task.define_planning_metadata(
            vec![" repo:src ".into(), "repo:src".into()],
            vec!["criterion-a".into()],
            " low ",
        )
        .unwrap();
        let snapshot = task.snapshot();
        assert_eq!(snapshot.allowed_paths, ["repo:src"]);
        assert_eq!(snapshot.acceptance, ["criterion-a"]);
        assert_eq!(snapshot.risk.as_deref(), Some("low"));
        assert_eq!(task.state(), TaskState::Draft);

        for (paths, acceptance, risk) in [
            (vec![], vec!["c".to_owned()], "low"),
            (vec!["repo:src".to_owned()], vec![], "low"),
            (vec!["repo:src".to_owned()], vec!["c".to_owned()], " "),
            (vec![" ".to_owned()], vec!["c".to_owned()], "low"),
            (vec!["repo:\u{0}".to_owned()], vec!["c".to_owned()], "low"),
            (
                vec!["x".repeat(MAX_TASK_METADATA_ITEM_LEN + 1)],
                vec!["c".to_owned()],
                "low",
            ),
            (
                (0..=MAX_TASK_ALLOWED_PATHS)
                    .map(|i| format!("repo:{i}"))
                    .collect(),
                vec!["c".to_owned()],
                "low",
            ),
        ] {
            assert!(matches!(
                task.define_planning_metadata(paths, acceptance, risk),
                Err(DomainError::InvalidInput(_))
            ));
        }
        assert_eq!(
            task.snapshot(),
            snapshot,
            "refusals leave metadata unchanged"
        );

        task.mark_ready().unwrap();
        assert!(matches!(
            task.define_planning_metadata(vec!["repo:other".into()], vec!["c".into()], "high"),
            Err(DomainError::InvalidTransition(_))
        ));
        assert_eq!(task.snapshot().allowed_paths, ["repo:src"]);
    }

    #[test]
    fn dependency_edges_enforce_local_invariants() {
        let mut a = draft_task("A");
        let b = draft_task("B");
        a.add_dependency(&b).unwrap();
        assert_eq!(a.depends_on(), [TaskId::new("B").unwrap()]);
        // Duplicate edge rejected.
        assert!(matches!(
            a.add_dependency(&b),
            Err(DomainError::DuplicateDependency(_))
        ));
        // Self edge rejected.
        let a_clone = a.clone();
        assert!(matches!(
            a.add_dependency(&a_clone),
            Err(DomainError::SelfDependency(_))
        ));
    }

    #[test]
    fn cross_project_dependency_is_rejected() {
        let mut a = draft_task("A");
        let foreign = Task::draft(
            TaskId::new("F").unwrap(),
            ProjectId::new("OTHER").unwrap(),
            "foreign",
        )
        .unwrap();
        assert!(matches!(
            a.add_dependency(&foreign),
            Err(DomainError::ProjectMismatch(_))
        ));
        assert!(a.depends_on().is_empty());
    }

    #[test]
    fn snapshot_round_trip_preserves_everything() {
        let mut task = draft_task("T-1");
        task.mark_ready().unwrap();
        task.add_acceptance("compiles").unwrap();
        let snapshot = task.snapshot();
        let rebuilt = Task::restore(snapshot).unwrap();
        assert_eq!(rebuilt, task);
    }

    #[test]
    fn restore_accepts_any_canonical_state_without_transition() {
        // Reconstruction is not a transition: states unreachable through
        // current public ops (FAILED, CANCELLED) still restore, so storage
        // can round-trip them without new transition authority.
        for state in TaskState::all() {
            let snapshot = TaskSnapshot {
                id: TaskId::new("T-1").unwrap(),
                project_id: ProjectId::new("P-1").unwrap(),
                objective: "objective".into(),
                parent: None,
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                allowed_paths: Vec::new(),
                forbidden_paths: Vec::new(),
                verification: Vec::new(),
                required_review: Vec::new(),
                risk: None,
                artifacts: Vec::new(),
                kind: None,
                state,
            };
            let rebuilt = Task::restore(snapshot).unwrap();
            assert_eq!(rebuilt.state(), state);
        }
    }

    #[test]
    fn restore_rejects_malformed_content() {
        let mut snapshot = draft_task("T-1").snapshot();
        snapshot.objective = "   ".into();
        assert!(Task::restore(snapshot).is_err());

        let mut snapshot = draft_task("T-1").snapshot();
        snapshot.parent = Some(TaskId::new("T-1").unwrap());
        assert!(Task::restore(snapshot).is_err());

        let mut snapshot = draft_task("T-1").snapshot();
        snapshot.depends_on = vec![TaskId::new("T-1").unwrap()];
        assert!(Task::restore(snapshot).is_err());

        let mut snapshot = draft_task("T-1").snapshot();
        snapshot.depends_on = vec![TaskId::new("B").unwrap(), TaskId::new("B").unwrap()];
        assert!(Task::restore(snapshot).is_err());

        let mut snapshot = draft_task("T-1").snapshot();
        snapshot.acceptance = vec!["  ".into()];
        assert!(Task::restore(snapshot).is_err());
    }
}
