use crate::blob::BlobStore;
use crate::ledger::{verify_event_signature, EventLedger, EventSignatureMode};
use crate::{EventInsertDisposition, LedgerError};
use std::sync::{Arc, Barrier};
use tempfile::tempdir;
use zaion_crypto::keypair::ZaionKeypair;
use zaion_types::event::{EventId, EventType, LedgerEvent};
use zaion_types::identity::PrincipalId;
use zaion_types::session::{NamespaceKey, RunId, SessionKey};

#[test]
fn test_append_and_list_events() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let ledger = EventLedger::new(&db);
    let kp = ZaionKeypair::generate();
    let principal_id = kp.principal_id();
    let ns_key = NamespaceKey("chief__ws__proj__telegram__thread".into());
    let session_key = SessionKey("chief__ws__proj__telegram__thread__sess123".into());
    let payload = serde_json::json!({ "text": "hello zaion" });
    let message = format!("{}:{}", "channel.received", payload);
    let sig = kp.sign(message.as_bytes());
    ledger
        .append_event(
            &principal_id,
            &ns_key,
            "channel.received",
            payload.clone(),
            None,
            Some(&sig),
        )
        .unwrap();
    let events = ledger.list_events(&session_key, None, 10).unwrap();
    assert_eq!(
        events.len(),
        0,
        "namespace_key != session_key so no match expected"
    );
    let events2 = ledger
        .list_events(&SessionKey(ns_key.0.clone()), None, 10)
        .unwrap();
    assert_eq!(events2.len(), 1);
    assert_eq!(events2[0].event_type, "channel.received");
    assert!(events2[0].signature.is_some());
}

#[test]
fn test_append_signed_typed_event_with_parent_preserves_wire_type_and_signature() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("typed_events.db");
    let ledger = EventLedger::new(&db);
    let kp = ZaionKeypair::generate();
    let ns_key = NamespaceKey(kp.principal_id().as_str().to_string());

    let received_event_id = ledger
        .append_signed_typed_event(
            &kp,
            &ns_key,
            EventType::ChannelReceived,
            serde_json::json!({
                "schema": "zaion.canonical_envelope.v1",
                "message": "typed ingress"
            }),
            None,
        )
        .unwrap();

    let turn_proof_event_id = ledger
        .append_signed_typed_event_with_parent(
            &kp,
            &ns_key,
            EventType::TurnProof,
            serde_json::json!({
                "schema": "zaion.turn_proof.v1",
                "answer": "typed proof"
            }),
            None,
            Some(&received_event_id),
        )
        .unwrap();

    let turn_proof = ledger
        .get_event(&turn_proof_event_id.0)
        .unwrap()
        .expect("typed turn.proof event");
    assert_eq!(turn_proof.event_type, EventType::TurnProof.as_str());
    assert_eq!(
        turn_proof
            .parent_event_id
            .as_ref()
            .map(|event_id| event_id.0.as_str()),
        Some(received_event_id.0.as_str())
    );
    assert_eq!(
        verify_event_signature(&kp.public_key_bytes(), &turn_proof).unwrap(),
        EventSignatureMode::CanonicalEnvelope
    );

    let events = ledger
        .list_typed_events(
            &SessionKey(ns_key.0.clone()),
            Some(&EventType::TurnProof),
            10,
        )
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_id.0, turn_proof_event_id.0);
}

#[test]
fn test_event_listing_uses_append_sequence_not_timestamp_ties() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("seq_order.db");
    let ledger = EventLedger::new(&db);
    let kp = ZaionKeypair::generate();
    let principal_id = kp.principal_id();
    let ns_key = NamespaceKey(principal_id.as_str().to_string());
    let created_at = "2026-05-18T00:00:00Z";

    let first = EventId("evt-first".to_string());
    ledger
        .insert_event_with_id_and_parent(
            &first,
            &principal_id,
            &ns_key,
            "turn.proof",
            serde_json::json!({"index": 1}),
            None,
            None,
            created_at,
            None,
        )
        .unwrap();
    let second = EventId("evt-second".to_string());
    ledger
        .insert_event_with_id_and_parent(
            &second,
            &principal_id,
            &ns_key,
            "turn.proof",
            serde_json::json!({"index": 2}),
            None,
            None,
            created_at,
            Some(&first),
        )
        .unwrap();

    let namespace_events = ledger
        .list_events(&SessionKey(ns_key.0.clone()), Some("turn.proof"), 10)
        .unwrap();
    assert_eq!(namespace_events[0].event_id.0, second.0);
    assert_eq!(namespace_events[1].event_id.0, first.0);

    let global_events = ledger.list_global_events(10).unwrap();
    assert_eq!(global_events[0].event_id.0, second.0);
    assert_eq!(global_events[1].event_id.0, first.0);
}

#[test]
fn test_genesis_event() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("genesis.db");
    let ledger = EventLedger::new(&db);
    let kp = ZaionKeypair::generate();
    let principal_id = kp.principal_id();
    let ns_key = NamespaceKey(principal_id.as_str().to_string());
    let payload = serde_json::json!({
        "principal_id": principal_id.as_str(),
        "public_key": hex::encode(&kp.public_key_bytes().0),
        "version": "1.0"
    });
    let sig = kp.sign(payload.to_string().as_bytes());
    let event_id = ledger
        .append_event(
            &principal_id,
            &ns_key,
            "process.created",
            payload,
            None,
            Some(&sig),
        )
        .unwrap();
    assert!(event_id.0.starts_with("evt-"));
    let events = ledger
        .list_events(&SessionKey(ns_key.0), Some("process.created"), 10)
        .unwrap();
    assert_eq!(events.len(), 1);
}

