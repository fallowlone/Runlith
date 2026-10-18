use app_services::worker_service::{OfferRequest, WorkerService};
use storage_sqlite::SqliteStore;
use worker::model::{sign_challenge, DeviceState, JobStatus, WorkerError};

fn fresh() -> (WorkerService<SqliteStore>, std::path::PathBuf) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static C: AtomicU64 = AtomicU64::new(0);
    let db = std::env::temp_dir().join(format!(
        "runlith-wsvc-{}-{}.sqlite",
        std::process::id(),
        C.fetch_add(1, Ordering::SeqCst)
    ));
    (WorkerService::new(SqliteStore::open(&db).unwrap()), db)
}

fn enrolled(
    svc: &mut WorkerService<SqliteStore>,
    seed: u8,
) -> (worker::model::DeviceRecord, [u8; 32]) {
    let (secret, public) = worker::model::keypair_from_secret(&[seed; 32]).unwrap();
    let device = svc.enroll("worker-1", &public, "operator", 1_000).unwrap();
    (device, secret)
}

#[test]
fn full_signed_cycle_claim_and_complete() {
    let (mut svc, db) = fresh();
    let (device, secret) = enrolled(&mut svc, 7);
    let job = svc
        .offer(
            OfferRequest {
                objective: "build".into(),
                capability_scope: worker::model::CapabilityScope::Execution,
                authorized_by: "operator".into(),
            },
            1_000,
        )
        .unwrap();
    // Claim requires a signed fresh challenge.
    let ch = svc
        .issue_challenge(&device.id, &job.id, [1u8; 32], 1_100)
        .unwrap();
    let sig = sign_challenge(&secret, &ch.nonce, &job.id).unwrap();
    let claimed = svc.claim(&job.id, &device.id, &ch.id, &sig, 1_200).unwrap();
    assert_eq!(claimed.status, JobStatus::Claimed);
    // Replay of the same challenge refuses (W-4).
    let err = svc
        .claim(&job.id, &device.id, &ch.id, &sig, 1_300)
        .unwrap_err();
    assert!(matches!(
        err,
        WorkerError::ChallengeUsed(_) | WorkerError::AlreadyClaimed(_)
    ));
    // Complete with a fresh challenge.
    let ch2 = svc
        .issue_challenge(&device.id, &job.id, [2u8; 32], 1_400)
        .unwrap();
    let sig2 = sign_challenge(&secret, &ch2.nonce, &job.id).unwrap();
    let done = svc
        .complete(&job.id, &device.id, &ch2.id, &sig2, "evidence:built", 1_500)
        .unwrap();
    assert_eq!(done.status, JobStatus::Completed);
    assert_eq!(done.evidence_ref.as_deref(), Some("evidence:built"));
    std::fs::remove_file(db).ok();
}

#[test]
fn bad_signature_and_revoked_device_refuse() {
    let (mut svc, db) = fresh();
    let (device, _secret) = enrolled(&mut svc, 8);
    let job = svc
        .offer(
            OfferRequest {
                objective: "build".into(),
                capability_scope: worker::model::CapabilityScope::ReadOnly,
                authorized_by: "operator".into(),
            },
            1_000,
        )
        .unwrap();
    let ch = svc
        .issue_challenge(&device.id, &job.id, [3u8; 32], 1_100)
        .unwrap();
    let bad = sign_challenge(&[9u8; 32], &ch.nonce, &job.id).unwrap();
    assert!(matches!(
        svc.claim(&job.id, &device.id, &ch.id, &bad, 1_200),
        Err(WorkerError::BadSignature(_))
    ));
    svc.revoke(&device.id).unwrap();
    assert_eq!(
        svc.get_device(&device.id).unwrap().state,
        DeviceState::Revoked
    );
    let ch2 = svc.issue_challenge(&device.id, &job.id, [4u8; 32], 1_300);
    assert!(ch2.is_err(), "revoked device gets no challenge");
    std::fs::remove_file(db).ok();
}
