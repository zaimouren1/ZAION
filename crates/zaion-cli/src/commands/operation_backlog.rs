use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use zaion_runtime::operation_stream::{OperationEvent, OperationStreamBacklog};
use zaion_types::event::EventType;

const SHARED_OPERATION_BACKLOG_CAPACITY: usize = 512;
const OPERATION_BACKLOG_JSONL: &str = "events.jsonl";
#[cfg(test)]
const TEST_PERSISTENCE_ENV: &str = "ZAION_OPERATION_BACKLOG_PERSISTENCE_FOR_TEST";
static SHARED_OPERATION_BACKLOG: OnceLock<SharedOperationBacklog> = OnceLock::new();

struct SharedOperationBacklog {
    state: Mutex<SharedOperationBacklogState>,
    changed: Condvar,
}

struct SharedOperationBacklogState {
    backlog: OperationStreamBacklog,
    generation: u64,
}

fn shared_operation_backlog_cell() -> &'static SharedOperationBacklog {
    SHARED_OPERATION_BACKLOG.get_or_init(|| SharedOperationBacklog {
        state: Mutex::new(SharedOperationBacklogState {
            backlog: OperationStreamBacklog::new(SHARED_OPERATION_BACKLOG_CAPACITY),
            generation: 0,
        }),
        changed: Condvar::new(),
    })
}

pub(crate) fn append_shared_operation_backlog(events: &[OperationEvent]) -> Vec<OperationEvent> {
    if events.is_empty() {
        return Vec::new();
    }

    let events = bind_operation_events_to_ledger(events);
    {
        let mut state = shared_operation_backlog_cell()
            .state
            .lock()
            .expect("shared operation backlog mutex poisoned");
        for event in &events {
            state.backlog.append(event.clone());
        }
        state.generation = state.generation.saturating_add(1);
    }
    if should_use_persisted_operation_backlog() {
        if let Err(error) = append_persisted_operation_backlog(&events) {
            eprintln!("warning: failed to persist operation backlog: {error}");
        }
    }
    shared_operation_backlog_cell().changed.notify_all();
    events
}

pub(crate) fn shared_operation_backlog() -> OperationStreamBacklog {
    let mut snapshot = if should_use_persisted_operation_backlog() {
        persisted_operation_backlog()
    } else {
        OperationStreamBacklog::new(SHARED_OPERATION_BACKLOG_CAPACITY)
    };
    let mut seen = snapshot
        .replay_after(None)
        .into_iter()
        .map(|event| operation_event_key(&event))
        .collect::<HashSet<_>>();

    let memory_events = shared_operation_backlog_cell()
        .state
        .lock()
        .expect("shared operation backlog mutex poisoned")
        .backlog
        .replay_after(None);
    for event in memory_events {
        if seen.insert(operation_event_key(&event)) {
            snapshot.append(event);
        }
    }

    snapshot
}

pub(crate) fn wait_for_shared_operation_backlog_after(
    after: Option<&str>,
    timeout: Duration,
) -> Vec<OperationEvent> {
    let deadline = Instant::now()
        .checked_add(timeout)
        .unwrap_or_else(Instant::now);

    loop {
        let replay = shared_operation_backlog().replay_after(after);
        if !replay.is_empty() || timeout.is_zero() {
            return replay;
        }

        let shared = shared_operation_backlog_cell();
        let state = shared
            .state
            .lock()
            .expect("shared operation backlog mutex poisoned");
        let replay = state.backlog.replay_after(after);
        if !replay.is_empty() {
            return replay;
        }

        let observed_generation = state.generation;
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            return shared_operation_backlog().replay_after(after);
        };
        if remaining.is_zero() {
            return shared_operation_backlog().replay_after(after);
        }

        let (state, wait_result) = shared
            .changed
            .wait_timeout_while(state, remaining, |state| {
                state.generation == observed_generation
            })
            .expect("shared operation backlog condvar poisoned");
        drop(state);
        if wait_result.timed_out() {
            return shared_operation_backlog().replay_after(after);
        }
    }
}

pub(crate) fn operation_backlog_path() -> PathBuf {
    crate::commands::data_dir()
        .join("operation-stream")
        .join(OPERATION_BACKLOG_JSONL)
}

fn bind_operation_events_to_ledger(events: &[OperationEvent]) -> Vec<OperationEvent> {
    events
        .iter()
        .cloned()
        .map(|mut event| {
            if event.ledger_event_id.is_none() {
                if let Some((ledger_event_id, proof_hash)) =
                    append_operation_event_to_ledger(&event)
                {
                    event.ledger_event_id = Some(ledger_event_id);
                    event.proof_hash = Some(proof_hash);
                }
            }
            event
        })
        .collect()
}

