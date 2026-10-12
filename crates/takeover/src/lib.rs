//! TAKEOVER continuation domain model (TASK-0172, TASK-0169 amendment).
//!
//! A human assumes execution authority for a HumanDecisionRequest scope
//! mid-flight. Authority transfers to the human and returns to the machine
//! only through a subsequent machine planning cycle; nothing resumes
//! automatically. This crate owns the record, status, authored-field,
//! anchor and rendering types; persistence lives in SQLite alongside HDR
//! state and the Planning Application Service owns the lifecycle.

pub mod model;

pub use model::{
    compare_anchors, validate_takeover_expiry, AuthoredField, HandoffPacket, TakeoverAdmission,
    TakeoverLeaseAnchor, TakeoverRecordId, TakeoverStateRefs, TakeoverStatus,
    TAKEOVER_AUTHORED_MAX_BYTES, TAKEOVER_MAX_TTL_MS,
};
