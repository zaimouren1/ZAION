use std::collections::HashMap;
use std::io::ErrorKind;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration as StdDuration, Instant};

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use zaion_crypto::{principal_id_from_public_key, ZaionKeypair};
use zaion_ledger::{EventLedger, LedgerError};
use zaion_types::identity::{PrincipalId, PublicKeyBytes};

use super::{DurableTurnStore, OutboxCompletion, TurnOutboxRecord, TurnStoreError};
use super::{MAX_OUTBOX_ERROR_BYTES, MAX_OUTBOX_LEASE_SECONDS};

mod persistence;
use persistence::claimed_principal_id;
pub(super) use persistence::{
    ensure_outbox_dispatcher_schema, outbox_is_quarantined, validate_outbox_dispatcher_triggers,
};
const OUTBOX_QUARANTINE_MIGRATION_ID: &str = "turn_outbox_quarantine_v1";
const OUTBOX_QUARANTINE_MIGRATION_KIND: &str = "immutable_dispatch_quarantine_v1";
const OUTBOX_QUARANTINE_TABLE: &str = "turn_outbox_quarantine_v2";
pub(super) const OUTBOX_QUARANTINE_INSERT_GUARD: &str = "turn_outbox_quarantine_v2_insert_state";
pub(super) const OUTBOX_QUARANTINE_UPDATE_GUARD: &str = "turn_outbox_quarantine_v2_no_update";
pub(super) const OUTBOX_QUARANTINE_DELETE_GUARD: &str = "turn_outbox_quarantine_v2_no_delete";

const CREATE_OUTBOX_QUARANTINE_TABLE: &str = r#"
CREATE TABLE turn_outbox_quarantine_v2 (
    tenant_id TEXT NOT NULL,
    outbox_id TEXT NOT NULL,
    commit_ordinal INTEGER NOT NULL CHECK (commit_ordinal > 0),
    principal_id TEXT NOT NULL CHECK (length(principal_id) > 0),
    failure_class TEXT NOT NULL CHECK (
        failure_class IN ('permanent', 'retry_exhausted')
    ),
    failure_code TEXT NOT NULL CHECK (
        length(failure_code) > 0
        AND length(CAST(failure_code AS BLOB)) <= 128
    ),
    failure_phase TEXT NOT NULL CHECK (
        failure_phase IN (
            'claim', 'resolve_signer', 'revalidate', 'append', 'complete',
            'retry_persist', 'quarantine_persist'
        )
    ),
    error_message TEXT NOT NULL CHECK (
        length(error_message) > 0
        AND length(CAST(error_message AS BLOB)) <= 4096
    ),
    attempts INTEGER NOT NULL CHECK (attempts > 0),
    lease_owner TEXT NOT NULL,
    lease_token TEXT NOT NULL,
    payload_hash TEXT NOT NULL,
    quarantined_at_ms INTEGER NOT NULL,
    PRIMARY KEY (tenant_id, outbox_id),
    UNIQUE (commit_ordinal),
    FOREIGN KEY (tenant_id, outbox_id)
        REFERENCES turn_outbox_v2(tenant_id, outbox_id)
        ON DELETE RESTRICT
);
"#;

const CREATE_OUTBOX_QUARANTINE_INSERT_GUARD: &str = r#"
CREATE TRIGGER turn_outbox_quarantine_v2_insert_state
BEFORE INSERT ON turn_outbox_quarantine_v2
WHEN NOT EXISTS (
    SELECT 1
    FROM turn_outbox_v2 o
    JOIN turn_outbox_commit_order_v2 c
      ON c.tenant_id = o.tenant_id AND c.outbox_id = o.outbox_id
    JOIN turn_state_v2 s
      ON s.tenant_id = o.tenant_id AND s.turn_id = o.turn_id
    WHERE o.tenant_id = NEW.tenant_id AND o.outbox_id = NEW.outbox_id
      AND o.status = 'leased'
      AND o.lease_owner = NEW.lease_owner
      AND o.lease_token = NEW.lease_token
      AND o.attempts = NEW.attempts
      AND o.payload_hash = NEW.payload_hash
      AND c.commit_ordinal = NEW.commit_ordinal
      AND s.principal_id = NEW.principal_id
      AND o.lease_until_ms > NEW.quarantined_at_ms
      AND c.commit_ordinal = (
          SELECT MIN(c2.commit_ordinal)
          FROM turn_outbox_commit_order_v2 c2
          JOIN turn_outbox_v2 o2
            ON o2.tenant_id = c2.tenant_id AND o2.outbox_id = c2.outbox_id
          WHERE o2.tenant_id = NEW.tenant_id AND o2.status != 'delivered'
      )
)
BEGIN
    SELECT RAISE(ABORT, 'outbox quarantine requires the matching fenced lease');
END;
"#;

const CREATE_OUTBOX_QUARANTINE_UPDATE_GUARD: &str = r#"
CREATE TRIGGER turn_outbox_quarantine_v2_no_update
BEFORE UPDATE ON turn_outbox_quarantine_v2
BEGIN
    SELECT RAISE(ABORT, 'outbox quarantine evidence is immutable');
END;
"#;

const CREATE_OUTBOX_QUARANTINE_DELETE_GUARD: &str = r#"
CREATE TRIGGER turn_outbox_quarantine_v2_no_delete
BEFORE DELETE ON turn_outbox_quarantine_v2
BEGIN
    SELECT RAISE(ABORT, 'outbox quarantine evidence is immutable');
END;
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutboxDispatchPhase {
    Claim,
    ResolveSigner,
    Revalidate,
    Append,
    Complete,
    RetryPersist,
    QuarantinePersist,
}

