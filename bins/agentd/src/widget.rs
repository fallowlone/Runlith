//! G16 widget authoring (TASK-0230).
//!
//! A person's description becomes a validated proposal through one provider
//! call. The call runs under the same G6 Policy and Runner boundary as the
//! planner, with no tools and an empty working directory, so the provider sees
//! only the description. Its answer is untrusted (INV-093) and crosses
//! [`widget_spec::parse_model_answer`] before anything else sees it.
//!
//! A call takes seconds, so a draft is a job: `start` returns at once, `status`
//! polls, `cancel` kills the child. One draft runs at a time, which also bounds
//! provider spend. Jobs live in memory only.

use std::collections::VecDeque;
use std::ffi::OsStr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ipc_contracts::proto::{
    widget_core_request, widget_core_response, BusinessError, BusinessErrorCode, WidgetCoreRequest,
    WidgetCoreResponse, WidgetDraftState, WidgetDraftStatus,
};
use provider_claude::{CancelFlag, ClaudeConfig, ClaudeFailure, ClaudeStructuredCall};
use widget_spec::{ModelAnswer, SourceType};

use crate::execution::{
    build_provider_launch_security_for_context, prepare_provider_launch, ExecutionComposition,
    ProviderLaunchContext, ProviderWorkspaceAccess,
};

const DRAFT_TIMEOUT: Duration = Duration::from_secs(60);
const KEEP_FINISHED: usize = 8;
const LAUNCH_REQUESTER: &str = "widget-authoring";
const LAUNCH_ORIGIN: &str = "widget-authoring";

/// How one draft ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DraftResult {
    Proposed {
        source: SourceType,
        repo: Option<String>,
    },
    Refused,
    Failed {
        code: &'static str,
        detail: String,
    },
    Cancelled,
}

type RunDraft = Arc<dyn Fn(&str, &CancelFlag) -> DraftResult + Send + Sync>;

#[derive(Default)]
struct Jobs {
    next_id: u64,
    running: Option<(String, CancelFlag)>,
    finished: VecDeque<(String, DraftResult)>,
}

/// Shared by every connection: a draft outlives the request that started it.
pub struct WidgetAuthoring {
    jobs: Arc<Mutex<Jobs>>,
    run: RunDraft,
}

pub type WidgetHandle = Arc<WidgetAuthoring>;

impl WidgetAuthoring {
    #[must_use]
    pub fn production(composition: crate::execution::ExecutionHandle) -> WidgetHandle {
        Self::with_runner(Arc::new(move |description, cancel| {
            run_secured_draft(&composition, description, cancel)
        }))
    }

    fn with_runner(run: RunDraft) -> WidgetHandle {
        Arc::new(Self {
            jobs: Arc::new(Mutex::new(Jobs::default())),
            run,
        })
    }

    fn start(&self, description: &str) -> WidgetCoreResponse {
        let description = match widget_spec::validate_description(description) {
            Ok(text) => text.to_owned(),
            Err(error) => return failure(BusinessErrorCode::InvalidArgument, error.to_string()),
        };
        let Ok(mut jobs) = self.jobs.lock() else {
            return poisoned();
        };
        if jobs.running.is_some() {
            return failure(
                BusinessErrorCode::Conflict,
                "another widget draft is still running".to_owned(),
            );
        }
        jobs.next_id += 1;
        let job_id = format!("draft-{}", jobs.next_id);
        let cancel = CancelFlag::fresh();
        jobs.running = Some((job_id.clone(), cancel.clone()));
        drop(jobs);

        let shared = Arc::clone(&self.jobs);
        let run = Arc::clone(&self.run);
        let finishing = job_id.clone();
        // A provider call blocks for seconds: its own thread, so no connection
        // or the serve loop ever waits on it.
        let spawned = std::thread::Builder::new()
            .name("widget-draft".to_owned())
            .spawn(move || {
                let result = run(&description, &cancel);
                if let Ok(mut jobs) = shared.lock() {
                    jobs.running = None;
                    jobs.finished.push_back((finishing, result));
                    while jobs.finished.len() > KEEP_FINISHED {
                        jobs.finished.pop_front();
                    }
                }
            });
        if spawned.is_err() {
            if let Ok(mut jobs) = self.jobs.lock() {
                jobs.running = None;
            }
            return failure(
                BusinessErrorCode::Internal,
                "could not start the widget draft".to_owned(),
            );
        }
        status_response(job_id, None)
    }

