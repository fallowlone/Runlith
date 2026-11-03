//! Bridge from Tauri commands to agentd.
//!
//! Each command opens one local IPC connection, handshakes as the recognized
//! `tauri-host` client kind, sends exactly one request and drops the
//! connection. The host never starts agentd: a missing daemon is reported as
//! `unavailable`, not spawned. Requests are pure reads, safe for the UI to
//! retry, except that starting a widget draft starts one provider call; agentd
//! allows one at a time and mutates no durable state for it.

#[cfg(not(unix))]
compile_error!(
    "the Runlith desktop host talks to agentd over a Unix socket; Windows is not wired yet"
);

use std::time::Duration;

use bytes::Bytes;
use ipc_contracts::proto::{
    control_request, control_response, human_decision_core_request, human_decision_core_response,
    task_core_request, task_core_response, widget_core_request, widget_core_response,
    BusinessError, ControlRequest, ControlResponse, HumanDecisionCoreRequest,
    ListDependenciesRequest, ListHumanDecisionsRequest, ListProjectsRequest,
    ListTasksForProjectRequest, TaskCoreRequest, WidgetCoreRequest, WidgetDraftCancelRequest,
    WidgetDraftStartRequest, WidgetDraftStatusRequest,
};
use ipc_control::{ClientConfig, ClientSession, ControlError, CLIENT_KIND_TAURI_HOST};
use ipc_transport::TransportError;
use prost::Message;
use tokio::io::{AsyncRead, AsyncWrite};

use crate::dto::{
    ConnectionDto, ConnectionState, DecisionDto, ProjectDto, TaskDto, UiError, UiErrorKind,
    WidgetDraftDto,
};

/// Same budget agentctl uses for business replies.
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);
/// Upper bound for ids accepted from the webview (trust boundary).
const MAX_ID_LEN: usize = 128;

#[tauri::command]
pub async fn connection_status() -> ConnectionDto {
    let client_build = env!("CARGO_PKG_VERSION").to_owned();
    match open_session().await {
        Ok(session) => ConnectionDto {
            state: ConnectionState::Connected,
            protocol: Some(session.protocol),
            daemon_build: Some(session.daemon_build),
            client_build,
            detail: None,
        },
        Err(error) => ConnectionDto {
            state: connection_state(error.kind),
            protocol: None,
            daemon_build: None,
            client_build,
            detail: Some(error.message),
        },
    }
}

#[tauri::command]
pub async fn list_projects() -> Result<Vec<ProjectDto>, UiError> {
    match task_core(task_core_request::Payload::ListProjects(
        ListProjectsRequest {},
    ))
    .await?
    {
        task_core_response::Payload::ListProjects(reply) => {
            Ok(reply.projects.into_iter().map(ProjectDto::from).collect())
        }
        _ => Err(unexpected_reply("ListProjects")),
    }
}

#[tauri::command]
pub async fn list_tasks(project_id: String) -> Result<Vec<TaskDto>, UiError> {
    let project_id = validate_id("project_id", &project_id)?;
    let request =
        task_core_request::Payload::ListTasksForProject(ListTasksForProjectRequest { project_id });
    match task_core(request).await? {
        task_core_response::Payload::ListTasks(reply) => {
            Ok(reply.tasks.into_iter().map(TaskDto::from).collect())
        }
        _ => Err(unexpected_reply("ListTasksForProject")),
    }
}

#[tauri::command]
pub async fn list_dependencies(task_id: String) -> Result<Vec<String>, UiError> {
    let task_id = validate_id("task_id", &task_id)?;
    let request = task_core_request::Payload::ListDependencies(ListDependenciesRequest { task_id });
    match task_core(request).await? {
        task_core_response::Payload::ListDependencies(reply) => Ok(reply.depends_on_ids),
        _ => Err(unexpected_reply("ListDependencies")),
    }
}

#[tauri::command]
pub async fn list_human_decisions() -> Result<Vec<DecisionDto>, UiError> {
    let session = open_session().await?;
    if !session.inner.human_decision_allowed() {
        return Err(unsupported("Human Decision", session.protocol));
    }
    let request = HumanDecisionCoreRequest {
        payload: Some(human_decision_core_request::Payload::ListPending(
            ListHumanDecisionsRequest {},
        )),
    };
    let reply = exchange(
        session.inner,
        control_request::Payload::HumanDecisionCore(request),
    )
    .await?;
    let control_response::Payload::HumanDecisionCore(reply) = reply else {
        return Err(unexpected_reply("HumanDecisionCore"));
    };
    match reply.payload {
        Some(human_decision_core_response::Payload::ListPending(list)) => {
            Ok(list.decisions.into_iter().map(DecisionDto::from).collect())
        }
        Some(human_decision_core_response::Payload::Error(error)) => Err(business_error(error)),
        _ => Err(unexpected_reply("ListPending")),
    }
}

