use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration as StdDuration, Instant};

use chrono::{Duration, Utc};
use rusqlite::params;
use serde_json::Value;
use tempfile::tempdir;
use zaion_crypto::ZaionKeypair;
use zaion_types::identity::PrincipalId;
use zaion_types::session::{SessionId, WorkspaceId};

use super::*;
use crate::{
    AuthenticatedIngress, AuthenticatedIngressInput, AuthenticatedSourceInput,
    DurableTurnAdmission, TurnActorIdentity, TurnOutboxStatus,
};

fn test_config() -> OutboxDispatcherConfig {
    OutboxDispatcherConfig {
        worker_count: 4,
        tenant_scan_limit: 32,
        lease_duration: StdDuration::from_secs(2),
        poll_interval: StdDuration::from_millis(5),
        initial_retry_delay: StdDuration::from_millis(5),
        maximum_retry_delay: StdDuration::from_millis(20),
        minimum_commit_window: StdDuration::from_millis(100),
        shutdown_timeout: StdDuration::from_secs(1),
        retry_jitter_percent: 0,
        maximum_attempts: 4,
        test_hook: None,
    }
}

fn open_store() -> (tempfile::TempDir, DurableTurnStore) {
    let directory = tempdir().unwrap();
    let store = DurableTurnStore::open(directory.path().join("ledger.db")).unwrap();
    (directory, store)
}

fn begin_turn(
    store: &DurableTurnStore,
    tenant_id: &str,
    idempotency_key: &str,
    keypair: &ZaionKeypair,
    now: DateTime<Utc>,
) -> TurnOutboxRecord {
    let canonical_idempotency_key = format!("request-{idempotency_key}");
    let ingress = AuthenticatedIngress::new(
        AuthenticatedIngressInput {
            tenant_id: tenant_id.to_string(),
            subject_id: format!("subject-{idempotency_key}"),
            principal_id: keypair.principal_id(),
            workspace_id: WorkspaceId("workspace-dispatcher-test".to_string()),
            profile_id: "default".to_string(),
            session_id: SessionId(format!("session-{idempotency_key}")),
            source: AuthenticatedSourceInput {
                surface: "cli".to_string(),
                source_id: format!("message-{idempotency_key}"),
            },
            deadline: now + Duration::minutes(5),
            scopes: vec!["turn:submit".to_string()],
            idempotency_key: canonical_idempotency_key.clone(),
            attachments: Vec::new(),
        },
        now,
    )
    .unwrap();
    let actor =
        TurnActorIdentity::for_ingress(&ingress, "terminal", format!("thread-{idempotency_key}"))
            .unwrap();
    let admission = DurableTurnAdmission::new(
        actor,
        serde_json::json!({"message": idempotency_key}),
        format!("turn-owner-{idempotency_key}"),
    )
    .unwrap();
    store.begin_turn(&ingress, &admission, now).unwrap();
    store
        .undelivered_outbox(tenant_id, 100)
        .unwrap()
        .into_iter()
        .find(|row| {
            row.payload
                .pointer("/idempotency_key")
                .and_then(Value::as_str)
                == Some(canonical_idempotency_key.as_str())
        })
        .unwrap()
}

fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + StdDuration::from_secs(10);
    while Instant::now() < deadline {
        if condition() {
            return;
        }
        std::thread::sleep(StdDuration::from_millis(5));
    }
    panic!("condition was not satisfied before the test deadline");
}

fn event_count(db_path: &Path, principal_id: &PrincipalId) -> i64 {
    rusqlite::Connection::open(db_path)
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM events WHERE principal_id = ?1",
            params![principal_id.as_str()],
            |row| row.get(0),
        )
        .unwrap()
}

