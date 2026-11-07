//! Worker IPC dispatch (TASK-0202, G12): WorkerCoreRequest in,
//! WorkerCoreResponse out over WorkerService (ADR-0046).
use app_services::worker_service::{OfferRequest, WorkerService};
use ipc_contracts::proto::{
    worker_core_request, worker_core_response, WorkerChallengeResponse, WorkerCoreRequest,
    WorkerCoreResponse, WorkerDeviceResponse, WorkerErrorResponse, WorkerJobResponse,
    WorkerListResponse,
};
use worker::model::{
    CapabilityScope, ChallengeId, DeviceId, DeviceRecord, JobId, WorkerError, WorkerJob,
};
use worker::port::WorkerStorePort;

fn error_response(code: &str, detail: &str) -> WorkerCoreResponse {
    WorkerCoreResponse {
        payload: Some(worker_core_response::Payload::Error(WorkerErrorResponse {
            code: code.to_owned(),
            detail: detail.chars().take(2048).collect(),
        })),
    }
}

pub fn unavailable() -> WorkerCoreResponse {
    error_response("unavailable", "worker composition is not available")
}

fn map_error(error: &WorkerError) -> WorkerCoreResponse {
    let code = match error {
        WorkerError::InvalidInput(_) => "invalid_input",
        WorkerError::UnknownKind(_) => "unknown_kind",
        WorkerError::Revoked(_) => "revoked",
        WorkerError::Unauthorized(_) => "unauthorized",
        WorkerError::AlreadyClaimed(_) => "already_claimed",
        WorkerError::BadSignature(_) => "bad_signature",
        WorkerError::ChallengeUsed(_) => "challenge_used",
        WorkerError::Conflict(_) => "conflict",
        WorkerError::NotFound(_) => "not_found",
        WorkerError::BackendFailure { .. } => "backend_failure",
    };
    error_response(code, &error.to_string())
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hex_decode<const N: usize>(raw: &str, what: &str) -> Result<[u8; N], WorkerError> {
    if raw.len() != N * 2 || !raw.is_ascii() {
        return Err(WorkerError::InvalidInput(format!(
            "{what} must be {} hex chars",
            N * 2
        )));
    }
    let bytes: Vec<u8> = (0..N)
        .map(|i| {
            u8::from_str_radix(&raw[i * 2..i * 2 + 2], 16)
                .map_err(|_| WorkerError::InvalidInput(format!("{what} is not hex")))
        })
        .collect::<Result<Vec<u8>, WorkerError>>()?;
    bytes
        .try_into()
        .map_err(|_| WorkerError::InvalidInput(format!("{what} has wrong length")))
}

fn device_response(d: &DeviceRecord) -> WorkerDeviceResponse {
    WorkerDeviceResponse {
        device_id: d.id.to_string(),
        name: d.name.clone(),
        public_key_hex: hex_encode(&d.public_key),
        fingerprint: d.fingerprint.clone(),
        state: d.state.as_str().to_owned(),
        enrolled_by: d.enrolled_by.clone(),
        enrolled_at: d.enrolled_at,
    }
}

fn job_response(j: &WorkerJob) -> WorkerJobResponse {
    WorkerJobResponse {
        job_id: j.id.to_string(),
        objective: j.objective.clone(),
        capability_scope: j.capability_scope.as_str().to_owned(),
        authorized_by: j.authorized_by.clone(),
        status: j.status.as_str().to_owned(),
        claimed_by: j
            .claimed_by
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default(),
        created_at: j.created_at,
        finished_at: j.finished_at.unwrap_or(0),
        evidence_ref: j.evidence_ref.clone().unwrap_or_default(),
    }
}

macro_rules! id_or_err {
    ($raw:expr, $ty:ty) => {
        match <$ty>::new($raw) {
            Ok(v) => v,
            Err(e) => return map_error(&e),
        }
    };
}

pub fn dispatch<S: WorkerStorePort>(
    service: &mut WorkerService<S>,
    request: WorkerCoreRequest,
) -> WorkerCoreResponse {
    match request.payload {
        Some(worker_core_request::Payload::Enroll(req)) => {
            let public = match hex_decode::<32>(&req.public_key_hex, "public_key") {
                Ok(v) => v,
                Err(e) => return map_error(&e),
            };
            match service.enroll(&req.name, &public, &req.enrolled_by, req.now_millis) {
                Ok(d) => WorkerCoreResponse {
                    payload: Some(worker_core_response::Payload::Device(device_response(&d))),
                },
                Err(e) => map_error(&e),
            }
        }
        Some(worker_core_request::Payload::Revoke(req)) => {
            let id = id_or_err!(req.device_id, DeviceId);
            match service.revoke(&id) {
                Ok(d) => WorkerCoreResponse {
                    payload: Some(worker_core_response::Payload::Device(device_response(&d))),
                },
                Err(e) => map_error(&e),
            }
        }
        Some(worker_core_request::Payload::GetDevice(req)) => {
            let id = id_or_err!(req.device_id, DeviceId);
            match service.get_device(&id) {
                Ok(d) => WorkerCoreResponse {
                    payload: Some(worker_core_response::Payload::Device(device_response(&d))),
                },
                Err(e) => map_error(&e),
            }
        }
        Some(worker_core_request::Payload::ListDevices(_)) => match service.list_devices() {
            Ok(devices) => WorkerCoreResponse {
                payload: Some(worker_core_response::Payload::List(WorkerListResponse {
                    devices: devices.iter().map(device_response).collect(),
                    jobs: Vec::new(),
                })),
            },
            Err(e) => map_error(&e),
        },
        Some(worker_core_request::Payload::Offer(req)) => {
            let scope = match CapabilityScope::parse(&req.capability_scope) {
                Ok(v) => v,
                Err(e) => return map_error(&e),
            };
            match service.offer(
                OfferRequest {
                    objective: req.objective,
                    capability_scope: scope,
                    authorized_by: req.authorized_by,
                },
                req.now_millis,
            ) {
                Ok(j) => WorkerCoreResponse {
                    payload: Some(worker_core_response::Payload::Job(job_response(&j))),
                },
                Err(e) => map_error(&e),
            }
        }
        Some(worker_core_request::Payload::IssueChallenge(req)) => {
            let device_id = id_or_err!(req.device_id, DeviceId);
            let job_id = id_or_err!(req.job_id, JobId);
            let nonce = match hex_decode::<32>(&req.nonce_hex, "nonce") {
                Ok(v) => v,
                Err(e) => return map_error(&e),
            };
            match service.issue_challenge(&device_id, &job_id, nonce, req.now_millis) {
                Ok(ch) => WorkerCoreResponse {
                    payload: Some(worker_core_response::Payload::Challenge(
                        WorkerChallengeResponse {
                            challenge_id: ch.id.to_string(),
                            device_id: ch.device_id.to_string(),
                            job_id: ch.job_id.to_string(),
                            nonce_hex: hex_encode(&ch.nonce),
                        },
                    )),
                },
                Err(e) => map_error(&e),
            }
        }
        Some(worker_core_request::Payload::Claim(req)) => {
            let job_id = id_or_err!(req.job_id, JobId);
            let device_id = id_or_err!(req.device_id, DeviceId);
            let challenge_id = id_or_err!(req.challenge_id, ChallengeId);
            let signature = match hex_decode::<64>(&req.signature_hex, "signature") {
                Ok(v) => v,
                Err(e) => return map_error(&e),
            };
            match service.claim(
                &job_id,
                &device_id,
                &challenge_id,
                &signature,
                req.now_millis,
            ) {
                Ok(j) => WorkerCoreResponse {
                    payload: Some(worker_core_response::Payload::Job(job_response(&j))),
                },
                Err(e) => map_error(&e),
            }
        }
        Some(worker_core_request::Payload::Complete(req)) => {
            let job_id = id_or_err!(req.job_id, JobId);
            let device_id = id_or_err!(req.device_id, DeviceId);
            let challenge_id = id_or_err!(req.challenge_id, ChallengeId);
            let signature = match hex_decode::<64>(&req.signature_hex, "signature") {
                Ok(v) => v,
                Err(e) => return map_error(&e),
            };
            match service.complete(
                &job_id,
                &device_id,
                &challenge_id,
                &signature,
                &req.evidence_ref,
                req.now_millis,
            ) {
                Ok(j) => WorkerCoreResponse {
                    payload: Some(worker_core_response::Payload::Job(job_response(&j))),
                },
                Err(e) => map_error(&e),
            }
        }
        Some(worker_core_request::Payload::ListJobs(_)) => match service.list_jobs() {
            Ok(jobs) => WorkerCoreResponse {
                payload: Some(worker_core_response::Payload::List(WorkerListResponse {
                    devices: Vec::new(),
                    jobs: jobs.iter().map(job_response).collect(),
                })),
            },
            Err(e) => map_error(&e),
        },
        None => error_response("invalid_input", "missing worker payload"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_hex_enrollment_refuses_without_mutating_the_store() {
        let root = tempfile::tempdir().unwrap();
        let store = storage_sqlite::SqliteStore::open(root.path().join("state.sqlite3")).unwrap();
        let mut service = WorkerService::new(store);
        for public_key_hex in [
            "\u{1f4a5}".repeat(16),
            format!("a{}", "\u{3042}".repeat(21)),
            "gg".repeat(32),
            String::new(),
        ] {
            let response = dispatch(
                &mut service,
                WorkerCoreRequest {
                    payload: Some(worker_core_request::Payload::Enroll(
                        ipc_contracts::proto::WorkerEnrollRequest {
                            name: "unicode-fixture".to_owned(),
                            public_key_hex,
                            enrolled_by: "fixture-operator".to_owned(),
                            now_millis: 1,
                        },
                    )),
                },
            );
            assert!(matches!(response.payload,
                Some(worker_core_response::Payload::Error(error)) if error.code == "invalid_input"));
        }
        assert!(service.list_devices().unwrap().is_empty());
        let (_, public) = worker::model::keypair_from_secret(&[7; 32]).unwrap();
        let enrolled = dispatch(
            &mut service,
            WorkerCoreRequest {
                payload: Some(worker_core_request::Payload::Enroll(
                    ipc_contracts::proto::WorkerEnrollRequest {
                        name: "valid-fixture".to_owned(),
                        public_key_hex: hex_encode(&public),
                        enrolled_by: "fixture-operator".to_owned(),
                        now_millis: 2,
                    },
                )),
            },
        );
        assert!(matches!(
            enrolled.payload,
            Some(worker_core_response::Payload::Device(_))
        ));
        assert_eq!(service.list_devices().unwrap().len(), 1);
        assert_eq!(hex_decode::<3>("00Ff80", "key").unwrap(), [0, 255, 128]);
        assert!(hex_decode::<64>(&"\u{1f4a5}".repeat(32), "signature").is_err());
    }
}