impl OutboxDispatchPhase {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Claim => "claim",
            Self::ResolveSigner => "resolve_signer",
            Self::Revalidate => "revalidate",
            Self::Append => "append",
            Self::Complete => "complete",
            Self::RetryPersist => "retry_persist",
            Self::QuarantinePersist => "quarantine_persist",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutboxDispatchFailureClass {
    Retryable,
    Permanent,
    LeaseLost,
    Fatal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutboxDispatchFailureCode {
    SignerMissing,
    SignerUnavailable,
    SignerInvalid,
    SignerMismatch,
    StoreBusy,
    StoreIo,
    StoreIntegrity,
    LedgerBusy,
    LedgerIo,
    LedgerIntegrity,
    LeaseLost,
    LeaseBudgetExhausted,
    ShutdownTimeout,
    InfrastructureFailure,
}

impl OutboxDispatchFailureCode {
    const fn as_str(self) -> &'static str {
        match self {
            Self::SignerMissing => "signer_missing",
            Self::SignerUnavailable => "signer_unavailable",
            Self::SignerInvalid => "signer_invalid",
            Self::SignerMismatch => "signer_mismatch",
            Self::StoreBusy => "store_busy",
            Self::StoreIo => "store_io",
            Self::StoreIntegrity => "store_integrity",
            Self::LedgerBusy => "ledger_busy",
            Self::LedgerIo => "ledger_io",
            Self::LedgerIntegrity => "ledger_integrity",
            Self::LeaseLost => "lease_lost",
            Self::LeaseBudgetExhausted => "lease_budget_exhausted",
            Self::ShutdownTimeout => "shutdown_timeout",
            Self::InfrastructureFailure => "infrastructure_failure",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboxDispatchFailure {
    pub phase: OutboxDispatchPhase,
    pub class: OutboxDispatchFailureClass,
    pub code: OutboxDispatchFailureCode,
    pub message: String,
}

impl OutboxDispatchFailure {
    fn new(
        phase: OutboxDispatchPhase,
        class: OutboxDispatchFailureClass,
        code: OutboxDispatchFailureCode,
        message: impl Into<String>,
    ) -> Self {
        Self {
            phase,
            class,
            code,
            message: bounded_error_message(message.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboxQuarantineRecord {
    pub tenant_id: String,
    pub outbox_id: String,
    pub commit_ordinal: u64,
    pub principal_id: String,
    pub failure_class: String,
    pub failure_code: String,
    pub failure_phase: String,
    pub error_message: String,
    pub attempts: u64,
    pub lease_owner: String,
    pub lease_token: String,
    pub payload_hash: String,
    pub quarantined_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutboxDispatcherLifecycle {
    Running,
    Stopping,
    Stopped,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboxDispatcherLastError {
    pub tenant_id: Option<String>,
    pub outbox_id: Option<String>,
    pub occurred_at: DateTime<Utc>,
    pub failure: OutboxDispatchFailure,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboxDispatcherHealth {
    pub lifecycle: OutboxDispatcherLifecycle,
    pub configured_workers: usize,
    pub running_workers: usize,
    pub queue_depth: Option<u64>,
    pub leased_count: Option<u64>,
    pub oldest_queued_age_ms: Option<u64>,
    pub persistent_dead_letters: Option<u64>,
    pub successes: u64,
    pub retries: u64,
    pub dead_letters: u64,
    pub last_error: Option<OutboxDispatcherLastError>,
    pub metrics_error: Option<String>,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum OutboxSignerResolveError {
    #[error("signer is not available for principal {principal_id}")]
    Missing { principal_id: String },
    #[error("signer backend is temporarily unavailable: {0}")]
    Unavailable(String),
    #[error("signer backend returned invalid key material: {0}")]
    Invalid(String),
}

pub trait OutboxSignerResolver: Send + Sync + 'static {
    /// Resolve non-secret verification material for a principal. Implementors
    /// that retire private keys must retain this material long enough to finish
    /// a deterministic append that was durably signed before a crash.
    fn resolve_public_key(
        &self,
        principal_id: &PrincipalId,
    ) -> Result<PublicKeyBytes, OutboxSignerResolveError> {
        self.resolve(principal_id)
            .map(|keypair| keypair.public_key_bytes())
    }

    fn resolve(
        &self,
        principal_id: &PrincipalId,
    ) -> Result<Arc<ZaionKeypair>, OutboxSignerResolveError>;
}

#[derive(Default)]
pub struct InMemoryOutboxSignerResolver {
    signers: RwLock<HashMap<String, Arc<ZaionKeypair>>>,
    public_keys: RwLock<HashMap<String, PublicKeyBytes>>,
}

impl InMemoryOutboxSignerResolver {
    pub fn new(keypairs: impl IntoIterator<Item = ZaionKeypair>) -> Self {
        let resolver = Self::default();
        for keypair in keypairs {
            resolver.insert(keypair);
        }
        resolver
    }

    pub fn insert(&self, keypair: ZaionKeypair) -> Option<Arc<ZaionKeypair>> {
        let principal_id = keypair.principal_id().as_str().to_string();
        self.public_keys
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(principal_id.clone(), keypair.public_key_bytes());
        self.signers
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(principal_id, Arc::new(keypair))
    }

    pub fn remove(&self, principal_id: &PrincipalId) -> Option<Arc<ZaionKeypair>> {
        self.signers
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(principal_id.as_str())
    }
}

impl OutboxSignerResolver for InMemoryOutboxSignerResolver {
    fn resolve_public_key(
        &self,
        principal_id: &PrincipalId,
    ) -> Result<PublicKeyBytes, OutboxSignerResolveError> {
        self.public_keys
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(principal_id.as_str())
            .cloned()
            .ok_or_else(|| OutboxSignerResolveError::Missing {
                principal_id: principal_id.as_str().to_string(),
            })
    }

    fn resolve(
        &self,
        principal_id: &PrincipalId,
    ) -> Result<Arc<ZaionKeypair>, OutboxSignerResolveError> {
        self.signers
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(principal_id.as_str())
            .cloned()
            .ok_or_else(|| OutboxSignerResolveError::Missing {
                principal_id: principal_id.as_str().to_string(),
            })
    }
}

#[derive(Clone)]
pub struct OutboxDispatcherConfig {
    pub worker_count: usize,
    pub tenant_scan_limit: usize,
    pub lease_duration: StdDuration,
    pub poll_interval: StdDuration,
    pub initial_retry_delay: StdDuration,
    pub maximum_retry_delay: StdDuration,
    pub minimum_commit_window: StdDuration,
    pub shutdown_timeout: StdDuration,
    pub retry_jitter_percent: u8,
    pub maximum_attempts: u64,
    #[cfg(test)]
    test_hook: Option<Arc<dyn DispatcherTestHook>>,
}

impl std::fmt::Debug for OutboxDispatcherConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OutboxDispatcherConfig")
            .field("worker_count", &self.worker_count)
            .field("tenant_scan_limit", &self.tenant_scan_limit)
            .field("lease_duration", &self.lease_duration)
            .field("poll_interval", &self.poll_interval)
            .field("initial_retry_delay", &self.initial_retry_delay)
            .field("maximum_retry_delay", &self.maximum_retry_delay)
            .field("minimum_commit_window", &self.minimum_commit_window)
            .field("shutdown_timeout", &self.shutdown_timeout)
            .field("retry_jitter_percent", &self.retry_jitter_percent)
            .field("maximum_attempts", &self.maximum_attempts)
            .finish_non_exhaustive()
    }
}

impl Default for OutboxDispatcherConfig {
    fn default() -> Self {
        Self {
            worker_count: 2,
            tenant_scan_limit: 128,
            lease_duration: StdDuration::from_secs(30),
            poll_interval: StdDuration::from_millis(100),
            initial_retry_delay: StdDuration::from_millis(250),
            maximum_retry_delay: StdDuration::from_secs(30),
            minimum_commit_window: StdDuration::from_secs(12),
            shutdown_timeout: StdDuration::from_secs(15),
            retry_jitter_percent: 20,
            maximum_attempts: 8,
            #[cfg(test)]
            test_hook: None,
        }
    }
}

impl OutboxDispatcherConfig {
    fn validate(&self) -> Result<(), OutboxDispatcherError> {
        if !(1..=32).contains(&self.worker_count) {
            return Err(OutboxDispatcherError::InvalidConfig(
                "worker_count must be between 1 and 32",
            ));
        }
        if self.tenant_scan_limit == 0 || self.tenant_scan_limit > 10_000 {
            return Err(OutboxDispatcherError::InvalidConfig(
                "tenant_scan_limit must be between 1 and 10000",
            ));
        }
        if self.lease_duration < StdDuration::from_secs(1)
            || self.lease_duration > StdDuration::from_secs(MAX_OUTBOX_LEASE_SECONDS as u64)
        {
            return Err(OutboxDispatcherError::InvalidConfig(
                "lease_duration is outside the TurnStore lease boundary",
            ));
        }
        if self.poll_interval.is_zero() {
            return Err(OutboxDispatcherError::InvalidConfig(
                "poll_interval must be non-zero",
            ));
        }
        if self.initial_retry_delay.is_zero() || self.initial_retry_delay > self.maximum_retry_delay
        {
            return Err(OutboxDispatcherError::InvalidConfig(
                "retry delays must be non-zero and monotonically bounded",
            ));
        }
        if self.minimum_commit_window.is_zero() || self.minimum_commit_window >= self.lease_duration
        {
            return Err(OutboxDispatcherError::InvalidConfig(
                "minimum_commit_window must be non-zero and shorter than lease_duration",
            ));
        }
        if self.shutdown_timeout.is_zero() || self.shutdown_timeout > StdDuration::from_secs(30) {
            return Err(OutboxDispatcherError::InvalidConfig(
                "shutdown_timeout must be between 1 nanosecond and 30 seconds",
            ));
        }
        if self.retry_jitter_percent > 100 {
            return Err(OutboxDispatcherError::InvalidConfig(
                "retry_jitter_percent must not exceed 100",
            ));
        }
        if self.maximum_attempts == 0 || self.maximum_attempts > 10_000 {
            return Err(OutboxDispatcherError::InvalidConfig(
                "maximum_attempts must be between 1 and 10000",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum OutboxDispatcherError {
    #[error("invalid outbox dispatcher configuration: {0}")]
    InvalidConfig(&'static str),
    #[error("failed to spawn outbox dispatcher worker: {0}")]
    WorkerSpawn(#[source] std::io::Error),
    #[error("outbox dispatcher worker panicked")]
    WorkerPanicked,
    #[error("outbox dispatcher failed: {message}")]
    WorkerFailed { message: String },
    #[error(
        "outbox dispatcher shutdown timed out with {remaining_workers} worker(s) still running"
    )]
    ShutdownTimeout { remaining_workers: usize },
}

#[derive(Debug)]
struct QueueSnapshot {
    ready_tenants: Vec<String>,
    queue_depth: u64,
    leased_count: u64,
    oldest_created_at: Option<DateTime<Utc>>,
    dead_letters: u64,
}

#[derive(Debug, Clone)]
struct RuntimeHealth {
    lifecycle: OutboxDispatcherLifecycle,
    running_workers: usize,
    successes: u64,
    retries: u64,
    dead_letters: u64,
    last_error: Option<OutboxDispatcherLastError>,
}

#[derive(Debug)]
struct AppendAdmissionState {
    accepting: bool,
    in_flight: usize,
}

struct AppendPermit<'a> {
    state: &'a Mutex<AppendAdmissionState>,
}

impl Drop for AppendPermit<'_> {
    fn drop(&mut self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.in_flight = state.in_flight.saturating_sub(1);
    }
}

struct DispatcherShared {
    store: DurableTurnStore,
    resolver: Arc<dyn OutboxSignerResolver>,
    config: OutboxDispatcherConfig,
    shutdown: AtomicBool,
    wake_generation: Mutex<u64>,
    wake_condvar: Condvar,
    startup_ready_workers: Mutex<usize>,
    startup_ready_condvar: Condvar,
    startup_released: AtomicBool,
    append_admission: Mutex<AppendAdmissionState>,
    health: Mutex<RuntimeHealth>,
}

struct WorkerLifecycleGuard<'a> {
    shared: &'a DispatcherShared,
}

impl Drop for WorkerLifecycleGuard<'_> {
    fn drop(&mut self) {
        if thread::panicking() {
            fail_dispatcher(
                self.shared,
                None,
                None,
                OutboxDispatchFailure::new(
                    OutboxDispatchPhase::Claim,
                    OutboxDispatchFailureClass::Fatal,
                    OutboxDispatchFailureCode::InfrastructureFailure,
                    "outbox dispatcher worker panicked",
                ),
            );
        }
        worker_stopped(self.shared);
    }
}

pub struct OutboxDispatcher {
    shared: Arc<DispatcherShared>,
    workers: Mutex<Vec<JoinHandle<()>>>,
    join_timed_out: AtomicBool,
    worker_panicked: AtomicBool,
}

impl OutboxDispatcher {
    pub fn start(
        store: DurableTurnStore,
        resolver: Arc<dyn OutboxSignerResolver>,
        config: OutboxDispatcherConfig,
    ) -> Result<Self, OutboxDispatcherError> {
        config.validate()?;
        let shared = Arc::new(DispatcherShared {
            store,
            resolver,
            config,
            shutdown: AtomicBool::new(false),
            wake_generation: Mutex::new(0),
            wake_condvar: Condvar::new(),
            startup_ready_workers: Mutex::new(0),
            startup_ready_condvar: Condvar::new(),
            startup_released: AtomicBool::new(false),
            append_admission: Mutex::new(AppendAdmissionState {
                accepting: true,
                in_flight: 0,
            }),
            health: Mutex::new(RuntimeHealth {
                lifecycle: OutboxDispatcherLifecycle::Running,
                running_workers: 0,
                successes: 0,
                retries: 0,
                dead_letters: 0,
                last_error: None,
            }),
        });
        let mut workers = Vec::with_capacity(shared.config.worker_count);
        for index in 0..shared.config.worker_count {
            let worker_shared = Arc::clone(&shared);
            match thread::Builder::new()
                .name(format!("zaion-outbox-{index}"))
                .spawn(move || worker_loop(worker_shared))
            {
                Ok(worker) => workers.push(worker),
                Err(error) => {
                    shared.shutdown.store(true, Ordering::Release);
                    shared.wake_condvar.notify_all();
                    shared.startup_ready_condvar.notify_all();
                    for worker in workers {
                        let _ = worker.join();
                    }
                    return Err(OutboxDispatcherError::WorkerSpawn(error));
                }
            }
        }
        {
            let ready = shared
                .startup_ready_workers
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            drop(
                shared
                    .startup_ready_condvar
                    .wait_while(ready, |ready| *ready < shared.config.worker_count)
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            );
        }
        shared.startup_released.store(true, Ordering::Release);
        shared.startup_ready_condvar.notify_all();
        Ok(Self {
            shared,
            workers: Mutex::new(workers),
            join_timed_out: AtomicBool::new(false),
            worker_panicked: AtomicBool::new(false),
        })
    }

    pub fn wake(&self) {
        let mut generation = self
            .shared
            .wake_generation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *generation = generation.wrapping_add(1);
        self.shared.wake_condvar.notify_all();
    }

    pub fn request_shutdown(&self) {
        {
            let mut admission = self
                .shared
                .append_admission
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            admission.accepting = false;
            let first_request = !self.shared.shutdown.load(Ordering::Acquire);
            if first_request {
                let mut health = self
                    .shared
                    .health
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if health.lifecycle == OutboxDispatcherLifecycle::Running {
                    health.lifecycle = OutboxDispatcherLifecycle::Stopping;
                }
                self.shared.shutdown.store(true, Ordering::Release);
            }
        }
        self.shared.wake_condvar.notify_all();
    }

    pub fn shutdown(&self) -> Result<(), OutboxDispatcherError> {
        let deadline = Instant::now() + self.shared.config.shutdown_timeout;
        self.shutdown_before(deadline)
    }

    /// Stop accepting work and wait only until a caller-owned aggregate
    /// deadline. This lets a supervisor bound many dispatchers as one unit.
    pub fn shutdown_before(&self, deadline: Instant) -> Result<(), OutboxDispatcherError> {
        self.request_shutdown();
        let mut remaining_workers;
        loop {
            remaining_workers = {
                let mut workers = self
                    .workers
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                reap_finished_workers(&mut workers, &self.worker_panicked);
                workers.len()
            };
            if remaining_workers == 0 || Instant::now() >= deadline {
                break;
            }
            thread::sleep(
                deadline
                    .saturating_duration_since(Instant::now())
                    .min(StdDuration::from_millis(5)),
            );
        }
        remaining_workers = {
            let mut workers = self
                .workers
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            reap_finished_workers(&mut workers, &self.worker_panicked);
            workers.len()
        };
        self.join_timed_out
            .store(remaining_workers > 0, Ordering::Release);
        let panicked = self.worker_panicked.load(Ordering::Acquire);
        let mut health = self
            .shared
            .health
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if panicked || remaining_workers > 0 {
            health.lifecycle = OutboxDispatcherLifecycle::Failed;
            if remaining_workers > 0
                && health.last_error.as_ref().is_none_or(|error| {
                    error.failure.code == OutboxDispatchFailureCode::ShutdownTimeout
                })
            {
                health.last_error = Some(OutboxDispatcherLastError {
                    tenant_id: None,
                    outbox_id: None,
                    occurred_at: Utc::now(),
                    failure: OutboxDispatchFailure::new(
                        OutboxDispatchPhase::ResolveSigner,
                        OutboxDispatchFailureClass::Fatal,
                        OutboxDispatchFailureCode::ShutdownTimeout,
                        format!(
                            "dispatcher shutdown deadline expired with {remaining_workers} worker(s) still running"
                        ),
                    ),
                });
            }
        } else if health.lifecycle != OutboxDispatcherLifecycle::Failed
            || health.last_error.as_ref().is_some_and(|error| {
                error.failure.code == OutboxDispatchFailureCode::ShutdownTimeout
            })
        {
            health.lifecycle = OutboxDispatcherLifecycle::Stopped;
        }
        let worker_failure = (health.lifecycle == OutboxDispatcherLifecycle::Failed).then(|| {
            health
                .last_error
                .as_ref()
                .map(|error| error.failure.message.clone())
                .unwrap_or_else(|| "worker pool failed without structured detail".to_string())
        });
        drop(health);
        if panicked {
            Err(OutboxDispatcherError::WorkerPanicked)
        } else if remaining_workers > 0 {
            Err(OutboxDispatcherError::ShutdownTimeout { remaining_workers })
        } else if let Some(message) = worker_failure {
            Err(OutboxDispatcherError::WorkerFailed { message })
        } else {
            Ok(())
        }
    }

    pub fn health(&self) -> OutboxDispatcherHealth {
        let runtime = self
            .shared
            .health
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let now = Utc::now();
        match self
            .shared
            .store
            .dispatch_queue_snapshot(now, self.shared.config.tenant_scan_limit)
        {
            Ok(queue) => OutboxDispatcherHealth {
                lifecycle: runtime.lifecycle,
                configured_workers: self.shared.config.worker_count,
                running_workers: runtime.running_workers,
                queue_depth: Some(queue.queue_depth),
                leased_count: Some(queue.leased_count),
                oldest_queued_age_ms: queue.oldest_created_at.map(|created_at| {
                    u64::try_from((now - created_at).num_milliseconds().max(0)).unwrap_or(u64::MAX)
                }),
                persistent_dead_letters: Some(queue.dead_letters),
                successes: runtime.successes,
                retries: runtime.retries,
                dead_letters: runtime.dead_letters,
                last_error: runtime.last_error.clone(),
                metrics_error: None,
            },
            Err(error) => OutboxDispatcherHealth {
                lifecycle: runtime.lifecycle,
                configured_workers: self.shared.config.worker_count,
                running_workers: runtime.running_workers,
                queue_depth: None,
                leased_count: None,
                oldest_queued_age_ms: None,
                persistent_dead_letters: None,
                successes: runtime.successes,
                retries: runtime.retries,
                dead_letters: runtime.dead_letters,
                last_error: runtime.last_error.clone(),
                metrics_error: Some(bounded_error_message(error.to_string())),
            },
        }
    }
}

fn reap_finished_workers(workers: &mut Vec<JoinHandle<()>>, panicked: &AtomicBool) {
    let mut index = 0usize;
    while index < workers.len() {
        if workers[index].is_finished() {
            let worker = workers.swap_remove(index);
            if worker.join().is_err() {
                panicked.store(true, Ordering::Release);
            }
        } else {
            index += 1;
        }
    }
}

impl Drop for OutboxDispatcher {
    fn drop(&mut self) {
        if !self.join_timed_out.load(Ordering::Acquire) {
            let _ = self.shutdown();
        }
        if self.join_timed_out.load(Ordering::Acquire) {
            self.request_shutdown();
            let workers = std::mem::take(
                self.workers
                    .get_mut()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            );
            handoff_workers_to_reaper(workers);
        }
    }
}

fn handoff_workers_to_reaper(workers: Vec<JoinHandle<()>>) {
    if workers.is_empty() {
        return;
    }
    let handoff = Arc::new(Mutex::new(Some(workers)));
    let reaper_handoff = Arc::clone(&handoff);
    match thread::Builder::new()
        .name("zaion-outbox-reaper".to_string())
        .spawn(move || {
            let workers = reaper_handoff
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
                .unwrap_or_default();
            for worker in workers {
                let _ = worker.join();
            }
        }) {
        Ok(reaper) => drop(reaper),
        Err(_) => {
            // Resource exhaustion prevented an ownership handoff. Preserve the
            // no-detach invariant even though this rare fallback can block.
            let workers = handoff
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
                .unwrap_or_default();
            for worker in workers {
                let _ = worker.join();
            }
        }
    }
}

fn admit_append(shared: &DispatcherShared) -> Option<AppendPermit<'_>> {
    let mut state = shared
        .append_admission
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !state.accepting || shared.shutdown.load(Ordering::Acquire) {
        return None;
    }
    state.in_flight = state.in_flight.saturating_add(1);
    Some(AppendPermit {
        state: &shared.append_admission,
    })
}

fn worker_loop(shared: Arc<DispatcherShared>) {
    {
        let mut health = shared
            .health
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        health.running_workers = health.running_workers.saturating_add(1);
    }
    let _lifecycle = WorkerLifecycleGuard { shared: &shared };
    {
        let mut ready = shared
            .startup_ready_workers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *ready = ready.saturating_add(1);
        shared.startup_ready_condvar.notify_all();
        drop(
            shared
                .startup_ready_condvar
                .wait_while(ready, |_| {
                    !shared.startup_released.load(Ordering::Acquire)
                        && !shared.shutdown.load(Ordering::Acquire)
                })
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
    }
    if shared.shutdown.load(Ordering::Acquire) {
        return;
    }
    let worker_id = format!("dispatcher-{}", uuid::Uuid::new_v4());
    let store = match DurableTurnStore::open(shared.store.db_path()) {
        Ok(store) => store,
        Err(error) => {
            fail_dispatcher(
                &shared,
                None,
                None,
                classify_turn_store_error(OutboxDispatchPhase::Claim, &error),
            );
            return;
        }
    };
    let ledger = EventLedger::new(store.db_path());
    let lease_duration = Duration::from_std(shared.config.lease_duration)
        .expect("validated dispatcher lease duration must fit chrono");

    while !shared.shutdown.load(Ordering::Acquire) {
        let now = Utc::now();
        let snapshot = match store.dispatch_queue_snapshot(now, shared.config.tenant_scan_limit) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                let failure = classify_turn_store_error(OutboxDispatchPhase::Claim, &error);
                if failure.class == OutboxDispatchFailureClass::Fatal {
                    fail_dispatcher(&shared, None, None, failure);
                    break;
                }
                record_failure(&shared, None, None, failure);
                wait_for_work(&shared);
                continue;
            }
        };
        if snapshot.ready_tenants.is_empty() {
            wait_for_work(&shared);
            continue;
        }

        let mut claimed_any = false;
        for tenant_id in snapshot.ready_tenants {
            if shared.shutdown.load(Ordering::Acquire) {
                break;
            }
            let claim_now = Utc::now();
            match store.claim_next_outbox(&tenant_id, &worker_id, claim_now, lease_duration) {
                Ok(Some(claim)) => {
                    claimed_any = true;
                    dispatch_claim(&shared, &store, &ledger, claim);
                }
                Ok(None) => {}
                Err(error) => {
                    let failure = classify_turn_store_error(OutboxDispatchPhase::Claim, &error);
                    if failure.class == OutboxDispatchFailureClass::Fatal {
                        fail_dispatcher(&shared, Some(tenant_id), None, failure);
                        break;
                    }
                    record_failure(&shared, Some(tenant_id), None, failure);
                }
            }
        }
        if !claimed_any {
            wait_for_work(&shared);
        }
    }
}

fn dispatch_claim(
    shared: &Arc<DispatcherShared>,
    store: &DurableTurnStore,
    ledger: &EventLedger,
    mut claim: TurnOutboxRecord,
) {
    phase_checkpoint(shared, OutboxDispatchPhase::Claim, &claim);
    if shared.shutdown.load(Ordering::Acquire) {
        defer_for_shutdown(shared, store, &claim);
        return;
    }

    let principal_id = match claimed_principal_id(&claim) {
        Ok(principal_id) => principal_id,
        Err(error) => {
            handle_claim_failure(
                shared,
                store,
                &claim,
                classify_turn_store_error(OutboxDispatchPhase::ResolveSigner, &error),
            );
            return;
        }
    };
    let public_key = match shared.resolver.resolve_public_key(&principal_id) {
        Ok(public_key) => public_key,
        Err(error) => {
            handle_claim_failure(shared, store, &claim, classify_signer_error(&error));
            return;
        }
    };
    phase_checkpoint(shared, OutboxDispatchPhase::ResolveSigner, &claim);
    if principal_id_from_public_key(&public_key) != principal_id {
        handle_claim_failure(
            shared,
            store,
            &claim,
            OutboxDispatchFailure::new(
                OutboxDispatchPhase::ResolveSigner,
                OutboxDispatchFailureClass::Permanent,
                OutboxDispatchFailureCode::SignerMismatch,
                "resolved verification key does not derive the requested PrincipalId",
            ),
        );
        return;
    }
    if shared.shutdown.load(Ordering::Acquire) {
        defer_for_shutdown(shared, store, &claim);
        return;
    }

    let lease_owner = claim
        .lease_owner
        .as_deref()
        .expect("claimed outbox must contain a lease owner")
        .to_string();
    let lease_token = claim
        .lease_token
        .as_deref()
        .expect("claimed outbox must contain a lease token")
        .to_string();
    let lease_duration = Duration::from_std(shared.config.lease_duration)
        .expect("validated dispatcher lease duration must fit chrono");
    let renewal_now = monotonic_now(&claim);
    claim = match store.renew_outbox_lease(
        &claim.tenant_id,
        &claim.outbox_id,
        &lease_owner,
        &lease_token,
        renewal_now,
        lease_duration,
    ) {
        Ok(claim) => claim,
        Err(error) => {
            handle_claim_failure(
                shared,
                store,
                &claim,
                classify_turn_store_error(OutboxDispatchPhase::Revalidate, &error),
            );
            return;
        }
    };

    let mut validated = match store.revalidate_outbox_for_signing(
        &claim.tenant_id,
        &claim.outbox_id,
        &lease_owner,
        &lease_token,
        monotonic_now(&claim),
        ledger,
        &public_key,
    ) {
        Ok(validated) => validated,
        Err(error) => {
            handle_claim_failure(
                shared,
                store,
                &claim,
                classify_turn_store_error(OutboxDispatchPhase::Revalidate, &error),
            );
            return;
        }
    };
    claim = validated.outbox().clone();
    phase_checkpoint(shared, OutboxDispatchPhase::Revalidate, &claim);
    if shared.shutdown.load(Ordering::Acquire) {
        defer_for_shutdown(shared, store, &claim);
        return;
    }

    if !has_commit_window(&claim, shared.config.minimum_commit_window) {
        let renewal_now = monotonic_now(&claim);
        claim = match store.renew_outbox_lease(
            &claim.tenant_id,
            &claim.outbox_id,
            &lease_owner,
            &lease_token,
            renewal_now,
            lease_duration,
        ) {
            Ok(claim) => claim,
            Err(error) => {
                handle_claim_failure(
                    shared,
                    store,
                    &claim,
                    classify_turn_store_error(OutboxDispatchPhase::Revalidate, &error),
                );
                return;
            }
        };
        validated = match store.revalidate_outbox_for_signing(
            &claim.tenant_id,
            &claim.outbox_id,
            &lease_owner,
            &lease_token,
            monotonic_now(&claim),
            ledger,
            &public_key,
        ) {
            Ok(validated) => validated,
            Err(error) => {
                handle_claim_failure(
                    shared,
                    store,
                    &claim,
                    classify_turn_store_error(OutboxDispatchPhase::Revalidate, &error),
                );
                return;
            }
        };
        claim = validated.outbox().clone();
    }
    if !has_commit_window(&claim, shared.config.minimum_commit_window) {
        handle_claim_failure(
            shared,
            store,
            &claim,
            OutboxDispatchFailure::new(
                OutboxDispatchPhase::Revalidate,
                OutboxDispatchFailureClass::Retryable,
                OutboxDispatchFailureCode::LeaseBudgetExhausted,
                "verified outbox does not retain the configured append/complete lease window",
            ),
        );
        return;
    }

    // Recover an append committed by an earlier process before requiring a
    // private key. The public key remains sufficient to verify and complete it
    // after key retirement or rotation.
    let commit = match ledger.verify_existing_idempotent_event(&public_key, validated.binding()) {
        Ok(commit) => commit,
        Err(LedgerError::NotFound(_)) => {
            let keypair = match shared.resolver.resolve(&principal_id) {
                Ok(keypair) => keypair,
                Err(error) => {
                    handle_claim_failure(shared, store, &claim, classify_signer_error(&error));
                    return;
                }
            };
            if shared.shutdown.load(Ordering::Acquire) {
                defer_for_shutdown(shared, store, &claim);
                return;
            }
            if keypair.principal_id() != principal_id
                || keypair.public_key_bytes().0.as_slice() != public_key.0.as_slice()
            {
                handle_claim_failure(
                    shared,
                    store,
                    &claim,
                    OutboxDispatchFailure::new(
                        OutboxDispatchPhase::ResolveSigner,
                        OutboxDispatchFailureClass::Permanent,
                        OutboxDispatchFailureCode::SignerMismatch,
                        "resolved signing key does not match the verified principal key",
                    ),
                );
                return;
            }

            // Private-key resolution may involve an external backend and may
            // outlive the lease used by the first validation. Reacquire the
            // fence and repeat the full validation immediately before append.
            let renewal_now = monotonic_now(&claim);
            claim = match store.renew_outbox_lease(
                &claim.tenant_id,
                &claim.outbox_id,
                &lease_owner,
                &lease_token,
                renewal_now,
                lease_duration,
            ) {
                Ok(claim) => claim,
                Err(error) => {
                    handle_claim_failure(
                        shared,
                        store,
                        &claim,
                        classify_turn_store_error(OutboxDispatchPhase::Revalidate, &error),
                    );
                    return;
                }
            };
            validated = match store.revalidate_outbox_for_signing(
                &claim.tenant_id,
                &claim.outbox_id,
                &lease_owner,
                &lease_token,
                monotonic_now(&claim),
                ledger,
                &public_key,
            ) {
                Ok(validated) => validated,
                Err(error) => {
                    handle_claim_failure(
                        shared,
                        store,
                        &claim,
                        classify_turn_store_error(OutboxDispatchPhase::Revalidate, &error),
                    );
                    return;
                }
            };
            claim = validated.outbox().clone();
            if !has_commit_window(&claim, shared.config.minimum_commit_window) {
                handle_claim_failure(
                    shared,
                    store,
                    &claim,
                    OutboxDispatchFailure::new(
                        OutboxDispatchPhase::Revalidate,
                        OutboxDispatchFailureClass::Retryable,
                        OutboxDispatchFailureCode::LeaseBudgetExhausted,
                        "private signer resolution exhausted the append/complete lease window",
                    ),
                );
                return;
            }

            match ledger.verify_existing_idempotent_event(&public_key, validated.binding()) {
                Ok(commit) => commit,
                Err(LedgerError::NotFound(_)) => {
                    let _append_permit = match admit_append(shared) {
                        Some(permit) => permit,
                        None => {
                            defer_for_shutdown(shared, store, &claim);
                            return;
                        }
                    };
                    phase_checkpoint(shared, OutboxDispatchPhase::Append, &claim);
                    match ledger.append_verified_idempotent_event(&keypair, validated.binding()) {
                        Ok(commit) => commit,
                        Err(append_error) => match ledger
                            .verify_existing_idempotent_event(&public_key, validated.binding())
                        {
                            Ok(commit) => commit,
                            Err(LedgerError::NotFound(_)) => {
                                handle_post_append_failure(
                                    shared,
                                    store,
                                    &claim,
                                    classify_ledger_error(
                                        OutboxDispatchPhase::Append,
                                        &append_error,
                                    ),
                                );
                                return;
                            }
                            Err(verify_error) => {
                                handle_post_append_failure(
                                    shared,
                                    store,
                                    &claim,
                                    classify_ledger_error(
                                        OutboxDispatchPhase::Append,
                                        &verify_error,
                                    ),
                                );
                                return;
                            }
                        },
                    }
                }
                Err(error) => {
                    handle_post_append_failure(
                        shared,
                        store,
                        &claim,
                        classify_ledger_error(OutboxDispatchPhase::Append, &error),
                    );
                    return;
                }
            }
        }
        Err(error) => {
            handle_post_append_failure(
                shared,
                store,
                &claim,
                classify_ledger_error(OutboxDispatchPhase::Append, &error),
            );
            return;
        }
    };
    phase_checkpoint(shared, OutboxDispatchPhase::Complete, &claim);
    match store.complete_outbox(
        &claim.tenant_id,
        &claim.outbox_id,
        &lease_owner,
        &lease_token,
        &commit,
        monotonic_now(&claim),
        ledger,
    ) {
        Ok(OutboxCompletion::Delivered | OutboxCompletion::AlreadyDelivered) => {
            let mut health = shared
                .health
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            health.successes = health.successes.saturating_add(1);
        }
        Err(error) => handle_post_append_failure(
            shared,
            store,
            &claim,
            classify_turn_store_error(OutboxDispatchPhase::Complete, &error),
        ),
    }
}

fn handle_post_append_failure(
    shared: &Arc<DispatcherShared>,
    store: &DurableTurnStore,
    claim: &TurnOutboxRecord,
    failure: OutboxDispatchFailure,
) {
    if shared.shutdown.load(Ordering::Acquire)
        && failure.class == OutboxDispatchFailureClass::Retryable
    {
        // The deterministic append may have committed even when its caller
        // observed an infrastructure error. Preserve the fence for crash
        // recovery and make shutdown fail closed until a later dispatcher
        // verifies and completes the event.
        fail_dispatcher(
            shared,
            Some(claim.tenant_id.clone()),
            Some(claim.outbox_id.clone()),
            failure,
        );
    } else {
        handle_claim_failure(shared, store, claim, failure);
    }
}

fn handle_claim_failure(
    shared: &Arc<DispatcherShared>,
    store: &DurableTurnStore,
    claim: &TurnOutboxRecord,
    failure: OutboxDispatchFailure,
) {
    match failure.class {
        OutboxDispatchFailureClass::Fatal => {
            fail_dispatcher(
                shared,
                Some(claim.tenant_id.clone()),
                Some(claim.outbox_id.clone()),
                failure,
            );
        }
        OutboxDispatchFailureClass::LeaseLost => {
            record_failure(
                shared,
                Some(claim.tenant_id.clone()),
                Some(claim.outbox_id.clone()),
                failure,
            );
        }
        OutboxDispatchFailureClass::Permanent | OutboxDispatchFailureClass::Retryable
            if failure.class == OutboxDispatchFailureClass::Permanent
                || claim.attempts >= shared.config.maximum_attempts =>
        {
            let exhausted = failure.class == OutboxDispatchFailureClass::Retryable;
            let now = monotonic_now(claim);
            match store.quarantine_outbox(claim, &failure, exhausted, now) {
                Ok(_) => {
                    let mut health = shared
                        .health
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    health.dead_letters = health.dead_letters.saturating_add(1);
                    health.last_error = Some(OutboxDispatcherLastError {
                        tenant_id: Some(claim.tenant_id.clone()),
                        outbox_id: Some(claim.outbox_id.clone()),
                        occurred_at: now,
                        failure,
                    });
                }
                Err(error) => {
                    let persist_failure =
                        classify_turn_store_error(OutboxDispatchPhase::QuarantinePersist, &error);
                    if persist_failure.class == OutboxDispatchFailureClass::Fatal {
                        fail_dispatcher(
                            shared,
                            Some(claim.tenant_id.clone()),
                            Some(claim.outbox_id.clone()),
                            persist_failure,
                        );
                    } else {
                        record_failure(
                            shared,
                            Some(claim.tenant_id.clone()),
                            Some(claim.outbox_id.clone()),
                            persist_failure,
                        );
                    }
                }
            }
        }
        OutboxDispatchFailureClass::Retryable => {
            let now = monotonic_now(claim);
            let available_at = now
                + Duration::from_std(retry_delay(&shared.config, claim))
                    .expect("validated retry delay must fit chrono");
            let lease_owner = claim.lease_owner.as_deref().unwrap_or("missing");
            let lease_token = claim.lease_token.as_deref().unwrap_or("missing");
            match store.release_outbox(
                &claim.tenant_id,
                &claim.outbox_id,
                lease_owner,
                lease_token,
                now,
                available_at,
                &failure.message,
            ) {
                Ok(()) => {
                    let mut health = shared
                        .health
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    health.retries = health.retries.saturating_add(1);
                    health.last_error = Some(OutboxDispatcherLastError {
                        tenant_id: Some(claim.tenant_id.clone()),
                        outbox_id: Some(claim.outbox_id.clone()),
                        occurred_at: now,
                        failure,
                    });
                }
                Err(error) => {
                    let persist_failure =
                        classify_turn_store_error(OutboxDispatchPhase::RetryPersist, &error);
                    if persist_failure.class == OutboxDispatchFailureClass::Fatal {
                        fail_dispatcher(
                            shared,
                            Some(claim.tenant_id.clone()),
                            Some(claim.outbox_id.clone()),
                            persist_failure,
                        );
                    } else {
                        record_failure(
                            shared,
                            Some(claim.tenant_id.clone()),
                            Some(claim.outbox_id.clone()),
                            persist_failure,
                        );
                    }
                }
            }
        }
        OutboxDispatchFailureClass::Permanent => unreachable!("permanent failures quarantine"),
    }
}

fn defer_for_shutdown(
    shared: &Arc<DispatcherShared>,
    store: &DurableTurnStore,
    claim: &TurnOutboxRecord,
) {
    let now = monotonic_now(claim);
    let Some(lease_owner) = claim.lease_owner.as_deref() else {
        return;
    };
    let Some(lease_token) = claim.lease_token.as_deref() else {
        return;
    };
    if let Err(error) = store.release_outbox(
        &claim.tenant_id,
        &claim.outbox_id,
        lease_owner,
        lease_token,
        now,
        now,
        "dispatcher shutdown before signed append",
    ) {
        let failure = classify_turn_store_error(OutboxDispatchPhase::RetryPersist, &error);
        if failure.class != OutboxDispatchFailureClass::LeaseLost {
            record_failure(
                shared,
                Some(claim.tenant_id.clone()),
                Some(claim.outbox_id.clone()),
                failure,
            );
        }
    }
}

fn wait_for_work(shared: &DispatcherShared) {
    let generation = shared
        .wake_generation
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let observed = *generation;
    let _ = shared
        .wake_condvar
        .wait_timeout_while(generation, shared.config.poll_interval, |generation| {
            *generation == observed && !shared.shutdown.load(Ordering::Acquire)
        })
        .unwrap_or_else(std::sync::PoisonError::into_inner);
}

fn worker_stopped(shared: &DispatcherShared) {
    let mut health = shared
        .health
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    health.running_workers = health.running_workers.saturating_sub(1);
    if health.running_workers == 0 && health.lifecycle == OutboxDispatcherLifecycle::Running {
        health.lifecycle = OutboxDispatcherLifecycle::Failed;
        health.last_error = Some(OutboxDispatcherLastError {
            tenant_id: None,
            outbox_id: None,
            occurred_at: Utc::now(),
            failure: OutboxDispatchFailure::new(
                OutboxDispatchPhase::Claim,
                OutboxDispatchFailureClass::Fatal,
                OutboxDispatchFailureCode::InfrastructureFailure,
                "all outbox dispatcher workers exited without a shutdown request",
            ),
        });
    }
}

fn fail_dispatcher(
    shared: &DispatcherShared,
    tenant_id: Option<String>,
    outbox_id: Option<String>,
    failure: OutboxDispatchFailure,
) {
    let occurred_at = Utc::now();
    {
        let mut health = shared
            .health
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        health.lifecycle = OutboxDispatcherLifecycle::Failed;
        health.last_error = Some(OutboxDispatcherLastError {
            tenant_id,
            outbox_id,
            occurred_at,
            failure,
        });
    }
    shared.shutdown.store(true, Ordering::Release);
    shared.wake_condvar.notify_all();
}

fn record_failure(
    shared: &DispatcherShared,
    tenant_id: Option<String>,
    outbox_id: Option<String>,
    failure: OutboxDispatchFailure,
) {
    let mut health = shared
        .health
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    health.last_error = Some(OutboxDispatcherLastError {
        tenant_id,
        outbox_id,
        occurred_at: Utc::now(),
        failure,
    });
}

fn phase_checkpoint(
    shared: &DispatcherShared,
    phase: OutboxDispatchPhase,
    claim: &TurnOutboxRecord,
) {
    #[cfg(test)]
    if let Some(hook) = &shared.config.test_hook {
        hook.reached(phase, claim);
    }
    #[cfg(not(test))]
    let _ = (shared, phase, claim);
}

fn has_commit_window(claim: &TurnOutboxRecord, minimum: StdDuration) -> bool {
    let Ok(minimum) = Duration::from_std(minimum) else {
        return false;
    };
    claim
        .lease_until
        .is_some_and(|lease_until| lease_until - Utc::now() >= minimum)
}

fn monotonic_now(claim: &TurnOutboxRecord) -> DateTime<Utc> {
    Utc::now().max(claim.updated_at)
}

fn retry_delay(config: &OutboxDispatcherConfig, claim: &TurnOutboxRecord) -> StdDuration {
    let exponent = u32::try_from(claim.attempts.saturating_sub(1).min(31)).unwrap_or(31);
    let multiplier = 1_u128 << exponent;
    let initial_ms = config.initial_retry_delay.as_millis();
    let maximum_ms = config.maximum_retry_delay.as_millis();
    let base_ms = initial_ms.saturating_mul(multiplier).min(maximum_ms);
    let jitter_limit = base_ms.saturating_mul(u128::from(config.retry_jitter_percent)) / 100;
    let jitter = if jitter_limit == 0 {
        0
    } else {
        let mut hasher = Sha256::new();
        hasher.update(claim.outbox_id.as_bytes());
        hasher.update(claim.attempts.to_le_bytes());
        let digest = hasher.finalize();
        let sample = u64::from_le_bytes(digest[..8].try_into().expect("SHA-256 prefix"));
        u128::from(sample) % (jitter_limit + 1)
    };
    let delay_ms = base_ms.saturating_add(jitter).min(u128::from(u64::MAX));
    StdDuration::from_millis(u64::try_from(delay_ms).unwrap_or(u64::MAX))
}

fn classify_signer_error(error: &OutboxSignerResolveError) -> OutboxDispatchFailure {
    match error {
        OutboxSignerResolveError::Missing { .. } => OutboxDispatchFailure::new(
            OutboxDispatchPhase::ResolveSigner,
            OutboxDispatchFailureClass::Retryable,
            OutboxDispatchFailureCode::SignerMissing,
            error.to_string(),
        ),
        OutboxSignerResolveError::Unavailable(_) => OutboxDispatchFailure::new(
            OutboxDispatchPhase::ResolveSigner,
            OutboxDispatchFailureClass::Retryable,
            OutboxDispatchFailureCode::SignerUnavailable,
            error.to_string(),
        ),
        OutboxSignerResolveError::Invalid(_) => OutboxDispatchFailure::new(
            OutboxDispatchPhase::ResolveSigner,
            OutboxDispatchFailureClass::Permanent,
            OutboxDispatchFailureCode::SignerInvalid,
            error.to_string(),
        ),
    }
}

fn classify_turn_store_error(
    phase: OutboxDispatchPhase,
    error: &TurnStoreError,
) -> OutboxDispatchFailure {
    match error {
        TurnStoreError::Sqlite(error) => classify_sqlite_error(
            phase,
            error,
            OutboxDispatchFailureCode::StoreBusy,
            OutboxDispatchFailureCode::InfrastructureFailure,
        ),
        TurnStoreError::Io(error) => {
            classify_io_error(phase, error, OutboxDispatchFailureCode::StoreIo)
        }
        TurnStoreError::Ledger(error) => classify_ledger_error(phase, error),
        TurnStoreError::OutboxLeaseLost { .. }
        | TurnStoreError::OutboxLeaseExpired { .. }
        | TurnStoreError::OutboxOrderConflict { .. }
        | TurnStoreError::CasLost => OutboxDispatchFailure::new(
            phase,
            OutboxDispatchFailureClass::LeaseLost,
            OutboxDispatchFailureCode::LeaseLost,
            error.to_string(),
        ),
        TurnStoreError::HashMismatch { .. }
        | TurnStoreError::RecordBindingMismatch { .. }
        | TurnStoreError::CorruptState(_)
        | TurnStoreError::CorruptOutboxStatus(_)
        | TurnStoreError::CorruptRevision(_)
        | TurnStoreError::CorruptTimestamp { .. }
        | TurnStoreError::Json(_)
        | TurnStoreError::OutboxHistoryIncomplete { .. }
        | TurnStoreError::OutboxHistoryRevision { .. }
        | TurnStoreError::OutboxHistoryGenesis { .. }
        | TurnStoreError::OutboxHistoryPreviousState { .. }
        | TurnStoreError::OutboxHistoryIllegalTransition { .. }
        | TurnStoreError::OutboxHistoryCurrentTurn { .. }
        | TurnStoreError::OutboxDeliveredPrefix { .. }
        | TurnStoreError::OutboxLedgerEventMismatch { .. }
        | TurnStoreError::OutboxCommitMismatch { .. }
        | TurnStoreError::OutboxPrincipalMismatch
        | TurnStoreError::OutboxSignerEvidenceMissing { .. } => OutboxDispatchFailure::new(
            phase,
            OutboxDispatchFailureClass::Permanent,
            OutboxDispatchFailureCode::StoreIntegrity,
            error.to_string(),
        ),
        TurnStoreError::MutexPoisoned
        | TurnStoreError::SchemaIntegrity(_)
        | TurnStoreError::OutboxLedgerPathMismatch
        | TurnStoreError::OutboxLedgerInstanceMismatch
        | TurnStoreError::InvalidOutboxLeaseDuration
        | TurnStoreError::OutboxErrorTooLong
        | TurnStoreError::OutboxRetryTimeInvalid
        | TurnStoreError::OutboxAttemptsExhausted
        | TurnStoreError::OutboxLeaseTimeOverflow
        | TurnStoreError::CommitOrdinalExhausted
        | TurnStoreError::OutboxCompletionTimeInvalid
        | TurnStoreError::NonMonotonicTimestamp => OutboxDispatchFailure::new(
            phase,
            OutboxDispatchFailureClass::Fatal,
            OutboxDispatchFailureCode::InfrastructureFailure,
            error.to_string(),
        ),
        _ => OutboxDispatchFailure::new(
            phase,
            OutboxDispatchFailureClass::Fatal,
            OutboxDispatchFailureCode::StoreIntegrity,
            error.to_string(),
        ),
    }
}

fn classify_ledger_error(phase: OutboxDispatchPhase, error: &LedgerError) -> OutboxDispatchFailure {
    match error {
        LedgerError::Sqlite(error) => classify_sqlite_error(
            phase,
            error,
            OutboxDispatchFailureCode::LedgerBusy,
            OutboxDispatchFailureCode::InfrastructureFailure,
        ),
        LedgerError::Io(error) => {
            classify_io_error(phase, error, OutboxDispatchFailureCode::LedgerIo)
        }
        LedgerError::NotFound(_)
        | LedgerError::CorruptPayload(_)
        | LedgerError::Serialization(_)
        | LedgerError::InvalidIdempotencyKey
        | LedgerError::EventIdConflict { .. }
        | LedgerError::EventBindingPrincipalMismatch { .. }
        | LedgerError::EventBindingMismatch { .. }
        | LedgerError::EventBindingSignatureInvalid
        | LedgerError::EventBindingNonCanonicalSignature => OutboxDispatchFailure::new(
            phase,
            OutboxDispatchFailureClass::Permanent,
            OutboxDispatchFailureCode::LedgerIntegrity,
            error.to_string(),
        ),
        LedgerError::EventBindingUnsupportedLedgerPath(_)
        | LedgerError::InvalidDatabaseInstanceIdentity(_)
        | LedgerError::DatabaseInstanceIdentityDrift { .. }
        | LedgerError::CorruptChain(_) => OutboxDispatchFailure::new(
            phase,
            OutboxDispatchFailureClass::Fatal,
            OutboxDispatchFailureCode::InfrastructureFailure,
            error.to_string(),
        ),
    }
}

fn classify_sqlite_error(
    phase: OutboxDispatchPhase,
    error: &rusqlite::Error,
    busy_code: OutboxDispatchFailureCode,
    fatal_code: OutboxDispatchFailureCode,
) -> OutboxDispatchFailure {
    let busy = matches!(
        error,
        rusqlite::Error::SqliteFailure(inner, _)
            if matches!(
                inner.code,
                rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
            )
    );
    OutboxDispatchFailure::new(
        phase,
        if busy {
            OutboxDispatchFailureClass::Retryable
        } else {
            OutboxDispatchFailureClass::Fatal
        },
        if busy { busy_code } else { fatal_code },
        error.to_string(),
    )
}

fn classify_io_error(
    phase: OutboxDispatchPhase,
    error: &std::io::Error,
    code: OutboxDispatchFailureCode,
) -> OutboxDispatchFailure {
    let retryable = matches!(
        error.kind(),
        ErrorKind::Interrupted | ErrorKind::WouldBlock | ErrorKind::TimedOut
    );
    OutboxDispatchFailure::new(
        phase,
        if retryable {
            OutboxDispatchFailureClass::Retryable
        } else {
            OutboxDispatchFailureClass::Fatal
        },
        code,
        error.to_string(),
    )
}

fn bounded_error_message(message: String) -> String {
    if message.len() <= MAX_OUTBOX_ERROR_BYTES {
        return if message.is_empty() {
            "unspecified outbox dispatcher failure".to_string()
        } else {
            message
        };
    }
    let mut end = MAX_OUTBOX_ERROR_BYTES;
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    message[..end].to_string()
}

#[cfg(test)]
trait DispatcherTestHook: Send + Sync {
    fn reached(&self, phase: OutboxDispatchPhase, outbox: &TurnOutboxRecord);
}

#[cfg(test)]
mod tests;