fn deliver_claim(
    store: &DurableTurnStore,
    tenant_id: &str,
    owner: &str,
    keypair: &ZaionKeypair,
    now: DateTime<Utc>,
) -> (TurnOutboxRecord, zaion_ledger::VerifiedEventCommit) {
    let claim = store
        .claim_next_outbox(tenant_id, owner, now, Duration::seconds(30))
        .unwrap()
        .unwrap();
    let ledger = EventLedger::new(store.db_path());
    let validated = store
        .revalidate_outbox_for_signing(
            tenant_id,
            &claim.outbox_id,
            owner,
            claim.lease_token.as_deref().unwrap(),
            now,
            &ledger,
            &keypair.public_key_bytes(),
        )
        .unwrap();
    let commit = ledger
        .append_verified_idempotent_event(keypair, validated.binding())
        .unwrap();
    store
        .complete_outbox(
            tenant_id,
            &claim.outbox_id,
            owner,
            claim.lease_token.as_deref().unwrap(),
            &commit,
            now,
            &ledger,
        )
        .unwrap();
    (claim, commit)
}

#[test]
fn default_dispatcher_config_is_valid() {
    OutboxDispatcherConfig::default().validate().unwrap();

    let config = OutboxDispatcherConfig {
        shutdown_timeout: StdDuration::ZERO,
        ..OutboxDispatcherConfig::default()
    };
    assert!(matches!(
        config.validate(),
        Err(OutboxDispatcherError::InvalidConfig(_))
    ));

    let config = OutboxDispatcherConfig {
        shutdown_timeout: StdDuration::from_secs(31),
        ..OutboxDispatcherConfig::default()
    };
    assert!(matches!(
        config.validate(),
        Err(OutboxDispatcherError::InvalidConfig(_))
    ));
}

#[test]
fn start_reports_each_worker_as_running_before_returning() {
    let (_directory, store) = open_store();
    let keypair = ZaionKeypair::generate();
    let dispatcher = OutboxDispatcher::start(
        store,
        Arc::new(InMemoryOutboxSignerResolver::new(vec![keypair])),
        test_config(),
    )
    .unwrap();
    let health = dispatcher.health();
    assert_eq!(health.lifecycle, OutboxDispatcherLifecycle::Running);
    assert_eq!(health.running_workers, health.configured_workers);
    dispatcher.shutdown().unwrap();
}

