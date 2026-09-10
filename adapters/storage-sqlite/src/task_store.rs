//! Durable storage for projects, tasks, dependency edges, and the Event
//! Journal.
//!
//! Write discipline: only validated domain values are persisted. Lifecycle
//! state changes originate from `task-domain` operations; the single
//! lifecycle commit path is [`SqliteStore::apply_lifecycle_transition`],
//! which persists the resulting state and appends the journal record in one
//! SQLite transaction — there is no untyped `UPDATE tasks SET state = ...`
//! path and no journal append path outside that transaction. Multi-row
//! writes elsewhere run in one SQLite transaction for storage consistency
//! (not event atomicity).

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use task_domain::{validate_acyclic, Project, ProjectId, Task, TaskId, TaskSnapshot, TaskState};

use agent_runtime::{AttemptId, ExecutionId};

use workspace::{LeaseLifecycle, WorkspaceId};

use crate::error::{map_constraint, StorageError, StorageResult};
use crate::event::{
    decode_state, AttemptOutcome, EventId, EventType, JournalEvent, NewJournalEvent,
    EVENT_SCHEMA_VERSION,
};
use crate::migration::{migrate, schema_version, verify_foreign_keys};

/// SQLite-backed durable store. Owns one connection to an explicit file;
/// tests use test-owned temporary files, never a user database.
pub struct SqliteStore {
    connection: Connection,
}

impl SqliteStore {
    /// Opens (creating if absent) the database at `path`, enables and
    /// verifies foreign keys, and applies pending migrations.
    pub fn open(path: impl AsRef<Path>) -> StorageResult<Self> {
        let mut connection = Connection::open(path.as_ref())?;
        connection.execute_batch("PRAGMA foreign_keys = ON")?;
        // Bounded lock patience (TASK-0047, G5): the daemon serves
        // concurrent connections (control dispatch, background provider
        // runs, status reads) on separate connections over one file.
        // Every transaction in this schema is short-lived by
        // construction, so waiting up to 30 s for a held lock serializes
        // instead of failing spuriously — and still fails closed past
        // the bound instead of blocking forever.
        // Write transactions across this adapter MUST use
        // TransactionBehavior::Immediate (never bare `transaction()`,
        // which is DEFERRED): concurrent daemon writers with DEFERRED
        // deadlock on lock upgrade (both hold RESERVED, both want
        // EXCLUSIVE) and SQLite returns BUSY immediately to break the
        // tie, bypassing the busy timeout (TASK-0047). IMMEDIATE
        // serializes contenders at BEGIN, where the busy wait applies.
        connection.busy_timeout(std::time::Duration::from_secs(30))?;
        verify_foreign_keys(&connection)?;
        migrate(&mut connection)?;
        Ok(Self { connection })
    }

    /// Current schema version of the open database.
    pub fn schema_version(&self) -> StorageResult<u32> {
        schema_version(&self.connection)
    }

    /// Borrowed connection for the sibling runtime module: SQL and
    /// `rusqlite` types still never leave the adapter crate.
    pub(crate) fn connection(&self) -> &Connection {
        &self.connection
    }

    /// Mutable borrow for the sibling runtime module's atomic commits.
    pub(crate) fn connection_mut(&mut self) -> &mut Connection {
        &mut self.connection
    }

