//! Trusted background routine composition (TASK-0192, ADR-0045).
//! Only AUTHORIZED routines tick; authorization by the author is
//! refused in the domain. Restart reconciliation interrupts (never
//! re-executes) runs left RUNNING by a dead daemon.
use std::sync::atomic::{AtomicU64, Ordering};

use background::mint::{mint_routine_id, mint_run_id};
use background::model::{
    BackgroundError, CapabilityScope, RoutineDefinition, RoutineId, RoutineRun,
};
use background::port::RoutineStorePort;

#[derive(Debug)]
pub struct DefineRequest {
    pub name: String,
    pub objective: String,
    pub interval_seconds: u64,
    pub capability_scope: CapabilityScope,
    pub budget_max_runs: u32,
    pub author_ref: String,
}

pub struct BackgroundService<S> {
    store: S,
    id_seq: AtomicU64,
    draining: bool,
}

impl<S: RoutineStorePort> BackgroundService<S> {
    pub fn new(store: S) -> Self {
        Self {
            store,
            id_seq: AtomicU64::new(0),
            draining: false,
        }
    }

    pub fn store(&self) -> &S {
        &self.store
    }

    pub fn store_mut(&mut self) -> &mut S {
        &mut self.store
    }

    pub fn define(
        &mut self,
        request: DefineRequest,
        now: u64,
    ) -> Result<RoutineDefinition, BackgroundError> {
        let seq = u128::from(self.id_seq.fetch_add(1, Ordering::SeqCst));
        let def = RoutineDefinition::define(
            mint_routine_id(now, seq),
            request.name,
            request.objective,
            request.interval_seconds,
            request.capability_scope,
            request.budget_max_runs,
            request.author_ref,
            now,
        )?;
        self.store.define(&def)?;
        Ok(def)
    }

    pub fn get(&self, id: &RoutineId) -> Result<RoutineDefinition, BackgroundError> {
        self.store
            .load(id)?
            .ok_or_else(|| BackgroundError::NotFound(id.to_string()))
    }

    pub fn authorize(
        &mut self,
        id: &RoutineId,
        authorizer: &str,
        now: u64,
    ) -> Result<RoutineDefinition, BackgroundError> {
        let mut def = self.get(id)?;
        def.authorize(authorizer, now)?;
        self.store.update(&def)?;
        Ok(def)
    }

    pub fn disable(&mut self, id: &RoutineId) -> Result<RoutineDefinition, BackgroundError> {
        let mut def = self.get(id)?;
        def.disable()?;
        self.store.update(&def)?;
        Ok(def)
    }

    pub fn list(&self) -> Result<Vec<RoutineDefinition>, BackgroundError> {
        self.store.list()
    }

    pub fn list_runs(&self, id: &RoutineId) -> Result<Vec<RoutineRun>, BackgroundError> {
        self.store.list_runs(id)
    }

    /// Marks the daemon draining: ticks refuse new runs (B-8).
    pub fn begin_drain(&mut self) {
        self.draining = true;
    }

    pub fn is_draining(&self) -> bool {
        self.draining
    }

    /// Restart reconciliation: RUNNING runs become INTERRUPTED (B-9).
    pub fn reconcile(&mut self, now: u64) -> Result<Vec<RoutineRun>, BackgroundError> {
        let mut out = Vec::new();
        for mut run in self.store.list_running()? {
            run.interrupt(now)?;
            self.store.update_run(&run)?;
            out.push(run);
        }
        Ok(out)
    }

    /// One background tick: each due routine runs at most once (B-7).
    /// A run completes with `evidence` as its evidence reference.
    pub fn tick(&mut self, now: u64, evidence: &str) -> Result<Vec<RoutineRun>, BackgroundError> {
        if self.draining {
            return Err(BackgroundError::Draining("daemon is draining".to_owned()));
        }
        let mut out = Vec::new();
        for mut def in self.store.list_due(now)? {
            let seq = u128::from(self.id_seq.fetch_add(1, Ordering::SeqCst));
            let mut run = RoutineRun::start(mint_run_id(now, seq), def.id.clone(), now)?;
            self.store.start_run(&run)?;
            run.complete(evidence, now)?;
            self.store.update_run(&run)?;
            def.record_completion(now)?;
            self.store.update(&def)?;
            out.push(run);
        }
        Ok(out)
    }

    /// Runs one routine immediately when it is authorized (run-now).
    pub fn run_now(
        &mut self,
        id: &RoutineId,
        now: u64,
        evidence: &str,
    ) -> Result<RoutineRun, BackgroundError> {
        if self.draining {
            return Err(BackgroundError::Draining("daemon is draining".to_owned()));
        }
        let mut def = self.get(id)?;
        if def.state != background::model::RoutineState::Authorized {
            return Err(BackgroundError::Unauthorized(format!(
                "routine {} is {}",
                def.id,
                def.state.as_str()
            )));
        }
        if def.runs_completed >= def.budget_max_runs {
            return Err(BackgroundError::BudgetExhausted(def.id.to_string()));
        }
        let seq = u128::from(self.id_seq.fetch_add(1, Ordering::SeqCst));
        let mut run = RoutineRun::start(mint_run_id(now, seq), def.id.clone(), now)?;
        self.store.start_run(&run)?;
        run.complete(evidence, now)?;
        self.store.update_run(&run)?;
        def.record_completion(now)?;
        self.store.update(&def)?;
        Ok(run)
    }
}
