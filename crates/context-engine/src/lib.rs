//! G9 Context Engine domain vocabulary (TASK-0170; ADR-0042).
//!
//! Pure selection vocabulary: items, plans, provenance envelopes and
//! retrieval traces. No I/O, no clock, no persistence — storage lives
//! behind [`port::ContextStorePort`], minting takes caller-supplied
//! time and sequence ([`mint`]), and admission is a pure function
//! ([`selection::admit`]).

pub mod mint;
pub mod model;
pub mod port;
pub mod selection;
