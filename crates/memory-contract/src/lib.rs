//! Shared `memory` store contract suite (TASK-0181, G10).
//!
//! The canonical durable memory seam is
//! [`memory::port::MemoryStorePort`] (ADR-0044): idempotent capture,
//! typed lifecycle persistence, links/entities, eligibility listing.
//! Every store implementing the seam must satisfy
//! [`run_memory_store_contract`].

use context_engine::model::Sensitivity;
use memory::mint::{mint_entity_id, mint_memory_id};
use memory::model::{
    AnchorFingerprint, AnchorKind, Entity, EntityLink, LinkKind, MemoryKind, MemoryLink,
    MemoryRecord, SourceRef, SourceRefKind, TrustState,
};
use memory::port::MemoryStorePort;

/// Test-only fixtures shared by store implementers.
pub mod fixtures {
    use super::*;

    pub fn fp(byte: &str) -> String {
        format!("sha256:{}", byte.repeat(32))
    }

    pub fn anchor(byte: &str) -> AnchorFingerprint {
        AnchorFingerprint::new(AnchorKind::Code, "crates/x/src/lib.rs", fp(byte)).unwrap()
    }

    pub fn record(seq: u128, content: &str) -> MemoryRecord {
        MemoryRecord::capture(
            mint_memory_id(1_000, seq),
            MemoryKind::Semantic,
            content,
            vec![SourceRef::new(SourceRefKind::Task, "TASK-1", "rev1").unwrap()],
            900,
            Sensitivity::Project,
            vec![anchor("ab")],
            "agent-a",
            1_000,
            None,
            None,
        )
        .unwrap()
    }

    pub fn confirmed(seq: u128, content: &str) -> MemoryRecord {
        let mut rec = record(seq, content);
        rec.validate("reviewer", 2_000).unwrap();
        rec
    }
}

fn assert_capture_dedup<S: MemoryStorePort>(store: &mut S) {
    let rec = fixtures::record(1, "fact one");
    let out = store.capture(&rec).unwrap();
    assert!(!out.deduplicated);
    assert_eq!(out.record, rec);
    // Byte-identical re-capture deduplicates to the same record.
    let again = store.capture(&rec).unwrap();
    assert!(again.deduplicated);
    assert_eq!(again.record.id, rec.id);
    // Same dedup shape under a new id also deduplicates.
    let twin = fixtures::record(2, "fact one");
    let deduped = store.capture(&twin).unwrap();
    assert!(deduped.deduplicated);
    assert_eq!(deduped.record.id, rec.id);
    // Stored record round-trips and is findable by dedup key.
    assert_eq!(store.load(&rec.id).unwrap(), Some(rec.clone()));
    assert_eq!(
        store.load_by_dedup_key(&rec.dedup_key()).unwrap(),
        Some(rec.clone())
    );
    assert_eq!(store.load(&mint_memory_id(9, 9)).unwrap(), None);
}

fn assert_lifecycle_persistence<S: MemoryStorePort>(store: &mut S) {
    let mut rec = fixtures::record(3, "fact two");
    store.capture(&rec).unwrap();
    assert!(store.list_eligible(3_000).unwrap().is_empty());
    rec.validate("reviewer", 2_000).unwrap();
    store.update(&rec).unwrap();
    assert_eq!(store.load(&rec.id).unwrap(), Some(rec.clone()));
    let eligible = store.list_eligible(3_000).unwrap();
    assert!(eligible.iter().any(|r| r.id == rec.id));
    // Anchor drift persists as Stale and leaves the eligible list.
    let changed = vec![fixtures::anchor("cd")];
    assert!(rec.mark_stale_if_anchor_changed(&changed).unwrap());
    store.update(&rec).unwrap();
    assert_eq!(
        store.load(&rec.id).unwrap().unwrap().trust_state,
        TrustState::Stale
    );
    assert!(!store
        .list_eligible(3_000)
        .unwrap()
        .iter()
        .any(|r| r.id == rec.id));
    // Retired history stays loadable by exact id.
    rec.retire().unwrap();
    store.update(&rec).unwrap();
    assert!(store.load(&rec.id).unwrap().is_some());
}

fn assert_links_entities<S: MemoryStorePort>(store: &mut S) {
    let old = fixtures::confirmed(4, "old fact");
    let mut new = fixtures::record(5, "new fact");
    new.supersedes = Some(old.id.clone());
    store.capture(&old).unwrap();
    store.capture(&new).unwrap();
    new.validate("reviewer", 2_000).unwrap();
    store.update(&new).unwrap();
    store
        .link(&MemoryLink {
            from: new.id.clone(),
            to: old.id.clone(),
            kind: LinkKind::Supersedes,
        })
        .unwrap();
    let links = store.links_from(&new.id).unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].kind, LinkKind::Supersedes);

    let entity = Entity::new(mint_entity_id(1_000, 1), "agentd", "COMPONENT").unwrap();
    store.upsert_entity(&entity).unwrap();
    store.upsert_entity(&entity).unwrap(); // idempotent
    store
        .link_entity(&EntityLink::new(new.id.clone(), entity.id.clone(), "ABOUT").unwrap())
        .unwrap();
    let found = store.entities_for(&new.id).unwrap();
    assert_eq!(found, vec![entity]);
}

fn assert_reopen_holds<S: MemoryStorePort>(store: &S) {
    let rec = store.load(&mint_memory_id(1_000, 1)).unwrap();
    assert!(rec.is_some(), "captured memory must survive reopen");
    assert!(store.load(&mint_memory_id(1_000, 4)).unwrap().is_some());
    assert!(!store.list_eligible(3_000).unwrap().is_empty());
}

/// Runs the full memory store contract against a fresh store.
/// `reopen` must yield the same durable state (restart durability).
pub fn run_memory_store_contract<S: MemoryStorePort>(store: &mut S, reopen: impl Fn() -> S) {
    assert_capture_dedup(store);
    assert_lifecycle_persistence(store);
    assert_links_entities(store);
    let fresh = reopen();
    assert_reopen_holds(&fresh);
}
