use std::sync::{Mutex, TryLockError};
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "macos")]
pub(crate) const TIMEOUT: Duration = Duration::from_secs(30);
pub(crate) const REQUIRED_CAUSE: &str = "user_presence_required";
const DENIAL_BACKOFF: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Error {
    #[cfg(any(target_os = "macos", test))]
    Denied,
    Unavailable,
    #[cfg(any(target_os = "macos", test))]
    TimedOut,
    Busy,
}

impl Error {
    pub(crate) const fn cause(self) -> &'static str {
        match self {
            #[cfg(any(target_os = "macos", test))]
            Self::Denied => "user_presence_denied",
            Self::Unavailable => "user_presence_unavailable",
            #[cfg(any(target_os = "macos", test))]
            Self::TimedOut => "user_presence_timed_out",
            Self::Busy => "user_presence_busy",
        }
    }
}

pub(crate) struct UserPresence {
    authenticate: fn(&str) -> Result<(), Error>,
    retry_at: Mutex<Option<Instant>>,
}

impl std::fmt::Debug for UserPresence {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("UserPresence")
    }
}

impl UserPresence {
    pub(crate) fn system() -> Self {
        Self {
            authenticate,
            retry_at: Mutex::new(None),
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn for_test(authenticate: fn(&str) -> Result<(), Error>) -> Self {
        Self {
            authenticate,
            retry_at: Mutex::new(None),
        }
    }

    pub(crate) fn confirm(&self, reason: &str) -> Result<(), Error> {
        let mut retry = match self.retry_at.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::WouldBlock) => return Err(Error::Busy),
            Err(TryLockError::Poisoned(_)) => return Err(Error::Unavailable),
        };
        if retry.is_some_and(|deadline| Instant::now() < deadline) {
            return Err(Error::Busy);
        }
        let result = (self.authenticate)(reason);
        if result.is_err() {
            *retry = Some(Instant::now() + DENIAL_BACKOFF);
        }
        result
    }
}

#[cfg(target_os = "macos")]
fn authenticate(reason: &str) -> Result<(), Error> {
    macos::authenticate(reason)
}

#[cfg(not(target_os = "macos"))]
fn authenticate(_: &str) -> Result<(), Error> {
    Err(Error::Unavailable)
}

pub(crate) fn execution_reason(
    request: &ipc_contracts::proto::ExecutionCoreRequest,
) -> Result<Option<String>, &'static str> {
    use ipc_contracts::proto::execution_core_request::Payload;
    match request.payload.as_ref() {
        Some(Payload::Run(request)) => {
            let id = task_domain::TaskId::new(request.task_id.clone())
                .map_err(|_| "invalid task identity")?;
            Ok(Some(format!("start Runlith task {:?}", id.as_str())))
        }
        Some(Payload::Resume(request)) => {
            let id = agent_runtime::ExecutionId::new(request.execution_id.clone())
                .map_err(|_| "invalid execution identity")?;
            Ok(Some(format!("resume Runlith execution {:?}", id.as_str())))
        }
        Some(Payload::Cancel(request)) => {
            let id = agent_runtime::ExecutionId::new(request.execution_id.clone())
                .map_err(|_| "invalid execution identity")?;
            Ok(Some(format!("cancel Runlith execution {:?}", id.as_str())))
        }
        Some(Payload::Get(_)) | None => Ok(None),
    }
}

/// ADR-0050 amendment: planning triggers and human-continuation applies start
/// provider work or advance a plan, so they need a fresh confirmation too.
pub(crate) fn planning_reason(
    request: &ipc_contracts::proto::PlanningCoreRequest,
) -> Result<Option<String>, &'static str> {
    use ipc_contracts::proto::planning_core_request::Payload;
    match request.payload.as_ref() {
        Some(Payload::Trigger(request)) => {
            let task = request
                .context
                .as_ref()
                .map(|context| context.task_id.as_str())
                .unwrap_or_default();
            let id =
                task_domain::TaskId::new(task.to_owned()).map_err(|_| "invalid task identity")?;
            Ok(Some(format!("plan Runlith task {:?}", id.as_str())))
        }
        Some(Payload::ApplyHumanContinuation(request)) => {
            let id = bounded_identity(&request.request_id)?;
            Ok(Some(format!(
                "apply human decision {id:?} to a Runlith plan"
            )))
        }
        None => Ok(None),
    }
}

/// ADR-0050 amendment: a widget draft starts a provider call.
pub(crate) fn widget_reason(request: &ipc_contracts::proto::WidgetCoreRequest) -> Option<String> {
    use ipc_contracts::proto::widget_core_request::Payload;
    match request.payload.as_ref() {
        Some(Payload::Start(request)) => {
            let shown: String = request.description.chars().take(80).collect();
            Some(format!("draft a Runlith widget: {shown:?}"))
        }
        Some(Payload::Status(_)) | Some(Payload::Cancel(_)) | None => None,
    }
}

