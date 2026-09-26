-- PROPOSED DERIVED INDEX ONLY. Open exactly ':memory:' before executing.
-- Never open a plaintext file-backed database for personal Notes content.
PRAGMA foreign_keys = ON;
PRAGMA temp_store = MEMORY;
PRAGMA journal_mode = MEMORY;

CREATE TABLE note_document (
    row_id INTEGER PRIMARY KEY,
    note_id TEXT NOT NULL CHECK(length(note_id) = 32),
    home_id TEXT NOT NULL,
    scope TEXT NOT NULL CHECK(scope IN ('folder', 'personal')),
    locator TEXT NOT NULL,
    content_sha256 TEXT NOT NULL CHECK(length(content_sha256) = 64),
    document_revision INTEGER NOT NULL CHECK(document_revision >= 1),
    generation INTEGER NOT NULL CHECK(generation >= 0),
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    tags_text TEXT NOT NULL DEFAULT '',
    source_text TEXT NOT NULL DEFAULT '',
    modified_ms INTEGER NOT NULL,
    deleted INTEGER NOT NULL DEFAULT 0 CHECK(deleted IN (0, 1)),
    -- Explicit inbox state: authored note or named-destination capture = 1;
    -- quick capture without a named destination = 0, until Keep as note.
    filed INTEGER NOT NULL DEFAULT 1 CHECK(filed IN (0, 1)),
    index_complete INTEGER NOT NULL DEFAULT 1 CHECK(index_complete IN (0, 1)),
    UNIQUE(home_id, note_id),
    UNIQUE(home_id, locator)
);
CREATE INDEX note_scope_recent ON note_document(scope, deleted, modified_ms DESC);

CREATE TABLE note_source (
    source_id TEXT NOT NULL CHECK(length(source_id) = 32),
    home_id TEXT NOT NULL,
    note_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK(kind IN ('terminal', 'web', 'file', 'reading', 'note')),
    target_note_id TEXT,
    target_home_id TEXT,
    browser_profile_id TEXT,
    browser_container_id TEXT,
    snapshot_sha256 TEXT CHECK(snapshot_sha256 IS NULL OR length(snapshot_sha256) = 64),
    source_json TEXT NOT NULL,
    label TEXT NOT NULL,
    captured_ms INTEGER NOT NULL,
    PRIMARY KEY(home_id, note_id, source_id),
    FOREIGN KEY(home_id, note_id) REFERENCES note_document(home_id, note_id) ON DELETE CASCADE
);
CREATE INDEX source_owner ON note_source(home_id, note_id);
CREATE INDEX note_backlinks ON note_source(target_home_id, target_note_id) WHERE kind = 'note';
CREATE INDEX pinned_reading_snapshots ON note_source(snapshot_sha256) WHERE kind = 'reading';

CREATE TABLE note_tag (
    home_id TEXT NOT NULL,
    note_id TEXT NOT NULL,
    tag TEXT NOT NULL,
    normalized_tag TEXT NOT NULL,
    PRIMARY KEY(home_id, note_id, normalized_tag),
    FOREIGN KEY(home_id, note_id) REFERENCES note_document(home_id, note_id) ON DELETE CASCADE
);
CREATE INDEX tag_filter ON note_tag(normalized_tag);

-- FTS tokenization discards punctuation: preserve code-like values separately.
-- Populate with NFC-normalized complete flags/paths/URLs, preserving case,
-- punctuation, URL query and fragment. Binding '-n' never matches '--n'.
CREATE TABLE note_literal (
    home_id TEXT NOT NULL,
    note_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK(kind IN ('flag', 'path', 'url', 'code')),
    value TEXT NOT NULL,
    normalized_value TEXT NOT NULL,
    PRIMARY KEY(home_id, note_id, kind, normalized_value),
    FOREIGN KEY(home_id, note_id) REFERENCES note_document(home_id, note_id) ON DELETE CASCADE
);
CREATE INDEX literal_match ON note_literal(kind, normalized_value);

-- An external-content index keeps one body copy in the ordinary table.
-- Its triggers must be created before importing rows or followed by 'rebuild'.
CREATE VIRTUAL TABLE note_fts USING fts5(
    title, body, tags_text, source_text,
    content = 'note_document', content_rowid = 'row_id',
    tokenize = 'unicode61 remove_diacritics 2', prefix = '2 3 4'
);

CREATE TRIGGER note_insert AFTER INSERT ON note_document BEGIN
    INSERT INTO note_fts(rowid, title, body, tags_text, source_text)
    VALUES (new.row_id, new.title, new.body, new.tags_text, new.source_text);
END;
CREATE TRIGGER note_delete AFTER DELETE ON note_document BEGIN
    INSERT INTO note_fts(note_fts, rowid, title, body, tags_text, source_text)
    VALUES ('delete', old.row_id, old.title, old.body, old.tags_text, old.source_text);
END;
CREATE TRIGGER note_update AFTER UPDATE ON note_document BEGIN
    INSERT INTO note_fts(note_fts, rowid, title, body, tags_text, source_text)
    VALUES ('delete', old.row_id, old.title, old.body, old.tags_text, old.source_text);
    INSERT INTO note_fts(rowid, title, body, tags_text, source_text)
    VALUES (new.row_id, new.title, new.body, new.tags_text, new.source_text);
END;

-- Application queries bind values. They also filter deleted=0 and the
-- authorized scope; private index rows are removed when the vault locks.
-- SELECT d.home_id, d.note_id, d.content_sha256,
--        snippet(note_fts, 1, '[', ']', '…', 24) AS excerpt
-- FROM note_fts JOIN note_document d ON d.row_id = note_fts.rowid
-- WHERE note_fts MATCH :quoted_terms AND d.deleted = 0
-- ORDER BY bm25(note_fts, 8.0, 1.0, 4.0, 2.0), d.modified_ms DESC, d.home_id, d.note_id
-- LIMIT :limit;

-- nus: exact source identities for "Notes here" (a file in a project, a
-- page's normalized address, a command in a project). Derived like the
-- rest; removed with its note.
CREATE TABLE note_lookup (
    home_id TEXT NOT NULL,
    note_id TEXT NOT NULL,
    lookup TEXT NOT NULL,
    reason TEXT NOT NULL,
    PRIMARY KEY(home_id, note_id, lookup),
    FOREIGN KEY(home_id, note_id) REFERENCES note_document(home_id, note_id) ON DELETE CASCADE
);
CREATE INDEX lookup_match ON note_lookup(lookup);
