use super::*;
use ipc_contracts::proto::{
    control_request, control_response, execution_core_request, execution_core_response,
    BusinessErrorCode, ControlRequest, ControlResponse, ExecutionCoreRequest, PingRequest,
    RunExecutionRequest, ShutdownRequest,
};
use ipc_control::{ClientConfig, ClientSession};
use prost::Message;

type Authenticate = fn(&str) -> Result<(), user_presence::Error>;

fn services(authenticate: Authenticate) -> (tempfile::TempDir, ConnectionServices) {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("data/state.sqlite3");
    let config = DaemonConfig::with_execution(&db);
    let services = ConnectionServices {
        user_presence: Arc::new(user_presence::UserPresence::for_test(authenticate)),
        business: open_business_service(&config).unwrap(),
        execution: Some(Arc::new(execution::ExecutionComposition::new(db, None))),
        approval: None,
        human_decision: None,
        vault: None,
        finder: open_finder(),
        memory: None,
        routine: None,
        worker: None,
        org: None,
        widget: None,
    };
    (dir, services)
}

async fn connect(
    services: ConnectionServices,
    shutdown: tokio::sync::watch::Sender<bool>,
) -> (
    FramedTransport<tokio::io::DuplexStream>,
    tokio::task::JoinHandle<ConnectionOutcome>,
) {
    connect_with_profile(
        services,
        shutdown,
        ClientConfig::execution_default(ipc_control::CLIENT_KIND_AGENTCTL),
    )
    .await
}

async fn connect_with_profile(
    services: ConnectionServices,
    shutdown: tokio::sync::watch::Sender<bool>,
    profile: ClientConfig,
) -> (
    FramedTransport<tokio::io::DuplexStream>,
    tokio::task::JoinHandle<ConnectionOutcome>,
) {
    let (client, server) = tokio::io::duplex(8192);
    let handle = tokio::spawn(async move {
        serve_connection(
            server,
            &ServerConfig::g17_default(),
            services,
            shutdown,
            None,
        )
        .await
    });
    let mut client = ClientSession::new(FramedTransport::new(client), profile);
    client.perform_handshake().await.unwrap();
    (client.into_inner(), handle)
}

async fn exchange(
    client: &mut FramedTransport<tokio::io::DuplexStream>,
    payload: control_request::Payload,
) -> ControlResponse {
    client
        .write_frame(bytes::Bytes::from(
            ControlRequest {
                payload: Some(payload),
            }
            .encode_to_vec(),
        ))
        .await
        .unwrap();
    let response = tokio::time::timeout(Duration::from_secs(2), client.read_frame())
        .await
        .unwrap()
        .unwrap();
    ControlResponse::decode(&response[..]).unwrap()
}

#[tokio::test]
async fn denied_os_confirmation_never_stops_daemon_or_enters_execution() {
    let checks: [Authenticate; 3] = [
        |_| Err(user_presence::Error::Denied),
        |_| Err(user_presence::Error::Unavailable),
        |_| Err(user_presence::Error::TimedOut),
    ];
    for authenticate in checks {
        for shutdown_request in [true, false] {
            let (dir, services) = services(authenticate);
            let (shutdown, receiver) = tokio::sync::watch::channel(false);
            let (mut client, server) = connect(services, shutdown).await;
            if shutdown_request {
                let reply = exchange(
                    &mut client,
                    control_request::Payload::Shutdown(ShutdownRequest {}),
                )
                .await;
                let Some(control_response::Payload::Shutdown(reply)) = reply.payload else {
                    panic!("unexpected shutdown reply");
                };
                assert!(reply.denial_reason.is_some());
            } else {
                let reply = exchange(
                    &mut client,
                    control_request::Payload::ExecutionCore(ExecutionCoreRequest {
                        payload: Some(execution_core_request::Payload::Run(RunExecutionRequest {
                            task_id: "TASK-CONFIRM".into(),
                            ..Default::default()
                        })),
                    }),
                )
                .await;
                let Some(control_response::Payload::ExecutionCore(reply)) = reply.payload else {
                    panic!("unexpected execution reply");
                };
                let Some(execution_core_response::Payload::Error(error)) = reply.payload else {
                    panic!("execution was admitted without OS confirmation");
                };
                assert_eq!(error.code(), BusinessErrorCode::UserPresenceRequired);
                assert!(error.message.starts_with("user_presence_"));
            }
            assert!(!*receiver.borrow(), "refused request signalled shutdown");
            let store =
                storage_sqlite::SqliteStore::open(dir.path().join("data/state.sqlite3")).unwrap();
            assert!(store.list_events_ordered().unwrap().is_empty());
            assert!(matches!(
                exchange(&mut client, control_request::Payload::Ping(PingRequest {}))
                    .await
                    .payload,
                Some(control_response::Payload::Ping(_))
            ));
            drop(client);
            assert!(matches!(server.await.unwrap(), ConnectionOutcome::Ended));
        }
    }
}

