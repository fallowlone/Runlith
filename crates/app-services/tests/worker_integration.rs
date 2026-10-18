//! TASK-0203: a job is claimed and completed by an enrolled worker
//! with signed challenges; state survives coordinator restart.
use app_services::worker_service::{OfferRequest, WorkerService};
use storage_sqlite::SqliteStore;
use worker::model::{sign_challenge, CapabilityScope, JobStatus};

#[test]
fn remote_job_cycle_across_coordinator_restart() {
    use std::sync::atomic::{AtomicU64, Ordering};
    static C: AtomicU64 = AtomicU64::new(0);
    let db = std::env::temp_dir().join(format!(
        "runlith-wint-{}-{}.sqlite",
        std::process::id(),
        C.fetch_add(1, Ordering::SeqCst)
    ));
    let (secret, public) = worker::model::keypair_from_secret(&[11u8; 32]).unwrap();
    let job_id;
    let device_id;
    {
        let mut svc = WorkerService::new(SqliteStore::open(&db).unwrap());
        let device = svc.enroll("mac-mini", &public, "operator", 1_000).unwrap();
        device_id = device.id.clone();
        let job = svc
            .offer(
                OfferRequest {
                    objective: "build release".into(),
                    capability_scope: CapabilityScope::Execution,
                    authorized_by: "operator".into(),
                },
                1_000,
            )
            .unwrap();
        job_id = job.id.clone();
        let ch = svc
            .issue_challenge(&device_id, &job_id, [5u8; 32], 1_100)
            .unwrap();
        let sig = sign_challenge(&secret, &ch.nonce, &job_id).unwrap();
        svc.claim(&job_id, &device_id, &ch.id, &sig, 1_200).unwrap();
    }
    // Coordinator restart: claim durable, completion still requires signer.
    {
        let mut svc = WorkerService::new(SqliteStore::open(&db).unwrap());
        assert_eq!(svc.get_job(&job_id).unwrap().status, JobStatus::Claimed);
        let ch = svc
            .issue_challenge(&device_id, &job_id, [6u8; 32], 1_300)
            .unwrap();
        let sig = sign_challenge(&secret, &ch.nonce, &job_id).unwrap();
        let done = svc
            .complete(
                &job_id,
                &device_id,
                &ch.id,
                &sig,
                "evidence:release-built",
                1_400,
            )
            .unwrap();
        assert_eq!(done.status, JobStatus::Completed);
        assert_eq!(done.evidence_ref.as_deref(), Some("evidence:release-built"));
    }
    std::fs::remove_file(db).ok();
}