#[test]
fn test_blob_store_roundtrip() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("blobs.db");
    let ledger = EventLedger::new(&db);
    ledger.ensure().unwrap();
    let store = BlobStore::new(&db);
    let data = b"zaion agentic process payload data";
    let hash = store.put(data).unwrap();
    assert_eq!(hash.len(), 64);
    let retrieved = store.get(&hash).unwrap().unwrap();
    assert_eq!(retrieved, data);
}

#[test]
fn test_chain_verify_passes_on_fresh_ledger() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("chain.db");
    let ledger = EventLedger::new(&db);
    let kp = ZaionKeypair::generate();
    let principal_id = kp.principal_id();
    let ns_key = NamespaceKey(principal_id.as_str().to_string());

    // Append 5 events
    for i in 0..5 {
        let payload = serde_json::json!({ "index": i });
        ledger
            .append_event(&principal_id, &ns_key, "test.event", payload, None, None)
            .unwrap();
    }

    let result = ledger.verify_chain(&principal_id).unwrap();
    assert_eq!(result.total, 5);
    assert_eq!(result.verified, 5);
    assert_eq!(result.broken_at, None);
}

#[test]
fn independent_ledgers_append_one_unique_continuous_signed_chain() {
    const WRITERS: usize = 12;

    let dir = tempdir().unwrap();
    let db = dir.path().join("concurrent-chain.db");
    EventLedger::new(&db).ensure().unwrap();
    let keypair = Arc::new(ZaionKeypair::generate());
    let principal_id = keypair.principal_id();
    let namespace = NamespaceKey(principal_id.as_str().to_string());
    let barrier = Arc::new(Barrier::new(WRITERS + 1));
    let mut handles = Vec::new();

    for index in 0..WRITERS {
        let db = db.clone();
        let keypair = Arc::clone(&keypair);
        let namespace = namespace.clone();
        let barrier = Arc::clone(&barrier);
        handles.push(std::thread::spawn(move || {
            let ledger = EventLedger::new(db);
            barrier.wait();
            ledger.append_signed_event(
                &keypair,
                &namespace,
                "test.concurrent",
                serde_json::json!({"index": index}),
                None,
            )
        }));
    }
    barrier.wait();
    let event_ids = handles
        .into_iter()
        .map(|handle| handle.join().unwrap().unwrap())
        .collect::<Vec<_>>();
    let unique_ids = event_ids
        .iter()
        .map(|event_id| event_id.0.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(unique_ids.len(), WRITERS);

    let connection = rusqlite::Connection::open(&db).unwrap();
    let mut statement = connection
        .prepare("SELECT seq_num FROM events WHERE principal_id = ?1 ORDER BY seq_num")
        .unwrap();
    let sequences = statement
        .query_map(rusqlite::params![principal_id.as_str()], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        sequences,
        (0..WRITERS as i64).collect::<Vec<_>>(),
        "independent connections must serialize tail-read and insert"
    );

    let ledger = EventLedger::new(&db);
    let chain = ledger.verify_chain(&principal_id).unwrap();
    assert_eq!(chain.total, WRITERS);
    assert_eq!(chain.verified, WRITERS);
    assert_eq!(chain.broken_at, None);
    for event in ledger
        .list_principal_events(&principal_id, WRITERS)
        .unwrap()
    {
        assert_eq!(
            verify_event_signature(&keypair.public_key_bytes(), &event).unwrap(),
            EventSignatureMode::CanonicalEnvelope
        );
    }
}

#[test]
fn signed_idempotent_append_reuses_one_event_and_rejects_key_conflicts() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("idempotent-append.db");
    let ledger = EventLedger::new(&db);
    let keypair = ZaionKeypair::generate();
    let principal_id = keypair.principal_id();
    let namespace = NamespaceKey(principal_id.as_str().to_string());
    let payload = serde_json::json!({
        "schema": "zaion.turn_state_outbox.v2",
        "outbox_id": "outbox-retry-0001",
        "state": "accepted"
    });

    let first = ledger
        .append_signed_idempotent_event(
            &keypair,
            &namespace,
            "turn.accepted",
            payload.clone(),
            None,
            "outbox-retry-0001",
        )
        .unwrap();
    let retry = ledger
        .append_signed_idempotent_event(
            &keypair,
            &namespace,
            "turn.accepted",
            payload,
            None,
            "outbox-retry-0001",
        )
        .unwrap();
    assert_eq!(retry.0, first.0);
    assert!(first.0.starts_with("evt-idem-"));
    assert_eq!(
        ledger
            .list_principal_events(&principal_id, 10)
            .unwrap()
            .len(),
        1
    );

    let conflict = ledger
        .append_signed_idempotent_event(
            &keypair,
            &namespace,
            "turn.accepted",
            serde_json::json!({
                "schema": "zaion.turn_state_outbox.v2",
                "outbox_id": "outbox-retry-0001",
                "state": "running"
            }),
            None,
            "outbox-retry-0001",
        )
        .unwrap_err();
    assert!(matches!(conflict, LedgerError::EventIdConflict { .. }));

    for conflict in [
        ledger.append_signed_idempotent_event(
            &keypair,
            &namespace,
            "turn.running",
            serde_json::json!({
                "schema": "zaion.turn_state_outbox.v2",
                "outbox_id": "outbox-retry-0001",
                "state": "accepted"
            }),
            None,
            "outbox-retry-0001",
        ),
        ledger.append_signed_idempotent_event(
            &keypair,
            &NamespaceKey("different-namespace".to_string()),
            "turn.accepted",
            serde_json::json!({
                "schema": "zaion.turn_state_outbox.v2",
                "outbox_id": "outbox-retry-0001",
                "state": "accepted"
            }),
            None,
            "outbox-retry-0001",
        ),
        ledger.append_signed_idempotent_event(
            &keypair,
            &namespace,
            "turn.accepted",
            serde_json::json!({
                "schema": "zaion.turn_state_outbox.v2",
                "outbox_id": "outbox-retry-0001",
                "state": "accepted"
            }),
            Some(&RunId("different-run".to_string())),
            "outbox-retry-0001",
        ),
        ledger.append_signed_idempotent_event_with_parent(
            &keypair,
            &namespace,
            "turn.accepted",
            serde_json::json!({
                "schema": "zaion.turn_state_outbox.v2",
                "outbox_id": "outbox-retry-0001",
                "state": "accepted"
            }),
            None,
            Some(&EventId("evt-different-parent".to_string())),
            "outbox-retry-0001",
        ),
    ] {
        assert!(matches!(conflict, Err(LedgerError::EventIdConflict { .. })));
    }

    let ordinary_first = ledger
        .append_signed_event(
            &keypair,
            &namespace,
            "test.ordinary",
            serde_json::json!({"same": true}),
            None,
        )
        .unwrap();
    let ordinary_second = ledger
        .append_signed_event(
            &keypair,
            &namespace,
            "test.ordinary",
            serde_json::json!({"same": true}),
            None,
        )
        .unwrap();
    assert_ne!(ordinary_first.0, ordinary_second.0);
}