    fn status(&self, job_id: &str) -> WidgetCoreResponse {
        let Ok(jobs) = self.jobs.lock() else {
            return poisoned();
        };
        if jobs.running.as_ref().is_some_and(|(id, _)| id == job_id) {
            return status_response(job_id.to_owned(), None);
        }
        match jobs.finished.iter().find(|(id, _)| id == job_id) {
            Some((id, result)) => status_response(id.clone(), Some(result)),
            None => failure(
                BusinessErrorCode::NotFound,
                format!("no widget draft {job_id}"),
            ),
        }
    }

    fn cancel(&self, job_id: &str) -> WidgetCoreResponse {
        {
            let Ok(jobs) = self.jobs.lock() else {
                return poisoned();
            };
            if let Some((id, cancel)) = jobs.running.as_ref() {
                if id == job_id {
                    cancel.cancel();
                }
            }
        }
        self.status(job_id)
    }
}

/// Answers one request without blocking: the provider call has its own thread.
pub fn dispatch(service: &WidgetAuthoring, request: WidgetCoreRequest) -> WidgetCoreResponse {
    match request.payload {
        Some(widget_core_request::Payload::Start(start)) => service.start(&start.description),
        Some(widget_core_request::Payload::Status(status)) => service.status(&status.job_id),
        Some(widget_core_request::Payload::Cancel(cancel)) => service.cancel(&cancel.job_id),
        None => failure(
            BusinessErrorCode::InvalidArgument,
            "empty widget request".to_owned(),
        ),
    }
}

#[must_use]
pub fn unavailable() -> WidgetCoreResponse {
    failure(
        BusinessErrorCode::ProviderUnavailable,
        "widget authoring needs a database-backed agentd".to_owned(),
    )
}

fn status_response(job_id: String, result: Option<&DraftResult>) -> WidgetCoreResponse {
    let mut status = WidgetDraftStatus {
        job_id,
        state: WidgetDraftState::WidgetDraftRunning as i32,
        ..WidgetDraftStatus::default()
    };
    match result {
        None => {}
        Some(DraftResult::Proposed { source, repo }) => {
            status.state = WidgetDraftState::WidgetDraftProposed as i32;
            status.source_type = source.as_wire().to_owned();
            status.repo = repo.clone().unwrap_or_default();
        }
        Some(DraftResult::Refused) => status.state = WidgetDraftState::WidgetDraftRefused as i32,
        Some(DraftResult::Cancelled) => {
            status.state = WidgetDraftState::WidgetDraftCancelled as i32;
        }
        Some(DraftResult::Failed { code, detail }) => {
            status.state = WidgetDraftState::WidgetDraftFailed as i32;
            status.failure_code = (*code).to_owned();
            status.failure_detail = detail.clone();
        }
    }
    WidgetCoreResponse {
        payload: Some(widget_core_response::Payload::Status(status)),
    }
}

fn failure(code: BusinessErrorCode, message: String) -> WidgetCoreResponse {
    WidgetCoreResponse {
        payload: Some(widget_core_response::Payload::Error(BusinessError {
            code: code as i32,
            message,
        })),
    }
}

fn poisoned() -> WidgetCoreResponse {
    failure(
        BusinessErrorCode::Internal,
        "widget authoring state is unavailable".to_owned(),
    )
}

fn failed(code: &'static str, detail: impl Into<String>) -> DraftResult {
    DraftResult::Failed {
        code,
        detail: provider_claude::bounded_detail(detail.into()),
    }
}

