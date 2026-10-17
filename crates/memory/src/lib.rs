//! G10 Memory Engine domain (TASK-0181, ADR-0044).
//!
//! Pure structured-memory vocabulary: typed records, provenance,
//! trust lifecycle, dedup, anchor staleness and context eligibility.
//! Durability lives behind [`port::MemoryStorePort`]; selection stays
//! in `context-engine`.

pub mod mint;
pub mod model;
pub mod port;