#[test]
fn concurrent_idempotent_append_across_connections_commits_once() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("concurrent-idempotent.db");
    EventLedger::new(&db).ensure().unwrap();
    let keypair = Arc::new(ZaionKeypair::generate());
    let principal_id = keypair.principal_id();
    let namespace = NamespaceKey(principal_id.as_str().to_string());
    let barrier = Arc::new(Barrier::new(3));
    let mut handles = Vec::new();
    for _ in 0..2 {
        let db = db.clone();
        let keypair = Arc::clone(&keypair);
        let namespace = namespace.clone();
        let barrier = Arc::clone(&barrier);
        handles.push(std::thread::spawn(move || {
            let ledger = EventLedger::new(db);
            barrier.wait();
            ledger.append_signed_idempotent_event(
                &keypair,
                &namespace,
                "turn.accepted",
                serde_json::json!({
                    "schema": "zaion.turn_state_outbox.v2",
                    "outbox_id": "outbox-concurrent-0001",
                    "state": "accepted"
                }),
                None,
                "outbox-concurrent-0001",
            )
        }));
    }
    barrier.wait();
    let results = handles
        .into_iter()
        .map(|handle| handle.join().unwrap().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(results[0].0, results[1].0);

    let ledger = EventLedger::new(&db);
    assert_eq!(
        ledger
            .list_principal_events(&principal_id, 10)
            .unwrap()
            .len(),
        1
    );
    let chain = ledger.verify_chain(&principal_id).unwrap();
    assert_eq!(chain.total, 1);
    assert_eq!(chain.verified, 1);
    assert_eq!(chain.broken_at, None);
}

#[test]
fn preassigned_event_id_is_idempotent_only_for_identical_content() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("preassigned-id.db");
    let ledger = EventLedger::new(&db);
    let keypair = ZaionKeypair::generate();
    let principal_id = keypair.principal_id();
    let namespace = NamespaceKey(principal_id.as_str().to_string());
    let event_id = EventId("evt-import-stable".to_string());
    let created_at = "2026-07-15T12:00:00Z";

    let inserted = ledger
        .insert_event_with_id_and_parent_disposition(
            &event_id,
            &principal_id,
            &namespace,
            "sync.event",
            serde_json::json!({"value": 1}),
            None,
            None,
            created_at,
            None,
        )
        .unwrap();
    assert_eq!(inserted, EventInsertDisposition::Inserted);
    let existing = ledger
        .insert_event_with_id_and_parent_disposition(
            &event_id,
            &principal_id,
            &namespace,
            "sync.event",
            serde_json::json!({"value": 1}),
            None,
            None,
            created_at,
            None,
        )
        .unwrap();
    assert_eq!(existing, EventInsertDisposition::Existing);

    let conflict = ledger
        .insert_event_with_id_and_parent_disposition(
            &event_id,
            &principal_id,
            &namespace,
            "sync.event",
            serde_json::json!({"value": 2}),
            None,
            None,
            created_at,
            None,
        )
        .unwrap_err();
    assert!(matches!(conflict, LedgerError::EventIdConflict { .. }));
}

