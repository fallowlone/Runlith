//! Trusted memory lifecycle composition (TASK-0183, ADR-0044).
//!
//! The service mints memory ids, runs the pure domain lifecycle and
//! persists through [`MemoryStorePort`]. Validation authority is the
//! domain rule (validator != author, INV-029); this layer never
//! weakens it and never lets an author promote their own memory.

use std::sync::atomic::{AtomicU64, Ordering};

use memory::mint::mint_memory_id;
use memory::model::{
    AnchorFingerprint, LinkKind, MemoryError, MemoryId, MemoryKind, MemoryLink, MemoryRecord,
    MemorySensitivity as Sensitivity, SourceRef,
};
use memory::port::{CaptureOutcome, MemoryStorePort};

/// One capture request: the fact plus its provenance.
#[derive(Debug)]
pub struct CaptureRequest {
    pub kind: MemoryKind,
    pub content: String,
    pub source_refs: Vec<SourceRef>,
    pub confidence: u16,
    pub sensitivity: Sensitivity,
    pub anchors: Vec<AnchorFingerprint>,
    pub author_ref: String,
    pub expires_at: Option<u64>,
    pub supersedes: Option<MemoryId>,
}

/// Trusted memory service over any [`MemoryStorePort`].
pub struct MemoryService<S> {
    store: S,
    id_seq: AtomicU64,
}

impl<S: MemoryStorePort> MemoryService<S> {
    /// Composes the service over its store.
    pub fn new(store: S) -> Self {
        Self {
            store,
            id_seq: AtomicU64::new(0),
        }
    }

    /// Borrows the underlying store (tests, composition).
    pub fn store(&self) -> &S {
        &self.store
    }

    /// CAPTURE -> NORMALIZE -> DEDUPLICATE: mints an id and persists
    /// a `Proposed` record. Byte-identical re-capture deduplicates.
    pub fn capture(
        &mut self,
        request: CaptureRequest,
        now: u64,
    ) -> Result<CaptureOutcome, MemoryError> {
        let seq = u128::from(self.id_seq.fetch_add(1, Ordering::SeqCst));
        let record = MemoryRecord::capture(
            mint_memory_id(now, seq),
            request.kind,
            request.content,
            request.source_refs,
            request.confidence,
            request.sensitivity,
            request.anchors,
            request.author_ref,
            now,
            request.expires_at,
            request.supersedes,
        )?;
        self.store.capture(&record)
    }

    /// Persists a fully built record (importers, tests).
    pub fn capture_record(&mut self, record: &MemoryRecord) -> Result<CaptureOutcome, MemoryError> {
        self.store.capture(record)
    }

    /// Loads one record by exact id; `NotFound` when unknown.
    pub fn get(&self, id: &MemoryId) -> Result<MemoryRecord, MemoryError> {
        self.store
            .load(id)?
            .ok_or_else(|| MemoryError::NotFound(id.to_string()))
    }

    /// VALIDATE: trusted validator promotes `Proposed|Stale`.
    /// The record's author is refused by the domain (INV-029).
    pub fn validate(
        &mut self,
        id: &MemoryId,
        validator: &str,
        now: u64,
    ) -> Result<MemoryRecord, MemoryError> {
        let mut record = self.get(id)?;
        record.validate(validator, now)?;
        self.store.update(&record)?;
        Ok(record)
    }

    /// VALIDATE (negative): mark `Proposed` contradicted with evidence.
    pub fn contradict(
        &mut self,
        id: &MemoryId,
        validator: &str,
        evidence: Vec<SourceRef>,
    ) -> Result<MemoryRecord, MemoryError> {
        let mut record = self.get(id)?;
        record.contradict(validator, evidence)?;
        self.store.update(&record)?;
        Ok(record)
    }