#[tokio::test]
async fn confirmed_shutdown_acknowledges_and_signals_once() {
    let (_dir, services) = services(|reason| {
        assert!(reason.contains("stop the Runlith daemon"));
        Ok(())
    });
    let (shutdown, receiver) = tokio::sync::watch::channel(false);
    let (mut client, server) = connect(services, shutdown).await;
    let reply = exchange(
        &mut client,
        control_request::Payload::Shutdown(ShutdownRequest {}),
    )
    .await;
    let Some(control_response::Payload::Shutdown(reply)) = reply.payload else {
        panic!("unexpected shutdown reply");
    };
    assert_eq!(reply.denial_reason, None);
    assert!(*receiver.borrow());
    assert!(matches!(
        server.await.unwrap(),
        ConnectionOutcome::ShutdownInitiated
    ));
}

#[tokio::test]
async fn resume_requires_confirmation_while_status_remains_available() {
    use ipc_contracts::proto::{GetExecutionRequest, ResumeExecutionRequest};
    let (dir, services) = services(|_| Err(user_presence::Error::Denied));
    let (shutdown, receiver) = tokio::sync::watch::channel(false);
    let (mut client, server) = connect(services, shutdown).await;
    let reply = exchange(
        &mut client,
        control_request::Payload::ExecutionCore(ExecutionCoreRequest {
            payload: Some(execution_core_request::Payload::Resume(
                ResumeExecutionRequest {
                    execution_id: "EX-CONFIRM".into(),
                    ..Default::default()
                },
            )),
        }),
    )
    .await;
    let Some(control_response::Payload::ExecutionCore(reply)) = reply.payload else {
        panic!("unexpected resume reply");
    };
    let Some(execution_core_response::Payload::Error(error)) = reply.payload else {
        panic!("resume was admitted without confirmation");
    };
    assert_eq!(error.code(), BusinessErrorCode::UserPresenceRequired);
    let reply = exchange(
        &mut client,
        control_request::Payload::ExecutionCore(ExecutionCoreRequest {
            payload: Some(execution_core_request::Payload::Get(GetExecutionRequest {
                execution_id: "EX-CONFIRM".into(),
            })),
        }),
    )
    .await;
    let Some(control_response::Payload::ExecutionCore(reply)) = reply.payload else {
        panic!("unexpected status reply");
    };
    let Some(execution_core_response::Payload::Error(error)) = reply.payload else {
        panic!("missing execution unexpectedly exists");
    };
    assert_eq!(error.code(), BusinessErrorCode::NotFound);
    assert!(!*receiver.borrow());
    assert!(
        storage_sqlite::SqliteStore::open(dir.path().join("data/state.sqlite3"))
            .unwrap()
            .list_events_ordered()
            .unwrap()
            .is_empty()
    );
    drop(client);
    assert!(matches!(server.await.unwrap(), ConnectionOutcome::Ended));
}