#[test]
fn atomic_preassigned_batch_rolls_back_on_late_conflict() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("atomic-preassigned-batch.db");
    let ledger = EventLedger::new(&db);
    let principal_id = PrincipalId("did:key:batch-atomic".to_string());
    let namespace = NamespaceKey(principal_id.as_str().to_string());
    let existing = LedgerEvent {
        event_id: EventId("evt-batch-existing".to_string()),
        principal_id: principal_id.clone(),
        namespace_key: namespace.clone(),
        run_id: None,
        event_type: "sync.event".to_string(),
        payload: serde_json::json!({"value": "existing"}),
        signature: None,
        created_at: "2026-07-15T12:00:00Z".to_string(),
        parent_event_id: None,
    };
    assert_eq!(
        ledger
            .insert_events_with_ids_atomic(std::slice::from_ref(&existing))
            .unwrap(),
        vec![EventInsertDisposition::Inserted]
    );

    let new_event = LedgerEvent {
        event_id: EventId("evt-batch-new".to_string()),
        principal_id: principal_id.clone(),
        namespace_key: namespace,
        run_id: None,
        event_type: "sync.event".to_string(),
        payload: serde_json::json!({"value": "new"}),
        signature: None,
        created_at: "2026-07-15T12:00:01Z".to_string(),
        parent_event_id: None,
    };
    let mut conflicting = existing.clone();
    conflicting.payload = serde_json::json!({"value": "conflict"});
    let error = ledger
        .insert_events_with_ids_atomic(&[new_event.clone(), conflicting])
        .unwrap_err();
    assert!(matches!(error, LedgerError::EventIdConflict { .. }));
    assert!(!ledger.event_id_exists(&new_event.event_id.0).unwrap());
    let chain = ledger.verify_chain(&principal_id).unwrap();
    assert_eq!(chain.total, 1);
    assert_eq!(chain.verified, 1);
}

#[test]
fn legacy_schema_migration_rechains_rows_builds_unique_index_and_rebuilds_fts() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("legacy-migration.db");
    let connection = rusqlite::Connection::open(&db).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE events (
                event_id TEXT PRIMARY KEY,
                principal_id TEXT NOT NULL,
                namespace_key TEXT NOT NULL,
                run_id TEXT,
                event_type TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                signature_hex TEXT,
                created_at TEXT NOT NULL
             );
             INSERT INTO events VALUES
                ('evt-a-first', 'did:key:legacy-a', 'did:key:legacy-a', NULL,
                 'legacy.event', '{\"text\":\"legacy alpha first\"}', NULL,
                 '2026-07-15T12:00:02Z'),
                ('evt-a-second', 'did:key:legacy-a', 'did:key:legacy-a', NULL,
                 'legacy.event', '{\"text\":\"legacy alpha second\"}', NULL,
                 '2026-07-15T12:00:01Z'),
                ('evt-b-first', 'did:key:legacy-b', 'did:key:legacy-b', NULL,
                 'legacy.event', '{\"text\":\"legacy beta\"}', NULL,
                 '2026-07-15T12:00:00Z');",
        )
        .unwrap();
    drop(connection);

    let ledger = EventLedger::new(&db);
    ledger.ensure().unwrap();
    for principal in ["did:key:legacy-a", "did:key:legacy-b"] {
        let result = ledger
            .verify_chain(&PrincipalId(principal.to_string()))
            .unwrap();
        assert_eq!(result.total, result.verified);
        assert_eq!(result.broken_at, None);
    }

    let connection = rusqlite::Connection::open(&db).unwrap();
    let ordered = connection
        .prepare("SELECT event_id, seq_num FROM events WHERE principal_id = 'did:key:legacy-a' ORDER BY seq_num")
        .unwrap()
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        ordered,
        vec![
            ("evt-a-first".to_string(), 0),
            ("evt-a-second".to_string(), 1)
        ],
        "legacy migration must preserve row append order, not old timestamps"
    );
    let unique_index: i64 = connection
        .query_row(
            "SELECT [unique] FROM pragma_index_list('events') WHERE name = 'ux_events_principal_seq'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(unique_index, 1);
    let migration = connection
        .query_row(
            "SELECT migration_kind, before_hash, after_hash, event_count
             FROM ledger_chain_migrations",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(migration.0, "legacy_chain_columns_v1");
    assert_ne!(migration.1, migration.2);
    assert_eq!(migration.3, 3);
    assert!(connection
        .execute(
            "INSERT INTO events (
                event_id, principal_id, namespace_key, event_type, payload_json,
                created_at, seq_num, prev_hash
             ) VALUES ('evt-duplicate-seq', 'did:key:legacy-a', 'did:key:legacy-a',
                       'legacy.event', '{}', '2026-07-15T12:00:03Z', 1, ?1)",
            rusqlite::params!["0".repeat(64)],
        )
        .is_err());
    drop(connection);

    let matches = ledger
        .fts_search(&PrincipalId("did:key:legacy-a".to_string()), "legacy", 10)
        .unwrap();
    assert_eq!(matches.len(), 2, "legacy rows must be added to FTS");
}

