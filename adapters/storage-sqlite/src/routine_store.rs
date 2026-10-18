//! SQLite routine seam (TASK-0191, ADR-0045).
//! Lifecycle decisions stay in the pure `background` domain; this
//! adapter only encodes, persists and decodes through validated
//! constructors, so stored bytes can never bypass domain validation.
use background::model::{
    BackgroundError, CapabilityScope, RoutineDefinition, RoutineId, RoutineRun, RoutineState,
    RunId, RunStatus,
};
use background::port::RoutineStorePort;
use rusqlite::{params, Connection, OptionalExtension};

use crate::task_store::SqliteStore;

fn backend(error: rusqlite::Error) -> BackgroundError {
    BackgroundError::BackendFailure {
        detail: error.to_string(),
    }
}

fn stored<T>(parsed: Result<T, BackgroundError>, what: &str) -> Result<T, BackgroundError> {
    parsed.map_err(|err| BackgroundError::BackendFailure {
        detail: format!("stored {what} is malformed: {err}"),
    })
}

fn u64_to_i64(value: u64, field: &str) -> Result<i64, BackgroundError> {
    i64::try_from(value)
        .map_err(|_| BackgroundError::InvalidInput(format!("{field} overflows i64")))
}

fn i64_to_u64(value: i64, field: &str) -> Result<u64, BackgroundError> {
    u64::try_from(value).map_err(|_| BackgroundError::BackendFailure {
        detail: format!("stored {field} is negative"),
    })
}

const DEF_COLUMNS: &str = "routine_id, name, objective, interval_seconds, capability_scope, \
    budget_max_runs, author_ref, authorized_by, state, created_at, next_run_at, runs_completed";
const RUN_COLUMNS: &str = "run_id, routine_id, status, started_at, finished_at, evidence_ref";

type DefRow = (
    String,
    String,
    String,
    i64,
    String,
    i64,
    String,
    Option<String>,
    String,
    i64,
    i64,
    i64,
);
type RunRow = (String, String, String, i64, Option<i64>, Option<String>);

fn decode_def(row: &rusqlite::Row<'_>) -> rusqlite::Result<DefRow> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
        row.get(10)?,
        row.get(11)?,
    ))
}

fn def_from(t: DefRow) -> Result<RoutineDefinition, BackgroundError> {
    let def = RoutineDefinition {
        id: stored(RoutineId::new(t.0), "routine id")?,
        name: t.1,
        objective: t.2,
        interval_seconds: i64_to_u64(t.3, "interval_seconds")?,
        capability_scope: stored(CapabilityScope::parse(&t.4), "capability scope")?,
        budget_max_runs: stored(
            u32::try_from(t.5)
                .map_err(|_| BackgroundError::InvalidInput("budget out of range".into())),
            "budget",
        )?,
        author_ref: t.6,
        authorized_by: t.7,
        state: stored(RoutineState::parse(&t.8), "routine state")?,
        created_at: i64_to_u64(t.9, "created_at")?,
        next_run_at: i64_to_u64(t.10, "next_run_at")?,
        runs_completed: stored(
            u32::try_from(t.11)
                .map_err(|_| BackgroundError::InvalidInput("runs out of range".into())),
            "runs_completed",
        )?,
    };
    def.revalidate()?;
    Ok(def)
}

fn decode_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<RunRow> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
    ))
}

fn run_from(t: RunRow) -> Result<RoutineRun, BackgroundError> {
    let run = RoutineRun {
        id: stored(RunId::new(t.0), "run id")?,
        routine_id: stored(RoutineId::new(t.1), "routine id")?,
        status: stored(RunStatus::parse(&t.2), "run status")?,
        started_at: i64_to_u64(t.3, "started_at")?,
        finished_at: t.4.map(|v| i64_to_u64(v, "finished_at")).transpose()?,
        evidence_ref: t.5,
    };
    run.revalidate()?;
    Ok(run)
}

fn load_def(
    conn: &Connection,
    id: &RoutineId,
) -> Result<Option<RoutineDefinition>, BackgroundError> {
    let row = conn
        .query_row(
            &format!("SELECT {DEF_COLUMNS} FROM routines WHERE routine_id = ?1"),
            params![id.as_str()],
            decode_def,
        )
        .optional()
        .map_err(backend)?;
    row.map(def_from).transpose()
}

fn query_defs(
    conn: &Connection,
    sql: &str,
    p: &[&dyn rusqlite::ToSql],
) -> Result<Vec<RoutineDefinition>, BackgroundError> {
    let mut stmt = conn.prepare(sql).map_err(backend)?;
    let rows = stmt.query_map(p, decode_def).map_err(backend)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(def_from(r.map_err(backend)?)?);
    }
    Ok(out)
}

fn query_runs(
    conn: &Connection,
    sql: &str,
    p: &[&dyn rusqlite::ToSql],
) -> Result<Vec<RoutineRun>, BackgroundError> {
    let mut stmt = conn.prepare(sql).map_err(backend)?;
    let rows = stmt.query_map(p, decode_run).map_err(backend)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(run_from(r.map_err(backend)?)?);
    }
    Ok(out)
}

