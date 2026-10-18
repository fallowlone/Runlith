//! Trusted remote-worker composition (TASK-0201, ADR-0046).
//! Enrollment, signed-challenge claim/complete, revocation. A result
//! is accepted only from the claiming device with a fresh, unused,
//! correctly signed challenge (W-2..W-4, W-8).
use std::sync::atomic::{AtomicU64, Ordering};

use worker::mint::{mint_challenge_id, mint_device_id, mint_job_id};
use worker::model::{
    verify_challenge, CapabilityScope, Challenge, ChallengeId, DeviceId, DeviceRecord, JobId,
    WorkerError, WorkerJob,
};
use worker::port::WorkerStorePort;

#[derive(Debug)]
pub struct OfferRequest {
    pub objective: String,
    pub capability_scope: CapabilityScope,
    pub authorized_by: String,
}

pub struct WorkerService<S> {
    store: S,
    id_seq: AtomicU64,
}

impl<S: WorkerStorePort> WorkerService<S> {
    pub fn new(store: S) -> Self {
        Self {
            store,
            id_seq: AtomicU64::new(0),
        }
    }

    pub fn store(&self) -> &S {
        &self.store
    }

    pub fn store_mut(&mut self) -> &mut S {
        &mut self.store
    }

    fn seq(&self) -> u128 {
        u128::from(self.id_seq.fetch_add(1, Ordering::SeqCst))
    }

    pub fn enroll(
        &mut self,
        name: &str,
        public_key: &[u8; 32],
        enrolled_by: &str,
        now: u64,
    ) -> Result<DeviceRecord, WorkerError> {
        let device = DeviceRecord::enroll(
            mint_device_id(now, self.seq()),
            name,
            public_key,
            enrolled_by,
            now,
        )?;
        self.store.enroll_device(&device)?;
        Ok(device)
    }

    pub fn get_device(&self, id: &DeviceId) -> Result<DeviceRecord, WorkerError> {
        self.store
            .load_device(id)?
            .ok_or_else(|| WorkerError::NotFound(id.to_string()))
    }

    pub fn revoke(&mut self, id: &DeviceId) -> Result<DeviceRecord, WorkerError> {
        let mut device = self.get_device(id)?;
        device.revoke()?;
        self.store.update_device(&device)?;
        Ok(device)
    }

    pub fn list_devices(&self) -> Result<Vec<DeviceRecord>, WorkerError> {
        self.store.list_devices()
    }

    pub fn offer(&mut self, request: OfferRequest, now: u64) -> Result<WorkerJob, WorkerError> {
        let job = WorkerJob::offer(
            mint_job_id(now, self.seq()),
            request.objective,
            request.capability_scope,
            request.authorized_by,
            now,
        )?;
        self.store.offer_job(&job)?;
        Ok(job)
    }

    pub fn get_job(&self, id: &JobId) -> Result<WorkerJob, WorkerError> {
        self.store
            .load_job(id)?
            .ok_or_else(|| WorkerError::NotFound(id.to_string()))
    }

    pub fn list_jobs(&self) -> Result<Vec<WorkerJob>, WorkerError> {
        self.store.list_jobs()
    }

    /// Issues a single-use challenge for an ENROLLED device (W-2).
    pub fn issue_challenge(
        &mut self,
        device_id: &DeviceId,
        job_id: &JobId,
        nonce: [u8; 32],
        now: u64,
    ) -> Result<Challenge, WorkerError> {
        let device = self.get_device(device_id)?;
        if !device.is_enrolled() {
            return Err(WorkerError::Revoked(device_id.to_string()));
        }
        self.get_job(job_id)?;
        let challenge = Challenge::issue(
            mint_challenge_id(now, self.seq()),
            device_id.clone(),
            job_id.clone(),
            nonce,
            now,
        );
        self.store.put_challenge(&challenge)?;
        Ok(challenge)
    }

    /// Verifies and consumes a challenge: device/job binding, unused,
    /// enrolled, signature valid. Any failure refuses without consuming
    /// except a validly signed use, which consumes exactly once.
    fn verify_and_consume(
        &mut self,
        job_id: &JobId,
        device_id: &DeviceId,
        challenge_id: &ChallengeId,
        signature: &[u8; 64],
    ) -> Result<DeviceRecord, WorkerError> {
        let mut challenge = self
            .store
            .load_challenge(challenge_id)?
            .ok_or_else(|| WorkerError::NotFound(challenge_id.to_string()))?;
        if challenge.device_id != *device_id || challenge.job_id != *job_id {
            return Err(WorkerError::BadSignature(
                "challenge binding mismatch".to_owned(),
            ));
        }
        if challenge.used {
            return Err(WorkerError::ChallengeUsed(challenge_id.to_string()));
        }
        let device = self.get_device(device_id)?;
        if !device.is_enrolled() {
            return Err(WorkerError::Revoked(device_id.to_string()));
        }
        if !verify_challenge(&device, &challenge.nonce, job_id, signature) {
            return Err(WorkerError::BadSignature(device_id.to_string()));
        }
        challenge.consume()?;
        self.store.update_challenge(&challenge)?;
        Ok(device)
    }

    /// CLAIM with signed challenge (W-5, W-7).
    pub fn claim(
        &mut self,
        job_id: &JobId,
        device_id: &DeviceId,
        challenge_id: &ChallengeId,
        signature: &[u8; 64],
        _now: u64,
    ) -> Result<WorkerJob, WorkerError> {
        let device = self.verify_and_consume(job_id, device_id, challenge_id, signature)?;
        let mut job = self.get_job(job_id)?;
        job.claim(&device)?;
        self.store.update_job(&job)?;
        Ok(job)
    }

    /// COMPLETE with a fresh signed challenge; only the claimant (W-8).
    pub fn complete(
        &mut self,
        job_id: &JobId,
        device_id: &DeviceId,
        challenge_id: &ChallengeId,
        signature: &[u8; 64],
        evidence: &str,
        now: u64,
    ) -> Result<WorkerJob, WorkerError> {
        self.verify_and_consume(job_id, device_id, challenge_id, signature)?;
        let mut job = self.get_job(job_id)?;
        if job.claimed_by.as_ref() != Some(device_id) {
            return Err(WorkerError::Unauthorized(format!(
                "job {} claimed by another device",
                job_id
            )));
        }
        job.complete(evidence, now)?;
        self.store.update_job(&job)?;
        Ok(job)
    }
}
