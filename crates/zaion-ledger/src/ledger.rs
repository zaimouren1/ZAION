use crate::LedgerError;
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use uuid::Uuid;
use zaion_crypto::{verify_signature, ZaionKeypair};
use zaion_types::{
    event::{EventId, EventType, LedgerEvent},
    identity::{PrincipalId, PublicKeyBytes, SignatureBytes},
    session::{NamespaceKey, RunId, SessionKey},
};

const GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const UNIQUE_SEQUENCE_INDEX: &str = "ux_events_principal_seq";
const FTS_REBUILD_MIGRATION: &str = "events_fts_rebuild_v1";
const MAX_IDEMPOTENCY_KEY_BYTES: usize = 256;
const DATABASE_IDENTITY_MIGRATION: &str = "ledger_database_instance_identity_v1";
const DATABASE_IDENTITY_TABLE: &str = "ledger_database_instance_identity_v1";
const DATABASE_IDENTITY_NO_INSERT_TRIGGER: &str = "ledger_database_instance_identity_no_insert_v1";
const DATABASE_IDENTITY_NO_UPDATE_TRIGGER: &str = "ledger_database_instance_identity_no_update_v1";
const DATABASE_IDENTITY_NO_DELETE_TRIGGER: &str = "ledger_database_instance_identity_no_delete_v1";
const CREATE_DATABASE_IDENTITY_TABLE: &str = r#"
CREATE TABLE ledger_database_instance_identity_v1 (
    singleton   INTEGER PRIMARY KEY CHECK (singleton = 1),
    instance_id TEXT NOT NULL UNIQUE
) WITHOUT ROWID;
"#;
const CREATE_DATABASE_IDENTITY_NO_INSERT_TRIGGER: &str = r#"
CREATE TRIGGER ledger_database_instance_identity_no_insert_v1
BEFORE INSERT ON ledger_database_instance_identity_v1
BEGIN
    SELECT RAISE(ABORT, 'ledger database instance identity is immutable');
END;
"#;
const CREATE_DATABASE_IDENTITY_NO_UPDATE_TRIGGER: &str = r#"
CREATE TRIGGER ledger_database_instance_identity_no_update_v1
BEFORE UPDATE ON ledger_database_instance_identity_v1
BEGIN
    SELECT RAISE(ABORT, 'ledger database instance identity is immutable');
END;
"#;
const CREATE_DATABASE_IDENTITY_NO_DELETE_TRIGGER: &str = r#"
CREATE TRIGGER ledger_database_instance_identity_no_delete_v1
BEFORE DELETE ON ledger_database_instance_identity_v1
BEGIN
    SELECT RAISE(ABORT, 'ledger database instance identity is immutable');
END;
"#;

/// Event-sourced ledger backed by SQLite.
///
/// Holds a single `Mutex<Option<Connection>>` that is lazily opened on first
/// use and shared across every method on the instance. This eliminates the
/// prior TOCTOU race where each method re-opened a new connection (H1).
///
/// Table creation is guarded by `tables_ensured` so the schema + migration
/// batch only runs once per instance (H18).
pub struct EventLedger {
    db_path: std::path::PathBuf,
    conn: Mutex<Option<Connection>>,
    tables_ensured: AtomicBool,
    database_instance_id: OnceLock<String>,
}