#[tokio::test]
async fn routine_authorization_and_run_require_confirmation_and_ticks_stay_blocked() {
    use app_services::background_service::DefineRequest;
    use ipc_contracts::proto::{
        routine_core_request as request, routine_core_response as response,
        RoutineAuthorizeRequest, RoutineCoreRequest, RoutineRunRequest, RoutineTickRequest,
    };
    let (dir, mut services) = services(|_| Err(user_presence::Error::Denied));
    let config = DaemonConfig::with_execution(dir.path().join("data/state.sqlite3"));
    let routine = open_routine_service(&config).unwrap().unwrap();
    let definition = routine
        .lock()
        .await
        .define(
            DefineRequest {
                name: "scheduled".into(),
                objective: "verify state".into(),
                interval_seconds: 60,
                capability_scope: background::model::CapabilityScope::ReadOnly,
                budget_max_runs: 2,
                author_ref: "agent".into(),
            },
            1_000,
        )
        .unwrap();
    services.routine = Some(routine.clone());
    let (shutdown, receiver) = tokio::sync::watch::channel(false);
    let (mut client, server) = connect_with_profile(
        services,
        shutdown,
        ClientConfig::routine_default(ipc_control::CLIENT_KIND_AGENTCTL),
    )
    .await;
    let reply = exchange(
        &mut client,
        control_request::Payload::RoutineCore(RoutineCoreRequest {
            payload: Some(request::Payload::Authorize(RoutineAuthorizeRequest {
                routine_id: definition.id.to_string(),
                authorizer_ref: "operator".into(),
                now_millis: 1_000,
            })),
        }),
    )
    .await;
    let Some(control_response::Payload::RoutineCore(reply)) = reply.payload else {
        panic!("unexpected authorization reply");
    };
    assert!(
        matches!(reply.payload, Some(response::Payload::Error(error)) if error.code == "user_presence_required")
    );
    assert_eq!(
        routine.lock().await.get(&definition.id).unwrap().state,
        background::model::RoutineState::Proposed
    );
    routine
        .lock()
        .await
        .authorize(&definition.id, "operator", 1_000)
        .unwrap();
    let reply = exchange(
        &mut client,
        control_request::Payload::RoutineCore(RoutineCoreRequest {
            payload: Some(request::Payload::Run(RoutineRunRequest {
                routine_id: definition.id.to_string(),
                now_millis: 2_000,
                evidence_ref: "client-claimed-approval".into(),
            })),
        }),
    )
    .await;
    let Some(control_response::Payload::RoutineCore(reply)) = reply.payload else {
        panic!("unexpected run reply");
    };
    assert!(
        matches!(reply.payload, Some(response::Payload::Error(error)) if error.code == "user_presence_required")
    );
    assert!(routine
        .lock()
        .await
        .list_runs(&definition.id)
        .unwrap()
        .is_empty());
    let reply = exchange(
        &mut client,
        control_request::Payload::RoutineCore(RoutineCoreRequest {
            payload: Some(request::Payload::Tick(RoutineTickRequest {
                now_millis: 2_000,
                evidence_ref: "client-claimed-approval".into(),
            })),
        }),
    )
    .await;
    let Some(control_response::Payload::RoutineCore(reply)) = reply.payload else {
        panic!("unexpected tick reply");
    };
    let Some(response::Payload::List(runs)) = reply.payload else {
        panic!("tick must record a blocked run");
    };
    assert_eq!(runs.runs.len(), 1);
    assert_eq!(runs.runs[0].status, "BLOCKED");
    assert_eq!(runs.runs[0].evidence_ref, user_presence::REQUIRED_CAUSE);
    assert_eq!(
        routine
            .lock()
            .await
            .get(&definition.id)
            .unwrap()
            .runs_completed,
        0
    );
    assert!(!*receiver.borrow());
    drop(client);
    assert!(matches!(server.await.unwrap(), ConnectionOutcome::Ended));
}

