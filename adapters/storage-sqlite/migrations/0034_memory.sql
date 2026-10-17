-- 0034: G10 Memory Engine (TASK-0182, ADR-0044).
--
-- Structured memory with provenance and lifecycle. Domain decode
-- (memory::model) stays fail-closed as a second layer over these
-- CHECKs. SECRET rows are expressible here only so a planted row
-- fails decode loudly; capture refuses SECRET before insert (M-3).

CREATE TABLE memories (
    memory_id TEXT PRIMARY KEY CHECK (
        memory_id GLOB 'mem_*' AND length(memory_id) = 30
    ),
    kind TEXT NOT NULL CHECK (
        kind IN ('SEMANTIC', 'EPISODIC', 'PROCEDURAL', 'DECISION')
    ),
    content TEXT NOT NULL CHECK (length(content) BETWEEN 1 AND 16384),
    content_hash TEXT NOT NULL CHECK (
        content_hash GLOB 'sha256:*' AND length(content_hash) = 71
    ),
    dedup_key TEXT NOT NULL UNIQUE CHECK (
        dedup_key GLOB 'sha256:*' AND length(dedup_key) = 71
    ),
    confidence INTEGER NOT NULL CHECK (confidence BETWEEN 0 AND 1000),
    sensitivity TEXT NOT NULL CHECK (
        sensitivity IN ('PUBLIC', 'PROJECT', 'PRIVATE', 'RESTRICTED', 'SECRET')
    ),
    trust_state TEXT NOT NULL CHECK (
        trust_state IN (
            'PROPOSED', 'CONFIRMED', 'CONTRADICTED',
            'STALE', 'SUPERSEDED', 'RETIRED'
        )
    ),
    author_ref TEXT NOT NULL CHECK (length(author_ref) BETWEEN 1 AND 1024),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    last_verified_at INTEGER NOT NULL CHECK (last_verified_at >= created_at),
    expires_at INTEGER CHECK (expires_at IS NULL OR expires_at > created_at),
    supersedes TEXT REFERENCES memories(memory_id)
) STRICT;

CREATE TABLE memory_source_refs (
    memory_id TEXT NOT NULL REFERENCES memories(memory_id),
    position INTEGER NOT NULL CHECK (position >= 0),
    kind TEXT NOT NULL CHECK (
        kind IN ('TASK', 'ATTEMPT', 'GIT', 'DECISION', 'EVENT', 'FILE')
    ),
    reference TEXT NOT NULL CHECK (length(reference) BETWEEN 1 AND 1024),
    revision TEXT NOT NULL CHECK (length(revision) BETWEEN 1 AND 1024),
    PRIMARY KEY (memory_id, position)
) STRICT;

CREATE TABLE memory_anchors (
    memory_id TEXT NOT NULL REFERENCES memories(memory_id),
    position INTEGER NOT NULL CHECK (position >= 0),
    kind TEXT NOT NULL CHECK (kind IN ('CODE', 'TASK', 'DECISION', 'EVENT')),
    reference TEXT NOT NULL CHECK (length(reference) BETWEEN 1 AND 1024),
    fingerprint TEXT NOT NULL CHECK (
        fingerprint GLOB 'sha256:*' AND length(fingerprint) = 71
    ),
    PRIMARY KEY (memory_id, position)
) STRICT;

CREATE TABLE memory_links (
    from_id TEXT NOT NULL REFERENCES memories(memory_id),
    to_id TEXT NOT NULL REFERENCES memories(memory_id),
    kind TEXT NOT NULL CHECK (
        kind IN ('SUPERSEDES', 'DERIVES_FROM', 'CONTRADICTS', 'RELATED')
    ),
    PRIMARY KEY (from_id, to_id, kind)
) STRICT;

CREATE TABLE entities (
    entity_id TEXT PRIMARY KEY CHECK (
        entity_id GLOB 'ent_*' AND length(entity_id) = 30
    ),
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 1024),
    entity_kind TEXT NOT NULL CHECK (length(entity_kind) BETWEEN 1 AND 1024)
) STRICT;

CREATE TABLE entity_links (
    memory_id TEXT NOT NULL REFERENCES memories(memory_id),
    entity_id TEXT NOT NULL REFERENCES entities(entity_id),
    relation TEXT NOT NULL CHECK (length(relation) BETWEEN 1 AND 1024),
    PRIMARY KEY (memory_id, entity_id, relation)
) STRICT;

CREATE INDEX memories_eligible_idx
    ON memories (trust_state, expires_at, memory_id);
