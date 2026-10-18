//! Pure ULID-shape minting from caller-supplied time + sequence.
//! No clock, no randomness inside (matches context-engine precedent).

use crate::model::{EntityId, MemoryId};

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

/// Mints a valid [`MemoryId`] (infallible by construction).
#[must_use]
pub fn mint_memory_id(now_millis: u64, seq: u128) -> MemoryId {
    MemoryId::new(mint("mem_", now_millis, seq)).expect("minted id")
}

/// Mints a valid [`EntityId`] (infallible by construction).
#[must_use]
pub fn mint_entity_id(now_millis: u64, seq: u128) -> EntityId {
    EntityId::new(mint("ent_", now_millis, seq)).expect("minted id")
}

#[cfg(test)]
mod tests {
    use super::{mint_entity_id, mint_memory_id};

    #[test]
    fn mint_produces_valid_prefixed_ids() {
        assert!(mint_memory_id(1_700_000_000_000, 7)
            .as_str()
            .starts_with("mem_"));
        assert!(mint_entity_id(1_700_000_000_000, 7)
            .as_str()
            .starts_with("ent_"));
    }

    #[test]
    fn mint_sorts_by_time_then_sequence() {
        assert!(mint_memory_id(100, 99).as_str() < mint_memory_id(101, 0).as_str());
        assert!(mint_memory_id(100, 1).as_str() < mint_memory_id(100, 2).as_str());
    }
}
