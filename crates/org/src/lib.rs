//! G13 Cloud/Team domain (TASK-0208, ADR-0047).
//! Pure organization vocabulary: account/org/team/membership identity,
//! layered org policy, server-blind sync envelopes, usage quota.
//! Private keys never enter records; signing takes secrets as input.
pub mod mint;
pub mod model;
pub mod port;
