use background::model::{
    BackgroundError, RoutineDefinition, RoutineId, RoutineRun, RunId, RunStatus,
};
use background::port::RoutineStorePort;
use background_contract::run_routine_store_contract;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Inner {
    defs: BTreeMap<RoutineId, RoutineDefinition>,
    runs: BTreeMap<RunId, RoutineRun>,
}
#[derive(Default, Clone)]
struct InMemory {
    inner: Arc<Mutex<Inner>>,
}

impl RoutineStorePort for InMemory {
    fn define(&mut self, def: &RoutineDefinition) -> Result<(), BackgroundError> {
        def.revalidate()?;
        let mut inner = self.inner.lock().unwrap();
        match inner.defs.get(&def.id) {
            Some(existing) if existing == def => Ok(()),
            Some(_) => Err(BackgroundError::Conflict(def.id.to_string())),
            None => {
                inner.defs.insert(def.id.clone(), def.clone());
                Ok(())
            }
        }
    }
    fn load(&self, id: &RoutineId) -> Result<Option<RoutineDefinition>, BackgroundError> {
        Ok(self.inner.lock().unwrap().defs.get(id).cloned())
    }
    fn update(&mut self, def: &RoutineDefinition) -> Result<(), BackgroundError> {
        def.revalidate()?;
        let mut inner = self.inner.lock().unwrap();
        if !inner.defs.contains_key(&def.id) {
            return Err(BackgroundError::NotFound(def.id.to_string()));
        }
        inner.defs.insert(def.id.clone(), def.clone());
        Ok(())
    }
    fn list(&self) -> Result<Vec<RoutineDefinition>, BackgroundError> {
        Ok(self.inner.lock().unwrap().defs.values().cloned().collect())
    }
    fn list_due(&self, now: u64) -> Result<Vec<RoutineDefinition>, BackgroundError> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .defs
            .values()
            .filter(|d| d.is_due(now))
            .cloned()
            .collect())
    }
    fn start_run(&mut self, run: &RoutineRun) -> Result<(), BackgroundError> {
        run.revalidate()?;
        let mut inner = self.inner.lock().unwrap();
        if !inner.defs.contains_key(&run.routine_id) {
            return Err(BackgroundError::NotFound(run.routine_id.to_string()));
        }
        inner.runs.insert(run.id.clone(), run.clone());
        Ok(())
    }
    fn load_run(&self, id: &RunId) -> Result<Option<RoutineRun>, BackgroundError> {
        Ok(self.inner.lock().unwrap().runs.get(id).cloned())
    }
    fn update_run(&mut self, run: &RoutineRun) -> Result<(), BackgroundError> {
        run.revalidate()?;
        let mut inner = self.inner.lock().unwrap();
        if !inner.runs.contains_key(&run.id) {
            return Err(BackgroundError::NotFound(run.id.to_string()));
        }
        inner.runs.insert(run.id.clone(), run.clone());
        Ok(())
    }
    fn list_runs(&self, routine_id: &RoutineId) -> Result<Vec<RoutineRun>, BackgroundError> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .runs
            .values()
            .filter(|r| &r.routine_id == routine_id)
            .cloned()
            .collect())
    }
    fn list_running(&self) -> Result<Vec<RoutineRun>, BackgroundError> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .runs
            .values()
            .filter(|r| r.status == RunStatus::Running)
            .cloned()
            .collect())
    }
}

#[test]
fn in_memory_store_satisfies_routine_contract() {
    let mut store = InMemory::default();
    let reopened = store.clone();
    run_routine_store_contract(&mut store, move || reopened.clone());
}