    /// Reports whether foreign-key enforcement is active (always true after
    /// [`SqliteStore::open`]; exposed so tests prove it rather than assume).
    pub fn foreign_keys_enabled(&self) -> StorageResult<bool> {
        let enabled: u32 = self
            .connection
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))?;
        Ok(enabled == 1)
    }

    // -- projects -------------------------------------------------------

    /// Persists a validated project. Existing ids fail as duplicates.
    pub fn create_project(&mut self, project: &Project) -> StorageResult<()> {
        if self.get_project(project.id())?.is_some() {
            return Err(StorageError::Duplicate(format!(
                "project {} already exists",
                project.id()
            )));
        }
        self.connection
            .execute(
                "INSERT INTO projects (id, name) VALUES (?1, ?2)",
                params![project.id().as_str(), project.name()],
            )
            .map_err(|error| map_constraint(error, "create project"))?;
        Ok(())
    }

    /// Persists a validated project together with its authoritative
    /// `project.created` journal evidence in one transaction. The trusted
    /// application service builds the event; this method commits it
    /// atomically or not at all.
    pub fn create_project_with_event(
        &mut self,
        project: &Project,
        event: &NewJournalEvent,
    ) -> StorageResult<JournalEvent> {
        require_project_created_envelope(project, event)?;
        if self.get_project(project.id())?.is_some() {
            return Err(StorageError::Duplicate(format!(
                "project {} already exists",
                project.id()
            )));
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction
            .execute(
                "INSERT INTO projects (id, name) VALUES (?1, ?2)",
                params![project.id().as_str(), project.name()],
            )
            .map_err(|error| map_constraint(error, "create project"))?;
        let committed = insert_journal_event(&transaction, event)?;
        transaction.commit()?;
        Ok(committed)
    }

    /// Loads a project by id, or `None` when absent.
    pub fn get_project(&self, id: &ProjectId) -> StorageResult<Option<Project>> {
        let row: Option<(String, String)> = self
            .connection
            .query_row(
                "SELECT id, name FROM projects WHERE id = ?1",
                params![id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        row.map(|(id, name)| {
            let id = ProjectId::new(id).map_err(|error| {
                StorageError::DomainReconstruction(format!("stored project id: {error}"))
            })?;
            Project::new(id, name).map_err(|error| {
                StorageError::DomainReconstruction(format!("stored project: {error}"))
            })
        })
        .transpose()
    }

    /// Lists all projects in id order.
    pub fn list_projects(&self) -> StorageResult<Vec<Project>> {
        let mut statement = self
            .connection
            .prepare("SELECT id, name FROM projects ORDER BY id")?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        let mut projects = Vec::new();
        for row in rows {
            let (id, name): (String, String) = row?;
            let id = ProjectId::new(id).map_err(|error| {
                StorageError::DomainReconstruction(format!("stored project id: {error}"))
            })?;
            projects.push(Project::new(id, name).map_err(|error| {
                StorageError::DomainReconstruction(format!("stored project: {error}"))
            })?);
        }
        Ok(projects)
    }

    // -- tasks ----------------------------------------------------------

    /// Persists a new validated task with its metadata and dependency
    /// edges. The owning project and every referenced task must already be
    /// stored; cross-project edges fail before any write.
    pub fn create_task(&mut self, task: &Task) -> StorageResult<()> {
        if self.get_task(task.id())?.is_some() {
            return Err(StorageError::Duplicate(format!(
                "task {} already exists",
                task.id()
            )));
        }
        self.require_project(task.project_id())?;
        self.require_dependency_targets(task)?;
        let snapshot = task.snapshot();
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        insert_task_row(&transaction, &snapshot)?;
        insert_task_strings(&transaction, &snapshot)?;
        insert_task_edges(&transaction, &snapshot)?;
        transaction.commit()?;
        Ok(())
    }

    /// Persists a new validated task together with its authoritative
    /// `task.created` journal evidence in one transaction: task row,
    /// metadata, edges, and journal record commit or roll back together.
    pub fn create_task_with_event(
        &mut self,
        task: &Task,
        event: &NewJournalEvent,
    ) -> StorageResult<JournalEvent> {
        require_task_created_envelope(task, event)?;
        if self.get_task(task.id())?.is_some() {
            return Err(StorageError::Duplicate(format!(
                "task {} already exists",
                task.id()
            )));
        }
        self.require_project(task.project_id())?;
        self.require_dependency_targets(task)?;
        let snapshot = task.snapshot();
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        insert_task_row(&transaction, &snapshot)?;
        insert_task_strings(&transaction, &snapshot)?;
        insert_task_edges(&transaction, &snapshot)?;
        let committed = insert_journal_event(&transaction, event)?;
        transaction.commit()?;
        Ok(committed)
    }

    /// Loads a task by id with metadata and edges, or `None` when absent.
    /// Stored content that cannot form a valid domain object fails as
    /// corruption/reconstruction error — never silently repaired.
    pub fn get_task(&self, id: &TaskId) -> StorageResult<Option<Task>> {
        Ok(match self.load_snapshot(id.as_str())? {
            None => None,
            Some(snapshot) => Some(Task::restore(snapshot).map_err(|error| {
                StorageError::DomainReconstruction(format!("stored task {id}: {error}"))
            })?),
        })
    }

    /// Lists every stored task of one project, fully hydrated, in id order.
    pub fn list_tasks_for_project(&self, project_id: &ProjectId) -> StorageResult<Vec<Task>> {
        let mut statement = self
            .connection
            .prepare("SELECT id FROM tasks WHERE project_id = ?1 ORDER BY id")?;
        let ids: Vec<String> = statement
            .query_map(params![project_id.as_str()], |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        let mut tasks = Vec::with_capacity(ids.len());
        for id in &ids {
            let snapshot = self.load_snapshot(id)?.ok_or_else(|| {
                StorageError::Corrupt(format!("task row {id} vanished mid-listing"))
            })?;
            tasks.push(Task::restore(snapshot).map_err(|error| {
                StorageError::DomainReconstruction(format!("stored task {id}: {error}"))
            })?);
        }
        Ok(tasks)
    }

    /// Persists a validated task, inserting or replacing its stored image
    /// (row, metadata, edges) atomically. The task object stays the source
    /// of truth; cross-project or unknown-reference content fails first.
    pub fn save_task(&mut self, task: &Task) -> StorageResult<()> {
        self.require_project(task.project_id())?;
        self.require_dependency_targets(task)?;
        let snapshot = task.snapshot();
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        // Replace the full image so the stored task always mirrors the
        // validated object (edges included); FK order: row first, then
        // children. INSERT OR REPLACE keeps the primary key stable.
        insert_task_row_replace(&transaction, &snapshot)?;
        transaction.execute(
            "DELETE FROM task_strings WHERE task_id = ?1",
            params![snapshot.id.as_str()],
        )?;
        transaction.execute(
            "DELETE FROM task_dependencies WHERE task_id = ?1",
            params![snapshot.id.as_str()],
        )?;
        insert_task_strings(&transaction, &snapshot)?;
        insert_task_edges(&transaction, &snapshot)?;
        transaction.commit()?;
        Ok(())
    }

    // -- dependencies ---------------------------------------------------

    /// Adds `task_id depends on depends_on_id` after domain validation:
    /// both tasks must exist, same project, no self-edge, no duplicate, no
    /// cycle. Nothing is written when validation fails.
    pub fn add_dependency(
        &mut self,
        task_id: &TaskId,
        depends_on_id: &TaskId,
    ) -> StorageResult<()> {
        let (task, target) = self.validated_dependency_pair(task_id, depends_on_id)?;
        // Cycle check on the in-memory project graph before touching SQL.
        let mut graph = self.load_project_graph(task.project_id())?;
        let position = graph
            .iter()
            .position(|candidate| candidate.id() == task_id)
            .ok_or_else(|| {
                StorageError::Corrupt(format!("task {task_id} missing from its project graph"))
            })?;
        let mut candidate = graph[position].clone();
        candidate
            .add_dependency(&target)
            .map_err(|error| StorageError::InvalidGraph(error.to_string()))?;
        graph[position] = candidate;
        validate_acyclic(&graph).map_err(|error| StorageError::InvalidGraph(error.to_string()))?;
        self.connection
            .execute(
                "INSERT INTO task_dependencies (task_id, depends_on_id) VALUES (?1, ?2)",
                params![task_id.as_str(), depends_on_id.as_str()],
            )
            .map_err(|error| map_constraint(error, "add dependency"))?;
        Ok(())
    }

    /// Adds `task_id depends on depends_on_id` together with its
    /// authoritative `task.dependency_added` journal evidence in one
    /// transaction. Validation is identical to [`SqliteStore::add_dependency`];
    /// the edge and the event commit or roll back together.
    pub fn add_dependency_with_event(
        &mut self,
        task_id: &TaskId,
        depends_on_id: &TaskId,
        event: &NewJournalEvent,
    ) -> StorageResult<JournalEvent> {
        let (task, target) = self.validated_dependency_pair(task_id, depends_on_id)?;
        require_dependency_envelope(&task, event)?;
        // Cycle check on the in-memory project graph before touching SQL.
        let mut graph = self.load_project_graph(task.project_id())?;
        let position = graph
            .iter()
            .position(|candidate| candidate.id() == task_id)
            .ok_or_else(|| {
                StorageError::Corrupt(format!("task {task_id} missing from its project graph"))
            })?;
        let mut candidate = graph[position].clone();
        candidate
            .add_dependency(&target)
            .map_err(|error| StorageError::InvalidGraph(error.to_string()))?;
        graph[position] = candidate;
        validate_acyclic(&graph).map_err(|error| StorageError::InvalidGraph(error.to_string()))?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction
            .execute(
                "INSERT INTO task_dependencies (task_id, depends_on_id) VALUES (?1, ?2)",
                params![task_id.as_str(), depends_on_id.as_str()],
            )
            .map_err(|error| map_constraint(error, "add dependency"))?;
        let committed = insert_journal_event(&transaction, event)?;
        transaction.commit()?;
        Ok(committed)
    }

    /// Shared domain validation for both dependency write paths: both
    /// tasks must exist, share a project, and not already be linked. No
    /// self-edge, no duplicate, no cross-project edge.
    fn validated_dependency_pair(
        &self,
        task_id: &TaskId,
        depends_on_id: &TaskId,
    ) -> StorageResult<(Task, Task)> {
        if task_id == depends_on_id {
            return Err(StorageError::InvalidGraph(format!(
                "task {task_id} cannot depend on itself"
            )));
        }
        let task = self
            .get_task(task_id)?
            .ok_or_else(|| StorageError::NotFound(format!("task {task_id} does not exist")))?;
        let target = self.get_task(depends_on_id)?.ok_or_else(|| {
            StorageError::NotFound(format!("task {depends_on_id} does not exist"))
        })?;
        if task.project_id() != target.project_id() {
            return Err(StorageError::InvalidGraph(format!(
                "task {task_id} (project {}) cannot depend on task {depends_on_id} (project {})",
                task.project_id(),
                target.project_id()
            )));
        }
        if task.depends_on().contains(depends_on_id) {
            return Err(StorageError::InvalidGraph(format!(
                "task {task_id} already depends on {depends_on_id}"
            )));
        }
        Ok((task, target))
    }

    /// Removes one edge; absent edges are a no-op (idempotent).
    pub fn remove_dependency(
        &mut self,
        task_id: &TaskId,
        depends_on_id: &TaskId,
    ) -> StorageResult<()> {
        self.connection.execute(
            "DELETE FROM task_dependencies WHERE task_id = ?1 AND depends_on_id = ?2",
            params![task_id.as_str(), depends_on_id.as_str()],
        )?;
        Ok(())
    }

    /// Lists the direct dependencies of one task in id order.
    pub fn list_dependencies(&self, task_id: &TaskId) -> StorageResult<Vec<TaskId>> {
        let mut statement = self.connection.prepare(
            "SELECT depends_on_id FROM task_dependencies WHERE task_id = ?1 ORDER BY depends_on_id",
        )?;
        let ids: Vec<String> = statement
            .query_map(params![task_id.as_str()], |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        ids.iter()
            .map(|id| {
                TaskId::new(id).map_err(|error| {
                    StorageError::DomainReconstruction(format!("stored dependency target: {error}"))
                })
            })
            .collect()
    }

    /// Loads every stored task of one project with edges — the unit the
    /// domain graph validator reasons about.
    pub fn load_project_graph(&self, project_id: &ProjectId) -> StorageResult<Vec<Task>> {
        self.list_tasks_for_project(project_id)
    }

    // -- event journal --------------------------------------------------

    /// Commits one validated lifecycle mutation: guarded current-state
    /// update plus journal append in a single SQLite transaction.
    ///
    /// The state write is guarded by `expected`: the row updates only when
    /// it still holds the state the domain operation was validated
    /// against. A concurrent writer that moved first turns this into a
    /// [`StorageError::Stale`] rollback instead of a silent overwrite — no
    /// revision column needed, the lifecycle state itself is the guard.
    /// Either both rows commit or neither does; callers cannot append an
    /// event without the state change or vice versa.
    pub fn apply_lifecycle_transition(
        &mut self,
        task_id: &TaskId,
        expected: TaskState,
        next: &Task,
        event: &NewJournalEvent,
    ) -> StorageResult<JournalEvent> {
        if next.id() != task_id {
            return Err(StorageError::ConstraintViolation(format!(
                "lifecycle write targets task {task_id} but carries task {}",
                next.id()
            )));
        }
        if event.event_type.is_project_scoped() {
            return Err(StorageError::ConstraintViolation(
                "lifecycle transition cannot carry a project-scoped event".to_owned(),
            ));
        }
        if event.event_type.is_runtime_scoped() {
            return Err(StorageError::ConstraintViolation(
                "lifecycle transition cannot carry a runtime event".to_owned(),
            ));
        }
        if event.task_id.as_ref() != Some(task_id) || event.project_id != *next.project_id() {
            return Err(StorageError::ConstraintViolation(
                "lifecycle event references a different task or project than the state write"
                    .to_owned(),
            ));
        }
        if event.from_state != Some(expected) || event.to_state != Some(next.state()) {
            return Err(StorageError::ConstraintViolation(
                "lifecycle event states do not match the guarded state write".to_owned(),
            ));
        }
        require_envelope(event)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let updated = transaction.execute(
            "UPDATE tasks SET state = ?1 WHERE id = ?2 AND state = ?3",
            params![next.state().as_str(), task_id.as_str(), expected.as_str()],
        )?;
        if updated == 0 {
            let current: Option<String> = transaction
                .query_row(
                    "SELECT state FROM tasks WHERE id = ?1",
                    params![task_id.as_str()],
                    |row| row.get(0),
                )
                .optional()?;
            return Err(match current {
                None => StorageError::NotFound(format!("task {task_id} does not exist")),
                Some(found) => StorageError::Stale {
                    task: task_id.to_string(),
                    expected: expected.as_str().to_owned(),
                    found,
                },
            });
        }
        let committed = insert_journal_event(&transaction, event)?;
        transaction.commit()?;
        Ok(committed)
    }

    /// Reads one journal record by identity, or `None` when absent.
    pub fn get_event(&self, id: &EventId) -> StorageResult<Option<JournalEvent>> {
        let row: Option<JournalRow> = self
            .connection
            .query_row(
                "SELECT seq, event_type, event_version, task_id, project_id,
                        from_state, to_state, reason, correlation_id,
                        causation_id, occurred_at, provenance,
                        execution_id, attempt_id, attempt_outcome,
                        workspace_id
                 FROM event_journal WHERE seq = ?1",
                params![id.seq()],
                JournalRow::read,
            )
            .optional()?;
        row.map(decode_journal_row).transpose()
    }

    /// Lists a task's journal records in commit order.
    pub fn list_events_for_task(&self, task_id: &TaskId) -> StorageResult<Vec<JournalEvent>> {
        self.list_events(
            "SELECT seq, event_type, event_version, task_id, project_id,
                    from_state, to_state, reason, correlation_id,
                    causation_id, occurred_at, provenance,
                    execution_id, attempt_id, attempt_outcome,
                    workspace_id
             FROM event_journal WHERE task_id = ?1 ORDER BY seq",
            params![task_id.as_str()],
        )
    }

    /// Lists a project's journal records in commit order.
    pub fn list_events_for_project(
        &self,
        project_id: &ProjectId,
    ) -> StorageResult<Vec<JournalEvent>> {
        self.list_events(
            "SELECT seq, event_type, event_version, task_id, project_id,
                    from_state, to_state, reason, correlation_id,
                    causation_id, occurred_at, provenance,
                    execution_id, attempt_id, attempt_outcome,
                    workspace_id
             FROM event_journal WHERE project_id = ?1 ORDER BY seq",
            params![project_id.as_str()],
        )
    }

    /// Traverses the whole journal in commit order (oldest first).
    pub fn list_events_ordered(&self) -> StorageResult<Vec<JournalEvent>> {
        self.list_events(
            "SELECT seq, event_type, event_version, task_id, project_id,
                    from_state, to_state, reason, correlation_id,
                    causation_id, occurred_at, provenance,
                    execution_id, attempt_id, attempt_outcome,
                    workspace_id
             FROM event_journal ORDER BY seq",
            [],
        )
    }

    fn list_events(
        &self,
        sql: &str,
        params: impl rusqlite::Params,
    ) -> StorageResult<Vec<JournalEvent>> {
        let mut statement = self.connection.prepare(sql)?;
        let rows = statement.query_map(params, JournalRow::read)?;
        let mut events = Vec::new();
        for row in rows {
            events.push(decode_journal_row(row?)?);
        }
        Ok(events)
    }

    // -- internals ------------------------------------------------------

    fn require_project(&self, project_id: &ProjectId) -> StorageResult<()> {
        if self.get_project(project_id)?.is_none() {
            return Err(StorageError::ConstraintViolation(format!(
                "project {project_id} does not exist"
            )));
        }
        Ok(())
    }

    /// Every dependency target must already be stored and live in the same
    /// project; the domain never accepts a graph this adapter would persist.
    fn require_dependency_targets(&self, task: &Task) -> StorageResult<()> {
        for dep in task.depends_on() {
            let target = self.get_task(dep)?.ok_or_else(|| {
                StorageError::ConstraintViolation(format!(
                    "task {} depends on unknown task {dep}",
                    task.id()
                ))
            })?;
            if target.project_id() != task.project_id() {
                return Err(StorageError::InvalidGraph(format!(
                    "task {} (project {}) cannot depend on task {dep} (project {})",
                    task.id(),
                    task.project_id(),
                    target.project_id()
                )));
            }
        }
        Ok(())
    }

    fn load_snapshot(&self, id: &str) -> StorageResult<Option<TaskSnapshot>> {
        let row: Option<TaskRow> = self
            .connection
            .query_row(
                "SELECT id, project_id, objective, parent_id, state, kind, risk
                 FROM tasks WHERE id = ?1",
                params![id],
                |row| {
                    Ok(TaskRow {
                        id: row.get(0)?,
                        project_id: row.get(1)?,
                        objective: row.get(2)?,
                        parent_id: row.get(3)?,
                        state: row.get(4)?,
                        kind: row.get(5)?,
                        risk: row.get(6)?,
                    })
                },
            )
            .optional()?;
        let row = match row {
            None => return Ok(None),
            Some(row) => row,
        };
        let id = TaskId::new(row.id).map_err(|error| {
            StorageError::DomainReconstruction(format!("stored task id: {error}"))
        })?;
        let project_id = ProjectId::new(row.project_id).map_err(|error| {
            StorageError::DomainReconstruction(format!("stored project id: {error}"))
        })?;
        // A task in a project that has no row is corrupt data (foreign keys
        // stop this on write; this stops it on read when the database was
        // mutated behind the adapter's back).
        let project_exists: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM projects WHERE id = ?1)",
            params![project_id.as_str()],
            |row| row.get(0),
        )?;
        if !project_exists {
            return Err(StorageError::Corrupt(format!(
                "task {id} references missing project {project_id}"
            )));
        }
        let state = decode_state(&row.state)?;
        let parent = row
            .parent_id
            .map(|parent| {
                TaskId::new(parent).map_err(|error| {
                    StorageError::DomainReconstruction(format!("stored parent reference: {error}"))
                })
            })
            .transpose()?;
        let mut strings: std::collections::BTreeMap<&'static str, Vec<(i64, String)>> =
            std::collections::BTreeMap::new();
        {
            let mut statement = self
                .connection
                .prepare("SELECT slot, position, value FROM task_strings WHERE task_id = ?1")?;
            let rows = statement.query_map(params![id.as_str()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?;
            for row in rows {
                let (slot, position, value) = row?;
                match slot.as_str() {
                    "acceptance" | "allowed_path" | "forbidden_path" | "verification"
                    | "required_review" | "artifact" => {
                        strings
                            .entry(owned_slot(&slot))
                            .or_default()
                            .push((position, value));
                    }
                    other => {
                        return Err(StorageError::Corrupt(format!(
                            "task {id} stores unknown metadata slot {other:?}"
                        )));
                    }
                }
            }
        }
        let mut depends_on = Vec::new();
        {
            let mut statement = self.connection.prepare(
                "SELECT depends_on_id FROM task_dependencies WHERE task_id = ?1 ORDER BY depends_on_id",
            )?;
            let rows = statement.query_map(params![id.as_str()], |row| row.get::<_, String>(0))?;
            for row in rows {
                depends_on.push(TaskId::new(row?).map_err(|error| {
                    StorageError::DomainReconstruction(format!("stored dependency target: {error}"))
                })?);
            }
        }
        Ok(Some(TaskSnapshot {
            id,
            project_id,
            objective: row.objective,
            parent,
            depends_on,
            acceptance: ordered_values(&strings, "acceptance"),
            allowed_paths: ordered_values(&strings, "allowed_path"),
            forbidden_paths: ordered_values(&strings, "forbidden_path"),
            verification: ordered_values(&strings, "verification"),
            required_review: ordered_values(&strings, "required_review"),
            risk: row.risk,
            artifacts: ordered_values(&strings, "artifact"),
            kind: row.kind,
            state,
        }))
    }
}

struct TaskRow {
    id: String,
    project_id: String,
    objective: String,
    parent_id: Option<String>,
    state: String,
    kind: Option<String>,
    risk: Option<String>,
}

struct JournalRow {
    seq: i64,
    event_type: String,
    event_version: u32,
    task_id: Option<String>,
    project_id: String,
    from_state: Option<String>,
    to_state: Option<String>,
    reason: Option<String>,
    correlation_id: String,
    causation_id: String,
    occurred_at: String,
    provenance: String,
    execution_id: Option<String>,
    attempt_id: Option<String>,
    attempt_outcome: Option<String>,
    workspace_id: Option<String>,
}

impl JournalRow {
    fn read(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            seq: row.get(0)?,
            event_type: row.get(1)?,
            event_version: row.get(2)?,
            task_id: row.get(3)?,
            project_id: row.get(4)?,
            from_state: row.get(5)?,
            to_state: row.get(6)?,
            reason: row.get(7)?,
            correlation_id: row.get(8)?,
            causation_id: row.get(9)?,
            occurred_at: row.get(10)?,
            provenance: row.get(11)?,
            execution_id: row.get(12)?,
            attempt_id: row.get(13)?,
            attempt_outcome: row.get(14)?,
            workspace_id: row.get(15)?,
        })
    }
}

/// Typed decode of one journal row. Unknown types/versions are corruption;
/// ids and states go through the same strict validation as task rows.
/// Scope is re-checked on read: a task-scoped row with a NULL task or a
/// project-scoped row with a task reference is corruption, never a default.
fn decode_journal_row(row: JournalRow) -> StorageResult<JournalEvent> {
    if row.event_version != EVENT_SCHEMA_VERSION {
        return Err(StorageError::Corrupt(format!(
            "unsupported persisted event version {} for journal seq {}",
            row.event_version, row.seq
        )));
    }
    let event_type = EventType::parse(&row.event_type)?;
    let task_id = row
        .task_id
        .map(|id| {
            TaskId::new(id).map_err(|error| {
                StorageError::DomainReconstruction(format!("journal task reference: {error}"))
            })
        })
        .transpose()?;
    let project_id = ProjectId::new(row.project_id).map_err(|error| {
        StorageError::DomainReconstruction(format!("journal project reference: {error}"))
    })?;
    // The shared state columns carry exactly one vocabulary per row:
    // lease lifecycle names on workspace rows, Task names (or NULL) on
    // every other row. Each column is consumed once into its pair.
    let (from_state, from_workspace_state) = match (&event_type, row.from_state) {
        (event_type, Some(raw)) if event_type.is_workspace_scoped() => (
            None,
            Some(LeaseLifecycle::parse(&raw).map_err(|_| {
                StorageError::Corrupt(format!(
                    "journal seq {} stores unknown lease lifecycle {raw:?}",
                    row.seq
                ))
            })?),
        ),
        (_, raw) => (
            raw.map(|state| {
                decode_state(&state).map_err(|_| {
                    StorageError::Corrupt(format!(
                        "journal seq {} stores unknown from_state {state:?}",
                        row.seq
                    ))
                })
            })
            .transpose()?,
            None,
        ),
    };
    let (to_state, to_workspace_state) = match (&event_type, row.to_state) {
        (event_type, Some(raw)) if event_type.is_workspace_scoped() => (
            None,
            Some(LeaseLifecycle::parse(&raw).map_err(|_| {
                StorageError::Corrupt(format!(
                    "journal seq {} stores unknown lease lifecycle {raw:?}",
                    row.seq
                ))
            })?),
        ),
        (_, raw) => (
            raw.map(|state| {
                decode_state(&state).map_err(|_| {
                    StorageError::Corrupt(format!(
                        "journal seq {} stores unknown to_state {state:?}",
                        row.seq
                    ))
                })
            })
            .transpose()?,
            None,
        ),
    };
    let execution_id = row
        .execution_id
        .map(|id| {
            ExecutionId::new(id).map_err(|error| {
                StorageError::DomainReconstruction(format!("journal execution reference: {error}"))
            })
        })
        .transpose()?;
    let attempt_id = row
        .attempt_id
        .map(|id| {
            AttemptId::new(id).map_err(|error| {
                StorageError::DomainReconstruction(format!("journal attempt reference: {error}"))
            })
        })
        .transpose()?;
    let attempt_outcome = row
        .attempt_outcome
        .map(|raw| AttemptOutcome::parse(&raw))
        .transpose()?;
    let workspace_id = row
        .workspace_id
        .map(|id| {
            WorkspaceId::new(id).map_err(|error| {
                StorageError::DomainReconstruction(format!("journal workspace reference: {error}"))
            })
        })
        .transpose()?;
    if event_type.is_project_scoped() {
        if task_id.is_some()
            || from_state.is_some()
            || to_state.is_some()
            || execution_id.is_some()
            || attempt_id.is_some()
            || attempt_outcome.is_some()
            || workspace_id.is_some()
            || from_workspace_state.is_some()
            || to_workspace_state.is_some()
        {
            return Err(StorageError::Corrupt(format!(
                "journal seq {} stores a project-scoped event with task, runtime, or workspace scope",
                row.seq
            )));
        }
    } else if event_type.is_runtime_scoped() {
        decode_runtime_scope(
            row.seq,
            &event_type,
            &task_id,
            &from_state,
            &to_state,
            &execution_id,
            &attempt_id,
            &attempt_outcome,
        )?;
        if workspace_id.is_some() || from_workspace_state.is_some() || to_workspace_state.is_some()
        {
            return Err(StorageError::Corrupt(format!(
                "journal seq {} stores a runtime event with workspace scope",
                row.seq
            )));
        }
    } else if event_type.is_workspace_scoped() {
        decode_workspace_scope(
            row.seq,
            &event_type,
            &task_id,
            &execution_id,
            &workspace_id,
            &attempt_id,
            &attempt_outcome,
            &from_workspace_state,
            &to_workspace_state,
        )?;
        if from_state.is_some() || to_state.is_some() {
            return Err(StorageError::Corrupt(format!(
                "journal seq {} stores a workspace event with Task lifecycle states",
                row.seq
            )));
        }
    } else if task_id.is_none()
        || from_state.is_none()
        || to_state.is_none()
        || execution_id.is_some()
        || attempt_id.is_some()
        || attempt_outcome.is_some()
        || workspace_id.is_some()
        || from_workspace_state.is_some()
        || to_workspace_state.is_some()
    {
        return Err(StorageError::Corrupt(format!(
            "journal seq {} stores a task-scoped event without task scope or with runtime/workspace scope",
            row.seq
        )));
    }
    Ok(JournalEvent {
        id: EventId::parse(&format!("evt-{}", row.seq))?,
        seq: row.seq,
        event_version: row.event_version,
        fact: NewJournalEvent {
            event_type,
            task_id,
            project_id,
            from_state,
            to_state,
            reason: row.reason,
            correlation_id: row.correlation_id,
            causation_id: row.causation_id,
            occurred_at: row.occurred_at,
            provenance: row.provenance,
            execution_id,
            attempt_id,
            attempt_outcome,
            workspace_id,
            from_workspace_state,
            to_workspace_state,
        },
    })
}

/// Read-path mirror of the ADR-0022 scope matrix: each runtime family
/// carries exactly its scope. `from_state`/`to_state` stay Task-only,
/// so any Task state on a runtime row is corruption, not data.
#[allow(clippy::too_many_arguments)]
fn decode_runtime_scope(
    seq: i64,
    event_type: &EventType,
    task_id: &Option<TaskId>,
    from_state: &Option<TaskState>,
    to_state: &Option<TaskState>,
    execution_id: &Option<ExecutionId>,
    attempt_id: &Option<AttemptId>,
    attempt_outcome: &Option<AttemptOutcome>,
) -> StorageResult<()> {
    let corrupt =
        |detail: &str| StorageError::Corrupt(format!("journal seq {seq} stores {detail}"));
    if task_id.is_none() || from_state.is_some() || to_state.is_some() {
        return Err(corrupt(
            "a runtime event without task scope or with Task lifecycle states",
        ));
    }
    let valid = execution_id.is_some()
        && match event_type {
            EventType::ExecutionCreated => attempt_id.is_none() && attempt_outcome.is_none(),
            EventType::AttemptCreated => attempt_id.is_some() && attempt_outcome.is_none(),
            EventType::AttemptConcluded => attempt_id.is_some() && attempt_outcome.is_some(),
            _ => false,
        };
    if !valid {
        return Err(corrupt(&format!(
            "runtime event {event_type} with invalid scope for its family"
        )));
    }
    Ok(())
}

/// Read-path mirror of the ADR-0023 §9 scope matrix: each workspace
/// family carries project + task + execution + workspace scope, no attempt
/// scope, no Task lifecycle states, and exactly its contract-fixed lease
/// lifecycle pair.
#[allow(clippy::too_many_arguments)]
fn decode_workspace_scope(
    seq: i64,
    event_type: &EventType,
    task_id: &Option<TaskId>,
    execution_id: &Option<ExecutionId>,
    workspace_id: &Option<WorkspaceId>,
    attempt_id: &Option<AttemptId>,
    attempt_outcome: &Option<AttemptOutcome>,
    from_workspace_state: &Option<LeaseLifecycle>,
    to_workspace_state: &Option<LeaseLifecycle>,
) -> StorageResult<()> {
    use workspace::WorkspaceEventKind;
    let expected = match event_type {
        EventType::WorkspaceCreated => WorkspaceEventKind::Created.transition(),
        EventType::WorkspaceRemoved => WorkspaceEventKind::Removed.transition(),
        _ => {
            return Err(StorageError::Corrupt(format!(
                "journal seq {seq} stores non-workspace event {event_type} in workspace scope decode"
            )));
        }
    };
    let valid = task_id.is_some()
        && execution_id.is_some()
        && workspace_id.is_some()
        && attempt_id.is_none()
        && attempt_outcome.is_none()
        && *from_workspace_state == Some(expected.0)
        && *to_workspace_state == Some(expected.1);
    if !valid {
        return Err(StorageError::Corrupt(format!(
            "journal seq {seq} stores workspace event {event_type} with invalid scope for its family"
        )));
    }
    Ok(())
}

/// Envelope invariants the schema requires (non-blank correlation and
/// causation) are checked before touching SQL so a violation never becomes
/// a half-written transaction.
pub(crate) fn require_envelope(event: &NewJournalEvent) -> StorageResult<()> {
    if event.correlation_id.trim().is_empty() {
        return Err(StorageError::ConstraintViolation(
            "journal event requires a non-blank correlation_id".to_owned(),
        ));
    }
    if event.causation_id.trim().is_empty() {
        return Err(StorageError::ConstraintViolation(
            "journal event requires a non-blank causation_id".to_owned(),
        ));
    }
    if event.occurred_at.trim().is_empty() {
        return Err(StorageError::ConstraintViolation(
            "journal event requires an occurred_at timestamp".to_owned(),
        ));
    }
    if event.provenance.trim().is_empty() {
        return Err(StorageError::ConstraintViolation(
            "journal event requires provenance".to_owned(),
        ));
    }
    require_scope(event)
}

/// Scope coherence between an event's type and its references, mirroring
/// the migration-0003/migration-0004/migration-0005 CHECKs:
/// `project.created` carries no task scope at all (never a fake or
/// sentinel id); G2 task/dependency/lifecycle facts carry full Task scope
/// and no runtime or workspace scope; runtime facts follow the ADR-0022
/// matrix (execution scope, attempt scope for attempt facts, outcome only
/// on conclusion, never Task lifecycle states, never workspace scope);
/// workspace facts follow the ADR-0023 §9 matrix (full Task scope minus
/// Task states, plus execution and workspace scope, never attempt scope,
/// lease lifecycle in the workspace-state fields). Checked before
/// touching SQL.
fn require_scope(event: &NewJournalEvent) -> StorageResult<()> {
    if event.event_type.is_project_scoped() {
        if event.task_id.is_some()
            || event.from_state.is_some()
            || event.to_state.is_some()
            || event.execution_id.is_some()
            || event.attempt_id.is_some()
            || event.attempt_outcome.is_some()
            || event.workspace_id.is_some()
            || event.from_workspace_state.is_some()
            || event.to_workspace_state.is_some()
        {
            return Err(StorageError::ConstraintViolation(
                "project-scoped journal event must not carry task, runtime, or workspace scope"
                    .to_owned(),
            ));
        }
    } else if event.event_type.is_runtime_scoped() {
        require_runtime_scope(event)?;
    } else if event.event_type.is_workspace_scoped() {
        require_workspace_scope(event)?;
    } else if event.task_id.is_none()
        || event.from_state.is_none()
        || event.to_state.is_none()
        || event.execution_id.is_some()
        || event.attempt_id.is_some()
        || event.attempt_outcome.is_some()
        || event.workspace_id.is_some()
        || event.from_workspace_state.is_some()
        || event.to_workspace_state.is_some()
    {
        return Err(StorageError::ConstraintViolation(
            "task-scoped journal event requires task_id, from_state, and to_state \
             and must not carry runtime or workspace scope"
                .to_owned(),
        ));
    }
    Ok(())
}

/// ADR-0022 scope matrix enforcement on the write path: each runtime
/// type carries exactly its family's scope, nothing more.
fn require_runtime_scope(event: &NewJournalEvent) -> StorageResult<()> {
    let valid = event.task_id.is_some()
        && event.from_state.is_none()
        && event.to_state.is_none()
        && event.workspace_id.is_none()
        && event.from_workspace_state.is_none()
        && event.to_workspace_state.is_none()
        && match event.event_type {
            EventType::ExecutionCreated => {
                event.execution_id.is_some()
                    && event.attempt_id.is_none()
                    && event.attempt_outcome.is_none()
            }
            EventType::AttemptCreated => {
                event.execution_id.is_some()
                    && event.attempt_id.is_some()
                    && event.attempt_outcome.is_none()
            }
            EventType::AttemptConcluded => {
                event.execution_id.is_some()
                    && event.attempt_id.is_some()
                    && event.attempt_outcome.is_some()
            }
            _ => false,
        };
    if !valid {
        return Err(StorageError::ConstraintViolation(format!(
            "runtime event {} carries invalid scope for its family",
            event.event_type
        )));
    }
    Ok(())
}

/// ADR-0023 §9 scope matrix enforcement on the write path: each workspace
/// type carries exactly its family's scope — project + task + execution +
/// workspace, no attempt scope, no Task lifecycle states — and exactly the
/// contract-fixed lease lifecycle pair (callers never choose it).
fn require_workspace_scope(event: &NewJournalEvent) -> StorageResult<()> {
    use workspace::WorkspaceEventKind;
    let expected = match event.event_type {
        EventType::WorkspaceCreated => WorkspaceEventKind::Created.transition(),
        EventType::WorkspaceRemoved => WorkspaceEventKind::Removed.transition(),
        _ => {
            return Err(StorageError::ConstraintViolation(format!(
                "non-workspace event {} reached workspace scope validation",
                event.event_type
            )));
        }
    };
    let valid = event.task_id.is_some()
        && event.from_state.is_none()
        && event.to_state.is_none()
        && event.execution_id.is_some()
        && event.workspace_id.is_some()
        && event.attempt_id.is_none()
        && event.attempt_outcome.is_none()
        && event.from_workspace_state == Some(expected.0)
        && event.to_workspace_state == Some(expected.1);
    if !valid {
        return Err(StorageError::ConstraintViolation(format!(
            "workspace event {} carries invalid scope for its family",
            event.event_type
        )));
    }
    Ok(())
}

/// Validates that `event` is the `project.created` evidence for exactly
/// `project` before any write.
fn require_project_created_envelope(
    project: &Project,
    event: &NewJournalEvent,
) -> StorageResult<()> {
    if event.event_type != EventType::ProjectCreated {
        return Err(StorageError::ConstraintViolation(
            "project creation requires a project.created event".to_owned(),
        ));
    }
    if event.project_id != *project.id() {
        return Err(StorageError::ConstraintViolation(
            "project.created event references a different project than the state write".to_owned(),
        ));
    }
    require_envelope(event)
}

/// Validates that `event` is the `task.created` evidence for exactly
/// `task` in its current (initial) state before any write.
fn require_task_created_envelope(task: &Task, event: &NewJournalEvent) -> StorageResult<()> {
    if event.event_type != EventType::TaskCreated {
        return Err(StorageError::ConstraintViolation(
            "task creation requires a task.created event".to_owned(),
        ));
    }
    if event.task_id.as_ref() != Some(task.id()) || event.project_id != *task.project_id() {
        return Err(StorageError::ConstraintViolation(
            "task.created event references a different task or project than the state write"
                .to_owned(),
        ));
    }
    if event.from_state != Some(task.state()) || event.to_state != Some(task.state()) {
        return Err(StorageError::ConstraintViolation(
            "task.created event states must be the task initial state".to_owned(),
        ));
    }
    require_envelope(event)
}

/// Validates that `event` is the `task.dependency_added` evidence for an
/// edge out of `task` before any write. Dependency edges never mutate
/// lifecycle state, so the event records the dependent task's unchanged
/// current state.
fn require_dependency_envelope(task: &Task, event: &NewJournalEvent) -> StorageResult<()> {
    if event.event_type != EventType::DependencyAdded {
        return Err(StorageError::ConstraintViolation(
            "dependency mutation requires a task.dependency_added event".to_owned(),
        ));
    }
    if event.task_id.as_ref() != Some(task.id()) || event.project_id != *task.project_id() {
        return Err(StorageError::ConstraintViolation(
            "task.dependency_added event references a different task or project than the edge write"
                .to_owned(),
        ));
    }
    if event.from_state != Some(task.state()) || event.to_state != Some(task.state()) {
        return Err(StorageError::ConstraintViolation(
            "task.dependency_added event states must be the unchanged task state".to_owned(),
        ));
    }
    require_envelope(event)
}

/// Appends one validated journal record inside the caller's transaction
/// and returns it with its assigned identity. The caller owns the atomic
/// boundary (state writes plus this append commit together); this helper
/// never commits on its own.
pub(crate) fn insert_journal_event(
    connection: &Connection,
    event: &NewJournalEvent,
) -> StorageResult<JournalEvent> {
    require_envelope(event)?;
    // Workspace rows carry lease lifecycle names in the shared state
    // columns; every other row carries Task names or NULL. The vocabularies
    // never mix: require_scope above already enforced which pair is set.
    let from_state = event
        .from_workspace_state
        .map(|state| state.as_str().to_owned())
        .or_else(|| event.from_state.map(|state| state.as_str().to_owned()));
    let to_state = event
        .to_workspace_state
        .map(|state| state.as_str().to_owned())
        .or_else(|| event.to_state.map(|state| state.as_str().to_owned()));
    connection.execute(
        "INSERT INTO event_journal
         (event_type, event_version, task_id, project_id, from_state, to_state,
          reason, correlation_id, causation_id, occurred_at, provenance,
          execution_id, attempt_id, attempt_outcome, workspace_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        params![
            event.event_type.as_str(),
            EVENT_SCHEMA_VERSION,
            event.task_id.as_ref().map(|id| id.as_str()),
            event.project_id.as_str(),
            from_state,
            to_state,
            event.reason,
            event.correlation_id,
            event.causation_id,
            event.occurred_at,
            event.provenance,
            event.execution_id.as_ref().map(|id| id.as_str()),
            event.attempt_id.as_ref().map(|id| id.as_str()),
            event.attempt_outcome.map(|outcome| outcome.as_str()),
            event.workspace_id.as_ref().map(|id| id.as_str()),
        ],
    )?;
    let seq = connection.last_insert_rowid();
    Ok(JournalEvent {
        id: EventId::parse(&format!("evt-{seq}"))?,
        seq,
        event_version: EVENT_SCHEMA_VERSION,
        fact: event.clone(),
    })
}

fn insert_task_row(connection: &Connection, snapshot: &TaskSnapshot) -> StorageResult<()> {
    connection
        .execute(
            "INSERT INTO tasks (id, project_id, objective, parent_id, state, kind, risk)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                snapshot.id.as_str(),
                snapshot.project_id.as_str(),
                snapshot.objective,
                snapshot.parent.as_ref().map(|parent| parent.as_str()),
                snapshot.state.as_str(),
                snapshot.kind,
                snapshot.risk,
            ],
        )
        .map_err(|error| map_constraint(error, "insert task"))?;
    Ok(())
}

fn insert_task_row_replace(connection: &Connection, snapshot: &TaskSnapshot) -> StorageResult<()> {
    connection
        .execute(
            "INSERT OR REPLACE INTO tasks (id, project_id, objective, parent_id, state, kind, risk)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                snapshot.id.as_str(),
                snapshot.project_id.as_str(),
                snapshot.objective,
                snapshot.parent.as_ref().map(|parent| parent.as_str()),
                snapshot.state.as_str(),
                snapshot.kind,
                snapshot.risk,
            ],
        )
        .map_err(|error| map_constraint(error, "save task"))?;
    Ok(())
}

fn insert_task_strings(connection: &Connection, snapshot: &TaskSnapshot) -> StorageResult<()> {
    let lists: [(&str, &[String]); 6] = [
        ("acceptance", &snapshot.acceptance),
        ("allowed_path", &snapshot.allowed_paths),
        ("forbidden_path", &snapshot.forbidden_paths),
        ("verification", &snapshot.verification),
        ("required_review", &snapshot.required_review),
        ("artifact", &snapshot.artifacts),
    ];
    for (slot, values) in lists {
        for (position, value) in values.iter().enumerate() {
            connection.execute(
                "INSERT INTO task_strings (task_id, slot, position, value)
                 VALUES (?1, ?2, ?3, ?4)",
                params![snapshot.id.as_str(), slot, position as i64, value,],
            )?;
        }
    }
    Ok(())
}

fn insert_task_edges(connection: &Connection, snapshot: &TaskSnapshot) -> StorageResult<()> {
    for dep in &snapshot.depends_on {
        connection
            .execute(
                "INSERT INTO task_dependencies (task_id, depends_on_id) VALUES (?1, ?2)",
                params![snapshot.id.as_str(), dep.as_str()],
            )
            .map_err(|error| map_constraint(error, "insert dependency"))?;
    }
    Ok(())
}

fn owned_slot(slot: &str) -> &'static str {
    // The caller already matched the closed slot vocabulary; this only
    // re-borrows the static spelling for the accumulation map.
    match slot {
        "acceptance" => "acceptance",
        "allowed_path" => "allowed_path",
        "forbidden_path" => "forbidden_path",
        "verification" => "verification",
        "required_review" => "required_review",
        "artifact" => "artifact",
        _ => unreachable!("slot vocabulary checked by caller"),
    }
}

fn ordered_values(
    strings: &std::collections::BTreeMap<&'static str, Vec<(i64, String)>>,
    slot: &str,
) -> Vec<String> {
    let mut items = strings.get(slot).cloned().unwrap_or_default();
    items.sort();
    items.into_iter().map(|(_, value)| value).collect()
}
