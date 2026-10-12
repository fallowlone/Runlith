//! Pure ULID-shape minting from caller-supplied time + sequence.
//!
//! No clock, no randomness inside: the service supplies millis and a
//! per-process sequence, so selection stays deterministic and tests
//! stay exact. Layout matches ULID (48-bit time, 80-bit sequence,
//! Crockford Base32): ids sort by time, then sequence.

use crate::model::{ContextItemId, ContextPlanId};

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

/// Mints a valid [`ContextItemId`] (infallible by construction).
#[must_use]
pub fn mint_item_id(now_millis: u64, seq: u128) -> ContextItemId {
    ContextItemId::new(mint("ctxitem_", now_millis, seq)).expect("minted id")
}

/// Mints a valid [`ContextPlanId`] (infallible by construction).
#[must_use]
pub fn mint_plan_id(now_millis: u64, seq: u128) -> ContextPlanId {
    ContextPlanId::new(mint("ctxplan_", now_millis, seq)).expect("minted id")
}

#[cfg(test)]
mod tests {
    use crate::mint::{mint_item_id, mint_plan_id};

    #[test]
    fn mint_produces_valid_prefixed_ids() {
        let item = mint_item_id(1_700_000_000_000, 7);
        assert!(item.as_str().starts_with("ctxitem_"));
        assert_eq!(item.as_str().len(), 8 + 26);
        let plan = mint_plan_id(1_700_000_000_000, 7);
        assert!(plan.as_str().starts_with("ctxplan_"));
        assert_eq!(plan.as_str().len(), 8 + 26);
    }

    #[test]
    fn mint_is_deterministic_for_same_inputs() {
        assert_eq!(mint_item_id(42, 9), mint_item_id(42, 9));
    }

    #[test]
    fn mint_sorts_by_time_then_sequence() {
        let early = mint_item_id(100, 99);
        let late = mint_item_id(101, 0);
        assert!(early.as_str() < late.as_str());
        let first = mint_item_id(100, 1);
        let second = mint_item_id(100, 2);
        assert!(first.as_str() < second.as_str());
    }
}
