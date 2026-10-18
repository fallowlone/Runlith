//! Durable routine seam: definitions and runs behind a port.
//! Lifecycle decisions stay pure (crate::model); the store never
//! invents authorization, eligibility or budgets.
use crate::model::{BackgroundError, RoutineDefinition, RoutineId, RoutineRun, RunId};

pub trait RoutineStorePort {
    /// Persists one definition. Conflict on differing payload under id.
    fn define(&mut self, def: &RoutineDefinition) -> Result<(), BackgroundError>;
    fn load(&self, id: &RoutineId) -> Result<Option<RoutineDefinition>, BackgroundError>;
    fn update(&mut self, def: &RoutineDefinition) -> Result<(), BackgroundError>;
    /// All definitions, sorted by id.
    fn list(&self) -> Result<Vec<RoutineDefinition>, BackgroundError>;
    /// Due definitions at `now` (domain predicate), sorted by id.
    fn list_due(&self, now: u64) -> Result<Vec<RoutineDefinition>, BackgroundError>;
    fn start_run(&mut self, run: &RoutineRun) -> Result<(), BackgroundError>;
    fn load_run(&self, id: &RunId) -> Result<Option<RoutineRun>, BackgroundError>;
    fn update_run(&mut self, run: &RoutineRun) -> Result<(), BackgroundError>;
    /// Runs of one routine, sorted by id.
    fn list_runs(&self, routine_id: &RoutineId) -> Result<Vec<RoutineRun>, BackgroundError>;
    /// Runs left RUNNING (restart reconciliation input), sorted by id.
    fn list_running(&self) -> Result<Vec<RoutineRun>, BackgroundError>;
}
