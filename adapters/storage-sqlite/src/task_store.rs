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

use rusqlite::{params, Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use task_domain::{
    validate_acyclic, EvidenceRequirement, Prerequisite, PrerequisiteKind, Project, ProjectId,
    RequirementKind, RequirementParam, SubjectBinding, Task, TaskId, TaskSnapshot, TaskState,
};

use crate::error::{map_constraint, StorageError, StorageResult};
use crate::event::{decode_state, EventId, EventType, JournalEvent, NewJournalEvent};
use crate::journal_store::{
    get_event as get_journal_event, insert_journal_event,
    list_events_for_project as list_journal_events_for_project,
    list_events_for_task as list_journal_events_for_task,
    list_events_ordered as list_journal_events_ordered, require_envelope,
};
use crate::migration::{migrate, schema_version, verify_foreign_keys};
use crate::path_security::{prepare_database_path, verify_database_path};

/// SQLite-backed durable store. Owns one connection to an explicit file;
/// tests use test-owned temporary files, never a user database.
pub struct SqliteStore {
    connection: Connection,
}

/// One recorded binding proof consumption (ADR-0036 IMPL-C): the
/// presence nonce that authorized binding one criterion of one task.
/// At most one row per (task, criterion) — the durable AlreadyBound.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BindingProofConsumption {
    pub nonce: String,
    pub task_id: String,
    pub criterion_id: String,
    pub operation: String,
    pub bound_at_ms: i64,
}

impl SqliteStore {
    /// Opens (creating if absent) the database at `path`, enables and
    /// verifies foreign keys, and applies pending migrations.
    pub fn open(path: impl AsRef<Path>) -> StorageResult<Self> {
        let prepared = prepare_database_path(path.as_ref())?;
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW;
        let mut connection = Connection::open_with_flags(prepared.path(), flags)?;
        verify_database_path(&prepared)?;
        connection.execute_batch("PRAGMA foreign_keys = ON; PRAGMA temp_store = MEMORY;")?;
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
        verify_database_path(&prepared)?;
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
        insert_task_evidence_requirements(&transaction, &snapshot)?;
        insert_task_prerequisites(&transaction, &snapshot)?;
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
        insert_task_evidence_requirements(&transaction, &snapshot)?;
        insert_task_prerequisites(&transaction, &snapshot)?;
        insert_task_edges(&transaction, &snapshot)?;
        let committed = insert_journal_event(&transaction, event)?;
        transaction.commit()?;
        Ok(committed)
    }

    /// Loads a task by id with metadata and edges, or `None` when absent.
    /// Stored content that cannot form a valid domain object fails as
    /// corruption/reconstruction error — never silently repaired.
    pub fn get_task(&self, id: &TaskId) -> StorageResult<Option<Task>> {
        load_task(&self.connection, id)
    }

    /// Lists every stored task of one project, fully hydrated, in id order.
    pub fn list_tasks_for_project(&self, project_id: &ProjectId) -> StorageResult<Vec<Task>> {
        list_project_tasks(&self.connection, project_id)
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
        save_task_rows(&transaction, &snapshot)?;
        transaction.commit()?;
        Ok(())
    }

    /// Persists `task` together with one binding proof consumption in a
    /// single transaction (ADR-0036 IMPL-C). The task save and the
    /// consumption commit or roll back together: the loser of a
    /// concurrent double-bind loses its task image too, so the stored
    /// params always match the recorded consumption. The consumed proof
    /// row must already exist (the service records it first); an
    /// orphaned proof binds nothing by itself.
    pub fn save_task_with_binding_proof(
        &mut self,
        task: &Task,
        consumption: &BindingProofConsumption,
    ) -> StorageResult<()> {
        self.require_project(task.project_id())?;
        self.require_dependency_targets(task)?;
        let snapshot = task.snapshot();
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        save_task_rows(&transaction, &snapshot)?;
        transaction
            .execute(
                "INSERT INTO binding_proof_consumption
                 (nonce, task_id, criterion_id, operation, bound_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    consumption.nonce.as_str(),
                    consumption.task_id.as_str(),
                    consumption.criterion_id.as_str(),
                    consumption.operation.as_str(),
                    consumption.bound_at_ms,
                ],
            )
            .map_err(|error| map_constraint(error, "consume binding proof"))?;
        transaction.commit()?;
        Ok(())
    }

