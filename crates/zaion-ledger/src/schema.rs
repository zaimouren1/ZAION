/// Base table creation for fresh databases.
///
/// `EventLedger::ensure_schema` still adds chain columns when an existing
/// legacy `events` table predates them.
pub const CREATE_TABLES_BASE: &str = r#"
CREATE TABLE IF NOT EXISTS events (
    event_id       TEXT PRIMARY KEY,
    principal_id   TEXT NOT NULL,
    namespace_key  TEXT NOT NULL,
    run_id         TEXT,
    event_type     TEXT NOT NULL,
    payload_json   TEXT NOT NULL,
    signature_hex  TEXT,
    created_at     TEXT NOT NULL,
    parent_event_id TEXT,
    seq_num        INTEGER NOT NULL DEFAULT 0,
    prev_hash      TEXT NOT NULL DEFAULT
        '0000000000000000000000000000000000000000000000000000000000000000'
);
CREATE INDEX IF NOT EXISTS idx_events_namespace  ON events(namespace_key);
CREATE INDEX IF NOT EXISTS idx_events_type       ON events(event_type);
CREATE INDEX IF NOT EXISTS idx_events_principal  ON events(principal_id);

CREATE TABLE IF NOT EXISTS ledger_chain_migrations (
    migration_id  TEXT PRIMARY KEY,
    migration_kind TEXT NOT NULL,
    before_hash   TEXT NOT NULL,
    after_hash    TEXT NOT NULL,
    event_count   INTEGER NOT NULL,
    applied_at    TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS ledger_schema_migrations (
    migration_id TEXT PRIMARY KEY,
    applied_at   TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS blobs (
    hash        TEXT PRIMARY KEY,
    data        BLOB NOT NULL,
    size        INTEGER NOT NULL,
    created_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS checkpoints (
    checkpoint_id   TEXT PRIMARY KEY,
    principal_id    TEXT NOT NULL,
    namespace_key   TEXT NOT NULL,
    layer           INTEGER NOT NULL,
    summary_json    TEXT NOT NULL,
    event_cursor    TEXT NOT NULL,
    created_at      TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_checkpoints_namespace ON checkpoints(namespace_key);
"#;

/// Keep CREATE_TABLES as an alias for backward compatibility with external users.
pub const CREATE_TABLES: &str = CREATE_TABLES_BASE;