/// Turns the provider's answer into a result. The only reader of model output.
fn admit(answer: &str) -> DraftResult {
    match widget_spec::parse_model_answer(answer) {
        Ok(ModelAnswer::Metric { source, repo }) => DraftResult::Proposed {
            source,
            repo: repo.map(String::from),
        },
        Ok(ModelAnswer::Refused {}) => DraftResult::Refused,
        // The answer itself is not echoed: it is untrusted text.
        Err(_) => failed(
            "bad_output",
            "the provider's answer did not pass validation",
        ),
    }
}

fn classify(failure: ClaudeFailure) -> DraftResult {
    let detail = failure.to_string();
    match failure {
        ClaudeFailure::Cancelled => DraftResult::Cancelled,
        ClaudeFailure::CliMissing { .. } => failed("provider_missing", detail),
        ClaudeFailure::Incompatible { .. } => failed("provider_incompatible", detail),
        ClaudeFailure::AuthRequired { .. } | ClaudeFailure::CredentialRejected { .. } => {
            failed("not_authenticated", detail)
        }
        ClaudeFailure::TimedOut { .. } => failed("timed_out", detail),
        ClaudeFailure::MalformedOutput { .. } => failed("bad_output", detail),
        ClaudeFailure::Transient { .. } => failed("provider_unavailable", detail),
        _ => failed("failed", detail),
    }
}

fn run_secured_draft(
    composition: &ExecutionComposition,
    description: &str,
    cancel: &CancelFlag,
) -> DraftResult {
    let executable = match composition.resolved_provider_executable() {
        Ok(path) => path,
        Err(detail) => return failed("provider_missing", detail),
    };
    let detection = provider_claude::detect(executable.as_os_str());
    match detection.status {
        provider_claude::ProviderAvailability::Available => {}
        provider_claude::ProviderAvailability::Missing => {
            return failed("provider_missing", "the claude CLI was not found");
        }
        _ => {
            return failed(
                "provider_incompatible",
                "the installed claude CLI did not answer as expected",
            );
        }
    }
    let capabilities =
        provider_claude::probe_capabilities(executable.as_os_str(), detection.version);
    if !capabilities.structured_answers {
        return failed(
            "provider_incompatible",
            "the installed claude CLI has no --json-schema option; update it",
        );
    }
    match draft_under_boundary(composition, executable.as_os_str(), description, cancel) {
        Ok(result) => result,
        Err(failure) => failure,
    }
}

fn draft_under_boundary(
    composition: &ExecutionComposition,
    executable: &OsStr,
    description: &str,
    cancel: &CancelFlag,
) -> Result<DraftResult, DraftResult> {
    let workdir = composition
        .db_path()
        .parent()
        .ok_or_else(|| failed("failed", "the database path has no parent"))?
        .join("widget-workdir");
    crate::data_dir::ensure_data_dir(&workdir)
        .map_err(|error| failed("failed", format!("widget workdir unavailable: {error}")))?;
    let base = composition
        .provider_environment_base()
        .map_err(|detail| failed("failed", detail))?;
    let config = ClaudeConfig::default()
        .with_executable(executable)
        .and_then(|config| config.with_timeout(DRAFT_TIMEOUT))
        .map_err(|detail| failed("failed", detail))?;
    let call = ClaudeStructuredCall::bind(
        config,
        widget_spec::AUTHOR_ROLE,
        widget_spec::MODEL_ANSWER_SCHEMA,
        workdir,
        base,
    )
    .map_err(|detail| failed("failed", detail))?;

    let credential = composition.provider_credential(executable);
    if let Ok(credential) = &credential {
        credential
            .preflight(executable)
            .map_err(|detail| failed("not_authenticated", detail))?;
    }
    let launch = build_provider_launch_security_for_context(ProviderLaunchContext {
        task_id: LAUNCH_ORIGIN,
        execution_id: None,
        attempt_id: None,
        operation_id: format!("widget.draft:{}", next_operation()),
        requester: LAUNCH_REQUESTER,
        workspace: call.workdir(),
        executable: call.executable(),
        workspace_access: ProviderWorkspaceAccess::ReadOnly,
        credential,
    })
    .map_err(|detail| failed("failed", detail))?;
    let support_read = call
        .isolation_support_read_roots()
        .map_err(|detail| failed("failed", detail))?;
    let support_write = call
        .isolation_support_write_roots()
        .map_err(|detail| failed("failed", detail))?;
    let environment = call
        .child_environment()
        .map_err(|detail| failed("failed", detail))?;
    let prepared = prepare_provider_launch(
        composition.db_path(),
        &launch,
        support_read,
        support_write,
        environment,
    )
    .map_err(|detail| failed("failed", detail))?;
    let answer = prepared
        .invoke(|sandboxed, secrets| {
            call.clone()
                .with_sandboxed_launch_and_secrets(sandboxed, secrets)
                .with_cancel_flag(cancel.clone())
                .run(description)
        })
        .map_err(|detail| failed("failed", detail))?;
    Ok(match answer {
        Ok(text) => admit(&text),
        Err(failure) => classify(failure),
    })
}

