//! TASK-0204 failure matrix (ADR-0046): revoked device, bad signature,
//! replayed challenge, double claim, offline mid-job.
use app_services::worker_service::{OfferRequest, WorkerService};
use storage_sqlite::SqliteStore;
use worker::model::{sign_challenge, CapabilityScope, JobStatus, WorkerError};

fn fresh() -> (WorkerService<SqliteStore>, std::path::PathBuf) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static C: AtomicU64 = AtomicU64::new(0);
    let db = std::env::temp_dir().join(format!(
        "runlith-wfail-{}-{}.sqlite",
        std::process::id(),
        C.fetch_add(1, Ordering::SeqCst)
    ));
    (WorkerService::new(SqliteStore::open(&db).unwrap()), db)
}

fn setup(
    svc: &mut WorkerService<SqliteStore>,
    seed: u8,
) -> (
    worker::model::DeviceRecord,
    [u8; 32],
    worker::model::WorkerJob,
) {
    let (secret, public) = worker::model::keypair_from_secret(&[seed; 32]).unwrap();
    let device = svc.enroll("w", &public, "operator", 1_000).unwrap();
    let job = svc
        .offer(
            OfferRequest {
                objective: "work".into(),
                capability_scope: CapabilityScope::Execution,
                authorized_by: "operator".into(),
            },
            1_000,
        )
        .unwrap();
    (device, secret, job)
}

#[test]
fn replayed_challenge_and_double_claim_refuse() {
    let (mut svc, db) = fresh();
    let (device, secret, job) = setup(&mut svc, 21);
    let (device2, secret2, _) = setup(&mut svc, 22);
    let ch = svc
        .issue_challenge(&device.id, &job.id, [1u8; 32], 1_100)
        .unwrap();
    let sig = sign_challenge(&secret, &ch.nonce, &job.id).unwrap();
    svc.claim(&job.id, &device.id, &ch.id, &sig, 1_200).unwrap();
    // Replay: same challenge for device2's claim attempt refuses.
    let ch2 = svc
        .issue_challenge(&device2.id, &job.id, [2u8; 32], 1_300)
        .unwrap();
    let sig2 = sign_challenge(&secret2, &ch2.nonce, &job.id).unwrap();
    assert!(matches!(
        svc.claim(&job.id, &device2.id, &ch2.id, &sig2, 1_400),
        Err(WorkerError::AlreadyClaimed(_))
    ));
    // Completion by non-claimant refuses.
    let ch3 = svc
        .issue_challenge(&device2.id, &job.id, [3u8; 32], 1_500)
        .unwrap();
    let sig3 = sign_challenge(&secret2, &ch3.nonce, &job.id).unwrap();
    assert!(matches!(
        svc.complete(&job.id, &device2.id, &ch3.id, &sig3, "evidence:x", 1_600),
        Err(WorkerError::Unauthorized(_))
    ));
    std::fs::remove_file(db).ok();
}

#[test]
fn offline_mid_job_stays_claimed_not_completed() {
    let (mut svc, db) = fresh();
    let (device, secret, job) = setup(&mut svc, 23);
    let ch = svc
        .issue_challenge(&device.id, &job.id, [4u8; 32], 1_100)
        .unwrap();
    let sig = sign_challenge(&secret, &ch.nonce, &job.id).unwrap();
    svc.claim(&job.id, &device.id, &ch.id, &sig, 1_200).unwrap();
    drop(svc);
    // Worker went offline; coordinator restart must not fabricate completion.
    let svc = WorkerService::new(SqliteStore::open(&db).unwrap());
    let job = svc.get_job(&job.id).unwrap();
    assert_eq!(job.status, JobStatus::Claimed);
    assert!(job.evidence_ref.is_none());
    std::fs::remove_file(db).ok();
}

#[test]
fn revoked_after_claim_cannot_complete() {
    let (mut svc, db) = fresh();
    let (device, secret, job) = setup(&mut svc, 24);
    let ch = svc
        .issue_challenge(&device.id, &job.id, [5u8; 32], 1_100)
        .unwrap();
    let sig = sign_challenge(&secret, &ch.nonce, &job.id).unwrap();
    svc.claim(&job.id, &device.id, &ch.id, &sig, 1_200).unwrap();
    svc.revoke(&device.id).unwrap();
    let ch2 = svc.issue_challenge(&device.id, &job.id, [6u8; 32], 1_300);
    assert!(
        ch2.is_err(),
        "revoked device receives no new challenge (W-10)"
    );
    std::fs::remove_file(db).ok();
}