fn append_operation_event_to_ledger(event: &OperationEvent) -> Option<(String, String)> {
    let store = zaion_core::process::ProcessStore::new(crate::commands::data_dir());
    let (_, keypair) = store.load(&event.principal_id).ok()?;
    if keypair.principal_id().as_str() != event.principal_id {
        return None;
    }

    let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&event.principal_id));
    let namespace_key = zaion_types::session::NamespaceKey(event.principal_id.clone());
    let payload = ledger_operation_event_payload(event);
    let event_id = ledger
        .append_signed_typed_event(
            &keypair,
            &namespace_key,
            EventType::OperationEvent,
            payload.clone(),
            None,
        )
        .ok()?;
    let proof_hash = ledger_operation_event_proof_hash(&event_id.0, &payload);
    Some((event_id.0, proof_hash))
}

fn ledger_operation_event_payload(event: &OperationEvent) -> serde_json::Value {
    let cursor = operation_event_cursor(event);
    serde_json::json!({
        "schema": "zaion.operation_event.v1",
        "storage": "ledger_native",
        "cursor": cursor,
        "stream_id": event.stream_id,
        "turn_id": event.turn_id,
        "sequence": event.sequence,
        "timestamp": event.timestamp,
        "principal_id": event.principal_id,
        "channel_id": event.channel_id,
        "thread_id": event.thread_id,
        "stage": event.stage,
        "kind": event.kind,
        "level": event.level,
        "display_text": event.display_text,
        "payload": event.payload,
        "redaction_class": event.redaction_class,
        "parent_sequence": event.parent_sequence,
        "operation_event": {
            "stream_id": event.stream_id,
            "turn_id": event.turn_id,
            "sequence": event.sequence,
            "timestamp": event.timestamp,
            "principal_id": event.principal_id,
            "channel_id": event.channel_id,
            "thread_id": event.thread_id,
            "stage": event.stage,
            "kind": event.kind,
            "level": event.level,
            "display_text": event.display_text,
            "payload": event.payload,
            "redaction_class": event.redaction_class,
            "ledger_event_id": event.ledger_event_id,
            "proof_hash": event.proof_hash,
            "parent_sequence": event.parent_sequence,
            "cursor": cursor,
        },
    })
}

fn ledger_operation_event_proof_hash(event_id: &str, payload: &serde_json::Value) -> String {
    let proof = serde_json::json!({
        "schema": "zaion.operation_event.ledger_proof.v1",
        "ledger_event_id": event_id,
        "payload": payload,
    });
    let bytes = serde_json::to_vec(&proof).unwrap_or_default();
    let digest = Sha256::digest(bytes);
    format!("sha256:{}", hex::encode(digest))
}

fn operation_event_cursor(event: &OperationEvent) -> String {
    zaion_runtime::operation_stream::OperationStreamCursor::new(
        event.stream_id.clone(),
        event.sequence,
    )
    .to_sse_id()
}

fn append_persisted_operation_backlog(events: &[OperationEvent]) -> std::io::Result<()> {
    let path = operation_backlog_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    for event in events {
        serde_json::to_writer(&mut file, event).map_err(std::io::Error::other)?;
        file.write_all(b"\n")?;
    }
    file.flush()
}

fn persisted_operation_backlog() -> OperationStreamBacklog {
    let path = operation_backlog_path();
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return OperationStreamBacklog::new(SHARED_OPERATION_BACKLOG_CAPACITY),
    };
    let mut backlog = OperationStreamBacklog::new(SHARED_OPERATION_BACKLOG_CAPACITY);
    let mut seen = HashSet::new();
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(event) = serde_json::from_str::<OperationEvent>(&line) else {
            continue;
        };
        if seen.insert(operation_event_key(&event)) {
            backlog.append(event);
        }
    }
    backlog
}

fn operation_event_key(event: &OperationEvent) -> (String, String, u64) {
    (
        event.stream_id.clone(),
        event.turn_id.clone(),
        event.sequence,
    )
}

fn should_use_persisted_operation_backlog() -> bool {
    should_use_persisted_operation_backlog_for_build()
}

#[cfg(not(test))]
fn should_use_persisted_operation_backlog_for_build() -> bool {
    true
}

#[cfg(test)]
fn should_use_persisted_operation_backlog_for_build() -> bool {
    std::env::var_os(TEST_PERSISTENCE_ENV).is_some()
}

#[cfg(test)]
pub(crate) fn reset_shared_operation_backlog_for_test() {
    reset_shared_operation_backlog_memory_only_for_test();
}

