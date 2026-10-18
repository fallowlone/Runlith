use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use worker::model::{
    Challenge, ChallengeId, DeviceId, DeviceRecord, JobId, WorkerError, WorkerJob,
};
use worker::port::WorkerStorePort;
use worker_contract::run_worker_store_contract;

#[derive(Default)]
struct Inner {
    devices: BTreeMap<DeviceId, DeviceRecord>,
    jobs: BTreeMap<JobId, WorkerJob>,
    challenges: BTreeMap<ChallengeId, Challenge>,
}
#[derive(Default, Clone)]
struct InMemory {
    inner: Arc<Mutex<Inner>>,
}

impl WorkerStorePort for InMemory {
    fn enroll_device(&mut self, d: &DeviceRecord) -> Result<(), WorkerError> {
        d.revalidate()?;
        let mut g = self.inner.lock().unwrap();
        match g.devices.get(&d.id) {
            Some(e) if e == d => Ok(()),
            Some(_) => Err(WorkerError::Conflict(d.id.to_string())),
            None => {
                g.devices.insert(d.id.clone(), d.clone());
                Ok(())
            }
        }
    }
    fn load_device(&self, id: &DeviceId) -> Result<Option<DeviceRecord>, WorkerError> {
        Ok(self.inner.lock().unwrap().devices.get(id).cloned())
    }
    fn update_device(&mut self, d: &DeviceRecord) -> Result<(), WorkerError> {
        d.revalidate()?;
        let mut g = self.inner.lock().unwrap();
        if !g.devices.contains_key(&d.id) {
            return Err(WorkerError::NotFound(d.id.to_string()));
        }
        g.devices.insert(d.id.clone(), d.clone());
        Ok(())
    }
    fn list_devices(&self) -> Result<Vec<DeviceRecord>, WorkerError> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .devices
            .values()
            .cloned()
            .collect())
    }
    fn offer_job(&mut self, j: &WorkerJob) -> Result<(), WorkerError> {
        j.revalidate()?;
        let mut g = self.inner.lock().unwrap();
        match g.jobs.get(&j.id) {
            Some(e) if e == j => Ok(()),
            Some(_) => Err(WorkerError::Conflict(j.id.to_string())),
            None => {
                g.jobs.insert(j.id.clone(), j.clone());
                Ok(())
            }
        }
    }
    fn load_job(&self, id: &JobId) -> Result<Option<WorkerJob>, WorkerError> {
        Ok(self.inner.lock().unwrap().jobs.get(id).cloned())
    }
    fn update_job(&mut self, j: &WorkerJob) -> Result<(), WorkerError> {
        j.revalidate()?;
        let mut g = self.inner.lock().unwrap();
        if !g.jobs.contains_key(&j.id) {
            return Err(WorkerError::NotFound(j.id.to_string()));
        }
        g.jobs.insert(j.id.clone(), j.clone());
        Ok(())
    }
    fn list_jobs(&self) -> Result<Vec<WorkerJob>, WorkerError> {
        Ok(self.inner.lock().unwrap().jobs.values().cloned().collect())
    }
    fn put_challenge(&mut self, c: &Challenge) -> Result<(), WorkerError> {
        let mut g = self.inner.lock().unwrap();
        g.challenges.insert(c.id.clone(), c.clone());
        Ok(())
    }
    fn load_challenge(&self, id: &ChallengeId) -> Result<Option<Challenge>, WorkerError> {
        Ok(self.inner.lock().unwrap().challenges.get(id).cloned())
    }
    fn update_challenge(&mut self, c: &Challenge) -> Result<(), WorkerError> {
        let mut g = self.inner.lock().unwrap();
        if !g.challenges.contains_key(&c.id) {
            return Err(WorkerError::NotFound(c.id.to_string()));
        }
        g.challenges.insert(c.id.clone(), c.clone());
        Ok(())
    }
}

#[test]
fn in_memory_store_satisfies_worker_contract() {
    let mut store = InMemory::default();
    let reopened = store.clone();
    run_worker_store_contract(&mut store, move || reopened.clone());
}
