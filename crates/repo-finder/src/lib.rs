//! Repository finder port owner (TASK-0147, G9).
//!
//! Validated domain values ([`model`]) and the retrieval seam
//! ([`port`]): search, span reopen, and revision binding. Retrieval
//! engines are replaceable behind this seam (ADR-0010); the shared
//! conformance runner lives in `repo-finder-contract`, and adapters
//! implement the traits without redefining them.

pub mod model;
pub mod port;

pub use model::*;
pub use port::*;