impl RoutineStorePort for SqliteStore {
    fn define(&mut self, def: &RoutineDefinition) -> Result<(), BackgroundError> {
        def.revalidate()?;
        if let Some(existing) = load_def(self.connection(), &def.id)? {
            if existing == *def {
                return Ok(());
            }
            return Err(BackgroundError::Conflict(def.id.to_string()));
        }
        self.connection()
            .execute(
                &format!("INSERT INTO routines ({DEF_COLUMNS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)"),
                params![
                    def.id.as_str(), def.name, def.objective,
                    u64_to_i64(def.interval_seconds, "interval")?,
                    def.capability_scope.as_str(),
                    i64::from(def.budget_max_runs), def.author_ref, def.authorized_by,
                    def.state.as_str(),
                    u64_to_i64(def.created_at, "created_at")?,
                    u64_to_i64(def.next_run_at, "next_run_at")?,
                    i64::from(def.runs_completed),
                ],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn load(&self, id: &RoutineId) -> Result<Option<RoutineDefinition>, BackgroundError> {
        load_def(self.connection(), id)
    }

    fn update(&mut self, def: &RoutineDefinition) -> Result<(), BackgroundError> {
        def.revalidate()?;
        let n = self.connection()
            .execute(
                "UPDATE routines SET name=?2, objective=?3, interval_seconds=?4, capability_scope=?5, budget_max_runs=?6, author_ref=?7, authorized_by=?8, state=?9, created_at=?10, next_run_at=?11, runs_completed=?12 WHERE routine_id=?1",
                params![
                    def.id.as_str(), def.name, def.objective,
                    u64_to_i64(def.interval_seconds, "interval")?,
                    def.capability_scope.as_str(),
                    i64::from(def.budget_max_runs), def.author_ref, def.authorized_by,
                    def.state.as_str(),
                    u64_to_i64(def.created_at, "created_at")?,
                    u64_to_i64(def.next_run_at, "next_run_at")?,
                    i64::from(def.runs_completed),
                ],
            )
            .map_err(backend)?;
        if n == 0 {
            return Err(BackgroundError::NotFound(def.id.to_string()));
        }
        Ok(())
    }

    fn list(&self) -> Result<Vec<RoutineDefinition>, BackgroundError> {
        query_defs(
            self.connection(),
            &format!("SELECT {DEF_COLUMNS} FROM routines ORDER BY routine_id"),
            &[],
        )
    }

    fn list_due(&self, now: u64) -> Result<Vec<RoutineDefinition>, BackgroundError> {
        let all = self.list()?;
        Ok(all.into_iter().filter(|d| d.is_due(now)).collect())
    }

    fn start_run(&mut self, run: &RoutineRun) -> Result<(), BackgroundError> {
        run.revalidate()?;
        if load_def(self.connection(), &run.routine_id)?.is_none() {
            return Err(BackgroundError::NotFound(run.routine_id.to_string()));
        }
        self.connection()
            .execute(
                &format!("INSERT INTO routine_runs ({RUN_COLUMNS}) VALUES (?1,?2,?3,?4,?5,?6)"),
                params![
                    run.id.as_str(),
                    run.routine_id.as_str(),
                    run.status.as_str(),
                    u64_to_i64(run.started_at, "started_at")?,
                    run.finished_at
                        .map(|v| u64_to_i64(v, "finished_at"))
                        .transpose()?,
                    run.evidence_ref,
                ],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn load_run(&self, id: &RunId) -> Result<Option<RoutineRun>, BackgroundError> {
        let row = self
            .connection()
            .query_row(
                &format!("SELECT {RUN_COLUMNS} FROM routine_runs WHERE run_id = ?1"),
                params![id.as_str()],
                decode_run,
            )
            .optional()
            .map_err(backend)?;
        row.map(run_from).transpose()
    }

    fn update_run(&mut self, run: &RoutineRun) -> Result<(), BackgroundError> {
        run.revalidate()?;
        let n = self.connection()
            .execute(
                "UPDATE routine_runs SET routine_id=?2, status=?3, started_at=?4, finished_at=?5, evidence_ref=?6 WHERE run_id=?1",
                params![
                    run.id.as_str(), run.routine_id.as_str(), run.status.as_str(),
                    u64_to_i64(run.started_at, "started_at")?,
                    run.finished_at.map(|v| u64_to_i64(v, "finished_at")).transpose()?,
                    run.evidence_ref,
                ],
            )
            .map_err(backend)?;
        if n == 0 {
            return Err(BackgroundError::NotFound(run.id.to_string()));
        }
        Ok(())
    }

    fn list_runs(&self, routine_id: &RoutineId) -> Result<Vec<RoutineRun>, BackgroundError> {
        query_runs(
            self.connection(),
            &format!(
                "SELECT {RUN_COLUMNS} FROM routine_runs WHERE routine_id = ?1 ORDER BY run_id"
            ),
            &[&routine_id.as_str()],
        )
    }

    fn list_running(&self) -> Result<Vec<RoutineRun>, BackgroundError> {
        query_runs(
            self.connection(),
            &format!(
                "SELECT {RUN_COLUMNS} FROM routine_runs WHERE status = 'RUNNING' ORDER BY run_id"
            ),
            &[],
        )
    }
}