async fn widget_core(payload: widget_core_request::Payload) -> Result<WidgetDraftDto, UiError> {
    let session = open_session().await?;
    if !session.inner.widget_allowed() {
        return Err(unsupported("Widget authoring", session.protocol));
    }
    let request = WidgetCoreRequest {
        payload: Some(payload),
    };
    let reply = exchange(session.inner, control_request::Payload::WidgetCore(request)).await?;
    let control_response::Payload::WidgetCore(reply) = reply else {
        return Err(unexpected_reply("WidgetCore"));
    };
    match reply.payload {
        Some(widget_core_response::Payload::Status(status)) => WidgetDraftDto::try_from(status),
        Some(widget_core_response::Payload::Error(error)) => Err(business_error(error)),
        None => Err(unexpected_reply("WidgetCore")),
    }
}

#[tauri::command]
pub async fn widget_draft_start(description: String) -> Result<WidgetDraftDto, UiError> {
    let description = widget_spec::validate_description(&description)
        .map_err(|error| UiError::new(UiErrorKind::Invalid, error.to_string()))?
        .to_owned();
    widget_core(widget_core_request::Payload::Start(
        WidgetDraftStartRequest { description },
    ))
    .await
}

#[tauri::command]
pub async fn widget_draft_status(job_id: String) -> Result<WidgetDraftDto, UiError> {
    let job_id = validate_id("job_id", &job_id)?;
    widget_core(widget_core_request::Payload::Status(
        WidgetDraftStatusRequest { job_id },
    ))
    .await
}

#[tauri::command]
pub async fn widget_draft_cancel(job_id: String) -> Result<WidgetDraftDto, UiError> {
    let job_id = validate_id("job_id", &job_id)?;
    widget_core(widget_core_request::Payload::Cancel(
        WidgetDraftCancelRequest { job_id },
    ))
    .await
}

struct OpenSession<S> {
    inner: ClientSession<S>,
    protocol: u32,
    daemon_build: String,
}

#[cfg(unix)]
async fn open_session() -> Result<OpenSession<impl AsyncRead + AsyncWrite + Unpin>, UiError> {
    use ipc_transport::platform::unix::{connect, resolve_endpoint};

    let endpoint = resolve_endpoint().map_err(|error| transport_error(&error))?;
    let stream = connect(&endpoint)
        .await
        .map_err(|error| transport_error(&error))?;
    let mut inner = ClientSession::new(
        ipc_transport::FramedTransport::new(stream),
        ClientConfig::widget_default(CLIENT_KIND_TAURI_HOST),
    );
    let info = inner
        .perform_handshake()
        .await
        .map_err(|error| control_error(&error))?;
    Ok(OpenSession {
        inner,
        protocol: info.selected_version,
        daemon_build: info.daemon_build_version,
    })
}

async fn task_core(
    payload: task_core_request::Payload,
) -> Result<task_core_response::Payload, UiError> {
    let session = open_session().await?;
    if !session.inner.business_allowed() {
        return Err(unsupported("Task", session.protocol));
    }
    let request = TaskCoreRequest {
        payload: Some(payload),
    };
    let reply = exchange(session.inner, control_request::Payload::TaskCore(request)).await?;
    let control_response::Payload::TaskCore(reply) = reply else {
        return Err(unexpected_reply("TaskCore"));
    };
    match reply.payload {
        Some(task_core_response::Payload::Error(error)) => Err(business_error(error)),
        Some(payload) => Ok(payload),
        None => Err(unexpected_reply("TaskCore")),
    }
}

