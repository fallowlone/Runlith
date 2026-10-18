//! G12 Remote Worker domain (TASK-0199, ADR-0046).
//! Pure device/job vocabulary with ed25519 challenge auth.
//! Private keys never enter records; signing takes secrets as input.
pub mod mint;
pub mod model;
pub mod port;
