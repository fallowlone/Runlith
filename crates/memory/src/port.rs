//! Durable memory seam: records, links and entities behind a port.
//!
//! The port promises round-trip persistence with reopen-after-restart
//! durability. Lifecycle decisions stay pure ([`crate::model`]); the
//! store never invents trust states, eligibility or deduplication.

use crate::model::{Entity, EntityLink, MemoryError, MemoryId, MemoryLink, MemoryRecord};

/// Outcome of a capture: the stored record plus whether dedup hit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureOutcome {
    pub record: MemoryRecord,
    pub deduplicated: bool,
}

/// Durable store for structured memory (ADR-0044).
pub trait MemoryStorePort {
    /// Persists one `Proposed` record atomically.
    ///
    /// Idempotent for a byte-identical dedup key (returns the existing
    /// record with `deduplicated = true`); a different payload under an
    /// existing id refuses with `Conflict`. The record must revalidate
    /// or the call refuses without persisting.
    fn capture(&mut self, record: &MemoryRecord) -> Result<CaptureOutcome, MemoryError>;

    /// Loads a record by id; `None` when unknown (never invented).
    fn load(&self, id: &MemoryId) -> Result<Option<MemoryRecord>, MemoryError>;

    /// Loads a record by ADR-0044 dedup key; `None` when unknown.
    fn load_by_dedup_key(&self, key: &str) -> Result<Option<MemoryRecord>, MemoryError>;

    /// Persists a lifecycle-updated record with the same id.
    /// `NotFound` when absent; the record must revalidate.
    fn update(&mut self, record: &MemoryRecord) -> Result<(), MemoryError>;

    /// Records a memory-to-memory link. Both ends must exist.
    fn link(&mut self, link: &MemoryLink) -> Result<(), MemoryError>;

    /// Loads links originating at `from`, sorted deterministically.
    fn links_from(&self, from: &MemoryId) -> Result<Vec<MemoryLink>, MemoryError>;

    /// Persists an entity (idempotent for identical payload).
    fn upsert_entity(&mut self, entity: &Entity) -> Result<(), MemoryError>;

    /// Loads an entity by id; `None` when unknown.
    fn load_entity(&self, id: &crate::model::EntityId) -> Result<Option<Entity>, MemoryError>;

    /// Records a memory-to-entity link. Both ends must exist.
    fn link_entity(&mut self, link: &EntityLink) -> Result<(), MemoryError>;

    /// Loads entities linked to a memory, sorted by entity id.
    fn entities_for(&self, memory: &MemoryId) -> Result<Vec<Entity>, MemoryError>;

    /// Lists context-eligible records at `now` (domain predicate),
    /// sorted by id. Ineligible history stays loadable by exact id.
    fn list_eligible(&self, now: u64) -> Result<Vec<MemoryRecord>, MemoryError>;
}