#[tokio::test]
async fn pending_os_confirmation_keeps_other_control_connections_responsive() {
    use std::sync::atomic::{AtomicBool, Ordering};
    type Signals = (
        tokio::sync::oneshot::Sender<()>,
        std::sync::mpsc::Receiver<()>,
    );
    static SIGNALS: std::sync::Mutex<Option<Signals>> = std::sync::Mutex::new(None);
    static FINISHED: AtomicBool = AtomicBool::new(false);
    let (entered, started) = tokio::sync::oneshot::channel();
    let (release, waiting) = std::sync::mpsc::channel();
    *SIGNALS.lock().unwrap() = Some((entered, waiting));
    let (_dir, services) = services(|_| {
        let (entered, waiting) = SIGNALS.lock().unwrap().take().unwrap();
        entered.send(()).unwrap();
        let _ = waiting.recv_timeout(Duration::from_secs(5));
        FINISHED.store(true, Ordering::SeqCst);
        Err(user_presence::Error::Denied)
    });
    let (shutdown, receiver) = tokio::sync::watch::channel(false);
    let (mut first, first_server) = connect(services.clone(), shutdown.clone()).await;
    first
        .write_frame(bytes::Bytes::from(
            ControlRequest {
                payload: Some(control_request::Payload::Shutdown(ShutdownRequest {})),
            }
            .encode_to_vec(),
        ))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), started)
        .await
        .unwrap()
        .unwrap();
    assert!(!FINISHED.load(Ordering::SeqCst));
    let (mut second, second_server) =
        tokio::time::timeout(Duration::from_secs(2), connect(services, shutdown))
            .await
            .unwrap();
    assert!(matches!(
        exchange(&mut second, control_request::Payload::Ping(PingRequest {}))
            .await
            .payload,
        Some(control_response::Payload::Ping(_))
    ));
    assert!(
        !FINISHED.load(Ordering::SeqCst),
        "ping waited for confirmation to finish"
    );
    assert!(!*receiver.borrow());
    release.send(()).unwrap();
    let reply = tokio::time::timeout(Duration::from_secs(2), first.read_frame())
        .await
        .unwrap()
        .unwrap();
    let reply = ControlResponse::decode(&reply[..]).unwrap();
    assert!(
        matches!(reply.payload, Some(control_response::Payload::Shutdown(reply)) if reply.denial_reason.is_some())
    );
    drop(first);
    drop(second);
    assert!(matches!(
        first_server.await.unwrap(),
        ConnectionOutcome::Ended
    ));
    assert!(matches!(
        second_server.await.unwrap(),
        ConnectionOutcome::Ended
    ));
}

#[tokio::test]
async fn cancel_planning_and_widget_start_are_refused_without_confirmation() {
    use ipc_contracts::proto::{
        planning_core_request, planning_core_response, widget_core_request, widget_core_response,
        CancelExecutionRequest, PlanningContext, PlanningCoreRequest, TriggerPlanningRequest,
        WidgetCoreRequest, WidgetDraftStartRequest,
    };
    let profile = ClientConfig::workspace_intent_default(ipc_control::CLIENT_KIND_AGENTCTL);
    let (dir, services) = services(|_| Err(user_presence::Error::Denied));
    let (shutdown, _receiver) = tokio::sync::watch::channel(false);
    let (mut client, server) = connect_with_profile(services, shutdown, profile).await;

    let reply = exchange(
        &mut client,
        control_request::Payload::ExecutionCore(ExecutionCoreRequest {
            payload: Some(execution_core_request::Payload::Cancel(
                CancelExecutionRequest {
                    execution_id: "EX-CANCEL".into(),
                },
            )),
        }),
    )
    .await;
    let Some(control_response::Payload::ExecutionCore(reply)) = reply.payload else {
        panic!("unexpected cancel reply");
    };
    let Some(execution_core_response::Payload::Error(error)) = reply.payload else {
        panic!("cancel was admitted without confirmation");
    };
    assert_eq!(error.code(), BusinessErrorCode::UserPresenceRequired);

    let reply = exchange(
        &mut client,
        control_request::Payload::PlanningCore(PlanningCoreRequest {
            payload: Some(planning_core_request::Payload::Trigger(
                TriggerPlanningRequest {
                    context: Some(PlanningContext {
                        task_id: "TASK-PLAN".into(),
                        ..Default::default()
                    }),
                },
            )),
        }),
    )
    .await;
    let Some(control_response::Payload::PlanningCore(reply)) = reply.payload else {
        panic!("unexpected planning reply");
    };
    let Some(planning_core_response::Payload::Error(error)) = reply.payload else {
        panic!("planning was admitted without confirmation");
    };
    assert_eq!(error.code(), BusinessErrorCode::UserPresenceRequired);

    let reply = exchange(
        &mut client,
        control_request::Payload::WidgetCore(WidgetCoreRequest {
            payload: Some(widget_core_request::Payload::Start(
                WidgetDraftStartRequest {
                    description: "a clock".into(),
                },
            )),
        }),
    )
    .await;
    let Some(control_response::Payload::WidgetCore(reply)) = reply.payload else {
        panic!("unexpected widget reply");
    };
    let Some(widget_core_response::Payload::Error(error)) = reply.payload else {
        panic!("widget draft was admitted without confirmation");
    };
    assert_eq!(error.code(), BusinessErrorCode::UserPresenceRequired);

    assert!(
        storage_sqlite::SqliteStore::open(dir.path().join("data/state.sqlite3"))
            .unwrap()
            .list_events_ordered()
            .unwrap()
            .is_empty()
    );
    drop(client);
    assert!(matches!(server.await.unwrap(), ConnectionOutcome::Ended));
}

