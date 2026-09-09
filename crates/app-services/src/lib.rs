//! Trusted application services: the only path that mints authoritative
//! Task lifecycle facts.
//!
//! A provider, agent, model, or frontend can supply evidence and request
//! work, but only this layer — after `task-domain` validation — commits a
//! durable Task mutation together with its Event Journal record in one
//! SQLite transaction (via `storage-sqlite`). G2 implements the Task
//! mutation capability only; no other bounded context lives here.

mod clock;
mod error;
mod task_service;

pub use clock::{format_rfc3339, Clock, FixedClock, SystemClock};
pub use error::{ServiceError, ServiceResult};
pub use task_service::{AppliedTransition, TaskService};