#[cfg(test)]
pub(crate) fn reset_shared_operation_backlog_memory_only_for_test() {
    let mut state = shared_operation_backlog_cell()
        .state
        .lock()
        .expect("shared operation backlog mutex poisoned");
    state.backlog = OperationStreamBacklog::new(SHARED_OPERATION_BACKLOG_CAPACITY);
    state.generation = state.generation.saturating_add(1);
    shared_operation_backlog_cell().changed.notify_all();
}

#[cfg(test)]
mod tests {
    use super::*;
    use zaion_runtime::operation_stream::{
        OperationEventKind, OperationLevel, OperationStage, RedactionClass,
    };

    struct EnvGuard {
        zaion_data_dir: Option<String>,
        persistence: Option<String>,
    }

    impl EnvGuard {
        fn capture() -> Self {
            Self {
                zaion_data_dir: std::env::var("ZAION_DATA_DIR").ok(),
                persistence: std::env::var(TEST_PERSISTENCE_ENV).ok(),
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match self.zaion_data_dir.take() {
                Some(value) => std::env::set_var("ZAION_DATA_DIR", value),
                None => std::env::remove_var("ZAION_DATA_DIR"),
            }
            match self.persistence.take() {
                Some(value) => std::env::set_var(TEST_PERSISTENCE_ENV, value),
                None => std::env::remove_var(TEST_PERSISTENCE_ENV),
            }
        }
    }