#[test]
fn schema_upgrade_rebuilds_an_existing_stale_fts_index_once() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("stale-fts.db");
    let connection = rusqlite::Connection::open(&db).unwrap();
    connection
        .execute_batch(&format!(
            "{}
             INSERT INTO events (
                 event_id, principal_id, namespace_key, event_type, payload_json,
                 created_at, seq_num, prev_hash
             ) VALUES (
                 'evt-stale-fts', 'did:key:stale-fts', 'did:key:stale-fts',
                 'legacy.event', '{{\"text\":\"stale searchable value\"}}',
                 '2026-07-15T12:00:00Z', 0, '{}'
             );
             CREATE UNIQUE INDEX ux_events_principal_seq
                 ON events(principal_id, seq_num);
             CREATE VIRTUAL TABLE events_fts USING fts5(
                 event_id UNINDEXED,
                 event_type,
                 payload_json,
                 content='events',
                 content_rowid='rowid'
             );
             CREATE TRIGGER events_fts_insert AFTER INSERT ON events BEGIN
                 INSERT INTO events_fts(rowid, event_id, event_type, payload_json)
                     VALUES (new.rowid, new.event_id, new.event_type, new.payload_json);
             END;",
            crate::schema::CREATE_TABLES_BASE,
            "0".repeat(64)
        ))
        .unwrap();
    let before: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM events_fts WHERE events_fts MATCH 'searchable'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(before, 0);
    drop(connection);

    let ledger = EventLedger::new(&db);
    ledger.ensure().unwrap();
    let matches = ledger
        .fts_search(
            &PrincipalId("did:key:stale-fts".to_string()),
            "searchable",
            10,
        )
        .unwrap();
    assert_eq!(matches.len(), 1);
    drop(ledger);

    EventLedger::new(&db).ensure().unwrap();
    let connection = rusqlite::Connection::open(&db).unwrap();
    let marker_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM ledger_schema_migrations
             WHERE migration_id = 'events_fts_rebuild_v1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(marker_count, 1);
}

#[test]
fn schema_upgrade_replaces_same_named_wrong_sequence_index() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("wrong-sequence-index.db");
    let connection = rusqlite::Connection::open(&db).unwrap();
    connection
        .execute_batch(&format!(
            "{}
             CREATE UNIQUE INDEX ux_events_principal_seq
                 ON events(principal_id COLLATE NOCASE, seq_num DESC);",
            crate::schema::CREATE_TABLES_BASE
        ))
        .unwrap();
    drop(connection);

    EventLedger::new(&db).ensure().unwrap();
    let connection = rusqlite::Connection::open(&db).unwrap();
    let columns = connection
        .prepare(
            "SELECT name, [desc], coll
             FROM pragma_index_xinfo('ux_events_principal_seq')
             WHERE key = 1
             ORDER BY seqno",
        )
        .unwrap()
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        columns,
        vec![
            ("principal_id".to_string(), 0, "BINARY".to_string()),
            ("seq_num".to_string(), 0, "BINARY".to_string())
        ]
    );
    let index_metadata = connection
        .query_row(
            "SELECT [unique], partial FROM pragma_index_list('events')
             WHERE name = 'ux_events_principal_seq'",
            [],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .unwrap();
    assert_eq!(index_metadata, (1, 0));
}

#[test]
fn partial_chain_column_migrations_are_rebuilt_and_audited() {
    for (name, chain_column, chain_values) in [
        (
            "seq-only",
            "seq_num INTEGER NOT NULL DEFAULT 0",
            "7), ('evt-second', 'did:key:partial', 'did:key:partial', NULL, 'test.event', '{}', NULL, '2026-07-15T12:00:01Z', 8",
        ),
        (
            "prev-only",
            "prev_hash TEXT NOT NULL DEFAULT '0000000000000000000000000000000000000000000000000000000000000000'",
            "'0000000000000000000000000000000000000000000000000000000000000000'), ('evt-second', 'did:key:partial', 'did:key:partial', NULL, 'test.event', '{}', NULL, '2026-07-15T12:00:01Z', 'ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff'",
        ),
    ] {
        let dir = tempdir().unwrap();
        let db = dir.path().join(format!("partial-{name}.db"));
        let connection = rusqlite::Connection::open(&db).unwrap();
        connection
            .execute_batch(&format!(
                "CREATE TABLE events (
                    event_id TEXT PRIMARY KEY,
                    principal_id TEXT NOT NULL,
                    namespace_key TEXT NOT NULL,
                    run_id TEXT,
                    event_type TEXT NOT NULL,
                    payload_json TEXT NOT NULL,
                    signature_hex TEXT,
                    created_at TEXT NOT NULL,
                    {chain_column}
                 );
                 INSERT INTO events VALUES
                    ('evt-first', 'did:key:partial', 'did:key:partial', NULL, 'test.event', '{{}}',
                     NULL, '2026-07-15T12:00:00Z', {chain_values});"
            ))
            .unwrap();
        drop(connection);

        let ledger = EventLedger::new(&db);
        ledger.ensure().unwrap();
        let principal = PrincipalId("did:key:partial".to_string());
        let chain = ledger.verify_chain(&principal).unwrap();
        assert_eq!(chain.total, 2, "{name}");
        assert_eq!(chain.verified, 2, "{name}");
        assert_eq!(chain.broken_at, None, "{name}");

        let connection = rusqlite::Connection::open(&db).unwrap();
        let migration_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM ledger_chain_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(migration_count, 1, "{name}");
    }
}