#[test]
fn workers_dispatch_concurrent_tenants_and_preserve_same_tenant_order() {
    let (_directory, store) = open_store();
    let first_key = ZaionKeypair::generate();
    let second_key = ZaionKeypair::generate();
    let now = Utc::now();
    let first = begin_turn(&store, "tenant-a", "a-first", &first_key, now);
    let second = begin_turn(
        &store,
        "tenant-a",
        "a-second",
        &first_key,
        now + Duration::milliseconds(1),
    );
    begin_turn(&store, "tenant-b", "b-first", &second_key, now);

    let resolver = Arc::new(InMemoryOutboxSignerResolver::new(vec![
        first_key.clone(),
        second_key.clone(),
    ]));
    let dispatcher = OutboxDispatcher::start(store.clone(), resolver, test_config()).unwrap();
    dispatcher.wake();
    wait_until(|| {
        let health = dispatcher.health();
        health.queue_depth == Some(0) && health.successes == 3
    });
    dispatcher.shutdown().unwrap();

    assert!(store.undelivered_outbox("tenant-a", 10).unwrap().is_empty());
    assert!(store.undelivered_outbox("tenant-b", 10).unwrap().is_empty());
    assert_eq!(event_count(store.db_path(), &first_key.principal_id()), 2);
    assert_eq!(event_count(store.db_path(), &second_key.principal_id()), 1);
    let connection = rusqlite::Connection::open(store.db_path()).unwrap();
    let payloads = connection
        .prepare("SELECT payload_json FROM events WHERE principal_id = ?1 ORDER BY seq_num")
        .unwrap()
        .query_map(params![first_key.principal_id().as_str()], |row| {
            row.get::<_, String>(0)
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    let outbox_ids = payloads
        .iter()
        .map(|payload| {
            serde_json::from_str::<Value>(payload).unwrap()["outbox_id"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect::<Vec<_>>();
    assert_eq!(outbox_ids, vec![first.outbox_id, second.outbox_id]);
}

#[test]
fn missing_signer_retries_then_persists_immutable_quarantine() {
    let (_directory, store) = open_store();
    let keypair = ZaionKeypair::generate();
    let pending = begin_turn(&store, "tenant-missing", "missing", &keypair, Utc::now());
    let follower = begin_turn(
        &store,
        "tenant-missing",
        "missing-follower",
        &keypair,
        Utc::now() + Duration::milliseconds(1),
    );
    let resolver = Arc::new(InMemoryOutboxSignerResolver::default());
    let mut config = test_config();
    config.maximum_attempts = 2;
    config.maximum_retry_delay = StdDuration::from_millis(5);
    let dispatcher = OutboxDispatcher::start(store.clone(), resolver, config).unwrap();
    dispatcher.wake();
    wait_until(|| {
        let health = dispatcher.health();
        health.persistent_dead_letters == Some(1) && health.dead_letters == 1
    });
    let health = dispatcher.health();
    assert_eq!(health.retries, 1);
    assert_eq!(health.dead_letters, 1);
    dispatcher.shutdown().unwrap();

    let quarantine = store
        .load_outbox_quarantine("tenant-missing", &pending.outbox_id)
        .unwrap()
        .unwrap();
    assert_eq!(quarantine.failure_class, "retry_exhausted");
    assert_eq!(quarantine.failure_code, "signer_missing");
    assert_eq!(quarantine.attempts, 2);
    let undelivered = store.undelivered_outbox("tenant-missing", 10).unwrap();
    assert_eq!(undelivered.len(), 2);
    assert_eq!(
        undelivered
            .iter()
            .find(|row| row.outbox_id == follower.outbox_id)
            .unwrap()
            .attempts,
        0
    );
    assert!(store
        .claim_next_outbox(
            "tenant-missing",
            "manual-worker",
            Utc::now() + Duration::minutes(1),
            Duration::seconds(30),
        )
        .unwrap()
        .is_none());

    let connection = rusqlite::Connection::open(store.db_path()).unwrap();
    assert!(connection
        .execute(
            "UPDATE turn_outbox_quarantine_v2 SET error_message = 'changed'
             WHERE tenant_id = ?1 AND outbox_id = ?2",
            params!["tenant-missing", pending.outbox_id],
        )
        .is_err());
    assert!(connection
        .execute(
            "DELETE FROM turn_outbox_quarantine_v2
             WHERE tenant_id = ?1 AND outbox_id = ?2",
            params!["tenant-missing", pending.outbox_id],
        )
        .is_err());
    DurableTurnStore::open(store.db_path()).unwrap();
}

#[test]
fn lease_renewal_is_fenced_and_preserves_attempt_count() {
    let (_directory, store) = open_store();
    let keypair = ZaionKeypair::generate();
    begin_turn(&store, "tenant-renew", "renew", &keypair, Utc::now());
    let claim = store
        .claim_next_outbox(
            "tenant-renew",
            "renew-worker",
            Utc::now(),
            Duration::seconds(1),
        )
        .unwrap()
        .unwrap();
    let renewal_now = claim.updated_at + Duration::milliseconds(1);
    let renewed = store
        .renew_outbox_lease(
            "tenant-renew",
            &claim.outbox_id,
            "renew-worker",
            claim.lease_token.as_deref().unwrap(),
            renewal_now,
            Duration::seconds(30),
        )
        .unwrap();
    assert_eq!(renewed.attempts, claim.attempts);
    assert!(renewed.lease_until > claim.lease_until);
    assert!(matches!(
        store.renew_outbox_lease(
            "tenant-renew",
            &claim.outbox_id,
            "renew-worker",
            "stale-token",
            renewal_now + Duration::milliseconds(1),
            Duration::seconds(30),
        ),
        Err(TurnStoreError::OutboxLeaseLost { .. })
    ));
}

#[test]
fn dispatcher_schema_tamper_fails_open_and_worker_health_closed() {
    let (_directory, store) = open_store();
    let keypair = ZaionKeypair::generate();
    begin_turn(&store, "tenant-schema", "schema", &keypair, Utc::now());
    let connection = rusqlite::Connection::open(store.db_path()).unwrap();
    connection
        .execute_batch(&format!("DROP TRIGGER {OUTBOX_QUARANTINE_DELETE_GUARD};"))
        .unwrap();
    assert!(matches!(
        DurableTurnStore::open(store.db_path()),
        Err(TurnStoreError::SchemaIntegrity(_))
    ));

    let resolver = Arc::new(InMemoryOutboxSignerResolver::new(vec![keypair]));
    let dispatcher = OutboxDispatcher::start(store.clone(), resolver, test_config()).unwrap();
    dispatcher.wake();
    wait_until(|| {
        let health = dispatcher.health();
        health.lifecycle == OutboxDispatcherLifecycle::Failed && health.running_workers == 0
    });
    assert!(matches!(
        dispatcher.shutdown(),
        Err(OutboxDispatcherError::WorkerFailed { .. })
    ));
    assert_eq!(
        dispatcher.health().lifecycle,
        OutboxDispatcherLifecycle::Failed
    );

    connection
        .execute_batch(CREATE_OUTBOX_QUARANTINE_DELETE_GUARD)
        .unwrap();
    DurableTurnStore::open(store.db_path()).unwrap();
}

struct WrongSignerResolver {
    keypair: Arc<ZaionKeypair>,
}

impl OutboxSignerResolver for WrongSignerResolver {
    fn resolve(
        &self,
        _principal_id: &PrincipalId,
    ) -> Result<Arc<ZaionKeypair>, OutboxSignerResolveError> {
        Ok(Arc::clone(&self.keypair))
    }
}

struct PanickingPrivateSignerResolver {
    keypair: Arc<ZaionKeypair>,
}

impl OutboxSignerResolver for PanickingPrivateSignerResolver {
    fn resolve_public_key(
        &self,
        _principal_id: &PrincipalId,
    ) -> Result<PublicKeyBytes, OutboxSignerResolveError> {
        Ok(self.keypair.public_key_bytes())
    }

    fn resolve(
        &self,
        _principal_id: &PrincipalId,
    ) -> Result<Arc<ZaionKeypair>, OutboxSignerResolveError> {
        panic!("injected private signer panic")
    }
}

#[test]
fn worker_panic_is_health_visible_and_shutdown_stays_failed() {
    let (_directory, store) = open_store();
    let keypair = ZaionKeypair::generate();
    begin_turn(&store, "tenant-panic", "panic", &keypair, Utc::now());
    let resolver = Arc::new(PanickingPrivateSignerResolver {
        keypair: Arc::new(keypair),
    });
    let mut config = test_config();
    config.worker_count = 1;
    let dispatcher = OutboxDispatcher::start(store, resolver, config).unwrap();
    dispatcher.wake();
    wait_until(|| {
        let health = dispatcher.health();
        health.lifecycle == OutboxDispatcherLifecycle::Failed && health.running_workers == 0
    });

    assert!(matches!(
        dispatcher.shutdown(),
        Err(OutboxDispatcherError::WorkerPanicked)
    ));
    assert!(matches!(
        dispatcher.shutdown(),
        Err(OutboxDispatcherError::WorkerPanicked)
    ));
}

#[test]
fn mismatched_signer_is_quarantined_before_any_signature() {
    let (_directory, store) = open_store();
    let expected = ZaionKeypair::generate();
    let wrong = ZaionKeypair::generate();
    let pending = begin_turn(&store, "tenant-wrong", "wrong", &expected, Utc::now());
    let resolver = Arc::new(WrongSignerResolver {
        keypair: Arc::new(wrong),
    });
    let dispatcher = OutboxDispatcher::start(store.clone(), resolver, test_config()).unwrap();
    dispatcher.wake();
    wait_until(|| dispatcher.health().persistent_dead_letters == Some(1));
    dispatcher.shutdown().unwrap();

    let quarantine = store
        .load_outbox_quarantine("tenant-wrong", &pending.outbox_id)
        .unwrap()
        .unwrap();
    assert_eq!(quarantine.failure_class, "permanent");
    assert_eq!(quarantine.failure_code, "signer_mismatch");
    assert_eq!(event_count(store.db_path(), &expected.principal_id()), 0);
}

struct BlockingPhaseHook {
    target: OutboxDispatchPhase,
    reached: (Mutex<bool>, Condvar),
    released: (Mutex<bool>, Condvar),
    consumed: AtomicBool,
}

struct BlockingPrivateSignerResolver {
    keypair: Arc<ZaionKeypair>,
    reached: (Mutex<bool>, Condvar),
    released: (Mutex<bool>, Condvar),
}

impl BlockingPrivateSignerResolver {
    fn new(keypair: ZaionKeypair) -> Self {
        Self {
            keypair: Arc::new(keypair),
            reached: (Mutex::new(false), Condvar::new()),
            released: (Mutex::new(false), Condvar::new()),
        }
    }

    fn wait_reached(&self) {
        let (lock, condvar) = &self.reached;
        let reached = lock.lock().unwrap();
        let (reached, timeout) = condvar
            .wait_timeout_while(reached, StdDuration::from_secs(10), |reached| !*reached)
            .unwrap();
        assert!(*reached && !timeout.timed_out());
    }

    fn release(&self) {
        let (lock, condvar) = &self.released;
        *lock.lock().unwrap() = true;
        condvar.notify_all();
    }
}

struct ReleaseBlockingResolver(Arc<BlockingPrivateSignerResolver>);

impl Drop for ReleaseBlockingResolver {
    fn drop(&mut self) {
        self.0.release();
    }
}

impl OutboxSignerResolver for BlockingPrivateSignerResolver {
    fn resolve_public_key(
        &self,
        _principal_id: &PrincipalId,
    ) -> Result<PublicKeyBytes, OutboxSignerResolveError> {
        Ok(self.keypair.public_key_bytes())
    }

    fn resolve(
        &self,
        _principal_id: &PrincipalId,
    ) -> Result<Arc<ZaionKeypair>, OutboxSignerResolveError> {
        let (reached_lock, reached_condvar) = &self.reached;
        *reached_lock.lock().unwrap() = true;
        reached_condvar.notify_all();

        let (released_lock, released_condvar) = &self.released;
        let released = released_lock.lock().unwrap();
        let (released, timeout) = released_condvar
            .wait_timeout_while(released, StdDuration::from_secs(5), |released| !*released)
            .unwrap();
        if timeout.timed_out() && !*released {
            return Err(OutboxSignerResolveError::Unavailable(
                "test signer release deadline expired".to_string(),
            ));
        }
        Ok(Arc::clone(&self.keypair))
    }
}

#[test]
fn shutdown_timeout_is_bounded_recoverable_and_prevents_late_signing() {
    let (_directory, store) = open_store();
    let keypair = ZaionKeypair::generate();
    begin_turn(
        &store,
        "tenant-blocked-signer",
        "blocked-signer",
        &keypair,
        Utc::now(),
    );
    let resolver = Arc::new(BlockingPrivateSignerResolver::new(keypair.clone()));
    let mut config = test_config();
    config.worker_count = 1;
    config.shutdown_timeout = StdDuration::from_millis(200);
    let dispatcher = OutboxDispatcher::start(store.clone(), resolver.clone(), config).unwrap();
    dispatcher.wake();
    resolver.wait_reached();
    let release_on_drop = ReleaseBlockingResolver(resolver.clone());

    let started = Instant::now();
    assert!(matches!(
        dispatcher.shutdown(),
        Err(OutboxDispatcherError::ShutdownTimeout {
            remaining_workers: 1
        })
    ));
    assert!(started.elapsed() < StdDuration::from_secs(2));
    let health = dispatcher.health();
    assert_eq!(health.lifecycle, OutboxDispatcherLifecycle::Failed);
    assert_eq!(health.running_workers, 1);
    assert!(dispatcher.join_timed_out.load(Ordering::Acquire));

    resolver.release();
    wait_until(|| dispatcher.health().running_workers == 0);
    dispatcher.shutdown().unwrap();
    assert!(!dispatcher.join_timed_out.load(Ordering::Acquire));
    assert_eq!(
        dispatcher.health().lifecycle,
        OutboxDispatcherLifecycle::Stopped
    );

    let pending = store
        .undelivered_outbox("tenant-blocked-signer", 10)
        .unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].status, TurnOutboxStatus::Pending);
    assert_eq!(event_count(store.db_path(), &keypair.principal_id()), 0);
    drop(release_on_drop);
}

#[test]
fn concurrent_shutdown_callers_honor_their_own_deadlines() {
    let (_directory, store) = open_store();
    let keypair = ZaionKeypair::generate();
    begin_turn(
        &store,
        "tenant-deadlines",
        "deadlines",
        &keypair,
        Utc::now(),
    );
    let resolver = Arc::new(BlockingPrivateSignerResolver::new(keypair.clone()));
    let mut config = test_config();
    config.worker_count = 1;
    config.shutdown_timeout = StdDuration::from_secs(2);
    let dispatcher = OutboxDispatcher::start(store.clone(), resolver.clone(), config).unwrap();
    dispatcher.wake();
    resolver.wait_reached();
    let release_on_drop = ReleaseBlockingResolver(resolver.clone());

    std::thread::scope(|scope| {
        let long =
            scope.spawn(|| dispatcher.shutdown_before(Instant::now() + StdDuration::from_secs(1)));
        std::thread::sleep(StdDuration::from_millis(25));
        let started = Instant::now();
        assert!(matches!(
            dispatcher.shutdown_before(started + StdDuration::from_millis(150)),
            Err(OutboxDispatcherError::ShutdownTimeout {
                remaining_workers: 1
            })
        ));
        assert!(started.elapsed() < StdDuration::from_millis(500));
        resolver.release();
        assert!(long.join().unwrap().is_ok());
    });

    assert_eq!(
        dispatcher.health().lifecycle,
        OutboxDispatcherLifecycle::Stopped
    );
    assert_eq!(event_count(store.db_path(), &keypair.principal_id()), 0);
    drop(release_on_drop);
}

#[test]
fn drop_after_shutdown_timeout_hands_the_worker_to_a_reaper() {
    let (_directory, store) = open_store();
    let keypair = ZaionKeypair::generate();
    begin_turn(&store, "tenant-reaper", "reaper", &keypair, Utc::now());
    let resolver = Arc::new(BlockingPrivateSignerResolver::new(keypair.clone()));
    let mut config = test_config();
    config.worker_count = 1;
    config.shutdown_timeout = StdDuration::from_millis(100);
    let dispatcher = OutboxDispatcher::start(store.clone(), resolver.clone(), config).unwrap();
    dispatcher.wake();
    resolver.wait_reached();
    let release_on_drop = ReleaseBlockingResolver(resolver.clone());
    assert!(matches!(
        dispatcher.shutdown(),
        Err(OutboxDispatcherError::ShutdownTimeout {
            remaining_workers: 1
        })
    ));

    let started = Instant::now();
    drop(dispatcher);
    assert!(started.elapsed() < StdDuration::from_secs(1));
    resolver.release();
    wait_until(|| {
        store
            .undelivered_outbox("tenant-reaper", 10)
            .is_ok_and(|pending| {
                pending.len() == 1 && pending[0].status == TurnOutboxStatus::Pending
            })
    });
    assert_eq!(event_count(store.db_path(), &keypair.principal_id()), 0);
    drop(release_on_drop);
}

impl BlockingPhaseHook {
    fn new(target: OutboxDispatchPhase) -> Self {
        Self {
            target,
            reached: (Mutex::new(false), Condvar::new()),
            released: (Mutex::new(false), Condvar::new()),
            consumed: AtomicBool::new(false),
        }
    }

    fn wait_reached(&self) {
        let (lock, condvar) = &self.reached;
        let reached = lock.lock().unwrap();
        let (reached, timeout) = condvar
            .wait_timeout_while(reached, StdDuration::from_secs(10), |reached| !*reached)
            .unwrap();
        assert!(*reached && !timeout.timed_out());
    }

    fn release(&self) {
        let (lock, condvar) = &self.released;
        *lock.lock().unwrap() = true;
        condvar.notify_all();
    }
}

impl DispatcherTestHook for BlockingPhaseHook {
    fn reached(&self, phase: OutboxDispatchPhase, _outbox: &TurnOutboxRecord) {
        if phase != self.target || self.consumed.swap(true, Ordering::AcqRel) {
            return;
        }
        let (reached_lock, reached_condvar) = &self.reached;
        *reached_lock.lock().unwrap() = true;
        reached_condvar.notify_all();
        let (released_lock, released_condvar) = &self.released;
        let released = released_lock.lock().unwrap();
        let _ = released_condvar
            .wait_timeout_while(released, StdDuration::from_secs(10), |released| !*released)
            .unwrap();
    }
}

fn assert_shutdown_at_phase(phase: OutboxDispatchPhase, should_commit: bool) {
    let (_directory, store) = open_store();
    let keypair = ZaionKeypair::generate();
    begin_turn(&store, "tenant-stop", "stop", &keypair, Utc::now());
    let hook = Arc::new(BlockingPhaseHook::new(phase));
    let mut config = test_config();
    config.worker_count = 1;
    config.test_hook = Some(hook.clone());
    let resolver = Arc::new(InMemoryOutboxSignerResolver::new(vec![keypair.clone()]));
    let dispatcher = OutboxDispatcher::start(store.clone(), resolver, config).unwrap();
    dispatcher.wake();
    hook.wait_reached();
    dispatcher.request_shutdown();
    hook.release();
    dispatcher.shutdown().unwrap();

    if should_commit {
        assert!(store
            .undelivered_outbox("tenant-stop", 10)
            .unwrap()
            .is_empty());
        assert_eq!(event_count(store.db_path(), &keypair.principal_id()), 1);
    } else {
        let pending = store.undelivered_outbox("tenant-stop", 10).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].status, TurnOutboxStatus::Pending);
        assert_eq!(event_count(store.db_path(), &keypair.principal_id()), 0);
    }
}

#[test]
fn shutdown_during_claim_resolve_and_revalidate_releases_without_signing() {
    for phase in [
        OutboxDispatchPhase::Claim,
        OutboxDispatchPhase::ResolveSigner,
        OutboxDispatchPhase::Revalidate,
    ] {
        assert_shutdown_at_phase(phase, false);
    }
}

#[test]
fn shutdown_after_append_admission_and_during_complete_finishes_verified_delivery() {
    for phase in [OutboxDispatchPhase::Append, OutboxDispatchPhase::Complete] {
        assert_shutdown_at_phase(phase, true);
    }
}

#[test]
fn append_crash_is_reclaimed_without_double_append_or_stale_release() {
    let (_directory, store) = open_store();
    let keypair = ZaionKeypair::generate();
    let base = Utc::now() - Duration::seconds(3);
    begin_turn(&store, "tenant-crash", "crash", &keypair, base);
    let old_claim = store
        .claim_next_outbox(
            "tenant-crash",
            "crashed-worker",
            base + Duration::seconds(1),
            Duration::seconds(1),
        )
        .unwrap()
        .unwrap();
    let ledger = EventLedger::new(store.db_path());
    let validated = store
        .revalidate_outbox_for_signing(
            "tenant-crash",
            &old_claim.outbox_id,
            "crashed-worker",
            old_claim.lease_token.as_deref().unwrap(),
            base + Duration::milliseconds(1500),
            &ledger,
            &keypair.public_key_bytes(),
        )
        .unwrap();
    ledger
        .append_verified_idempotent_event(&keypair, validated.binding())
        .unwrap();

    let resolver = Arc::new(InMemoryOutboxSignerResolver::new(vec![keypair.clone()]));
    let dispatcher = OutboxDispatcher::start(store.clone(), resolver, test_config()).unwrap();
    dispatcher.wake();
    wait_until(|| {
        store
            .undelivered_outbox("tenant-crash", 10)
            .unwrap()
            .is_empty()
    });
    dispatcher.shutdown().unwrap();
    assert_eq!(event_count(store.db_path(), &keypair.principal_id()), 1);
    assert!(store
        .release_outbox(
            "tenant-crash",
            &old_claim.outbox_id,
            "crashed-worker",
            old_claim.lease_token.as_deref().unwrap(),
            Utc::now(),
            Utc::now(),
            "stale release",
        )
        .is_err());
}

#[test]
fn retired_private_key_completes_an_already_signed_deterministic_append() {
    let (_directory, store) = open_store();
    let keypair = ZaionKeypair::generate();
    let base = Utc::now() - Duration::seconds(3);
    begin_turn(&store, "tenant-key-retired", "retired", &keypair, base);
    let claim = store
        .claim_next_outbox(
            "tenant-key-retired",
            "crashed-worker",
            base + Duration::seconds(1),
            Duration::seconds(1),
        )
        .unwrap()
        .unwrap();
    let ledger = EventLedger::new(store.db_path());
    let validated = store
        .revalidate_outbox_for_signing(
            "tenant-key-retired",
            &claim.outbox_id,
            "crashed-worker",
            claim.lease_token.as_deref().unwrap(),
            base + Duration::milliseconds(1500),
            &ledger,
            &keypair.public_key_bytes(),
        )
        .unwrap();
    ledger
        .append_verified_idempotent_event(&keypair, validated.binding())
        .unwrap();

    let resolver = Arc::new(InMemoryOutboxSignerResolver::new(vec![keypair.clone()]));
    assert!(resolver.remove(&keypair.principal_id()).is_some());
    let dispatcher = OutboxDispatcher::start(store.clone(), resolver, test_config()).unwrap();
    dispatcher.wake();
    wait_until(|| {
        store
            .undelivered_outbox("tenant-key-retired", 10)
            .unwrap()
            .is_empty()
    });
    dispatcher.shutdown().unwrap();

    assert_eq!(event_count(store.db_path(), &keypair.principal_id()), 1);
    assert!(store
        .load_outbox_quarantine("tenant-key-retired", &claim.outbox_id)
        .unwrap()
        .is_none());
}

#[test]
fn concurrent_dispatcher_instances_fence_to_one_event_and_delivery() {
    let (_directory, store) = open_store();
    let keypair = ZaionKeypair::generate();
    begin_turn(&store, "tenant-race", "race", &keypair, Utc::now());
    let resolver = Arc::new(InMemoryOutboxSignerResolver::new(vec![keypair.clone()]));
    let first = OutboxDispatcher::start(store.clone(), resolver.clone(), test_config()).unwrap();
    let second = OutboxDispatcher::start(store.clone(), resolver, test_config()).unwrap();
    first.wake();
    second.wake();
    wait_until(|| {
        store
            .undelivered_outbox("tenant-race", 10)
            .unwrap()
            .is_empty()
    });
    first.shutdown().unwrap();
    second.shutdown().unwrap();
    assert_eq!(event_count(store.db_path(), &keypair.principal_id()), 1);
    assert_eq!(first.health().successes + second.health().successes, 1);
}

#[test]
fn tampered_delivered_prefix_fails_closed_and_quarantines_follower() {
    let (_directory, store) = open_store();
    let keypair = ZaionKeypair::generate();
    let now = Utc::now();
    begin_turn(&store, "tenant-tamper", "first", &keypair, now);
    deliver_claim(
        &store,
        "tenant-tamper",
        "manual-first",
        &keypair,
        now + Duration::milliseconds(1),
    );
    let follower = begin_turn(
        &store,
        "tenant-tamper",
        "second",
        &keypair,
        now + Duration::milliseconds(2),
    );
    rusqlite::Connection::open(store.db_path())
        .unwrap()
        .execute(
            "UPDATE events SET signature_hex = lower(hex(zeroblob(64)))
             WHERE principal_id = ?1",
            params![keypair.principal_id().as_str()],
        )
        .unwrap();

    let resolver = Arc::new(InMemoryOutboxSignerResolver::new(vec![keypair]));
    let dispatcher = OutboxDispatcher::start(store.clone(), resolver, test_config()).unwrap();
    dispatcher.wake();
    wait_until(|| {
        store
            .load_outbox_quarantine("tenant-tamper", &follower.outbox_id)
            .unwrap()
            .is_some()
    });
    let quarantine = store
        .load_outbox_quarantine("tenant-tamper", &follower.outbox_id)
        .unwrap()
        .unwrap();
    assert_eq!(quarantine.failure_code, "ledger_integrity");
    assert_eq!(dispatcher.health().dead_letters, 1);
    dispatcher.shutdown().unwrap();
}