fn next_operation() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    COUNTER.fetch_add(1, Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ipc_contracts::proto::{
        WidgetDraftCancelRequest, WidgetDraftStartRequest, WidgetDraftStatusRequest,
    };
    use std::time::Instant;

    #[test]
    fn provider_outage_is_not_a_generic_failure() {
        let result = classify(ClaudeFailure::Transient {
            detail: "exit 1: overloaded".to_owned(),
        });
        assert!(matches!(
            result,
            DraftResult::Failed {
                code: "provider_unavailable",
                ..
            }
        ));
    }

    fn start_request(description: &str) -> WidgetCoreRequest {
        WidgetCoreRequest {
            payload: Some(widget_core_request::Payload::Start(
                WidgetDraftStartRequest {
                    description: description.to_owned(),
                },
            )),
        }
    }

    fn status_of(service: &WidgetAuthoring, job_id: &str) -> WidgetCoreResponse {
        dispatch(
            service,
            WidgetCoreRequest {
                payload: Some(widget_core_request::Payload::Status(
                    WidgetDraftStatusRequest {
                        job_id: job_id.to_owned(),
                    },
                )),
            },
        )
    }

    fn job_of(response: &WidgetCoreResponse) -> WidgetDraftStatus {
        match &response.payload {
            Some(widget_core_response::Payload::Status(status)) => status.clone(),
            other => panic!("expected a status, got {other:?}"),
        }
    }

    fn error_code(response: &WidgetCoreResponse) -> i32 {
        match &response.payload {
            Some(widget_core_response::Payload::Error(error)) => error.code,
            other => panic!("expected an error, got {other:?}"),
        }
    }

    fn wait_done(service: &WidgetAuthoring, job_id: &str) -> WidgetDraftStatus {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let status = job_of(&status_of(service, job_id));
            if status.state != WidgetDraftState::WidgetDraftRunning as i32 {
                return status;
            }
            assert!(Instant::now() < deadline, "draft never finished");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn start_returns_at_once_and_a_status_call_stays_responsive() {
        let service = WidgetAuthoring::with_runner(Arc::new(|_, _| {
            std::thread::sleep(Duration::from_millis(300));
            DraftResult::Refused
        }));
        let started = Instant::now();
        let job = job_of(&dispatch(&service, start_request("погода")));
        let status = job_of(&status_of(&service, &job.job_id));
        assert_eq!(status.state, WidgetDraftState::WidgetDraftRunning as i32);
        assert!(
            started.elapsed() < Duration::from_millis(200),
            "start and status must not wait for the provider"
        );
        assert_eq!(
            wait_done(&service, &job.job_id).state,
            WidgetDraftState::WidgetDraftRefused as i32
        );
    }

    #[test]
    fn only_one_draft_runs_at_a_time() {
        let service = WidgetAuthoring::with_runner(Arc::new(|_, _| {
            std::thread::sleep(Duration::from_millis(200));
            DraftResult::Refused
        }));
        let first = job_of(&dispatch(&service, start_request("раз")));
        assert_eq!(
            error_code(&dispatch(&service, start_request("два"))),
            BusinessErrorCode::Conflict as i32
        );
        wait_done(&service, &first.job_id);
        job_of(&dispatch(&service, start_request("три")));
    }

    #[test]
    fn cancel_reaches_the_running_call() {
        let service = WidgetAuthoring::with_runner(Arc::new(|_, cancel| {
            let deadline = Instant::now() + Duration::from_secs(5);
            while !cancel.is_cancelled() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            DraftResult::Cancelled
        }));
        let job = job_of(&dispatch(&service, start_request("звёзды")));
        dispatch(
            &service,
            WidgetCoreRequest {
                payload: Some(widget_core_request::Payload::Cancel(
                    WidgetDraftCancelRequest {
                        job_id: job.job_id.clone(),
                    },
                )),
            },
        );
        assert_eq!(
            wait_done(&service, &job.job_id).state,
            WidgetDraftState::WidgetDraftCancelled as i32
        );
    }

    #[test]
    fn a_bad_description_never_reaches_the_provider() {
        let service = WidgetAuthoring::with_runner(Arc::new(|_, _| panic!("must not run")));
        for bad in ["   ".to_owned(), "я".repeat(501)] {
            assert_eq!(
                error_code(&dispatch(&service, start_request(&bad))),
                BusinessErrorCode::InvalidArgument as i32
            );
        }
    }

    #[test]
    fn an_unknown_job_is_not_found_and_old_jobs_are_forgotten() {
        let service = WidgetAuthoring::with_runner(Arc::new(|_, _| DraftResult::Refused));
        assert_eq!(
            error_code(&status_of(&service, "draft-404")),
            BusinessErrorCode::NotFound as i32
        );
        let mut first = None;
        for _ in 0..=KEEP_FINISHED {
            let job = job_of(&dispatch(&service, start_request("погода")));
            wait_done(&service, &job.job_id);
            first.get_or_insert(job.job_id);
        }
        assert_eq!(
            error_code(&status_of(&service, &first.expect("job"))),
            BusinessErrorCode::NotFound as i32
        );
    }

    #[test]
    fn a_proposal_crosses_the_wire_as_validated_fields_only() {
        let service = WidgetAuthoring::with_runner(Arc::new(|_, _| {
            admit(r#"{"outcome":"metric","source":"github.repo.open_prs","repo":"cli/cli"}"#)
        }));
        let job = job_of(&dispatch(&service, start_request("пулреквесты cli/cli")));
        let status = wait_done(&service, &job.job_id);
        assert_eq!(status.state, WidgetDraftState::WidgetDraftProposed as i32);
        assert_eq!(status.source_type, "github.repo.open_prs");
        assert_eq!(status.repo, "cli/cli");
    }

    #[test]
    fn hostile_model_output_is_not_admitted_or_echoed() {
        for hostile in [
            r#"{"outcome":"metric","source":"http://example.com/steal"}"#,
            r#"{"outcome":"metric","source":"github.repo.stars","repo":"a/b","url":"http://x"}"#,
            "<script>alert(1)</script>",
        ] {
            match admit(hostile) {
                DraftResult::Failed { code, detail } => {
                    assert_eq!(code, "bad_output");
                    assert!(!detail.contains("example.com") && !detail.contains("script"));
                }
                other => panic!("admitted {hostile}: {other:?}"),
            }
        }
    }

    #[test]
    fn provider_failures_keep_their_meaning() {
        let code_of = |failure| match classify(failure) {
            DraftResult::Failed { code, .. } => code,
            other => panic!("unexpected {other:?}"),
        };
        assert_eq!(
            code_of(ClaudeFailure::TimedOut {
                detail: "x".to_owned()
            }),
            "timed_out"
        );
        assert_eq!(classify(ClaudeFailure::Cancelled), DraftResult::Cancelled);
    }
}
