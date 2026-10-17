//! SQLite memory seam (TASK-0182, ADR-0044).
//!
//! Multi-row writes run in one IMMEDIATE transaction (storage
//! consistency). Lifecycle decisions stay in the pure `memory`
//! domain; this adapter only encodes, persists and decodes through
//! validated constructors, so stored bytes can never bypass domain
//! validation on read.

use memory::model::{
    AnchorFingerprint, AnchorKind, Entity, EntityId, EntityLink, LinkKind, MemoryError, MemoryId,
    MemoryKind, MemoryLink, MemoryRecord, MemorySensitivity as Sensitivity, SourceRef,
    SourceRefKind, TrustState,
};
use memory::port::{CaptureOutcome, MemoryStorePort};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

use crate::task_store::SqliteStore;

fn backend(error: rusqlite::Error) -> MemoryError {
    MemoryError::BackendFailure {
        detail: error.to_string(),
    }
}

fn stored<T>(parsed: Result<T, MemoryError>, what: &str) -> Result<T, MemoryError> {
    parsed.map_err(|err| MemoryError::BackendFailure {
        detail: format!("stored {what} is malformed: {err}"),
    })
}

fn u64_to_i64(value: u64, field: &str) -> Result<i64, MemoryError> {
    i64::try_from(value).map_err(|_| MemoryError::InvalidInput(format!("{field} overflows i64")))
}

fn i64_to_u64(value: i64, field: &str) -> Result<u64, MemoryError> {
    u64::try_from(value).map_err(|_| MemoryError::BackendFailure {
        detail: format!("stored {field} is negative"),
    })
}

const RECORD_COLUMNS: &str = "memory_id, kind, content, content_hash, confidence, \
    sensitivity, trust_state, author_ref, created_at, last_verified_at, expires_at, supersedes";

fn insert_children(
    tx: &rusqlite::Transaction<'_>,
    record: &MemoryRecord,
) -> Result<(), MemoryError> {
    for (pos, source) in record.source_refs.iter().enumerate() {
        tx.execute(
            "INSERT INTO memory_source_refs (memory_id, position, kind, reference, revision)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                record.id.as_str(),
                i64::try_from(pos).unwrap_or(i64::MAX),
                source.kind.as_str(),
                source.reference,
                source.revision
            ],
        )
        .map_err(backend)?;
    }
    for (pos, anchor) in record.anchors.iter().enumerate() {
        tx.execute(
            "INSERT INTO memory_anchors (memory_id, position, kind, reference, fingerprint)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                record.id.as_str(),
                i64::try_from(pos).unwrap_or(i64::MAX),
                anchor.kind.as_str(),
                anchor.reference,
                anchor.fingerprint
            ],
        )
        .map_err(backend)?;
    }
    Ok(())
}

fn insert_record(tx: &rusqlite::Transaction<'_>, record: &MemoryRecord) -> Result<(), MemoryError> {
    tx.execute(
        "INSERT INTO memories (memory_id, kind, content, content_hash, dedup_key, confidence,
            sensitivity, trust_state, author_ref, created_at, last_verified_at, expires_at,
            supersedes)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            record.id.as_str(),
            record.kind.as_str(),
            record.content,
            record.content_hash,
            record.dedup_key(),
            i64::from(record.confidence),
            record.sensitivity.as_str(),
            record.trust_state.as_str(),
            record.author_ref,
            u64_to_i64(record.created_at, "created_at")?,
            u64_to_i64(record.last_verified_at, "last_verified_at")?,
            record
                .expires_at
                .map(|v| u64_to_i64(v, "expires_at"))
                .transpose()?,
            record.supersedes.as_ref().map(|id| id.as_str()),
        ],
    )
    .map_err(backend)?;
    insert_children(tx, record)
}

