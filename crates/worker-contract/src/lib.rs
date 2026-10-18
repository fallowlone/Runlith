//! Shared `worker` store contract suite (TASK-0199, G12).
//! Every store implementing `WorkerStorePort` must satisfy
//! [`run_worker_store_contract`].
use worker::mint::{mint_challenge_id, mint_device_id, mint_job_id};
use worker::model::{
    sign_challenge, verify_challenge, CapabilityScope, Challenge, DeviceRecord, DeviceState,
    JobStatus, WorkerJob,
};
use worker::port::WorkerStorePort;

pub mod fixtures {
    use super::*;
    pub fn secret(seed: u8) -> [u8; 32] {
        [seed; 32]
    }
    pub fn device(seq: u128, seed: u8) -> (DeviceRecord, [u8; 32]) {
        let (secret, public) = worker::model::keypair_from_secret(&secret(seed)).unwrap();
        (
            DeviceRecord::enroll(
                mint_device_id(1_000, seq),
                "worker",
                &public,
                "operator",
                1_000,
            )
            .unwrap(),
            secret,
        )
    }
    pub fn job(seq: u128) -> WorkerJob {
        WorkerJob::offer(
            mint_job_id(1_000, seq),
            "remote work",
            CapabilityScope::Execution,
            "operator",
            1_000,
        )
        .unwrap()
    }
}

fn assert_enroll_claim<S: WorkerStorePort>(store: &mut S) {
    let (device, secret) = fixtures::device(1, 7);
    store.enroll_device(&device).unwrap();
    store.enroll_device(&device).unwrap();
    let mut changed = device.clone();
    changed.name = "changed".to_owned();
    assert!(
        store.enroll_device(&changed).is_err(),
        "differing payload conflicts"
    );
    assert_eq!(store.load_device(&device.id).unwrap(), Some(device.clone()));

    let mut job = fixtures::job(1);
    store.offer_job(&job).unwrap();
    job.claim(&device).unwrap();
    store.update_job(&job).unwrap();

    // Signed challenge round-trips and verifies against stored device.
    let challenge = Challenge::issue(
        mint_challenge_id(1_000, 1),
        device.id.clone(),
        job.id.clone(),
        std::array::from_fn(|i| (i as u8).wrapping_mul(31).wrapping_add(17)),
        1_500,
    );
    store.put_challenge(&challenge).unwrap();
    let loaded = store.load_challenge(&challenge.id).unwrap().unwrap();
    let signature = sign_challenge(&secret, &loaded.nonce, &job.id).unwrap();
    let stored_device = store.load_device(&device.id).unwrap().unwrap();
    assert!(verify_challenge(
        &stored_device,
        &loaded.nonce,
        &job.id,
        &signature
    ));

    let mut done = job.clone();
    done.complete("evidence:ok", 2_000).unwrap();
    store.update_job(&done).unwrap();
    assert_eq!(
        store.load_job(&job.id).unwrap().unwrap().status,
        JobStatus::Completed
    );
    assert!(store.load_device(&mint_device_id(9, 9)).unwrap().is_none());
}

fn assert_revocation_persists<S: WorkerStorePort>(store: &mut S) {
    let (mut device, _) = fixtures::device(2, 8);
    store.enroll_device(&device).unwrap();
    device.revoke().unwrap();
    store.update_device(&device).unwrap();
    assert_eq!(
        store.load_device(&device.id).unwrap().unwrap().state,
        DeviceState::Revoked
    );
    let mut job = fixtures::job(2);
    store.offer_job(&job).unwrap();
    assert!(job.claim(&device).is_err(), "revoked device never claims");
}

fn assert_reopen_holds<S: WorkerStorePort>(store: &S) {
    assert!(store
        .load_device(&mint_device_id(1_000, 1))
        .unwrap()
        .is_some());
    assert!(store.load_job(&mint_job_id(1_000, 1)).unwrap().is_some());
    assert!(store
        .load_challenge(&mint_challenge_id(1_000, 1))
        .unwrap()
        .is_some());
}

/// Runs the full worker store contract against a fresh store.
/// `reopen` must yield the same durable state (restart durability).
pub fn run_worker_store_contract<S: WorkerStorePort>(store: &mut S, reopen: impl Fn() -> S) {
    assert_enroll_claim(store);
    assert_revocation_persists(store);
    let fresh = reopen();
    assert_reopen_holds(&fresh);
}
