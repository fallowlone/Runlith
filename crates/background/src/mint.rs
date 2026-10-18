//! Pure ULID-shape minting from caller-supplied time + sequence.
use crate::model::{RoutineId, RunId};

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

#[must_use]
pub fn mint_routine_id(now_millis: u64, seq: u128) -> RoutineId {
    RoutineId::new(mint("rou_", now_millis, seq)).expect("minted id")
}

#[must_use]
pub fn mint_run_id(now_millis: u64, seq: u128) -> RunId {
    RunId::new(mint("run_", now_millis, seq)).expect("minted id")
}
