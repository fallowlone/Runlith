//! G11 Background daemon domain (TASK-0190, ADR-0045).
//! Pure routine vocabulary: definitions, authorization lifecycle,
//! runs, budgets and background eligibility. No I/O, no clock.
pub mod mint;
pub mod model;
pub mod port;