#[test]
fn migration_refuses_nonlegacy_broken_chain_and_verify_rejects_sequence_gaps() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("broken-migration.db");
    let ledger = EventLedger::new(&db);
    let principal_id = PrincipalId("did:key:broken-chain".to_string());
    let namespace = NamespaceKey(principal_id.as_str().to_string());
    for index in 0..3 {
        ledger
            .append_event(
                &principal_id,
                &namespace,
                "test.event",
                serde_json::json!({"index": index}),
                None,
                None,
            )
            .unwrap();
    }
    let connection = rusqlite::Connection::open(&db).unwrap();
    connection
        .execute(
            "UPDATE events SET seq_num = 5 WHERE principal_id = 'did:key:broken-chain'
                 AND seq_num = 2",
            [],
        )
        .unwrap();
    drop(connection);
    let verification = ledger.verify_chain(&principal_id).unwrap();
    assert_eq!(verification.broken_at, Some(5));
    drop(ledger);

    let broken = EventLedger::new(&db);
    let error = broken.ensure().unwrap_err();
    assert!(matches!(error, LedgerError::CorruptChain(_)));

    let connection = rusqlite::Connection::open(&db).unwrap();
    let gap: i64 = connection
        .query_row(
            "SELECT seq_num FROM events WHERE principal_id = 'did:key:broken-chain' ORDER BY seq_num DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        gap, 5,
        "failed migration must not rewrite the broken ledger"
    );
}

#[test]
fn mixed_default_prefix_chain_requires_explicit_repair() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("mixed-default-prefix.db");
    let connection = rusqlite::Connection::open(&db).unwrap();
    connection
        .execute_batch(crate::schema::CREATE_TABLES_BASE)
        .unwrap();
    for (event_id, seq_num, prev_hash) in [
        ("evt-legacy-a", 0, "0".repeat(64)),
        ("evt-legacy-b", 0, "0".repeat(64)),
        ("evt-post-migration", 1, "f".repeat(64)),
    ] {
        connection
            .execute(
                "INSERT INTO events (
                    event_id, principal_id, namespace_key, event_type, payload_json,
                    created_at, seq_num, prev_hash
                 ) VALUES (?1, 'did:key:mixed-prefix', 'did:key:mixed-prefix',
                           'test.event', '{}', '2026-07-15T12:00:00Z', ?2, ?3)",
                rusqlite::params![event_id, seq_num, prev_hash],
            )
            .unwrap();
    }
    drop(connection);

    let error = EventLedger::new(&db).ensure().unwrap_err();
    assert!(matches!(error, LedgerError::CorruptChain(_)));
    let connection = rusqlite::Connection::open(&db).unwrap();
    let duplicate_genesis_rows: i64 = connection
        .query_row("SELECT COUNT(*) FROM events WHERE seq_num = 0", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(duplicate_genesis_rows, 2);
    let unique_index_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM pragma_index_list('events')
             WHERE name = 'ux_events_principal_seq'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(unique_index_count, 0);
}

#[test]
fn test_chain_detects_deletion() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("chain_broken.db");
    let ledger = EventLedger::new(&db);
    let kp = ZaionKeypair::generate();
    let principal_id = kp.principal_id();
    let ns_key = NamespaceKey(principal_id.as_str().to_string());

    // Append 5 events
    for i in 0..5 {
        let payload = serde_json::json!({ "index": i });
        ledger
            .append_event(&principal_id, &ns_key, "test.event", payload, None, None)
            .unwrap();
    }

    // Manually delete event with seq_num=2
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute(
        "DELETE FROM events WHERE principal_id=?1 AND seq_num=2",
        rusqlite::params![principal_id.as_str()],
    )
    .unwrap();

    // Chain should detect break at seq_num=3 (since event 2 is missing, event 3's prev_hash won't match)
    let result = ledger.verify_chain(&principal_id).unwrap();
    assert_eq!(result.total, 4); // 4 events remain
    assert!(result.broken_at.is_some());
    assert_eq!(result.broken_at.unwrap(), 3);
}

#[test]
fn test_thread_scoped_history_isolation() {
    // Verify that channel events from different Telegram thread_ids
    // do NOT bleed into each other when filtering by thread_id.
    let dir = tempdir().unwrap();
    let db = dir.path().join("thread_iso.db");
    let ledger = EventLedger::new(&db);
    let kp = ZaionKeypair::generate();
    let principal_id = kp.principal_id();
    let ns_key = NamespaceKey(principal_id.as_str().to_string());

    // Thread A: chat_id 111
    let recv_a = serde_json::json!({ "thread_id": "111", "message": "hello from A", "principal_id": principal_id.as_str() });
    ledger
        .append_event(
            &principal_id,
            &ns_key,
            "channel.received",
            recv_a,
            None,
            None,
        )
        .unwrap();
    let sent_a = serde_json::json!({ "to": "111", "response": "hi A", "principal_id": principal_id.as_str() });
    ledger
        .append_event(&principal_id, &ns_key, "channel.sent", sent_a, None, None)
        .unwrap();

    // Thread B: chat_id 222
    let recv_b = serde_json::json!({ "thread_id": "222", "message": "hello from B", "principal_id": principal_id.as_str() });
    ledger
        .append_event(
            &principal_id,
            &ns_key,
            "channel.received",
            recv_b,
            None,
            None,
        )
        .unwrap();
    let sent_b = serde_json::json!({ "to": "222", "response": "hi B", "principal_id": principal_id.as_str() });
    ledger
        .append_event(&principal_id, &ns_key, "channel.sent", sent_b, None, None)
        .unwrap();

    // Check all 4 events exist
    let all = ledger
        .list_events(&SessionKey(ns_key.0.clone()), None, 20)
        .unwrap();
    assert_eq!(all.len(), 4, "should have 4 events total");

    // Filter to thread A — should only see thread A's events
    let thread_a_events: Vec<_> = all
        .iter()
        .filter(|e| {
            let tid = e
                .payload
                .get("thread_id")
                .or_else(|| e.payload.get("to"))
                .and_then(|v| v.as_str());
            tid == Some("111")
        })
        .collect();
    assert_eq!(
        thread_a_events.len(),
        2,
        "thread A should have exactly 2 events"
    );

    // Filter to thread B — should only see thread B's events
    let thread_b_events: Vec<_> = all
        .iter()
        .filter(|e| {
            let tid = e
                .payload
                .get("thread_id")
                .or_else(|| e.payload.get("to"))
                .and_then(|v| v.as_str());
            tid == Some("222")
        })
        .collect();
    assert_eq!(
        thread_b_events.len(),
        2,
        "thread B should have exactly 2 events"
    );

    // Ensure A's messages don't appear when looking at B's filter
    assert!(
        thread_b_events.iter().all(|e| {
            e.payload.get("message").and_then(|v| v.as_str()) != Some("hello from A")
                && e.payload.get("response").and_then(|v| v.as_str()) != Some("hi A")
        }),
        "thread B events must not contain thread A content"
    );
}