    /// Reads the recorded binding proof consumption for one
    /// (task, criterion), if the criterion was bound through the
    /// presence-authorized boundary.
    pub fn get_binding_consumption(
        &self,
        task_id: &str,
        criterion_id: &str,
    ) -> StorageResult<Option<BindingProofConsumption>> {
        let row: Option<(String, String, String, String, i64)> = self
            .connection
            .query_row(
                "SELECT nonce, task_id, criterion_id, operation, bound_at_ms
                 FROM binding_proof_consumption
                 WHERE task_id = ?1 AND criterion_id = ?2",
                params![task_id, criterion_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .optional()?;
        Ok(
            row.map(|(nonce, task_id, criterion_id, operation, bound_at_ms)| {
                BindingProofConsumption {
                    nonce,
                    task_id,
                    criterion_id,
                    operation,
                    bound_at_ms,
                }
            }),
        )
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
        if event.task_id.as_ref() != Some(task_id)
            || event.project_id.as_ref() != Some(next.project_id())
        {
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
        get_journal_event(&self.connection, id)
    }

    /// Lists a task's journal records in commit order.
    pub fn list_events_for_task(&self, task_id: &TaskId) -> StorageResult<Vec<JournalEvent>> {
        list_journal_events_for_task(&self.connection, task_id)
    }

    /// Lists a project's journal records in commit order.
    pub fn list_events_for_project(
        &self,
        project_id: &ProjectId,
    ) -> StorageResult<Vec<JournalEvent>> {
        list_journal_events_for_project(&self.connection, project_id)
    }

    /// Traverses the whole journal in commit order (oldest first).
    pub fn list_events_ordered(&self) -> StorageResult<Vec<JournalEvent>> {
        list_journal_events_ordered(&self.connection)
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
}

/// Task repository read over any connection, including an open transaction.
/// Other storage modules that must observe canonical Task state inside their
/// own atomic boundary use this instead of reading Task tables themselves.
pub(crate) fn load_task(connection: &Connection, id: &TaskId) -> StorageResult<Option<Task>> {
    Ok(match load_task_snapshot(connection, id.as_str())? {
        None => None,
        Some(snapshot) => Some(Task::restore(snapshot).map_err(|error| {
            StorageError::DomainReconstruction(format!("stored task {id}: {error}"))
        })?),
    })
}

/// Project listing counterpart of [`load_task`], in id order.
pub(crate) fn list_project_tasks(
    connection: &Connection,
    project_id: &ProjectId,
) -> StorageResult<Vec<Task>> {
    let mut statement =
        connection.prepare("SELECT id FROM tasks WHERE project_id = ?1 ORDER BY id")?;
    let ids: Vec<String> = statement
        .query_map(params![project_id.as_str()], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    let mut tasks = Vec::with_capacity(ids.len());
    for id in &ids {
        let snapshot = load_task_snapshot(connection, id)?
            .ok_or_else(|| StorageError::Corrupt(format!("task row {id} vanished mid-listing")))?;
        tasks.push(Task::restore(snapshot).map_err(|error| {
            StorageError::DomainReconstruction(format!("stored task {id}: {error}"))
        })?);
    }
    Ok(tasks)
}

fn load_task_snapshot(connection: &Connection, id: &str) -> StorageResult<Option<TaskSnapshot>> {
    let row: Option<TaskRow> = connection
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
    let id = TaskId::new(row.id)
        .map_err(|error| StorageError::DomainReconstruction(format!("stored task id: {error}")))?;
    let project_id = ProjectId::new(row.project_id).map_err(|error| {
        StorageError::DomainReconstruction(format!("stored project id: {error}"))
    })?;
    // A task in a project that has no row is corrupt data (foreign keys
    // stop this on write; this stops it on read when the database was
    // mutated behind the adapter's back).
    let project_exists: bool = connection.query_row(
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
        let mut statement = connection
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
        let mut statement = connection.prepare(
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
        id: id.clone(),
        project_id,
        objective: row.objective,
        parent,
        depends_on,
        acceptance: ordered_values(&strings, "acceptance"),
        evidence_requirements: load_evidence_requirements(connection, id.as_str())?,
        prerequisites: load_task_prerequisites(connection, id.as_str())?,
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

/// Loads one Task's declared activation prerequisites in declaration order.
/// Unknown kinds or values that fail domain validation fail closed as
/// reconstruction errors — never silently dropped (TASK-0135).
fn load_task_prerequisites(
    connection: &Connection,
    task_id: &str,
) -> StorageResult<Vec<Prerequisite>> {
    let mut statement = connection.prepare(
        "SELECT prerequisite_kind, value FROM task_prerequisites
         WHERE task_id = ?1 ORDER BY position",
    )?;
    let rows = statement.query_map(params![task_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut prerequisites = Vec::new();
    for row in rows {
        let (kind, value) = row?;
        let kind = PrerequisiteKind::parse(&kind).map_err(|error| {
            StorageError::Corrupt(format!("stored prerequisite kind {kind:?}: {error}"))
        })?;
        prerequisites.push(Prerequisite::new(kind, value).map_err(|error| {
            StorageError::DomainReconstruction(format!("stored prerequisite: {error}"))
        })?);
    }
    Ok(prerequisites)
}

/// Loads one Task's explicit evidence requirements in stored order. Stored
/// values that cannot reconstruct a valid domain requirement (unknown kind,
/// unknown binding, non-contiguous order, orphaned parameters) fail closed as
/// corruption — never silently repaired (ADR-0030 §2).
fn load_evidence_requirements(
    connection: &Connection,
    task_id: &str,
) -> StorageResult<Vec<EvidenceRequirement>> {
    let mut params_by_criterion: std::collections::BTreeMap<String, Vec<(i64, String, String)>> =
        std::collections::BTreeMap::new();
    {
        let mut statement = connection.prepare(
            "SELECT criterion_id, position, param_key, param_value
             FROM task_evidence_requirement_params WHERE task_id = ?1
             ORDER BY criterion_id, position",
        )?;
        let rows = statement.query_map(params![task_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        for row in rows {
            let (criterion, position, key, value) = row?;
            params_by_criterion
                .entry(criterion)
                .or_default()
                .push((position, key, value));
        }
    }
    let mut statement = connection.prepare(
        "SELECT position, criterion_id, requirement_kind, subject_binding, required,
                policy_fingerprint
         FROM task_evidence_requirements WHERE task_id = ?1 ORDER BY position",
    )?;
    let rows = statement.query_map(params![task_id], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, Option<String>>(5)?,
        ))
    })?;
    let mut requirements = Vec::new();
    let mut seen_criteria: Vec<String> = Vec::new();
    for (expected, row) in (0_i64..).zip(rows) {
        let (position, criterion_id, raw_kind, raw_binding, required, policy_fingerprint) = row?;
        if position != expected {
            return Err(StorageError::Corrupt(format!(
                "task {task_id} stores non-contiguous evidence requirement order"
            )));
        }
        let requirement_kind = RequirementKind::parse(&raw_kind).map_err(|_| {
            StorageError::Corrupt(format!(
                "task {task_id} stores unknown evidence requirement kind {raw_kind:?}"
            ))
        })?;
        let subject_binding = SubjectBinding::parse(&raw_binding).map_err(|_| {
            StorageError::Corrupt(format!(
                "task {task_id} stores unknown subject binding {raw_binding:?}"
            ))
        })?;
        let required = match required {
            0 => false,
            1 => true,
            other => {
                return Err(StorageError::Corrupt(format!(
                    "task {task_id} stores invalid required flag {other}"
                )))
            }
        };
        let params = params_by_criterion
            .remove(&criterion_id)
            .unwrap_or_default();
        let mut parameters = Vec::new();
        for (expected, (position, key, value)) in (0_i64..).zip(params) {
            if position != expected {
                return Err(StorageError::Corrupt(format!(
                    "task {task_id} criterion {criterion_id:?} stores non-contiguous parameter order"
                )));
            }
            parameters.push(
                RequirementParam::new(key, value).map_err(|error| {
                    StorageError::Corrupt(format!(
                        "task {task_id} criterion {criterion_id:?} stores an invalid parameter: {error}"
                    ))
                })?,
            );
        }
        let requirement = EvidenceRequirement::new(
            criterion_id.clone(),
            requirement_kind,
            subject_binding,
            parameters,
            policy_fingerprint,
            required,
        )
        .map_err(|error| {
            StorageError::Corrupt(format!(
                "task {task_id} stores an invalid evidence requirement: {error}"
            ))
        })?;
        seen_criteria.push(criterion_id);
        requirements.push(requirement);
    }
    if !params_by_criterion.is_empty() {
        return Err(StorageError::Corrupt(format!(
            "task {task_id} stores parameters for unknown evidence requirements"
        )));
    }
    let unique: std::collections::BTreeSet<&String> = seen_criteria.iter().collect();
    if unique.len() != seen_criteria.len() {
        return Err(StorageError::Corrupt(format!(
            "task {task_id} stores duplicate evidence requirements for one criterion"
        )));
    }
    Ok(requirements)
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
    if event.project_id.as_ref() != Some(project.id()) {
        return Err(StorageError::ConstraintViolation(
            "project.created event references a different project than the state write".to_owned(),
        ));
    }
    require_envelope(event)
}

/// Validates that `event` is the `task.created` evidence for exactly
/// `task` in its current (initial) state before any write.
pub(crate) fn require_task_created_envelope(
    task: &Task,
    event: &NewJournalEvent,
) -> StorageResult<()> {
    if event.event_type != EventType::TaskCreated {
        return Err(StorageError::ConstraintViolation(
            "task creation requires a task.created event".to_owned(),
        ));
    }
    if event.task_id.as_ref() != Some(task.id())
        || event.project_id.as_ref() != Some(task.project_id())
    {
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
    if event.task_id.as_ref() != Some(task.id())
        || event.project_id.as_ref() != Some(task.project_id())
    {
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

pub(crate) fn insert_task_row(
    connection: &Connection,
    snapshot: &TaskSnapshot,
) -> StorageResult<()> {
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
    crate::test_mutation_store::insert_default_policy(connection, snapshot.id.as_str())?;
    Ok(())
}

/// Writes the full task image inside an open transaction (shared by
/// `save_task` and the atomic binding save).
fn save_task_rows(
    transaction: &rusqlite::Transaction<'_>,
    snapshot: &TaskSnapshot,
) -> StorageResult<()> {
    let task_exists: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM tasks WHERE id = ?1)",
        params![snapshot.id.as_str()],
        |row| row.get(0),
    )?;
    // Replace the full image so the stored task always mirrors the
    // validated object (edges included); FK order: row first, then
    // children. The UPSERT keeps the primary key and dependent policy
    // revisions stable.
    insert_task_row_replace(transaction, snapshot)?;
    if !task_exists {
        crate::test_mutation_store::insert_default_policy(transaction, snapshot.id.as_str())?;
    }
    transaction.execute(
        "DELETE FROM task_strings WHERE task_id = ?1",
        params![snapshot.id.as_str()],
    )?;
    transaction.execute(
        "DELETE FROM task_dependencies WHERE task_id = ?1",
        params![snapshot.id.as_str()],
    )?;
    // Parameters reference requirements (FK), so they go first.
    transaction.execute(
        "DELETE FROM task_evidence_requirement_params WHERE task_id = ?1",
        params![snapshot.id.as_str()],
    )?;
    transaction.execute(
        "DELETE FROM task_evidence_requirements WHERE task_id = ?1",
        params![snapshot.id.as_str()],
    )?;
    transaction.execute(
        "DELETE FROM task_prerequisites WHERE task_id = ?1",
        params![snapshot.id.as_str()],
    )?;
    insert_task_strings(transaction, snapshot)?;
    insert_task_evidence_requirements(transaction, snapshot)?;
    insert_task_prerequisites(transaction, snapshot)?;
    insert_task_edges(transaction, snapshot)?;
    Ok(())
}

fn insert_task_row_replace(connection: &Connection, snapshot: &TaskSnapshot) -> StorageResult<()> {
    connection
        .execute(
            "INSERT INTO tasks (id, project_id, objective, parent_id, state, kind, risk)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET project_id=excluded.project_id,
                objective=excluded.objective, parent_id=excluded.parent_id,
                state=excluded.state, kind=excluded.kind, risk=excluded.risk",
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

pub(crate) fn insert_task_strings(
    connection: &Connection,
    snapshot: &TaskSnapshot,
) -> StorageResult<()> {
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

pub(crate) fn insert_task_evidence_requirements(
    connection: &Connection,
    snapshot: &TaskSnapshot,
) -> StorageResult<()> {
    for (position, requirement) in snapshot.evidence_requirements.iter().enumerate() {
        connection
            .execute(
                "INSERT INTO task_evidence_requirements
                 (task_id, position, criterion_id, requirement_kind, subject_binding,
                  required, policy_fingerprint)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    snapshot.id.as_str(),
                    position as i64,
                    requirement.criterion_id,
                    requirement.requirement_kind.as_str(),
                    requirement.subject_binding.as_str(),
                    if requirement.required { 1_i64 } else { 0_i64 },
                    requirement.policy_fingerprint,
                ],
            )
            .map_err(|error| map_constraint(error, "insert evidence requirement"))?;
        for (param_position, param) in requirement.params.iter().enumerate() {
            connection
                .execute(
                    "INSERT INTO task_evidence_requirement_params
                     (task_id, criterion_id, position, param_key, param_value)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        snapshot.id.as_str(),
                        requirement.criterion_id,
                        param_position as i64,
                        param.key(),
                        param.value(),
                    ],
                )
                .map_err(|error| map_constraint(error, "insert evidence requirement parameter"))?;
        }
    }
    Ok(())
}

pub(crate) fn insert_task_prerequisites(
    connection: &Connection,
    snapshot: &TaskSnapshot,
) -> StorageResult<()> {
    for (position, prerequisite) in snapshot.prerequisites.iter().enumerate() {
        connection
            .execute(
                "INSERT INTO task_prerequisites
                 (task_id, position, prerequisite_kind, value)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    snapshot.id.as_str(),
                    position as i64,
                    prerequisite.kind().as_str(),
                    prerequisite.value(),
                ],
            )
            .map_err(|error| map_constraint(error, "insert task prerequisite"))?;
    }
    Ok(())
}

pub(crate) fn insert_task_edges(
    connection: &Connection,
    snapshot: &TaskSnapshot,
) -> StorageResult<()> {
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