    #[test]
    fn shared_operation_backlog_survives_memory_reset_from_persisted_jsonl() {
        let _guard = crate::config::env_test_lock();
        let _env_guard = EnvGuard::capture();
        let temp_data =
            std::env::temp_dir().join(format!("zaion-operation-backlog-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_data).expect("temp data dir");
        std::env::set_var("ZAION_DATA_DIR", &temp_data);
        std::env::set_var(TEST_PERSISTENCE_ENV, "1");

        reset_shared_operation_backlog_for_test();
        let event = test_operation_event("persisted-stream", "turn-persisted", 2);
        append_shared_operation_backlog(std::slice::from_ref(&event));

        reset_shared_operation_backlog_memory_only_for_test();

        let replay = shared_operation_backlog().replay_after(Some("operation:persisted-stream:1"));

        assert_eq!(replay.len(), 1);
        assert_eq!(replay[0].stream_id, event.stream_id);
        assert_eq!(replay[0].sequence, event.sequence);
        assert_eq!(replay[0].display_text, "persisted provider calling");

        let _ = std::fs::remove_dir_all(temp_data);
    }

    #[test]
    fn shared_operation_backlog_writes_operation_events_to_signed_ledger() {
        let _guard = crate::config::env_test_lock();
        let _env_guard = EnvGuard::capture();
        let temp_data =
            std::env::temp_dir().join(format!("zaion-operation-ledger-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_data).expect("temp data dir");
        std::env::set_var("ZAION_DATA_DIR", &temp_data);
        std::env::set_var(TEST_PERSISTENCE_ENV, "1");

        reset_shared_operation_backlog_for_test();
        let store = zaion_core::process::ProcessStore::new(&temp_data);
        let (process, keypair) = store.create("workspace-ledger", "project-ledger").unwrap();
        let event = test_operation_event_for_principal(
            &process.principal_id,
            "ledger-stream",
            "turn-ledger",
            7,
        );
        append_shared_operation_backlog(std::slice::from_ref(&event));

        let replay = shared_operation_backlog().replay_after(Some("operation:ledger-stream:6"));

        assert_eq!(replay.len(), 1);
        let ledger_event_id = replay[0]
            .ledger_event_id
            .as_ref()
            .expect("operation backlog event should carry ledger event id");
        assert_eq!(
            replay[0].proof_hash.as_deref().unwrap_or_default().len(),
            71
        );
        assert!(replay[0]
            .proof_hash
            .as_deref()
            .unwrap()
            .starts_with("sha256:"));

        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let principal_id = zaion_types::identity::PrincipalId(process.principal_id.clone());
        let events = ledger
            .list_principal_events(&principal_id, 8)
            .expect("principal events");
        let operation_event = events
            .iter()
            .find(|candidate| candidate.event_id.0 == *ledger_event_id)
            .expect("operation event exists in ledger");

        assert_eq!(operation_event.event_type, "operation.event");
        assert_eq!(
            operation_event.payload["schema"],
            "zaion.operation_event.v1"
        );
        assert_eq!(
            operation_event.payload["cursor"],
            "operation:ledger-stream:7"
        );
        assert_eq!(operation_event.payload["stream_id"], "ledger-stream");
        assert_eq!(
            operation_event.payload["display_text"],
            "persisted provider calling"
        );
        assert_eq!(
            operation_event.payload["operation_event"]["payload"]["provider"],
            "test"
        );

        let chain = ledger.verify_chain(&principal_id).expect("verified chain");
        assert_eq!(chain.total, 2);
        assert_eq!(chain.broken_at, None);
        assert_eq!(chain.verified, 2);
        assert_eq!(
            zaion_ledger::verify_event_signature(&keypair.public_key_bytes(), operation_event)
                .expect("valid signature"),
            zaion_ledger::EventSignatureMode::CanonicalEnvelope
        );

        let _ = std::fs::remove_dir_all(temp_data);
    }

    #[test]
    fn append_shared_operation_backlog_returns_ledger_bound_events() {
        let _guard = crate::config::env_test_lock();
        let _env_guard = EnvGuard::capture();
        let temp_data = std::env::temp_dir().join(format!(
            "zaion-operation-ledger-return-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&temp_data).expect("temp data dir");
        std::env::set_var("ZAION_DATA_DIR", &temp_data);
        std::env::set_var(TEST_PERSISTENCE_ENV, "1");

        reset_shared_operation_backlog_for_test();
        let store = zaion_core::process::ProcessStore::new(&temp_data);
        let (process, _) = store.create("workspace-return", "project-return").unwrap();
        let event = test_operation_event_for_principal(
            &process.principal_id,
            "ledger-return-stream",
            "turn-return",
            3,
        );

        let enriched = append_shared_operation_backlog(&[event]);

        assert_eq!(enriched.len(), 1);
        assert!(
            enriched[0]
                .ledger_event_id
                .as_deref()
                .is_some_and(|id| id.starts_with("evt-")),
            "append caller should receive the ledger-bound operation event: {enriched:#?}"
        );
        assert!(
            enriched[0]
                .proof_hash
                .as_deref()
                .is_some_and(|hash| hash.starts_with("sha256:")),
            "append caller should receive operation proof hash: {enriched:#?}"
        );

        let _ = std::fs::remove_dir_all(temp_data);
    }

    #[test]
    fn wait_for_shared_operation_backlog_after_wakes_when_event_is_appended() {
        let _guard = crate::config::env_test_lock();
        let _env_guard = EnvGuard::capture();
        std::env::remove_var(TEST_PERSISTENCE_ENV);

        reset_shared_operation_backlog_for_test();
        append_shared_operation_backlog(&[test_operation_event(
            "blocking-stream",
            "turn-blocking",
            1,
        )]);

        let started = std::time::Instant::now();
        let waiter = std::thread::spawn(|| {
            wait_for_shared_operation_backlog_after(
                Some("operation:blocking-stream:1"),
                std::time::Duration::from_millis(750),
            )
        });

        std::thread::sleep(std::time::Duration::from_millis(80));
        append_shared_operation_backlog(&[test_operation_event(
            "blocking-stream",
            "turn-blocking",
            2,
        )]);

        let replay = waiter.join().expect("waiter should not panic");
        let elapsed = started.elapsed();

        assert!(
            elapsed >= std::time::Duration::from_millis(50),
            "wait should block until append instead of returning immediately: {elapsed:?}"
        );
        assert!(
            elapsed < std::time::Duration::from_millis(700),
            "wait should wake on append before the timeout: {elapsed:?}"
        );
        assert_eq!(replay.len(), 1);
        assert_eq!(replay[0].sequence, 2);
        assert_eq!(replay[0].display_text, "persisted provider calling");
    }

    fn test_operation_event(stream_id: &str, thread_id: &str, sequence: u64) -> OperationEvent {
        test_operation_event_for_principal(
            "did:key:operation-backlog",
            stream_id,
            thread_id,
            sequence,
        )
    }

    fn test_operation_event_for_principal(
        principal_id: &str,
        stream_id: &str,
        thread_id: &str,
        sequence: u64,
    ) -> OperationEvent {
        OperationEvent {
            stream_id: stream_id.to_string(),
            turn_id: thread_id.to_string(),
            sequence,
            timestamp: "2026-05-06T00:00:00Z".to_string(),
            principal_id: principal_id.to_string(),
            channel_id: "api".to_string(),
            thread_id: thread_id.to_string(),
            stage: OperationStage::Reasoning,
            kind: OperationEventKind::ProviderCalling,
            level: OperationLevel::Info,
            display_text: "persisted provider calling".to_string(),
            payload: serde_json::json!({"provider": "test"}),
            redaction_class: RedactionClass::Public,
            ledger_event_id: None,
            proof_hash: None,
            parent_sequence: None,
        }
    }
}
