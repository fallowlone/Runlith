//! ADR-0045 background vocabulary: routine definitions, authorization
//! lifecycle, run lifecycle and background eligibility.
//! Pure data + validation; no I/O, no clock.

use std::fmt;

pub const MAX_TEXT_CHARS: usize = 1024;
pub const MAX_OBJECTIVE_CHARS: usize = 16_384;
pub const MIN_INTERVAL_SECONDS: u64 = 60;
pub const MAX_INTERVAL_SECONDS: u64 = 2_592_000;
pub const MAX_BUDGET_RUNS: u32 = 10_000;

const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const ID_RANDOM_LEN: usize = 26;

fn valid_prefixed(raw: &str, prefix: &str) -> bool {
    raw.strip_prefix(prefix).is_some_and(|rest| {
        rest.len() == ID_RANDOM_LEN && rest.bytes().all(|b| CROCKFORD.contains(&b))
    })
}

/// Canonical background failure (ADR-0045).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackgroundError {
    InvalidInput(String),
    UnknownKind(String),
    SelfAuthorization(String),
    Unauthorized(String),
    NotDue(String),
    BudgetExhausted(String),
    Draining(String),
    Conflict(String),
    NotFound(String),
    BackendFailure { detail: String },
}

impl fmt::Display for BackgroundError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(d) => write!(f, "invalid input: {d}"),
            Self::UnknownKind(d) => write!(f, "unknown kind: {d}"),
            Self::SelfAuthorization(d) => write!(f, "self authorization refused: {d}"),
            Self::Unauthorized(d) => write!(f, "unauthorized: {d}"),
            Self::NotDue(d) => write!(f, "not due: {d}"),
            Self::BudgetExhausted(d) => write!(f, "budget exhausted: {d}"),
            Self::Draining(d) => write!(f, "daemon draining: {d}"),
            Self::Conflict(d) => write!(f, "conflict: {d}"),
            Self::NotFound(d) => write!(f, "not found: {d}"),
            Self::BackendFailure { detail } => write!(f, "backend failure: {detail}"),
        }
    }
}

impl std::error::Error for BackgroundError {}

macro_rules! bg_id {
    ($name:ident, $prefix:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(String);
        impl $name {
            pub fn new(raw: impl Into<String>) -> Result<Self, BackgroundError> {
                let raw = raw.into();
                if !valid_prefixed(&raw, $prefix) {
                    return Err(BackgroundError::InvalidInput(format!(
                        "id must be {} + 26 Crockford chars",
                        $prefix
                    )));
                }
                Ok(Self(raw))
            }
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

bg_id!(RoutineId, "rou_");
bg_id!(RunId, "run_");

macro_rules! wire_enum {
    ($name:ident, [$($variant:ident => $wire:literal),+ $(,)?]) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum $name { $($variant,)+ }
        impl $name {
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $wire,)+ }
            }
            pub fn parse(raw: &str) -> Result<Self, BackgroundError> {
                match raw {
                    $($wire => Ok(Self::$variant),)+
                    _ => Err(BackgroundError::UnknownKind(raw.to_owned())),
                }
            }
        }
    };
}

wire_enum!(CapabilityScope, [
    ReadOnly => "READ_ONLY",
    WorkspaceWrite => "WORKSPACE_WRITE",
    Execution => "EXECUTION",
]);

wire_enum!(RoutineState, [
    Proposed => "PROPOSED",
    Authorized => "AUTHORIZED",
    Disabled => "DISABLED",
    Exhausted => "EXHAUSTED",
]);

wire_enum!(RunStatus, [
    Running => "RUNNING",
    Completed => "COMPLETED",
    Failed => "FAILED",
    Blocked => "BLOCKED",
    Interrupted => "INTERRUPTED",
]);

fn bounded(value: &str, field: &str, max: usize) -> Result<String, BackgroundError> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.chars().count() > max {
        return Err(BackgroundError::InvalidInput(format!(
            "{field} must be 1..={max} chars"
        )));
    }
    Ok(trimmed.to_owned())
}

/// One durable routine definition (ADR-0045 §1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutineDefinition {
    pub id: RoutineId,
    pub name: String,
    pub objective: String,
    pub interval_seconds: u64,
    pub capability_scope: CapabilityScope,
    pub budget_max_runs: u32,
    pub author_ref: String,
    pub authorized_by: Option<String>,
    pub state: RoutineState,
    pub created_at: u64,
    pub next_run_at: u64,
    pub runs_completed: u32,
}

impl RoutineDefinition {
    /// DEFINE: builds a validated `Proposed` definition (B-1).
    #[allow(clippy::too_many_arguments)]
    pub fn define(
        id: RoutineId,
        name: impl Into<String>,
        objective: impl Into<String>,
        interval_seconds: u64,
        capability_scope: CapabilityScope,
        budget_max_runs: u32,
        author_ref: impl Into<String>,
        now: u64,
    ) -> Result<Self, BackgroundError> {
        let name = bounded(&name.into(), "name", MAX_TEXT_CHARS)?;
        let objective = bounded(&objective.into(), "objective", MAX_OBJECTIVE_CHARS)?;
        if !(MIN_INTERVAL_SECONDS..=MAX_INTERVAL_SECONDS).contains(&interval_seconds) {
            return Err(BackgroundError::InvalidInput(format!(
                "interval_seconds must be {MIN_INTERVAL_SECONDS}..={MAX_INTERVAL_SECONDS}"
            )));
        }
        if budget_max_runs == 0 || budget_max_runs > MAX_BUDGET_RUNS {
            return Err(BackgroundError::InvalidInput(format!(
                "budget_max_runs must be 1..={MAX_BUDGET_RUNS}"
            )));
        }
        let author_ref = bounded(&author_ref.into(), "author_ref", MAX_TEXT_CHARS)?;
        Ok(Self {
            id,
            name,
            objective,
            interval_seconds,
            capability_scope,
            budget_max_runs,
            author_ref,
            authorized_by: None,
            state: RoutineState::Proposed,
            created_at: now,
            next_run_at: now,
            runs_completed: 0,
        })
    }

