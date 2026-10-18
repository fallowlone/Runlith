use worker::mint::{mint_device_id, mint_job_id};
use worker::model::{
    sign_challenge, verify_challenge, CapabilityScope, DeviceRecord, JobStatus, WorkerError,
    WorkerJob,
};

fn keypair(seed: u8) -> ([u8; 32], [u8; 32]) {
    worker::model::keypair_from_secret(&[seed; 32]).unwrap()
}

#[test]
fn device_fingerprint_binds_public_key() {
    let (_, public) = keypair(7);
    let device = DeviceRecord::enroll(
        mint_device_id(1_000, 1),
        "mac-mini",
        &public,
        "operator",
        1_000,
    )
    .unwrap();
    assert_eq!(device.fingerprint, worker::model::fingerprint(&public));
    assert!(device.is_enrolled());
    let other =
        DeviceRecord::enroll(mint_device_id(1_000, 2), "x", &[9u8; 32], "operator", 1_000).unwrap();
    assert_ne!(device.fingerprint, other.fingerprint);
}

#[test]
fn signed_challenge_verifies_only_for_enrolled_key() {
    let (secret, public) = keypair(3);
    let device =
        DeviceRecord::enroll(mint_device_id(1_000, 1), "w", &public, "operator", 1_000).unwrap();
    let job = WorkerJob::offer(
        mint_job_id(1_000, 1),
        "build",
        CapabilityScope::Execution,
        "operator",
        1_000,
    )
    .unwrap();
    let nonce = [42u8; 32];
    let signature = sign_challenge(&secret, &nonce, &job.id).unwrap();
    assert!(verify_challenge(&device, &nonce, &job.id, &signature));
    // Wrong key signature refuses.
    let (bad_secret, _) = keypair(9);
    let bad = sign_challenge(&bad_secret, &nonce, &job.id).unwrap();
    assert!(!verify_challenge(&device, &nonce, &job.id, &bad));
    // Signature for another job refuses.
    let job2 = WorkerJob::offer(
        mint_job_id(1_000, 2),
        "build2",
        CapabilityScope::Execution,
        "operator",
        1_000,
    )
    .unwrap();
    assert!(!verify_challenge(&device, &nonce, &job2.id, &signature));
}

#[test]
fn job_lifecycle_single_claim_and_revocation() {
    let (_, public) = keypair(5);
    let mut device =
        DeviceRecord::enroll(mint_device_id(1_000, 1), "w", &public, "operator", 1_000).unwrap();
    let mut job = WorkerJob::offer(
        mint_job_id(1_000, 1),
        "work",
        CapabilityScope::ReadOnly,
        "operator",
        1_000,
    )
    .unwrap();
    assert_eq!(job.status, JobStatus::Offered);
    job.claim(&device).unwrap();
    assert_eq!(job.status, JobStatus::Claimed);
    // Second claim refuses (W-7).
    assert!(job.claim(&device).is_err());
    job.complete("evidence:done", 2_000).unwrap();
    assert_eq!(job.status, JobStatus::Completed);
    // Revoked device cannot claim new work (W-10).
    device.revoke().unwrap();
    let mut job2 = WorkerJob::offer(
        mint_job_id(1_000, 2),
        "work2",
        CapabilityScope::ReadOnly,
        "operator",
        1_000,
    )
    .unwrap();
    assert!(matches!(job2.claim(&device), Err(WorkerError::Revoked(_))));
}

#[test]
fn unauthorized_job_never_offers_and_wire_roundtrip() {
    assert!(WorkerJob::offer(
        mint_job_id(1_000, 1),
        "w",
        CapabilityScope::ReadOnly,
        "",
        1_000
    )
    .is_err());
    assert_eq!(
        CapabilityScope::parse("EXECUTION").unwrap(),
        CapabilityScope::Execution
    );
    assert!(CapabilityScope::parse("ROOT").is_err());
    assert_eq!(JobStatus::parse("CLAIMED").unwrap(), JobStatus::Claimed);
}
