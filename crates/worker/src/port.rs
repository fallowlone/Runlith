//! Durable worker seam: devices, jobs, challenges behind a port.
use crate::model::{Challenge, ChallengeId, DeviceId, DeviceRecord, JobId, WorkerError, WorkerJob};

pub trait WorkerStorePort {
    fn enroll_device(&mut self, device: &DeviceRecord) -> Result<(), WorkerError>;
    fn load_device(&self, id: &DeviceId) -> Result<Option<DeviceRecord>, WorkerError>;
    fn update_device(&mut self, device: &DeviceRecord) -> Result<(), WorkerError>;
    fn list_devices(&self) -> Result<Vec<DeviceRecord>, WorkerError>;
    fn offer_job(&mut self, job: &WorkerJob) -> Result<(), WorkerError>;
    fn load_job(&self, id: &JobId) -> Result<Option<WorkerJob>, WorkerError>;
    fn update_job(&mut self, job: &WorkerJob) -> Result<(), WorkerError>;
    fn list_jobs(&self) -> Result<Vec<WorkerJob>, WorkerError>;
    fn put_challenge(&mut self, challenge: &Challenge) -> Result<(), WorkerError>;
    fn load_challenge(&self, id: &ChallengeId) -> Result<Option<Challenge>, WorkerError>;
    fn update_challenge(&mut self, challenge: &Challenge) -> Result<(), WorkerError>;
}