#[test]
fn test_no_thread_id_events_excluded_from_bot_mode() {
    // CLI events (no thread_id field) must not appear in thread-scoped bot history.
    let dir = tempdir().unwrap();
    let db = dir.path().join("cli_vs_bot.db");
    let ledger = EventLedger::new(&db);
    let kp = ZaionKeypair::generate();
    let principal_id = kp.principal_id();
    let ns_key = NamespaceKey(principal_id.as_str().to_string());

    // CLI event — no thread_id
    let cli_recv =
        serde_json::json!({ "message": "cli question", "principal_id": principal_id.as_str() });
    ledger
        .append_event(
            &principal_id,
            &ns_key,
            "channel.received",
            cli_recv,
            None,
            None,
        )
        .unwrap();
    let cli_sent =
        serde_json::json!({ "response": "cli answer", "principal_id": principal_id.as_str() });
    ledger
        .append_event(&principal_id, &ns_key, "channel.sent", cli_sent, None, None)
        .unwrap();

    // Telegram event — with thread_id
    let tg_recv = serde_json::json!({ "thread_id": "999", "message": "tg question", "principal_id": principal_id.as_str() });
    ledger
        .append_event(
            &principal_id,
            &ns_key,
            "channel.received",
            tg_recv,
            None,
            None,
        )
        .unwrap();
    let tg_sent = serde_json::json!({ "to": "999", "response": "tg answer", "principal_id": principal_id.as_str() });
    ledger
        .append_event(&principal_id, &ns_key, "channel.sent", tg_sent, None, None)
        .unwrap();

    // All 4 events exist
    let all = ledger
        .list_events(&SessionKey(ns_key.0.clone()), None, 20)
        .unwrap();
    assert_eq!(all.len(), 4);

    // Thread 999 filter: should only see tg events, not cli events
    let tid_999: Vec<_> = all
        .iter()
        .filter(|e| {
            let tid = e
                .payload
                .get("thread_id")
                .or_else(|| e.payload.get("to"))
                .and_then(|v| v.as_str());
            tid == Some("999")
        })
        .collect();
    assert_eq!(tid_999.len(), 2, "only tg events in thread 999");
    assert!(
        tid_999
            .iter()
            .all(|e| { e.payload.get("message").and_then(|v| v.as_str()) != Some("cli question") }),
        "CLI events must not bleed into thread-scoped history"
    );
}

#[test]
fn test_list_events_by_payload_string_returns_latest_exact_matches() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("payload_string_lookup.db");
    let ledger = EventLedger::new(&db);
    let kp = ZaionKeypair::generate();
    let principal_id = kp.principal_id();
    let ns_key = NamespaceKey(principal_id.as_str().to_string());
    let session_key = SessionKey(ns_key.0.clone());

    ledger
        .append_event(
            &principal_id,
            &ns_key,
            "todo.state",
            serde_json::json!({"thread_id": "target", "state": "older-target"}),
            None,
            None,
        )
        .unwrap();
    for index in 0..100 {
        ledger
            .append_event(
                &principal_id,
                &ns_key,
                "todo.state",
                serde_json::json!({
                    "thread_id": format!("other-{index}"),
                    "state": "newer-other"
                }),
                None,
                None,
            )
            .unwrap();
    }
    ledger
        .append_event(
            &principal_id,
            &ns_key,
            "todo.state",
            serde_json::json!({"thread_id": 42, "state": "non-string-target"}),
            None,
            None,
        )
        .unwrap();
    ledger
        .append_event(
            &principal_id,
            &ns_key,
            "other.event",
            serde_json::json!({"thread_id": "target", "state": "wrong-type"}),
            None,
            None,
        )
        .unwrap();
    ledger
        .append_event(
            &principal_id,
            &ns_key,
            "todo.state",
            serde_json::json!({"thread_id": "target", "state": "newer-target"}),
            None,
            None,
        )
        .unwrap();

    let found = ledger
        .list_events_by_payload_string(&session_key, "todo.state", "thread_id", "target", 10)
        .unwrap();

    assert_eq!(found.len(), 2);
    assert_eq!(found[0].payload["state"], "newer-target");
    assert_eq!(found[1].payload["state"], "older-target");
}