    /// ANCHOR DRIFT check: marks the record stale when any anchor
    /// fingerprint no longer matches `current`. Returns staleness.
    pub fn check_staleness(
        &mut self,
        id: &MemoryId,
        current: &[AnchorFingerprint],
    ) -> Result<bool, MemoryError> {
        let mut record = self.get(id)?;
        let stale = record.mark_stale_if_anchor_changed(current)?;
        if stale {
            self.store.update(&record)?;
        }
        Ok(stale)
    }

    /// SUPERSEDE: successor must already be captured and confirmed.
    /// Persists the state change plus the SUPERSEDES link atomically
    /// at the service level (two store calls, each transactional).
    pub fn supersede(
        &mut self,
        old_id: &MemoryId,
        successor_id: &MemoryId,
    ) -> Result<MemoryRecord, MemoryError> {
        let mut old = self.get(old_id)?;
        let successor = self.get(successor_id)?;
        old.supersede_by(&successor)?;
        self.store.update(&old)?;
        self.store.link(&MemoryLink {
            from: successor_id.clone(),
            to: old_id.clone(),
            kind: LinkKind::Supersedes,
        })?;
        Ok(old)
    }

    /// ARCHIVE: retire a record (terminal, history retained).
    pub fn retire(&mut self, id: &MemoryId) -> Result<MemoryRecord, MemoryError> {
        let mut record = self.get(id)?;
        record.retire()?;
        self.store.update(&record)?;
        Ok(record)
    }

    /// Lists context-eligible memory at `now` (ADR-0044 §5).
    pub fn list_eligible(&self, now: u64) -> Result<Vec<MemoryRecord>, MemoryError> {
        self.store.list_eligible(now)
    }

    /// Builds ADR-0042 selection candidates from eligible memory only
    /// (TASK-0185). Ineligible records (proposed, stale, superseded,
    /// retired, expired, SECRET) never become candidates by
    /// construction; admission, budget and provenance rules still
    /// apply on top in `ContextService`.
    pub fn context_candidates(
        &self,
        now: u64,
    ) -> Result<Vec<(context_engine::model::ContextItem, String)>, MemoryError> {
        let eligible = self.store.list_eligible(now)?;
        let mut out = Vec::with_capacity(eligible.len());
        for (index, record) in eligible.into_iter().enumerate() {
            out.push(memory_candidate(&record, now, index as u128)?);
        }
        Ok(out)
    }
}

/// Maps one eligible memory record to a `MemoryNote` context candidate.
fn memory_candidate(
    record: &MemoryRecord,
    now: u64,
    seq: u128,
) -> Result<(context_engine::model::ContextItem, String), MemoryError> {
    use context_engine::mint::mint_item_id;
    use context_engine::model::{
        CacheClass, ContentHash, ContextItem, ContextKind, Freshness, ProvenanceEnvelope,
        ReopenRef, SourceRef, Trust, TrustLevel,
    };
    let invalid = |e: context_engine::model::ContextError| MemoryError::InvalidInput(e.to_string());
    let binding = format!("{}@{}", record.id, record.content_hash);
    let source = SourceRef::memory(&binding).map_err(invalid)?;
    let envelope = ProvenanceEnvelope::new(
        source.clone(),
        &record.content_hash,
        now,
        "memory-engine",
        "memory:eligible",
        record.confidence,
    )
    .map_err(invalid)?
    .with_reopen(ReopenRef::Handle(record.id.to_string()));
    let item = ContextItem {
        id: mint_item_id(now, seq),
        kind: ContextKind::MemoryNote,
        source_ref: source,
        content_hash: ContentHash::new(record.content_hash.clone()).map_err(invalid)?,
        priority: 100,
        freshness: Freshness::new(
            record.last_verified_at,
            &record.content_hash,
            record.expires_at,
        )
        .map_err(invalid)?,
        trust: Trust::new(TrustLevel::Verified, "memory trust_state CONFIRMED").map_err(invalid)?,
        sensitivity: record.sensitivity,
        required: false,
        cache_class: CacheClass::SessionEphemeral,
        provenance: envelope,
    };
    item.validate().map_err(invalid)?;
    Ok((item, record.content.clone()))
}