    /// Revalidates a stored shape (used on decode). Fails closed.
    pub fn revalidate(&self) -> Result<(), BackgroundError> {
        if self.runs_completed > self.budget_max_runs {
            return Err(BackgroundError::Conflict(format!(
                "runs_completed exceeds budget for {}",
                self.id
            )));
        }
        if self.state == RoutineState::Authorized && self.authorized_by.is_none() {
            return Err(BackgroundError::Unauthorized(format!(
                "authorized routine {} lacks authorizer",
                self.id
            )));
        }
        if self.next_run_at < self.created_at {
            return Err(BackgroundError::InvalidInput(
                "next_run_at before created_at".to_owned(),
            ));
        }
        Ok(())
    }

    /// AUTHORIZE: `Proposed -> Authorized` by a distinct authorizer (B-2).
    pub fn authorize(&mut self, authorizer: &str, _now: u64) -> Result<(), BackgroundError> {
        let authorizer = bounded(authorizer, "authorizer", MAX_TEXT_CHARS)?;
        if authorizer == self.author_ref {
            return Err(BackgroundError::SelfAuthorization(format!(
                "author {} cannot authorize own routine {}",
                self.author_ref, self.id
            )));
        }
        if self.state != RoutineState::Proposed {
            return Err(BackgroundError::InvalidInput(format!(
                "cannot authorize routine in state {}",
                self.state.as_str()
            )));
        }
        self.authorized_by = Some(authorizer);
        self.state = RoutineState::Authorized;
        Ok(())
    }

    /// DISABLE: stops scheduling; history retained.
    pub fn disable(&mut self) -> Result<(), BackgroundError> {
        if self.state == RoutineState::Disabled {
            return Err(BackgroundError::InvalidInput(
                "routine already disabled".to_owned(),
            ));
        }
        self.state = RoutineState::Disabled;
        Ok(())
    }

    /// Background eligibility (ADR-0045 §4, B-5).
    #[must_use]
    pub fn is_due(&self, now: u64) -> bool {
        self.state == RoutineState::Authorized
            && self.next_run_at <= now
            && self.runs_completed < self.budget_max_runs
    }

    /// Records one completed run; advances schedule, may exhaust (B-4).
    pub fn record_completion(&mut self, now: u64) -> Result<(), BackgroundError> {
        if self.runs_completed >= self.budget_max_runs {
            return Err(BackgroundError::BudgetExhausted(self.id.to_string()));
        }
        self.runs_completed += 1;
        self.next_run_at = now.saturating_add(self.interval_seconds.saturating_mul(1000));
        if self.runs_completed >= self.budget_max_runs {
            self.state = RoutineState::Exhausted;
        }
        Ok(())
    }
}

/// One routine execution record (ADR-0045 §2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutineRun {
    pub id: RunId,
    pub routine_id: RoutineId,
    pub status: RunStatus,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    pub evidence_ref: Option<String>,
}

impl RoutineRun {
    pub fn start(id: RunId, routine_id: RoutineId, now: u64) -> Result<Self, BackgroundError> {
        Ok(Self {
            id,
            routine_id,
            status: RunStatus::Running,
            started_at: now,
            finished_at: None,
            evidence_ref: None,
        })
    }

    fn finish(&mut self, status: RunStatus, now: u64) -> Result<(), BackgroundError> {
        if self.status != RunStatus::Running {
            return Err(BackgroundError::InvalidInput(format!(
                "cannot finish run in state {}",
                self.status.as_str()
            )));
        }
        if now < self.started_at {
            return Err(BackgroundError::InvalidInput(
                "finish time before started_at".to_owned(),
            ));
        }
        self.status = status;
        self.finished_at = Some(now);
        Ok(())
    }

    pub fn complete(&mut self, evidence: &str, now: u64) -> Result<(), BackgroundError> {
        self.evidence_ref = Some(bounded(evidence, "evidence_ref", MAX_TEXT_CHARS)?);
        self.finish(RunStatus::Completed, now)
    }

    pub fn fail(&mut self, now: u64) -> Result<(), BackgroundError> {
        self.finish(RunStatus::Failed, now)
    }

    pub fn block(&mut self, now: u64) -> Result<(), BackgroundError> {
        self.finish(RunStatus::Blocked, now)
    }

    /// Restart reconciliation: `Running -> Interrupted` (B-9).
    pub fn interrupt(&mut self, now: u64) -> Result<(), BackgroundError> {
        self.finish(RunStatus::Interrupted, now)
    }

    pub fn revalidate(&self) -> Result<(), BackgroundError> {
        if let Some(fin) = self.finished_at {
            if fin < self.started_at {
                return Err(BackgroundError::InvalidInput(
                    "finished_at before started_at".to_owned(),
                ));
            }
        }
        Ok(())
    }
}