pub struct ChainVerifyResult {
    pub total: usize,
    pub verified: usize,
    /// seq_num of the first broken link, if any
    pub broken_at: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventSignatureMode {
    /// Signature covers the v2 event envelope.
    CanonicalEnvelope,
    /// Legacy signature only covers `payload.to_string()`.
    LegacyPayloadOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventInsertDisposition {
    Inserted,
    Existing,
}

impl EventLedger {
    pub fn new(db_path: impl AsRef<Path>) -> Self {
        Self {
            db_path: db_path.as_ref().to_path_buf(),
            conn: Mutex::new(None),
            tables_ensured: AtomicBool::new(false),
            database_instance_id: OnceLock::new(),
        }
    }

    pub fn db_path(&self) -> &std::path::Path {
        &self.db_path
    }

    /// Explicit schema initialization.
    ///
    /// Idempotent: after the first successful invocation `tables_ensured`
    /// is set and subsequent calls short-circuit without touching the DB.
    pub fn ensure(&self) -> Result<(), LedgerError> {
        self.with_conn(|_| Ok(()))
    }

    /// Open the backing SQLite connection with the tuned PRAGMA set.
    ///
    /// Called at most once per `EventLedger` instance, from `with_conn`
    /// under the connection mutex.
    fn open_connection(path: &Path) -> Result<Connection, LedgerError> {
        if let Some(parent) = path.parent().filter(|_| !is_sqlite_uri_path(path)) {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; \
             PRAGMA synchronous=FULL; \
             PRAGMA cache_size=-64000; \
             PRAGMA temp_store=MEMORY; \
             PRAGMA mmap_size=268435456; \
             PRAGMA page_size=4096;",
        )?;
        Ok(conn)
    }

    /// Create tables + apply migrations. Runs at most once per instance.
    fn ensure_schema(conn: &mut Connection) -> Result<(), LedgerError> {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        // Step 1: create base tables (no seq_num column — safe for both new and old DBs)
        tx.execute_batch(crate::schema::CREATE_TABLES_BASE)?;
        ensure_database_instance_identity(&tx)?;
        // Step 2: migrate — add seq_num + prev_hash if missing (databases pre-2026-04-07)
        let has_seq: bool = tx
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('events') WHERE name='seq_num'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0)
            > 0;
        if !has_seq {
            tx.execute_batch("ALTER TABLE events ADD COLUMN seq_num INTEGER NOT NULL DEFAULT 0;")?;
        }
        let has_prev_hash: bool = tx
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('events') WHERE name='prev_hash'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0)
            > 0;
        if !has_prev_hash {
            tx.execute_batch(
                "ALTER TABLE events ADD COLUMN prev_hash TEXT NOT NULL DEFAULT \
                 '0000000000000000000000000000000000000000000000000000000000000000';",
            )?;
        }
        // Step 2b: migrate — add parent_event_id if missing (databases pre-2026-04-22)
        let has_parent: bool = tx
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('events') WHERE name='parent_event_id'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0)
            > 0;
        if !has_parent {
            tx.execute_batch("ALTER TABLE events ADD COLUMN parent_event_id TEXT;")?;
        }
        let sequence_index_is_valid = sequence_index_has_expected_shape(&tx)?;
        migrate_or_validate_event_chain(&tx, !has_seq || !has_prev_hash)?;
        if !sequence_index_is_valid {
            tx.execute_batch(&format!("DROP INDEX IF EXISTS {UNIQUE_SEQUENCE_INDEX};"))?;
            tx.execute_batch(&format!(
                "CREATE UNIQUE INDEX {UNIQUE_SEQUENCE_INDEX} \
                 ON events(principal_id, seq_num);"
            ))?;
        }
        // Step 3: ensure seq index exists (safe to run even if already present)
        tx.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_events_seq ON events(principal_id, seq_num); \
             CREATE INDEX IF NOT EXISTS idx_events_parent ON events(parent_event_id); \
             CREATE INDEX IF NOT EXISTS idx_events_namespace_type_seq \
                ON events(namespace_key, event_type, seq_num DESC);",
        )?;
        // Step 4: FTS5 full-text search virtual table over payload_json + event_type
        // The FTS5 table is kept in sync via triggers (insert-only append model).
        tx.execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS events_fts USING fts5(
                event_id UNINDEXED,
                event_type,
                payload_json,
                content='events',
                content_rowid='rowid'
            );
            -- Trigger to keep FTS in sync on INSERT
            CREATE TRIGGER IF NOT EXISTS events_fts_insert AFTER INSERT ON events BEGIN
                INSERT INTO events_fts(rowid, event_id, event_type, payload_json)
                    VALUES (new.rowid, new.event_id, new.event_type, new.payload_json);
            END;",
        )?;
        let fts_rebuild_applied = tx
            .query_row(
                "SELECT 1 FROM ledger_schema_migrations WHERE migration_id = ?1",
                params![FTS_REBUILD_MIGRATION],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !fts_rebuild_applied {
            tx.execute("INSERT INTO events_fts(events_fts) VALUES('rebuild')", [])?;
            tx.execute(
                "INSERT INTO ledger_schema_migrations (migration_id, applied_at) VALUES (?1, ?2)",
                params![FTS_REBUILD_MIGRATION, chrono::Utc::now().to_rfc3339()],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Run `f` with exclusive access to the backing `Connection`.
    ///
    /// Lazily opens the connection on first invocation and ensures the
    /// schema is up-to-date. The mutex guard is held for the entire closure,
    /// which closes the TOCTOU window between read and write.
    pub(crate) fn with_conn<F, R>(&self, f: F) -> Result<R, LedgerError>
    where
        F: FnOnce(&mut Connection) -> Result<R, LedgerError>,
    {
        let mut guard = self.conn.lock().map_err(|_| {
            LedgerError::Io(std::io::Error::other(
                "event ledger connection mutex poisoned",
            ))
        })?;
        if guard.is_none() {
            *guard = Some(Self::open_connection(&self.db_path)?);
        }
        let conn = guard.as_mut().expect("connection initialized above");
        if !self.tables_ensured.load(Ordering::Acquire) {
            Self::ensure_schema(conn)?;
            self.tables_ensured.store(true, Ordering::Release);
        }
        let live_instance_id = read_database_instance_id(conn)?;
        match self.database_instance_id.get() {
            Some(expected) if expected != &live_instance_id => {
                return Err(LedgerError::DatabaseInstanceIdentityDrift {
                    expected: expected.clone(),
                    actual: live_instance_id,
                });
            }
            Some(_) => {}
            None => {
                self.database_instance_id
                    .set(live_instance_id)
                    .map_err(|actual| LedgerError::DatabaseInstanceIdentityDrift {
                        expected: self.database_instance_id.get().cloned().unwrap_or_default(),
                        actual,
                    })?;
            }
        }
        f(conn)
    }

    /// Return the persisted identity of SQLite's live `main` database.
    ///
    /// The UUID is created transactionally for fresh and legacy ledgers. Once
    /// observed by this `EventLedger`, any row drift fails every subsequent
    /// operation closed.
    pub fn database_instance_id(&self) -> Result<String, LedgerError> {
        self.with_conn(|_| {
            self.database_instance_id.get().cloned().ok_or_else(|| {
                LedgerError::InvalidDatabaseInstanceIdentity(
                    "live database identity was not initialized".to_string(),
                )
            })
        })
    }

    pub fn append_event(
        &self,
        principal_id: &PrincipalId,
        namespace_key: &NamespaceKey,
        event_type: &str,
        payload: serde_json::Value,
        run_id: Option<&RunId>,
        signature: Option<&SignatureBytes>,
    ) -> Result<EventId, LedgerError> {
        self.append_event_with_parent(
            principal_id,
            namespace_key,
            event_type,
            payload,
            run_id,
            signature,
            None,
        )
    }

    pub fn append_typed_event(
        &self,
        principal_id: &PrincipalId,
        namespace_key: &NamespaceKey,
        event_type: EventType,
        payload: serde_json::Value,
        run_id: Option<&RunId>,
        signature: Option<&SignatureBytes>,
    ) -> Result<EventId, LedgerError> {
        self.append_event(
            principal_id,
            namespace_key,
            event_type.as_str(),
            payload,
            run_id,
            signature,
        )
    }

    pub fn append_signed_event(
        &self,
        keypair: &ZaionKeypair,
        namespace_key: &NamespaceKey,
        event_type: &str,
        payload: serde_json::Value,
        run_id: Option<&RunId>,
    ) -> Result<EventId, LedgerError> {
        self.append_signed_event_with_parent(
            keypair,
            namespace_key,
            event_type,
            payload,
            run_id,
            None,
        )
    }

    pub fn append_signed_typed_event(
        &self,
        keypair: &ZaionKeypair,
        namespace_key: &NamespaceKey,
        event_type: EventType,
        payload: serde_json::Value,
        run_id: Option<&RunId>,
    ) -> Result<EventId, LedgerError> {
        self.append_signed_event(keypair, namespace_key, event_type.as_str(), payload, run_id)
    }

    pub fn append_signed_event_with_parent(
        &self,
        keypair: &ZaionKeypair,
        namespace_key: &NamespaceKey,
        event_type: &str,
        payload: serde_json::Value,
        run_id: Option<&RunId>,
        parent_event_id: Option<&EventId>,
    ) -> Result<EventId, LedgerError> {
        let principal_id = keypair.principal_id();
        let sig = sign_event_envelope(
            keypair,
            &principal_id,
            namespace_key,
            run_id,
            event_type,
            &payload,
            parent_event_id,
        );
        self.append_event_with_parent(
            &principal_id,
            namespace_key,
            event_type,
            payload,
            run_id,
            Some(&sig),
            parent_event_id,
        )
    }

    pub fn append_signed_typed_event_with_parent(
        &self,
        keypair: &ZaionKeypair,
        namespace_key: &NamespaceKey,
        event_type: EventType,
        payload: serde_json::Value,
        run_id: Option<&RunId>,
        parent_event_id: Option<&EventId>,
    ) -> Result<EventId, LedgerError> {
        self.append_signed_event_with_parent(
            keypair,
            namespace_key,
            event_type.as_str(),
            payload,
            run_id,
            parent_event_id,
        )
    }

    /// Append one signed event for a caller-owned idempotency key.
    ///
    /// A retry with the same key and immutable event content returns the first
    /// event ID without appending another chain entry. Reusing the key for
    /// different content fails closed with [`LedgerError::EventIdConflict`].
    pub fn append_signed_idempotent_event(
        &self,
        keypair: &ZaionKeypair,
        namespace_key: &NamespaceKey,
        event_type: &str,
        payload: serde_json::Value,
        run_id: Option<&RunId>,
        idempotency_key: &str,
    ) -> Result<EventId, LedgerError> {
        self.append_signed_idempotent_event_with_parent(
            keypair,
            namespace_key,
            event_type,
            payload,
            run_id,
            None,
            idempotency_key,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn append_signed_idempotent_event_with_parent(
        &self,
        keypair: &ZaionKeypair,
        namespace_key: &NamespaceKey,
        event_type: &str,
        payload: serde_json::Value,
        run_id: Option<&RunId>,
        parent_event_id: Option<&EventId>,
        idempotency_key: &str,
    ) -> Result<EventId, LedgerError> {
        validate_idempotency_key(idempotency_key)?;
        let principal_id = keypair.principal_id();
        let event_id = deterministic_idempotent_event_id(&principal_id, idempotency_key);
        let signature = sign_event_envelope(
            keypair,
            &principal_id,
            namespace_key,
            run_id,
            event_type,
            &payload,
            parent_event_id,
        );
        let created_at = chrono::Utc::now().to_rfc3339();
        let payload_json = serde_json::to_string(&payload)?;
        let signature_hex = hex::encode(&signature.0);

        self.with_conn(|conn| {
            append_prepared_event(
                conn,
                PreparedEvent {
                    event_id: &event_id.0,
                    principal_id: principal_id.as_str(),
                    namespace_key: &namespace_key.0,
                    run_id: run_id.map(|run| run.0.as_str()),
                    event_type,
                    payload_json: &payload_json,
                    signature_hex: Some(&signature_hex),
                    created_at: &created_at,
                    parent_event_id: parent_event_id.map(|parent| parent.0.as_str()),
                },
                ExistingEventPolicy::MatchIgnoringCreatedAt,
            )?;
            Ok(event_id.clone())
        })
    }

    /// Append an event with an optional parent_event_id for DAG lineage.
    #[allow(clippy::too_many_arguments)]
    pub fn append_event_with_parent(
        &self,
        principal_id: &PrincipalId,
        namespace_key: &NamespaceKey,
        event_type: &str,
        payload: serde_json::Value,
        run_id: Option<&RunId>,
        signature: Option<&SignatureBytes>,
        parent_event_id: Option<&EventId>,
    ) -> Result<EventId, LedgerError> {
        let event_id = EventId(format!("evt-{}", uuid::Uuid::new_v4()));
        let created_at = chrono::Utc::now().to_rfc3339();
        let payload_json = serde_json::to_string(&payload)?;
        let sig_hex = signature.map(|s| hex::encode(&s.0));
        let parent_id_str = parent_event_id.map(|p| p.0.clone());

        self.with_conn(|conn| {
            append_prepared_event(
                conn,
                PreparedEvent {
                    event_id: &event_id.0,
                    principal_id: principal_id.as_str(),
                    namespace_key: &namespace_key.0,
                    run_id: run_id.map(|run| run.0.as_str()),
                    event_type,
                    payload_json: &payload_json,
                    signature_hex: sig_hex.as_deref(),
                    created_at: &created_at,
                    parent_event_id: parent_id_str.as_deref(),
                },
                ExistingEventPolicy::Reject,
            )?;
            Ok(event_id.clone())
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn append_typed_event_with_parent(
        &self,
        principal_id: &PrincipalId,
        namespace_key: &NamespaceKey,
        event_type: EventType,
        payload: serde_json::Value,
        run_id: Option<&RunId>,
        signature: Option<&SignatureBytes>,
        parent_event_id: Option<&EventId>,
    ) -> Result<EventId, LedgerError> {
        self.append_event_with_parent(
            principal_id,
            namespace_key,
            event_type.as_str(),
            payload,
            run_id,
            signature,
            parent_event_id,
        )
    }

    /// Insert an event using a pre-assigned `event_id` and `created_at`.
    ///
    /// Used exclusively by `zaion-sync` to replay foreign events verbatim,
    /// preserving original identifiers across devices.  The chain hash is
    /// recomputed based on the destination ledger's current tail, so the
    /// imported events form a valid local chain.
    #[allow(clippy::too_many_arguments)]
    pub fn insert_event_with_id(
        &self,
        event_id: &EventId,
        principal_id: &PrincipalId,
        namespace_key: &NamespaceKey,
        event_type: &str,
        payload: serde_json::Value,
        run_id: Option<&RunId>,
        signature: Option<&SignatureBytes>,
        created_at: &str,
    ) -> Result<(), LedgerError> {
        self.insert_event_with_id_and_parent(
            event_id,
            principal_id,
            namespace_key,
            event_type,
            payload,
            run_id,
            signature,
            created_at,
            None,
        )
    }

    /// Insert an event using a pre-assigned `event_id`, `created_at`, and optional `parent_event_id`.
    ///
    /// Exact retries are accepted. Reusing an event ID with different immutable
    /// content fails closed.
    #[allow(clippy::too_many_arguments)]
    pub fn insert_event_with_id_and_parent(
        &self,
        event_id: &EventId,
        principal_id: &PrincipalId,
        namespace_key: &NamespaceKey,
        event_type: &str,
        payload: serde_json::Value,
        run_id: Option<&RunId>,
        signature: Option<&SignatureBytes>,
        created_at: &str,
        parent_event_id: Option<&EventId>,
    ) -> Result<(), LedgerError> {
        self.insert_event_with_id_and_parent_disposition(
            event_id,
            principal_id,
            namespace_key,
            event_type,
            payload,
            run_id,
            signature,
            created_at,
            parent_event_id,
        )
        .map(|_| ())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn insert_event_with_id_and_parent_disposition(
        &self,
        event_id: &EventId,
        principal_id: &PrincipalId,
        namespace_key: &NamespaceKey,
        event_type: &str,
        payload: serde_json::Value,
        run_id: Option<&RunId>,
        signature: Option<&SignatureBytes>,
        created_at: &str,
        parent_event_id: Option<&EventId>,
    ) -> Result<EventInsertDisposition, LedgerError> {
        let payload_json = serde_json::to_string(&payload)?;
        let sig_hex = signature.map(|s| hex::encode(&s.0));
        let parent_id_str = parent_event_id.map(|p| p.0.clone());

        self.with_conn(|conn| {
            let inserted = append_prepared_event(
                conn,
                PreparedEvent {
                    event_id: &event_id.0,
                    principal_id: principal_id.as_str(),
                    namespace_key: &namespace_key.0,
                    run_id: run_id.map(|run| run.0.as_str()),
                    event_type,
                    payload_json: &payload_json,
                    signature_hex: sig_hex.as_deref(),
                    created_at,
                    parent_event_id: parent_id_str.as_deref(),
                },
                ExistingEventPolicy::MatchAll,
            )?;
            Ok(if inserted {
                EventInsertDisposition::Inserted
            } else {
                EventInsertDisposition::Existing
            })
        })
    }

    /// Insert a batch of preassigned events in one SQLite transaction.
    ///
    /// Exact existing events return [`EventInsertDisposition::Existing`]. If
    /// any event conflicts or cannot be inserted, every new event in the batch
    /// is rolled back.
    pub fn insert_events_with_ids_atomic(
        &self,
        events: &[LedgerEvent],
    ) -> Result<Vec<EventInsertDisposition>, LedgerError> {
        self.with_conn(|conn| {
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let mut dispositions = Vec::with_capacity(events.len());
            for event in events {
                let payload_json = serde_json::to_string(&event.payload)?;
                let signature_hex = event
                    .signature
                    .as_ref()
                    .map(|signature| hex::encode(&signature.0));
                let inserted = append_prepared_event_in_transaction(
                    &tx,
                    PreparedEvent {
                        event_id: &event.event_id.0,
                        principal_id: event.principal_id.as_str(),
                        namespace_key: &event.namespace_key.0,
                        run_id: event.run_id.as_ref().map(|run| run.0.as_str()),
                        event_type: &event.event_type,
                        payload_json: &payload_json,
                        signature_hex: signature_hex.as_deref(),
                        created_at: &event.created_at,
                        parent_event_id: event
                            .parent_event_id
                            .as_ref()
                            .map(|parent| parent.0.as_str()),
                    },
                    ExistingEventPolicy::MatchAll,
                )?;
                dispositions.push(if inserted {
                    EventInsertDisposition::Inserted
                } else {
                    EventInsertDisposition::Existing
                });
            }
            tx.commit()?;
            Ok(dispositions)
        })
    }

    /// Walk all events for principal in seq_num order, recompute each prev_hash link.
    /// Returns a ChainVerifyResult describing chain integrity.
    pub fn verify_chain(
        &self,
        principal_id: &PrincipalId,
    ) -> Result<ChainVerifyResult, LedgerError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT event_id, event_type, payload_json, created_at, seq_num, prev_hash \
                 FROM events WHERE principal_id=?1 ORDER BY seq_num ASC",
            )?;
            let rows: Vec<(String, String, String, String, i64, String)> = stmt
                .query_map(params![principal_id.as_str()], |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                    ))
                })?
                .collect::<Result<_, _>>()?;

            let total = rows.len();
            let mut verified = 0;
            let mut expected_prev = GENESIS_HASH.to_string();
            let mut expected_seq = 0i64;

            for (eid, etype, payload, created_at, seq, prev_hash) in &rows {
                if *seq != expected_seq || prev_hash != &expected_prev {
                    return Ok(ChainVerifyResult {
                        total,
                        verified,
                        broken_at: Some(*seq),
                    });
                }
                verified += 1;
                expected_prev =
                    event_hash(eid, principal_id.as_str(), etype, payload, created_at, *seq);
                expected_seq = expected_seq.checked_add(1).ok_or_else(|| {
                    LedgerError::CorruptChain("event sequence exhausted i64".to_string())
                })?;
            }

            Ok(ChainVerifyResult {
                total,
                verified,
                broken_at: None,
            })
        })
    }

    pub fn list_events(
        &self,
        session_key: &SessionKey,
        event_type: Option<&str>,
        limit: usize,
    ) -> Result<Vec<LedgerEvent>, LedgerError> {
        self.with_conn(|conn| {
            let mut query = "SELECT event_id, principal_id, namespace_key, run_id, event_type, payload_json, signature_hex, created_at, parent_event_id FROM events WHERE namespace_key = ?1".to_string();
            if event_type.is_some() {
                query.push_str(" AND event_type = ?2 ORDER BY seq_num DESC LIMIT ?3");
            } else {
                query.push_str(" ORDER BY seq_num DESC LIMIT ?2");
            }
            let mut stmt = conn.prepare(&query)?;
            let rows: Vec<LedgerEvent> = if let Some(et) = event_type {
                stmt.query_map(params![session_key.0, et, limit as i64], row_to_event)?
                    .collect::<Result<Vec<_>, _>>()?
            } else {
                stmt.query_map(params![session_key.0, limit as i64], row_to_event)?
                    .collect::<Result<Vec<_>, _>>()?
            };
            Ok(rows)
        })
    }

    pub fn list_typed_events(
        &self,
        session_key: &SessionKey,
        event_type: Option<&EventType>,
        limit: usize,
    ) -> Result<Vec<LedgerEvent>, LedgerError> {
        self.list_events(session_key, event_type.map(EventType::as_str), limit)
    }

    /// List newest events whose JSON payload contains an exact string field match.
    ///
    /// This intentionally parses payload JSON in Rust rather than relying on
    /// SQLite JSON1 being available in every embedded build. SQL still narrows
    /// the candidate set to the indexed `(namespace_key, event_type)` slice.
    pub fn list_events_by_payload_string(
        &self,
        session_key: &SessionKey,
        event_type: &str,
        payload_key: &str,
        payload_value: &str,
        limit: usize,
    ) -> Result<Vec<LedgerEvent>, LedgerError> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT event_id, principal_id, namespace_key, run_id, event_type, payload_json, \
                        signature_hex, created_at, parent_event_id \
                 FROM events \
                 WHERE namespace_key = ?1 AND event_type = ?2 \
                 ORDER BY seq_num DESC",
            )?;
            let rows = stmt.query_map(params![session_key.0, event_type], row_to_event)?;
            let mut matches = Vec::new();
            for row in rows {
                let event = row?;
                if event
                    .payload
                    .get(payload_key)
                    .and_then(|value| value.as_str())
                    == Some(payload_value)
                {
                    matches.push(event);
                    if matches.len() >= limit {
                        break;
                    }
                }
            }
            Ok(matches)
        })
    }

    /// List newest events whose JSON payload contains `payload_value` inside a
    /// top-level string array field.
    ///
    /// This mirrors `list_events_by_payload_string(...)` for array membership
    /// lookups such as `tool_receipt_ids`, while still avoiding any dependency
    /// on SQLite JSON1 availability.
    pub fn list_events_by_payload_string_array_contains(
        &self,
        session_key: &SessionKey,
        event_type: &str,
        payload_key: &str,
        payload_value: &str,
        limit: usize,
    ) -> Result<Vec<LedgerEvent>, LedgerError> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT event_id, principal_id, namespace_key, run_id, event_type, payload_json, \
                        signature_hex, created_at, parent_event_id \
                 FROM events \
                 WHERE namespace_key = ?1 AND event_type = ?2 \
                 ORDER BY seq_num DESC",
            )?;
            let rows = stmt.query_map(params![session_key.0, event_type], row_to_event)?;
            let mut matches = Vec::new();
            for row in rows {
                let event = row?;
                let contains_value = event
                    .payload
                    .get(payload_key)
                    .and_then(|value| value.as_array())
                    .is_some_and(|values| {
                        values
                            .iter()
                            .any(|value| value.as_str() == Some(payload_value))
                    });
                if contains_value {
                    matches.push(event);
                    if matches.len() >= limit {
                        break;
                    }
                }
            }
            Ok(matches)
        })
    }

    pub fn list_global_events(&self, limit: usize) -> Result<Vec<LedgerEvent>, LedgerError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT event_id, principal_id, namespace_key, run_id, event_type, payload_json, signature_hex, created_at, parent_event_id FROM events ORDER BY seq_num DESC LIMIT ?1",
            )?;
            let rows = stmt.query_map(params![limit as i64], row_to_event)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    pub fn get_event(&self, event_id: &str) -> Result<Option<LedgerEvent>, LedgerError> {
        self.with_conn(|conn| get_event_from_connection(conn, event_id))
    }

    pub fn list_principal_events(
        &self,
        principal_id: &PrincipalId,
        limit: usize,
    ) -> Result<Vec<LedgerEvent>, LedgerError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT event_id, principal_id, namespace_key, run_id, event_type, payload_json, signature_hex, created_at, parent_event_id \
                 FROM events WHERE principal_id = ?1 ORDER BY seq_num DESC LIMIT ?2",
            )?;
            let rows = stmt
                .query_map(params![principal_id.as_str(), limit as i64], row_to_event)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    /// List all events for `principal_id` with seq_num >= `from_seq`, ordered ascending.
    /// Used by zaion-sync for event log tail export.
    pub fn list_events_from_seq(
        &self,
        principal_id: &PrincipalId,
        from_seq: u64,
    ) -> Result<Vec<LedgerEvent>, LedgerError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT event_id, principal_id, namespace_key, run_id, event_type, payload_json, \
                 signature_hex, created_at, parent_event_id FROM events \
                 WHERE principal_id = ?1 AND seq_num >= ?2 ORDER BY seq_num ASC",
            )?;
            let rows = stmt
                .query_map(
                    params![principal_id.as_str(), from_seq as i64],
                    row_to_event,
                )?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    /// Count events for `principal_id` and return the created_at of the latest event.
    pub fn event_stats(
        &self,
        principal_id: &PrincipalId,
    ) -> Result<(usize, Option<String>), LedgerError> {
        self.with_conn(|conn| {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM events WHERE principal_id = ?1",
                params![principal_id.as_str()],
                |r| r.get(0),
            )?;
            let last_at: Option<String> = conn
                .query_row(
                    "SELECT created_at FROM events WHERE principal_id = ?1 ORDER BY seq_num DESC LIMIT 1",
                    params![principal_id.as_str()],
                    |r| r.get(0),
                )
                .optional()?;
            Ok((count as usize, last_at))
        })
    }

    /// Check whether an event with the given `event_id` already exists in the ledger.
    /// Used by zaion-sync for idempotent import.
    pub fn event_id_exists(&self, event_id: &str) -> Result<bool, LedgerError> {
        self.with_conn(|conn| {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM events WHERE event_id = ?1",
                params![event_id],
                |r| r.get(0),
            )?;
            Ok(count > 0)
        })
    }

    /// Full-text search across all events using FTS5.
    ///
    /// `query` uses standard FTS5 query syntax (e.g. `"hello world"`, `hello AND world`,
    /// `payload:token`). Results are returned newest-first, limited to `limit`.
    ///
    /// Returns matching events from the base `events` table (full LedgerEvent).
    pub fn fts_search(
        &self,
        principal_id: &PrincipalId,
        query: &str,
        limit: usize,
    ) -> Result<Vec<LedgerEvent>, LedgerError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT e.event_id, e.principal_id, e.namespace_key, e.run_id, \
                        e.event_type, e.payload_json, e.signature_hex, e.created_at, e.parent_event_id \
                 FROM events_fts \
                 JOIN events e ON events_fts.event_id = e.event_id \
                 WHERE events_fts MATCH ?1 \
                   AND e.principal_id = ?2 \
                 ORDER BY e.created_at DESC \
                 LIMIT ?3",
            )?;
            let rows = stmt
                .query_map(params![query, principal_id.as_str(), limit as i64], row_to_event)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    /// Full-text search across all principals (unfiltered).
    /// Used by `zaion sessions search` without a specific process.
    pub fn fts_search_global(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<LedgerEvent>, LedgerError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT e.event_id, e.principal_id, e.namespace_key, e.run_id, \
                        e.event_type, e.payload_json, e.signature_hex, e.created_at, e.parent_event_id \
                 FROM events_fts \
                 JOIN events e ON events_fts.event_id = e.event_id \
                 WHERE events_fts MATCH ?1 \
                 ORDER BY e.created_at DESC \
                 LIMIT ?2",
            )?;
            let rows = stmt
                .query_map(params![query, limit as i64], row_to_event)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }
}

