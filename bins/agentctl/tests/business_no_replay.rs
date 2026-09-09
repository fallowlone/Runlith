//! G2 §25 — business mutations are never blindly replayed (TASK-0030 audit).
//!
//! A fake v1 daemon accepts the handshake, reads exactly one business
//! frame, then goes silent forever. The client must report
//! [`agentctl::ClientError::BusinessAmbiguous`] without sending the
//! mutation a second time. Mirrors the shutdown anti-replay test in the
//! library unit tests, but for the mutation path where a replay would
//! risk a double commit.
//!
//! Unix-only: uses the local UDS transport like the other real-transport
//! harnesses. Test-owned sandbox endpoint/database only; the production
//! application-data directory is never touched.

#![cfg(unix)]

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use ipc_contracts::proto::{
    control_request, control_response, handshake_response, task_core_request, ControlRequest,
    ControlResponse, CreateProjectRequest, HandshakeAccepted, HandshakeResponse, TaskCoreRequest,
};
use ipc_transport::platform::unix::{bind_server, UnixEndpoint, SOCKET_FILE_NAME};
use ipc_transport::FramedTransport;
use prost::Message;

struct Sandbox {
    root: PathBuf,
    endpoint: UnixEndpoint,
}

impl Sandbox {
    fn new() -> Self {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let root = PathBuf::from("/tmp").join(format!("g2-noreplay-{}-{id}", std::process::id()));
        std::fs::create_dir_all(&root).expect("sandbox root");
        let runtime_dir = root.join("run");
        let endpoint = UnixEndpoint {
            socket_path: runtime_dir.join(SOCKET_FILE_NAME),
            runtime_dir,
        };
        Self { root, endpoint }
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn accept_v1_frame() -> Bytes {
    Bytes::from(
        ControlResponse {
            payload: Some(control_response::Payload::Handshake(HandshakeResponse {
                outcome: Some(handshake_response::Outcome::Accepted(HandshakeAccepted {
                    selected_protocol_version: 1,
                    daemon_build_version: "audit-fake-daemon".to_owned(),
                    accepted_capabilities: Vec::new(),
                })),
            })),
        }
        .encode_to_vec(),
    )
}

fn create_project_request() -> TaskCoreRequest {
    TaskCoreRequest {
        payload: Some(task_core_request::Payload::CreateProject(
            CreateProjectRequest {
                id: "P-1".to_owned(),
                name: "NoReplay".to_owned(),
            },
        )),
    }
}

#[tokio::test]
async fn business_mutation_is_sent_once_then_reported_ambiguous() {
    let sandbox = Sandbox::new();
    let listener = bind_server(&sandbox.endpoint).expect("bind sandbox endpoint");

    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut transport = FramedTransport::new(stream);
        // Frame 1: handshake. Must offer v1 or the client never proceeds.
        let frame = transport.read_frame().await.expect("read handshake");
        let request = ControlRequest::decode(&frame[..]).expect("decode handshake");
        match request.payload {
            Some(control_request::Payload::Handshake(handshake)) => {
                assert!(
                    handshake.supported_protocol_versions.contains(&1),
                    "client must offer v1 for business"
                );
            }
            other => panic!("expected handshake first, got {other:?}"),
        }
        transport
            .write_frame(accept_v1_frame())
            .await
            .expect("write accept");
        // Frame 2: the one business mutation. Then silence forever.
        let frame = transport.read_frame().await.expect("read business");
        let request = ControlRequest::decode(&frame[..]).expect("decode business");
        assert!(
            matches!(request.payload, Some(control_request::Payload::TaskCore(_))),
            "expected exactly one TaskCore frame, got {request:?}"
        );
        let frames_seen = 1u32;
        // Catch any (forbidden) replay within a bounded window that starts
        // after the client has had time to time out and (mis)behave. Only
        // a successfully read frame counts as a replay: inner timeout or
        // connection-close errors mean silence, which is the correct
        // client behavior (the inner frame timeout fires first at 5s).
        let third = tokio::time::timeout(Duration::from_millis(7000), transport.read_frame()).await;
        let replayed = match &third {
            Ok(Ok(frame)) => {
                let decoded = ControlRequest::decode(&frame[..]);
                panic!("forbidden business replay arrived: {decoded:?}");
            }
            Ok(Err(_)) | Err(_) => false,
        };
        (frames_seen, replayed)
    });

    let spawns = Arc::new(AtomicU32::new(0));
    let spawn_counter = spawns.clone();
    let outcome = agentctl::business_request(
        &sandbox.endpoint,
        || {
            spawn_counter.fetch_add(1, Ordering::SeqCst);
            Command::new("true").spawn()
        },
        create_project_request(),
    )
    .await;

    match outcome {
        Err(agentctl::ClientError::BusinessAmbiguous(_)) => {}
        other => panic!("expected BusinessAmbiguous, got {other:?}"),
    }
    assert_eq!(
        spawns.load(Ordering::SeqCst),
        0,
        "no daemon spawn when the endpoint answers"
    );
    let (frames_seen, replayed) = server.await.expect("server joins");
    assert_eq!(frames_seen, 1, "exactly one business frame may be sent");
    assert!(!replayed, "business mutation must not be replayed");
}
