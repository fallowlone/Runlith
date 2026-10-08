//! G9 execution continuity domain (TASK-0171, ADR-0043).
//!
//! Pure survival vocabulary: epoch and checkpoint identity, rollover
//! gating, history ranking. Durability lives behind
//! [`port::ContinuityStorePort`]; selection stays in `context-engine`.

pub mod history;
pub mod mint;
pub mod model;
pub mod port;
pub mod rollover;