fn bounded_identity(value: &str) -> Result<&str, &'static str> {
    if value.trim().is_empty() || value.len() > 256 {
        return Err("invalid request identity");
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn successful_confirmation_is_not_cached_for_the_next_request() {
        static CALLS: AtomicUsize = AtomicUsize::new(0);
        let gate = UserPresence::for_test(|_| {
            if CALLS.fetch_add(1, Ordering::SeqCst) == 0 {
                Ok(())
            } else {
                Err(Error::Denied)
            }
        });
        assert_eq!(gate.confirm("start task A"), Ok(()));
        assert_eq!(gate.confirm("start task B"), Err(Error::Denied));
        assert_eq!(gate.confirm("retry task B"), Err(Error::Busy));
        assert_eq!(CALLS.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn simultaneous_and_poisoned_confirmation_fail_closed() {
        let gate = UserPresence::for_test(|_| panic!("authentication must not start"));
        let guard = gate.retry_at.lock().unwrap();
        assert_eq!(gate.confirm("stop daemon"), Err(Error::Busy));
        drop(guard);
        let _ = std::panic::catch_unwind(|| {
            let _guard = gate.retry_at.lock().unwrap();
            panic!("fixture poison");
        });
        assert_eq!(gate.confirm("stop daemon"), Err(Error::Unavailable));
    }

    #[test]
    fn cancel_planning_and_widget_start_need_confirmation_and_reads_do_not() {
        use ipc_contracts::proto::{
            execution_core_request, planning_core_request, widget_core_request,
            ApplyHumanContinuationRequest, CancelExecutionRequest, ExecutionCoreRequest,
            GetExecutionRequest, PlanningContext, PlanningCoreRequest, TriggerPlanningRequest,
            WidgetCoreRequest, WidgetDraftStartRequest, WidgetDraftStatusRequest,
        };
        let cancel = ExecutionCoreRequest {
            payload: Some(execution_core_request::Payload::Cancel(
                CancelExecutionRequest {
                    execution_id: "E-1".into(),
                },
            )),
        };
        assert!(execution_reason(&cancel)
            .unwrap()
            .unwrap()
            .contains("cancel"));
        let get = ExecutionCoreRequest {
            payload: Some(execution_core_request::Payload::Get(
                GetExecutionRequest::default(),
            )),
        };
        assert_eq!(execution_reason(&get), Ok(None));

        let trigger = PlanningCoreRequest {
            payload: Some(planning_core_request::Payload::Trigger(
                TriggerPlanningRequest {
                    context: Some(PlanningContext {
                        task_id: "T-1".into(),
                        ..Default::default()
                    }),
                },
            )),
        };
        assert!(planning_reason(&trigger).unwrap().unwrap().contains("T-1"));
        let apply = PlanningCoreRequest {
            payload: Some(planning_core_request::Payload::ApplyHumanContinuation(
                ApplyHumanContinuationRequest {
                    request_id: "x".repeat(257),
                    context: None,
                },
            )),
        };
        assert!(planning_reason(&apply).is_err());

        let start = WidgetCoreRequest {
            payload: Some(widget_core_request::Payload::Start(
                WidgetDraftStartRequest {
                    description: "a\nb".repeat(100),
                },
            )),
        };
        let reason = widget_reason(&start).unwrap();
        assert!(!reason.contains('\n') && reason.len() < 140);
        let status = WidgetCoreRequest {
            payload: Some(widget_core_request::Payload::Status(
                WidgetDraftStatusRequest::default(),
            )),
        };
        assert_eq!(widget_reason(&status), None);
    }

    #[test]
    fn execution_prompt_escapes_identity_and_refuses_oversized_input() {
        use ipc_contracts::proto::{
            execution_core_request::Payload, ExecutionCoreRequest, ResumeExecutionRequest,
            RunExecutionRequest,
        };
        let request = ExecutionCoreRequest {
            payload: Some(Payload::Run(RunExecutionRequest {
                task_id: "A\nB".into(),
                ..Default::default()
            })),
        };
        let reason = execution_reason(&request).unwrap().unwrap();
        assert!(reason.contains("start Runlith task"));
        assert!(reason.contains("A\\nB"));
        assert!(!reason.contains('\n'));
        let request = ExecutionCoreRequest {
            payload: Some(Payload::Resume(ResumeExecutionRequest {
                execution_id: "x".repeat(257),
                ..Default::default()
            })),
        };
        assert!(execution_reason(&request).is_err());
    }
}