pub(crate) fn get_event_from_connection(
    conn: &Connection,
    event_id: &str,
) -> Result<Option<LedgerEvent>, LedgerError> {
    conn.query_row(
        "SELECT event_id, principal_id, namespace_key, run_id, event_type, payload_json, signature_hex, created_at, parent_event_id \
         FROM events WHERE event_id = ?1 LIMIT 1",
        params![event_id],
        row_to_event,
    )
    .optional()
    .map_err(LedgerError::from)
}

fn is_sqlite_uri_path(path: &Path) -> bool {
    path.as_os_str().as_encoded_bytes().starts_with(b"file:")
}

fn ensure_database_instance_identity(tx: &Transaction<'_>) -> Result<(), LedgerError> {
    let marker_exists = tx
        .query_row(
            "SELECT 1 FROM ledger_schema_migrations WHERE migration_id = ?1",
            params![DATABASE_IDENTITY_MIGRATION],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    let table_exists = database_schema_object_sql(tx, "table", DATABASE_IDENTITY_TABLE)?.is_some();
    let insert_guard_exists =
        database_schema_object_sql(tx, "trigger", DATABASE_IDENTITY_NO_INSERT_TRIGGER)?.is_some();
    let update_guard_exists =
        database_schema_object_sql(tx, "trigger", DATABASE_IDENTITY_NO_UPDATE_TRIGGER)?.is_some();
    let delete_guard_exists =
        database_schema_object_sql(tx, "trigger", DATABASE_IDENTITY_NO_DELETE_TRIGGER)?.is_some();
    let any_object_exists =
        table_exists || insert_guard_exists || update_guard_exists || delete_guard_exists;
    let every_object_exists =
        table_exists && insert_guard_exists && update_guard_exists && delete_guard_exists;

    if !marker_exists {
        if any_object_exists {
            return Err(LedgerError::InvalidDatabaseInstanceIdentity(
                "database identity objects exist without their migration marker".to_string(),
            ));
        }
        tx.execute_batch(CREATE_DATABASE_IDENTITY_TABLE)?;
        tx.execute(
            "INSERT INTO ledger_database_instance_identity_v1 (singleton, instance_id) \
             VALUES (1, ?1)",
            params![Uuid::new_v4().to_string()],
        )?;
        tx.execute_batch(CREATE_DATABASE_IDENTITY_NO_INSERT_TRIGGER)?;
        tx.execute_batch(CREATE_DATABASE_IDENTITY_NO_UPDATE_TRIGGER)?;
        tx.execute_batch(CREATE_DATABASE_IDENTITY_NO_DELETE_TRIGGER)?;
        tx.execute(
            "INSERT INTO ledger_schema_migrations (migration_id, applied_at) VALUES (?1, ?2)",
            params![DATABASE_IDENTITY_MIGRATION, chrono::Utc::now().to_rfc3339()],
        )?;
    } else if !every_object_exists {
        return Err(LedgerError::InvalidDatabaseInstanceIdentity(
            "database identity migration marker exists but a required object is missing"
                .to_string(),
        ));
    }

    validated_database_instance_id(tx).map(|_| ())
}

/// Validate the immutable Ledger identity schema on an already-open SQLite
/// connection and return its canonical logical instance UUID.
///
/// This lets another owner of the same database transaction prove that it is
/// operating on the same Ledger instance without acquiring `EventLedger`'s
/// connection mutex.
pub fn validated_database_instance_id(conn: &Connection) -> Result<String, LedgerError> {
    let marker_exists = conn
        .query_row(
            "SELECT 1 FROM ledger_schema_migrations WHERE migration_id = ?1",
            params![DATABASE_IDENTITY_MIGRATION],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !marker_exists {
        return Err(LedgerError::InvalidDatabaseInstanceIdentity(
            "database identity migration marker is missing".to_string(),
        ));
    }
    validate_database_schema_object(
        conn,
        "table",
        DATABASE_IDENTITY_TABLE,
        CREATE_DATABASE_IDENTITY_TABLE,
    )?;
    validate_database_schema_object(
        conn,
        "trigger",
        DATABASE_IDENTITY_NO_INSERT_TRIGGER,
        CREATE_DATABASE_IDENTITY_NO_INSERT_TRIGGER,
    )?;
    validate_database_schema_object(
        conn,
        "trigger",
        DATABASE_IDENTITY_NO_UPDATE_TRIGGER,
        CREATE_DATABASE_IDENTITY_NO_UPDATE_TRIGGER,
    )?;
    validate_database_schema_object(
        conn,
        "trigger",
        DATABASE_IDENTITY_NO_DELETE_TRIGGER,
        CREATE_DATABASE_IDENTITY_NO_DELETE_TRIGGER,
    )?;
    let extra_trigger = conn
        .query_row(
            "SELECT name FROM sqlite_master
             WHERE type = 'trigger' AND tbl_name = ?1
               AND name NOT IN (?2, ?3, ?4)
             ORDER BY name LIMIT 1",
            params![
                DATABASE_IDENTITY_TABLE,
                DATABASE_IDENTITY_NO_INSERT_TRIGGER,
                DATABASE_IDENTITY_NO_UPDATE_TRIGGER,
                DATABASE_IDENTITY_NO_DELETE_TRIGGER,
            ],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if let Some(name) = extra_trigger {
        return Err(LedgerError::InvalidDatabaseInstanceIdentity(format!(
            "unexpected database identity trigger is installed: {name}"
        )));
    }
    read_database_instance_id(conn)
}

fn database_schema_object_sql(
    conn: &Connection,
    object_type: &str,
    name: &str,
) -> Result<Option<String>, LedgerError> {
    conn.query_row(
        "SELECT sql FROM sqlite_master WHERE type = ?1 AND name = ?2",
        params![object_type, name],
        |row| row.get(0),
    )
    .optional()
    .map_err(Into::into)
}

fn validate_database_schema_object(
    conn: &Connection,
    object_type: &str,
    name: &str,
    expected_sql: &str,
) -> Result<(), LedgerError> {
    let actual = database_schema_object_sql(conn, object_type, name)?.ok_or_else(|| {
        LedgerError::InvalidDatabaseInstanceIdentity(format!(
            "required database identity {object_type} {name} is missing"
        ))
    })?;
    if normalize_database_schema_sql(&actual) == normalize_database_schema_sql(expected_sql) {
        Ok(())
    } else {
        Err(LedgerError::InvalidDatabaseInstanceIdentity(format!(
            "database identity {object_type} {name} has an unexpected definition"
        )))
    }
}

fn normalize_database_schema_sql(sql: &str) -> String {
    sql.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_end_matches(';')
        .to_string()
}

fn read_database_instance_id(conn: &Connection) -> Result<String, LedgerError> {
    let rows = {
        let mut statement = conn
            .prepare("SELECT singleton, instance_id FROM ledger_database_instance_identity_v1")?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    let [(singleton, instance_id)] = rows.as_slice() else {
        return Err(LedgerError::InvalidDatabaseInstanceIdentity(
            "database identity table must contain exactly one row".to_string(),
        ));
    };
    if *singleton != 1 {
        return Err(LedgerError::InvalidDatabaseInstanceIdentity(
            "database identity singleton key is invalid".to_string(),
        ));
    }
    let parsed = Uuid::parse_str(instance_id).map_err(|_| {
        LedgerError::InvalidDatabaseInstanceIdentity(
            "database identity is not a canonical UUID".to_string(),
        )
    })?;
    if parsed.to_string() != *instance_id {
        return Err(LedgerError::InvalidDatabaseInstanceIdentity(
            "database identity is not a canonical UUID".to_string(),
        ));
    }
    Ok(instance_id.clone())
}

#[derive(Clone, Copy)]
enum ExistingEventPolicy {
    Reject,
    MatchAll,
    MatchIgnoringCreatedAt,
}

struct PreparedEvent<'a> {
    event_id: &'a str,
    principal_id: &'a str,
    namespace_key: &'a str,
    run_id: Option<&'a str>,
    event_type: &'a str,
    payload_json: &'a str,
    signature_hex: Option<&'a str>,
    created_at: &'a str,
    parent_event_id: Option<&'a str>,
}

struct StoredEventIdentity {
    principal_id: String,
    namespace_key: String,
    run_id: Option<String>,
    event_type: String,
    payload_json: String,
    signature_hex: Option<String>,
    created_at: String,
    parent_event_id: Option<String>,
}

impl StoredEventIdentity {
    fn matches(&self, event: &PreparedEvent<'_>, include_created_at: bool) -> bool {
        self.principal_id == event.principal_id
            && self.namespace_key == event.namespace_key
            && self.run_id.as_deref() == event.run_id
            && self.event_type == event.event_type
            && self.payload_json == event.payload_json
            && self.signature_hex.as_deref() == event.signature_hex
            && (!include_created_at || self.created_at == event.created_at)
            && self.parent_event_id.as_deref() == event.parent_event_id
    }
}

fn append_prepared_event(
    conn: &mut Connection,
    event: PreparedEvent<'_>,
    existing_policy: ExistingEventPolicy,
) -> Result<bool, LedgerError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let inserted = append_prepared_event_in_transaction(&tx, event, existing_policy)?;
    tx.commit()?;
    Ok(inserted)
}

fn append_prepared_event_in_transaction(
    tx: &Transaction<'_>,
    event: PreparedEvent<'_>,
    existing_policy: ExistingEventPolicy,
) -> Result<bool, LedgerError> {
    let existing = tx
        .query_row(
            "SELECT principal_id, namespace_key, run_id, event_type, payload_json, \
                    signature_hex, created_at, parent_event_id \
             FROM events WHERE event_id = ?1",
            params![event.event_id],
            |row| {
                Ok(StoredEventIdentity {
                    principal_id: row.get(0)?,
                    namespace_key: row.get(1)?,
                    run_id: row.get(2)?,
                    event_type: row.get(3)?,
                    payload_json: row.get(4)?,
                    signature_hex: row.get(5)?,
                    created_at: row.get(6)?,
                    parent_event_id: row.get(7)?,
                })
            },
        )
        .optional()?;
    if let Some(existing) = existing {
        let equivalent = match existing_policy {
            ExistingEventPolicy::Reject => false,
            ExistingEventPolicy::MatchAll => existing.matches(&event, true),
            ExistingEventPolicy::MatchIgnoringCreatedAt => existing.matches(&event, false),
        };
        if !equivalent {
            return Err(LedgerError::EventIdConflict {
                event_id: event.event_id.to_string(),
            });
        }
        return Ok(false);
    }

    let (seq_num, prev_hash) = next_chain_position(tx, event.principal_id)?;
    tx.execute(
        "INSERT INTO events \
         (event_id, principal_id, namespace_key, run_id, event_type, payload_json, \
          signature_hex, created_at, seq_num, prev_hash, parent_event_id) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            event.event_id,
            event.principal_id,
            event.namespace_key,
            event.run_id,
            event.event_type,
            event.payload_json,
            event.signature_hex,
            event.created_at,
            seq_num,
            prev_hash,
            event.parent_event_id,
        ],
    )?;
    Ok(true)
}

fn sequence_index_has_expected_shape(tx: &Transaction<'_>) -> Result<bool, LedgerError> {
    let metadata = tx
        .query_row(
            "SELECT [unique], partial FROM pragma_index_list('events') WHERE name = ?1",
            params![UNIQUE_SEQUENCE_INDEX],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()?;
    if metadata != Some((1, 0)) {
        return Ok(false);
    }

    let columns = {
        let mut statement = tx.prepare(
            "SELECT name, [desc], coll
             FROM pragma_index_xinfo('ux_events_principal_seq')
             WHERE key = 1
             ORDER BY seqno",
        )?;
        let columns = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        columns
    };
    Ok(columns
        == vec![
            (
                Some("principal_id".to_string()),
                0,
                Some("BINARY".to_string()),
            ),
            (Some("seq_num".to_string()), 0, Some("BINARY".to_string())),
        ])
}

fn next_chain_position(
    tx: &Transaction<'_>,
    principal_id: &str,
) -> Result<(i64, String), LedgerError> {
    let row: Option<(String, String, String, String, i64)> = tx
        .query_row(
            "SELECT event_id, event_type, payload_json, created_at, seq_num \
             FROM events WHERE principal_id = ?1 ORDER BY seq_num DESC LIMIT 1",
            params![principal_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .optional()?;
    match row {
        None => Ok((0, GENESIS_HASH.to_string())),
        Some((event_id, event_type, payload_json, created_at, seq_num)) => {
            let next_seq = seq_num.checked_add(1).ok_or_else(|| {
                LedgerError::CorruptChain("event sequence exhausted i64".to_string())
            })?;
            Ok((
                next_seq,
                event_hash(
                    &event_id,
                    principal_id,
                    &event_type,
                    &payload_json,
                    &created_at,
                    seq_num,
                ),
            ))
        }
    }
}

struct ChainMigrationRow {
    row_id: i64,
    event_id: String,
    principal_id: String,
    event_type: String,
    payload_json: String,
    created_at: String,
}

fn migrate_or_validate_event_chain(
    tx: &Transaction<'_>,
    chain_columns_added: bool,
) -> Result<(), LedgerError> {
    match validate_chain_metadata(tx) {
        Ok(()) => Ok(()),
        Err(_) if chain_columns_added || chain_metadata_is_all_defaults(tx)? => {
            let migration_kind = if chain_columns_added {
                "legacy_chain_columns_v1"
            } else {
                "legacy_default_chain_repair_v1"
            };
            let (event_count, before_hash) = chain_metadata_digest(tx)?;
            rebuild_event_chain_metadata(tx)?;
            validate_chain_metadata(tx)?;
            let (after_count, after_hash) = chain_metadata_digest(tx)?;
            if after_count != event_count {
                return Err(LedgerError::CorruptChain(
                    "chain migration changed event cardinality".to_string(),
                ));
            }
            record_chain_migration(tx, migration_kind, &before_hash, &after_hash, event_count)
        }
        Err(error) => Err(error),
    }
}

fn chain_metadata_digest(tx: &Transaction<'_>) -> Result<(usize, String), LedgerError> {
    let rows = {
        let mut statement = tx.prepare(
            "SELECT event_id, principal_id, seq_num, prev_hash \
             FROM events ORDER BY principal_id, rowid",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    let mut hasher = Sha256::new();
    for (event_id, principal_id, seq_num, prev_hash) in &rows {
        for part in [event_id.as_str(), principal_id.as_str(), prev_hash.as_str()] {
            hasher.update((part.len() as u64).to_le_bytes());
            hasher.update(part.as_bytes());
            hasher.update([0x1f]);
        }
        hasher.update(seq_num.to_le_bytes());
    }
    Ok((
        rows.len(),
        format!("sha256:{}", hex::encode(hasher.finalize())),
    ))
}

fn record_chain_migration(
    tx: &Transaction<'_>,
    migration_kind: &str,
    before_hash: &str,
    after_hash: &str,
    event_count: usize,
) -> Result<(), LedgerError> {
    let mut hasher = Sha256::new();
    for part in [migration_kind, before_hash, after_hash] {
        hasher.update((part.len() as u64).to_le_bytes());
        hasher.update(part.as_bytes());
        hasher.update([0x1f]);
    }
    let migration_id = format!(
        "ledger-chain-migration-{}",
        &hex::encode(hasher.finalize())[..40]
    );
    let event_count = i64::try_from(event_count).map_err(|_| {
        LedgerError::CorruptChain("chain migration event count exceeds i64".to_string())
    })?;
    tx.execute(
        "INSERT INTO ledger_chain_migrations (
            migration_id, migration_kind, before_hash, after_hash, event_count, applied_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            migration_id,
            migration_kind,
            before_hash,
            after_hash,
            event_count,
            chrono::Utc::now().to_rfc3339(),
        ],
    )?;
    Ok(())
}

fn chain_metadata_is_all_defaults(tx: &Transaction<'_>) -> Result<bool, LedgerError> {
    let non_default: i64 = tx.query_row(
        "SELECT COUNT(*) FROM events WHERE seq_num != 0 OR prev_hash != ?1",
        params![GENESIS_HASH],
        |row| row.get(0),
    )?;
    Ok(non_default == 0)
}

fn rebuild_event_chain_metadata(tx: &Transaction<'_>) -> Result<(), LedgerError> {
    let rows = {
        let mut statement = tx.prepare(
            "SELECT rowid, event_id, principal_id, event_type, payload_json, created_at \
             FROM events ORDER BY principal_id, seq_num, rowid",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok(ChainMigrationRow {
                    row_id: row.get(0)?,
                    event_id: row.get(1)?,
                    principal_id: row.get(2)?,
                    event_type: row.get(3)?,
                    payload_json: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };

    let mut active_principal = String::new();
    let mut seq_num = 0i64;
    let mut prev_hash = GENESIS_HASH.to_string();
    for row in rows {
        if row.principal_id != active_principal {
            active_principal.clone_from(&row.principal_id);
            seq_num = 0;
            prev_hash = GENESIS_HASH.to_string();
        }
        tx.execute(
            "UPDATE events SET seq_num = ?2, prev_hash = ?3 WHERE rowid = ?1",
            params![row.row_id, seq_num, prev_hash],
        )?;
        prev_hash = event_hash(
            &row.event_id,
            &row.principal_id,
            &row.event_type,
            &row.payload_json,
            &row.created_at,
            seq_num,
        );
        seq_num = seq_num
            .checked_add(1)
            .ok_or_else(|| LedgerError::CorruptChain("event sequence exhausted i64".to_string()))?;
    }
    Ok(())
}

fn validate_chain_metadata(tx: &Transaction<'_>) -> Result<(), LedgerError> {
    let rows = {
        let mut statement = tx.prepare(
            "SELECT event_id, principal_id, event_type, payload_json, created_at, \
                    seq_num, prev_hash \
             FROM events ORDER BY principal_id, seq_num, event_id",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, String>(6)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };

    let mut active_principal = String::new();
    let mut expected_seq = 0i64;
    let mut expected_prev = GENESIS_HASH.to_string();
    for (event_id, principal_id, event_type, payload_json, created_at, seq_num, prev_hash) in rows {
        if principal_id != active_principal {
            active_principal.clone_from(&principal_id);
            expected_seq = 0;
            expected_prev = GENESIS_HASH.to_string();
        }
        if seq_num != expected_seq || prev_hash != expected_prev {
            return Err(LedgerError::CorruptChain(format!(
                "principal {principal_id} expected seq {expected_seq} with prev {expected_prev}, \
                 found seq {seq_num} with prev {prev_hash}"
            )));
        }
        expected_prev = event_hash(
            &event_id,
            &principal_id,
            &event_type,
            &payload_json,
            &created_at,
            seq_num,
        );
        expected_seq = expected_seq
            .checked_add(1)
            .ok_or_else(|| LedgerError::CorruptChain("event sequence exhausted i64".to_string()))?;
    }
    Ok(())
}

pub(crate) fn validate_idempotency_key(idempotency_key: &str) -> Result<(), LedgerError> {
    let valid = (8..=MAX_IDEMPOTENCY_KEY_BYTES).contains(&idempotency_key.len())
        && idempotency_key.trim() == idempotency_key
        && idempotency_key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:-@".contains(&byte));
    if valid {
        Ok(())
    } else {
        Err(LedgerError::InvalidIdempotencyKey)
    }
}

pub(crate) fn deterministic_idempotent_event_id(
    principal_id: &PrincipalId,
    idempotency_key: &str,
) -> EventId {
    let mut hasher = Sha256::new();
    for part in [principal_id.as_str(), idempotency_key] {
        hasher.update((part.len() as u64).to_le_bytes());
        hasher.update(part.as_bytes());
        hasher.update([0x1f]);
    }
    EventId(format!(
        "evt-idem-{}",
        &hex::encode(hasher.finalize())[..40]
    ))
}

pub fn event_envelope_signing_bytes(
    principal_id: &PrincipalId,
    namespace_key: &NamespaceKey,
    run_id: Option<&RunId>,
    event_type: &str,
    payload: &serde_json::Value,
    parent_event_id: Option<&EventId>,
) -> Vec<u8> {
    let envelope = serde_json::json!({
        "schema": "zaion.event.signature.v2",
        "principal_id": principal_id.as_str(),
        "namespace_key": namespace_key.0,
        "run_id": run_id.map(|r| r.0.as_str()),
        "event_type": event_type,
        "payload": payload,
        "parent_event_id": parent_event_id.map(|p| p.0.as_str()),
    });
    serde_json::to_vec(&envelope).expect("serde_json::Value serialization is infallible")
}

pub fn sign_event_envelope(
    keypair: &ZaionKeypair,
    principal_id: &PrincipalId,
    namespace_key: &NamespaceKey,
    run_id: Option<&RunId>,
    event_type: &str,
    payload: &serde_json::Value,
    parent_event_id: Option<&EventId>,
) -> SignatureBytes {
    keypair.sign(&event_envelope_signing_bytes(
        principal_id,
        namespace_key,
        run_id,
        event_type,
        payload,
        parent_event_id,
    ))
}

pub fn verify_event_signature(
    public_key: &PublicKeyBytes,
    event: &LedgerEvent,
) -> Result<EventSignatureMode, zaion_crypto::CryptoError> {
    let Some(signature) = &event.signature else {
        return Err(zaion_crypto::CryptoError::VerificationFailed);
    };

    let envelope = event_envelope_signing_bytes(
        &event.principal_id,
        &event.namespace_key,
        event.run_id.as_ref(),
        &event.event_type,
        &event.payload,
        event.parent_event_id.as_ref(),
    );
    if verify_signature(public_key, &envelope, signature).is_ok() {
        return Ok(EventSignatureMode::CanonicalEnvelope);
    }

    let legacy_payload = event.payload.to_string();
    verify_signature(public_key, legacy_payload.as_bytes(), signature)
        .map(|_| EventSignatureMode::LegacyPayloadOnly)
}

/// SHA-256 of the canonical event representation used for hash chaining.
fn event_hash(
    event_id: &str,
    principal_id: &str,
    event_type: &str,
    payload_json: &str,
    created_at: &str,
    seq_num: i64,
) -> String {
    let mut h = Sha256::new();
    h.update(
        format!(
            "{}|{}|{}|{}|{}|{}",
            event_id, principal_id, event_type, payload_json, created_at, seq_num
        )
        .as_bytes(),
    );
    hex::encode(h.finalize())
}

fn row_to_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<LedgerEvent> {
    let payload_json: String = row.get(5)?;
    let sig_hex: Option<String> = row.get(6)?;

    let payload: serde_json::Value = serde_json::from_str(&payload_json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            5,
            rusqlite::types::Type::Text,
            Box::new(CorruptPayloadMarker(format!(
                "corrupt event payload (column 5): {}",
                e
            ))),
        )
    })?;

    let signature = match sig_hex {
        None => None,
        Some(h) => Some(SignatureBytes(hex::decode(&h).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(
                6,
                rusqlite::types::Type::Text,
                Box::new(CorruptPayloadMarker(format!(
                    "corrupt signature hex (column 6): {}",
                    e
                ))),
            )
        })?)),
    };

    let parent_event_id: Option<String> = row.get(8)?;

    Ok(LedgerEvent {
        event_id: EventId(row.get(0)?),
        principal_id: PrincipalId(row.get(1)?),
        namespace_key: NamespaceKey(row.get(2)?),
        run_id: row.get::<_, Option<String>>(3)?.map(RunId),
        event_type: row.get(4)?,
        payload,
        signature,
        created_at: row.get(7)?,
        parent_event_id: parent_event_id.map(EventId),
    })
}

/// Error marker carried inside `rusqlite::Error::FromSqlConversionFailure`
/// so that corruption of event rows is never silently dropped.
#[derive(Debug)]
struct CorruptPayloadMarker(String);

impl std::fmt::Display for CorruptPayloadMarker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CorruptPayloadMarker {}