/// One request frame out, one response frame back, then the connection drops.
async fn exchange<S>(
    session: ClientSession<S>,
    request: control_request::Payload,
) -> Result<control_response::Payload, UiError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut transport = session.into_inner();
    let frame = Bytes::from(
        ControlRequest {
            payload: Some(request),
        }
        .encode_to_vec(),
    );
    transport
        .write_frame(frame)
        .await
        .map_err(|error| transport_error(&error))?;
    let reply = tokio::time::timeout(RESPONSE_TIMEOUT, transport.read_frame())
        .await
        .map_err(|_| {
            UiError::new(
                UiErrorKind::Ambiguous,
                "agentd did not answer within 5 s; this read is safe to retry",
            )
        })?
        .map_err(|error| transport_error(&error))?;
    ControlResponse::decode(reply.as_ref())
        .map_err(|error| {
            UiError::new(UiErrorKind::Internal, format!("undecodable reply: {error}"))
        })?
        .payload
        .ok_or_else(|| unexpected_reply("empty ControlResponse"))
}

/// Ids cross the webview trust boundary: bounded, trimmed, printable.
fn validate_id(label: &str, value: &str) -> Result<String, UiError> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_ID_LEN || trimmed.chars().any(char::is_control) {
        return Err(UiError::new(
            UiErrorKind::Invalid,
            format!("{label} must be 1 to {MAX_ID_LEN} printable characters"),
        ));
    }
    Ok(trimmed.to_owned())
}

fn connection_state(kind: UiErrorKind) -> ConnectionState {
    match kind {
        UiErrorKind::Unavailable => ConnectionState::Unavailable,
        UiErrorKind::Incompatible => ConnectionState::Incompatible,
        UiErrorKind::Security => ConnectionState::Security,
        UiErrorKind::Invalid
        | UiErrorKind::Business
        | UiErrorKind::Ambiguous
        | UiErrorKind::Internal => ConnectionState::Error,
    }
}

fn transport_error(error: &TransportError) -> UiError {
    match error {
        TransportError::EndpointUnavailable { .. } => UiError::new(
            UiErrorKind::Unavailable,
            "agentd is not running; start it with `agentctl start`",
        ),
        TransportError::EndpointSecurity(detail) => UiError::new(
            UiErrorKind::Security,
            format!("the agentd endpoint failed its security check: {detail}"),
        ),
        other => UiError::new(UiErrorKind::Internal, other.to_string()),
    }
}

fn control_error(error: &ControlError) -> UiError {
    match error {
        ControlError::Transport(transport) => transport_error(transport),
        ControlError::IncompatibleProtocol { .. }
        | ControlError::HandshakeRejected { .. }
        | ControlError::UnsupportedProtocol { .. }
        | ControlError::UnknownClientKind(_) => {
            UiError::new(UiErrorKind::Incompatible, error.to_string())
        }
        ControlError::Timeout { .. } => UiError::new(UiErrorKind::Unavailable, error.to_string()),
        ControlError::MalformedProtobuf(_)
        | ControlError::UnexpectedMessage(_)
        | ControlError::HandshakeRequired
        | ControlError::DuplicateHandshake
        | ControlError::InvalidHandshake(_)
        | ControlError::SessionTerminated => UiError::new(UiErrorKind::Internal, error.to_string()),
    }
}

fn business_error(error: BusinessError) -> UiError {
    UiError::new(UiErrorKind::Business, error.message)
}

fn unsupported(surface: &str, protocol: u32) -> UiError {
    UiError::new(
        UiErrorKind::Incompatible,
        format!("protocol {protocol} does not carry {surface} reads"),
    )
}

fn unexpected_reply(what: &str) -> UiError {
    UiError::new(
        UiErrorKind::Internal,
        format!("agentd answered with an unexpected message for {what}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_id_trims_and_rejects_empty_long_or_control_input() {
        assert_eq!(validate_id("task_id", "  TASK-0229 ").unwrap(), "TASK-0229");
        for bad in ["", "   ", "a\nb", &"x".repeat(MAX_ID_LEN + 1)] {
            let error = validate_id("task_id", bad).unwrap_err();
            assert_eq!(error.kind, UiErrorKind::Invalid, "accepted {bad:?}");
        }
    }

    #[test]
    fn missing_daemon_and_version_mismatch_map_to_distinct_states() {
        let missing = transport_error(&TransportError::EndpointUnavailable {
            endpoint: "/tmp/agentd.sock".to_owned(),
        });
        assert_eq!(connection_state(missing.kind), ConnectionState::Unavailable);

        let mismatch = control_error(&ControlError::UnsupportedProtocol {
            negotiated: 3,
            required: 4,
        });
        assert_eq!(
            connection_state(mismatch.kind),
            ConnectionState::Incompatible
        );

        let security = transport_error(&TransportError::EndpointSecurity("owner".to_owned()));
        assert_eq!(connection_state(security.kind), ConnectionState::Security);
    }
}