fn decode_record(connection: &Connection, id: &str) -> Result<Option<MemoryRecord>, MemoryError> {
    let row = connection
        .query_row(
            &format!("SELECT {RECORD_COLUMNS} FROM memories WHERE memory_id = ?1"),
            params![id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, i64>(9)?,
                    row.get::<_, Option<i64>>(10)?,
                    row.get::<_, Option<String>>(11)?,
                ))
            },
        )
        .optional()
        .map_err(backend)?;
    let Some((
        id_raw,
        kind_raw,
        content,
        content_hash,
        confidence,
        sensitivity_raw,
        state_raw,
        author_ref,
        created_at,
        last_verified_at,
        expires_at,
        supersedes_raw,
    )) = row
    else {
        return Ok(None);
    };

    let mut stmt = connection
        .prepare(
            "SELECT kind, reference, revision FROM memory_source_refs
             WHERE memory_id = ?1 ORDER BY position",
        )
        .map_err(backend)?;
    let source_rows = stmt
        .query_map(params![id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(backend)?;
    let mut source_refs = Vec::new();
    for row in source_rows {
        let (kind, reference, revision) = row.map_err(backend)?;
        let kind = stored(SourceRefKind::parse(&kind), "source ref kind")?;
        source_refs.push(stored(
            SourceRef::new(kind, reference, revision),
            "source ref",
        )?);
    }

    let mut stmt = connection
        .prepare(
            "SELECT kind, reference, fingerprint FROM memory_anchors
             WHERE memory_id = ?1 ORDER BY position",
        )
        .map_err(backend)?;
    let anchor_rows = stmt
        .query_map(params![id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(backend)?;
    let mut anchors = Vec::new();
    for row in anchor_rows {
        let (kind, reference, fingerprint) = row.map_err(backend)?;
        let kind = stored(AnchorKind::parse(&kind), "anchor kind")?;
        anchors.push(stored(
            AnchorFingerprint::new(kind, reference, fingerprint),
            "anchor",
        )?);
    }

    let record = MemoryRecord {
        id: stored(MemoryId::new(id_raw), "memory id")?,
        kind: stored(MemoryKind::parse(&kind_raw), "memory kind")?,
        content,
        content_hash,
        source_refs,
        confidence: stored(
            u16::try_from(confidence)
                .map_err(|_| MemoryError::InvalidInput("confidence out of range".to_owned())),
            "confidence",
        )?,
        sensitivity: stored(
            Sensitivity::parse(&sensitivity_raw)
                .map_err(|e| MemoryError::InvalidInput(e.to_string())),
            "sensitivity",
        )?,
        trust_state: stored(TrustState::parse(&state_raw), "trust state")?,
        anchors,
        author_ref,
        created_at: i64_to_u64(created_at, "created_at")?,
        last_verified_at: i64_to_u64(last_verified_at, "last_verified_at")?,
        expires_at: expires_at
            .map(|v| i64_to_u64(v, "expires_at"))
            .transpose()?,
        supersedes: supersedes_raw
            .map(|raw| stored(MemoryId::new(raw), "supersedes id"))
            .transpose()?,
    };
    record.revalidate()?;
    Ok(Some(record))
}

impl MemoryStorePort for SqliteStore {
    fn capture(&mut self, record: &MemoryRecord) -> Result<CaptureOutcome, MemoryError> {
        record.revalidate()?;
        let connection = self.connection_mut();
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(backend)?;
        if let Some(existing) = decode_record(&tx, record.id.as_str())? {
            if existing == *record {
                return Ok(CaptureOutcome {
                    record: existing,
                    deduplicated: true,
                });
            }
            return Err(MemoryError::Conflict(format!(
                "memory id {} exists with a different payload",
                record.id
            )));
        }
        let existing_id: Option<String> = tx
            .query_row(
                "SELECT memory_id FROM memories WHERE dedup_key = ?1",
                params![record.dedup_key()],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        if let Some(existing_id) = existing_id {
            let existing =
                decode_record(&tx, &existing_id)?.ok_or_else(|| MemoryError::BackendFailure {
                    detail: "dedup row vanished".to_owned(),
                })?;
            return Ok(CaptureOutcome {
                record: existing,
                deduplicated: true,
            });
        }
        insert_record(&tx, record)?;
        tx.commit().map_err(backend)?;
        Ok(CaptureOutcome {
            record: record.clone(),
            deduplicated: false,
        })
    }

    fn load(&self, id: &MemoryId) -> Result<Option<MemoryRecord>, MemoryError> {
        decode_record(self.connection(), id.as_str())
    }

    fn load_by_dedup_key(&self, key: &str) -> Result<Option<MemoryRecord>, MemoryError> {
        let id: Option<String> = self
            .connection()
            .query_row(
                "SELECT memory_id FROM memories WHERE dedup_key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        match id {
            Some(id) => decode_record(self.connection(), &id),
            None => Ok(None),
        }
    }

    fn update(&mut self, record: &MemoryRecord) -> Result<(), MemoryError> {
        record.revalidate()?;
        let connection = self.connection_mut();
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(backend)?;
        if decode_record(&tx, record.id.as_str())?.is_none() {
            return Err(MemoryError::NotFound(record.id.to_string()));
        }
        tx.execute(
            "DELETE FROM memory_source_refs WHERE memory_id = ?1",
            params![record.id.as_str()],
        )
        .map_err(backend)?;
        tx.execute(
            "DELETE FROM memory_anchors WHERE memory_id = ?1",
            params![record.id.as_str()],
        )
        .map_err(backend)?;
        tx.execute(
            "UPDATE memories SET kind = ?2, content = ?3, content_hash = ?4, dedup_key = ?5,
                confidence = ?6, sensitivity = ?7, trust_state = ?8, author_ref = ?9,
                created_at = ?10, last_verified_at = ?11, expires_at = ?12, supersedes = ?13
             WHERE memory_id = ?1",
            params![
                record.id.as_str(),
                record.kind.as_str(),
                record.content,
                record.content_hash,
                record.dedup_key(),
                i64::from(record.confidence),
                record.sensitivity.as_str(),
                record.trust_state.as_str(),
                record.author_ref,
                u64_to_i64(record.created_at, "created_at")?,
                u64_to_i64(record.last_verified_at, "last_verified_at")?,
                record
                    .expires_at
                    .map(|v| u64_to_i64(v, "expires_at"))
                    .transpose()?,
                record.supersedes.as_ref().map(|id| id.as_str()),
            ],
        )
        .map_err(backend)?;
        insert_children(&tx, record)?;
        tx.commit().map_err(backend)?;
        Ok(())
    }

    fn link(&mut self, link: &MemoryLink) -> Result<(), MemoryError> {
        let connection = self.connection_mut();
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(backend)?;
        for end in [&link.from, &link.to] {
            if decode_record(&tx, end.as_str())?.is_none() {
                return Err(MemoryError::NotFound(end.to_string()));
            }
        }
        tx.execute(
            "INSERT OR IGNORE INTO memory_links (from_id, to_id, kind) VALUES (?1, ?2, ?3)",
            params![link.from.as_str(), link.to.as_str(), link.kind.as_str()],
        )
        .map_err(backend)?;
        tx.commit().map_err(backend)?;
        Ok(())
    }

    fn links_from(&self, from: &MemoryId) -> Result<Vec<MemoryLink>, MemoryError> {
        let mut stmt = self
            .connection()
            .prepare(
                "SELECT to_id, kind FROM memory_links WHERE from_id = ?1
                 ORDER BY to_id, kind",
            )
            .map_err(backend)?;
        let rows = stmt
            .query_map(params![from.as_str()], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(backend)?;
        let mut out = Vec::new();
        for row in rows {
            let (to, kind) = row.map_err(backend)?;
            out.push(MemoryLink {
                from: from.clone(),
                to: stored(MemoryId::new(to), "link target")?,
                kind: stored(LinkKind::parse(&kind), "link kind")?,
            });
        }
        Ok(out)
    }

    fn upsert_entity(&mut self, entity: &Entity) -> Result<(), MemoryError> {
        let connection = self.connection_mut();
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(backend)?;
        let existing: Option<(String, String)> = tx
            .query_row(
                "SELECT name, entity_kind FROM entities WHERE entity_id = ?1",
                params![entity.id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        match existing {
            Some((name, kind)) if name == entity.name && kind == entity.entity_kind => {}
            Some(_) => return Err(MemoryError::Conflict(entity.id.to_string())),
            None => {
                tx.execute(
                    "INSERT INTO entities (entity_id, name, entity_kind) VALUES (?1, ?2, ?3)",
                    params![entity.id.as_str(), entity.name, entity.entity_kind],
                )
                .map_err(backend)?;
            }
        }
        tx.commit().map_err(backend)?;
        Ok(())
    }

    fn load_entity(&self, id: &EntityId) -> Result<Option<Entity>, MemoryError> {
        let row = self
            .connection()
            .query_row(
                "SELECT name, entity_kind FROM entities WHERE entity_id = ?1",
                params![id.as_str()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(backend)?;
        match row {
            Some((name, entity_kind)) => Ok(Some(stored(
                Entity::new(id.clone(), name, entity_kind),
                "entity",
            )?)),
            None => Ok(None),
        }
    }

    fn link_entity(&mut self, link: &EntityLink) -> Result<(), MemoryError> {
        let connection = self.connection_mut();
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(backend)?;
        if decode_record(&tx, link.memory.as_str())?.is_none() {
            return Err(MemoryError::NotFound(link.memory.to_string()));
        }
        let entity_exists: Option<String> = tx
            .query_row(
                "SELECT entity_id FROM entities WHERE entity_id = ?1",
                params![link.entity.as_str()],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        if entity_exists.is_none() {
            return Err(MemoryError::NotFound(link.entity.to_string()));
        }
        tx.execute(
            "INSERT OR IGNORE INTO entity_links (memory_id, entity_id, relation)
             VALUES (?1, ?2, ?3)",
            params![link.memory.as_str(), link.entity.as_str(), link.relation],
        )
        .map_err(backend)?;
        tx.commit().map_err(backend)?;
        Ok(())
    }

    fn entities_for(&self, memory_id: &MemoryId) -> Result<Vec<Entity>, MemoryError> {
        let mut stmt = self
            .connection()
            .prepare(
                "SELECT e.entity_id, e.name, e.entity_kind FROM entity_links l
                 JOIN entities e ON e.entity_id = l.entity_id
                 WHERE l.memory_id = ?1 ORDER BY e.entity_id",
            )
            .map_err(backend)?;
        let rows = stmt
            .query_map(params![memory_id.as_str()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(backend)?;
        let mut out = Vec::new();
        for row in rows {
            let (id, name, kind) = row.map_err(backend)?;
            out.push(stored(
                Entity::new(stored(EntityId::new(id), "entity id")?, name, kind),
                "entity",
            )?);
        }
        Ok(out)
    }

    fn list_eligible(&self, now: u64) -> Result<Vec<MemoryRecord>, MemoryError> {
        let mut stmt = self
            .connection()
            .prepare("SELECT memory_id FROM memories ORDER BY memory_id")
            .map_err(backend)?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(backend)?;
        let mut ids = Vec::new();
        for row in rows {
            ids.push(row.map_err(backend)?);
        }
        let mut out = Vec::new();
        for id in ids {
            if let Some(record) = decode_record(self.connection(), &id)? {
                if record.is_context_eligible(now) {
                    out.push(record);
                }
            }
        }
        Ok(out)
    }
}