#[test]
fn test_list_events_by_payload_string_array_contains_returns_latest_exact_matches() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("payload_array_lookup.db");
    let ledger = EventLedger::new(&db);
    let kp = ZaionKeypair::generate();
    let principal_id = kp.principal_id();
    let ns_key = NamespaceKey(principal_id.as_str().to_string());
    let session_key = SessionKey(ns_key.0.clone());

    ledger
        .append_event(
            &principal_id,
            &ns_key,
            EventType::ToolReceiptProofJoin.as_str(),
            serde_json::json!({
                "tool_receipt_ids": ["receipt-a", "receipt-b"],
                "join_hash": "older-target"
            }),
            None,
            None,
        )
        .unwrap();
    for index in 0..100 {
        ledger
            .append_event(
                &principal_id,
                &ns_key,
                EventType::ToolReceiptProofJoin.as_str(),
                serde_json::json!({
                    "tool_receipt_ids": [format!("other-{index}")],
                    "join_hash": "newer-other"
                }),
                None,
                None,
            )
            .unwrap();
    }
    ledger
        .append_event(
            &principal_id,
            &ns_key,
            EventType::ToolReceiptProofJoin.as_str(),
            serde_json::json!({
                "tool_receipt_ids": "receipt-a",
                "join_hash": "non-array-target"
            }),
            None,
            None,
        )
        .unwrap();
    ledger
        .append_event(
            &principal_id,
            &ns_key,
            "other.event",
            serde_json::json!({
                "tool_receipt_ids": ["receipt-a"],
                "join_hash": "wrong-type"
            }),
            None,
            None,
        )
        .unwrap();
    ledger
        .append_event(
            &principal_id,
            &ns_key,
            EventType::ToolReceiptProofJoin.as_str(),
            serde_json::json!({
                "tool_receipt_ids": ["receipt-c", "receipt-a"],
                "join_hash": "newer-target"
            }),
            None,
            None,
        )
        .unwrap();

    let found = ledger
        .list_events_by_payload_string_array_contains(
            &session_key,
            EventType::ToolReceiptProofJoin.as_str(),
            "tool_receipt_ids",
            "receipt-a",
            10,
        )
        .unwrap();

    assert_eq!(found.len(), 2);
    assert_eq!(found[0].payload["join_hash"], "newer-target");
    assert_eq!(found[1].payload["join_hash"], "older-target");
}

#[test]
fn test_fts5_search_finds_payload_text() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("fts_test.db");
    let ledger = EventLedger::new(&db);
    let kp = ZaionKeypair::generate();
    let pid = kp.principal_id();
    let ns = NamespaceKey("ns".into());

    // Insert two events with distinct payloads
    ledger
        .append_event(
            &pid,
            &ns,
            "channel.received",
            serde_json::json!({"message": "hello zaion world"}),
            None,
            None,
        )
        .unwrap();
    ledger
        .append_event(
            &pid,
            &ns,
            "channel.received",
            serde_json::json!({"message": "goodbye forever"}),
            None,
            None,
        )
        .unwrap();
    ledger
        .append_event(
            &pid,
            &ns,
            "skill.applied",
            serde_json::json!({"skill": "summarize", "result": "zaion summarized"}),
            None,
            None,
        )
        .unwrap();

    // FTS search for "zaion" — should match first and third events
    let results = ledger.fts_search(&pid, "zaion", 10).unwrap();
    assert!(
        !results.is_empty(),
        "FTS5 search for 'zaion' should return results"
    );
    assert!(
        results
            .iter()
            .all(|e| { e.payload.to_string().contains("zaion") }),
        "All results should contain 'zaion' in payload"
    );

    // FTS search for "goodbye" — should match second event only
    let results2 = ledger.fts_search(&pid, "goodbye", 10).unwrap();
    assert_eq!(results2.len(), 1);
    assert!(results2[0].payload.to_string().contains("goodbye"));
}

#[test]
fn test_fts5_search_global_spans_principals() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("fts_global_test.db");
    let ledger = EventLedger::new(&db);
    let kp1 = ZaionKeypair::generate();
    let kp2 = ZaionKeypair::generate();
    let pid1 = kp1.principal_id();
    let pid2 = kp2.principal_id();
    let ns = NamespaceKey("ns".into());

    ledger
        .append_event(
            &pid1,
            &ns,
            "channel.received",
            serde_json::json!({"message": "uniquephrasealpha"}),
            None,
            None,
        )
        .unwrap();
    ledger
        .append_event(
            &pid2,
            &ns,
            "channel.received",
            serde_json::json!({"message": "uniquephrasealpha too"}),
            None,
            None,
        )
        .unwrap();

    // Global search finds both
    let results = ledger.fts_search_global("uniquephrasealpha", 10).unwrap();
    assert_eq!(
        results.len(),
        2,
        "global FTS should find events from both principals"
    );
}

#[test]
fn test_fts5_no_results_for_missing_term() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("fts_empty_test.db");
    let ledger = EventLedger::new(&db);
    let kp = ZaionKeypair::generate();
    let pid = kp.principal_id();
    let ns = NamespaceKey("ns".into());

    ledger
        .append_event(
            &pid,
            &ns,
            "channel.received",
            serde_json::json!({"message": "hello world"}),
            None,
            None,
        )
        .unwrap();

    let results = ledger.fts_search(&pid, "xyzzy_not_present_42", 10).unwrap();
    assert!(
        results.is_empty(),
        "FTS search for absent term should return empty"
    );
}
