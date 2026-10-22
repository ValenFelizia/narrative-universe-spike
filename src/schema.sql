CREATE TABLE metadata (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    universe_id TEXT NOT NULL CHECK (length(universe_id) = 36)
);

CREATE TABLE pages (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) = 36),
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    revision INTEGER NOT NULL DEFAULT 0 CHECK (
        typeof(revision) = 'integer' AND revision >= 0 AND revision <= 9223372036854775807
    )
);

CREATE TABLE types (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) = 36),
    label TEXT NOT NULL
);

CREATE TABLE entities (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) = 36),
    label TEXT NOT NULL,
    type_id TEXT NULL REFERENCES types (id),
    page_id TEXT NULL REFERENCES pages (id)
);

-- One occurrence of an entity inside a page. The id is unique across every
-- page, so an occurrence keeps its identity wherever the prose moves.
CREATE TABLE mentions (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) = 36),
    page_id TEXT NOT NULL REFERENCES pages (id) ON DELETE RESTRICT,
    entity_id TEXT NOT NULL REFERENCES entities (id) ON DELETE RESTRICT,
    start_utf16 INTEGER NOT NULL CHECK (
        typeof(start_utf16) = 'integer' AND start_utf16 >= 0 AND start_utf16 <= 4294967295
    ),
    end_utf16 INTEGER NOT NULL CHECK (
        typeof(end_utf16) = 'integer' AND end_utf16 > start_utf16 AND end_utf16 <= 4294967295
    )
);

CREATE INDEX idx_mentions_page ON mentions (page_id, start_utf16, end_utf16);
CREATE INDEX idx_mentions_entity ON mentions (entity_id, page_id, start_utf16);

CREATE TABLE definitions (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) = 36),
    owner_type_id TEXT NULL REFERENCES types (id),
    owner_entity_id TEXT NULL REFERENCES entities (id),
    value_class TEXT NOT NULL CHECK (value_class IN ('text', 'boolean', 'reference')),
    cardinality TEXT NOT NULL CHECK (cardinality IN ('singular', 'multiple')),
    archived INTEGER NOT NULL CHECK (archived IN (0, 1)),
    -- IS NOT NULL is 0 or 1. A NULL owner cannot make the CHECK itself NULL.
    CHECK ((owner_type_id IS NOT NULL) + (owner_entity_id IS NOT NULL) = 1)
);

CREATE TABLE assertions (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) = 36),
    subject_id TEXT NOT NULL REFERENCES entities (id),
    definition_id TEXT NOT NULL REFERENCES definitions (id),
    value_kind TEXT NOT NULL CHECK (value_kind IN ('text', 'boolean', 'reference', 'unknown')),
    text_value TEXT NULL,
    bool_value INTEGER NULL CHECK (bool_value IS NULL OR bool_value IN (0, 1)),
    target_entity_id TEXT NULL REFERENCES entities (id),
    canon TEXT NOT NULL CHECK (
        canon IN ('ACORDADO', 'PROVISIONAL', 'ABIERTO', 'MISTERIO_INTENCIONAL')
    ),
    source_status_raw TEXT NULL,
    provenance_external_ref TEXT NULL,
    provenance_locator TEXT NULL,
    provenance_fragment TEXT NULL,
    provenance_source_revision TEXT NULL,
    scope_note TEXT NULL,
    archived INTEGER NOT NULL CHECK (archived IN (0, 1)),
    CHECK (
        CASE value_kind
            WHEN 'text' THEN
                (text_value IS NOT NULL)
                AND (bool_value IS NULL)
                AND (target_entity_id IS NULL)
            WHEN 'boolean' THEN
                (text_value IS NULL)
                AND (bool_value IS NOT NULL)
                AND (target_entity_id IS NULL)
            WHEN 'reference' THEN
                (text_value IS NULL)
                AND (bool_value IS NULL)
                AND (target_entity_id IS NOT NULL)
            WHEN 'unknown' THEN
                (text_value IS NULL)
                AND (bool_value IS NULL)
                AND (target_entity_id IS NULL)
            ELSE 0
        END = 1
    )
);

CREATE INDEX idx_assertions_subject_definition ON assertions (subject_id, definition_id);

CREATE INDEX idx_assertions_target ON assertions (target_entity_id);
