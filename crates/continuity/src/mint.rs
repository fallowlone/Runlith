//! Pure ULID-shape minting from caller-supplied time + sequence.
//!
//! Same shape as `context-engine` mint (48-bit time, 80-bit sequence,
//! Crockford Base32): ids sort by time, then sequence. Kept local so
//! each crate owns its id vocabulary (no clock, no randomness inside).

use crate::model::{CheckpointId, EpochId, HistoryRef, NoteId};

const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

fn encode(mut time48: u64, mut seq80: u128) -> [u8; 26] {
    let mut out = [0u8; 26];
    for slot in out.iter_mut().rev().take(16) {
        *slot = CROCKFORD[(seq80 & 31) as usize];
        seq80 >>= 5;
    }
    for slot in out.iter_mut().rev().skip(16) {
        *slot = CROCKFORD[(time48 & 31) as usize];
        time48 >>= 5;
    }
    out
}

fn mint(prefix: &str, now_millis: u64, seq: u128) -> String {
    let time48 = now_millis & 0xFFFF_FFFF_FFFF;
    let seq80 = seq & ((1 << 80) - 1);
    let mut id = String::with_capacity(prefix.len() + 26);
    id.push_str(prefix);
    id.push_str(std::str::from_utf8(&encode(time48, seq80)).expect("crockford"));
    id
}

/// Mints a valid [`EpochId`] (infallible by construction).
#[must_use]
pub fn mint_epoch_id(now_millis: u64, seq: u128) -> EpochId {
    EpochId::new(mint("ctxepoch_", now_millis, seq)).expect("minted id")
}

/// Mints a valid [`CheckpointId`] (infallible by construction).
#[must_use]
pub fn mint_checkpoint_id(now_millis: u64, seq: u128) -> CheckpointId {
    CheckpointId::new(mint("ctxckpt_", now_millis, seq)).expect("minted id")
}

/// Mints a valid [`HistoryRef`] (infallible by construction).
#[must_use]
pub fn mint_history_ref(now_millis: u64, seq: u128) -> HistoryRef {
    HistoryRef::new(mint("histref_", now_millis, seq)).expect("minted id")
}

/// Mints a valid [`NoteId`] (infallible by construction).
#[must_use]
pub fn mint_note_id(now_millis: u64, seq: u128) -> NoteId {
    NoteId::new(mint("cknote_", now_millis, seq)).expect("minted id")
}