static SNAPSHOT_DB: std::sync::Mutex<Option<std::path::PathBuf>> = std::sync::Mutex::new(None);

async fn run_after_task_change(authenticate: Authenticate) -> execution_core_response::Payload {
    let (dir, services) = services(authenticate);
    let db = dir.path().join("data/state.sqlite3");
    {
        let handle = services.business.as_ref().unwrap();
        let mut tasks = handle.lock().await;
        let project = task_domain::ProjectId::new("P-1").unwrap();
        tasks
            .create_project(&task_domain::Project::new(project.clone(), "p").unwrap())
            .unwrap();
        tasks
            .create_planning_task(
                task_domain::TaskId::new("T-SNAP").unwrap(),
                project,
                "inspect the parser",
                vec!["repo:src".to_owned()],
                vec!["c1".to_owned()],
                "low",
                true,
            )
            .unwrap();
    }
    *SNAPSHOT_DB.lock().unwrap() = Some(db);
    let (shutdown, _receiver) = tokio::sync::watch::channel(false);
    let (mut client, _server) = connect(services, shutdown).await;
    let reply = exchange(
        &mut client,
        control_request::Payload::ExecutionCore(ExecutionCoreRequest {
            payload: Some(execution_core_request::Payload::Run(RunExecutionRequest {
                task_id: "T-SNAP".into(),
                ..Default::default()
            })),
        }),
    )
    .await;
    let Some(control_response::Payload::ExecutionCore(reply)) = reply.payload else {
        panic!("unexpected run reply");
    };
    reply.payload.unwrap()
}

#[tokio::test]
async fn run_confirmation_shows_daemon_resolved_task_facts_and_rejects_a_changed_task() {
    // The dialog names the objective, risk and scope size the daemon read.
    let payload = run_after_task_change(|reason| {
        assert!(reason.contains("inspect the parser"), "{reason}");
        assert!(reason.contains("risk low"), "{reason}");
        assert!(reason.contains("1 scope path"), "{reason}");
        Err(user_presence::Error::Denied)
    })
    .await;
    assert!(matches!(
        payload,
        execution_core_response::Payload::Error(_)
    ));
}

#[tokio::test]
async fn run_is_refused_when_the_task_changes_during_confirmation() {
    let payload = run_after_task_change(|_| {
        // Another writer readies the task while the dialog is open.
        let db = SNAPSHOT_DB.lock().unwrap().clone().unwrap();
        let mut tasks = app_services::TaskService::open(db, app_services::SystemClock).unwrap();
        tasks
            .mark_ready(&task_domain::TaskId::new("T-SNAP").unwrap())
            .unwrap();
        Ok(())
    })
    .await;
    let execution_core_response::Payload::Error(error) = payload else {
        panic!("run was admitted for a task that changed during confirmation");
    };
    assert_eq!(error.code(), BusinessErrorCode::Conflict);
    assert!(error
        .message
        .contains("changed while awaiting confirmation"));
}
