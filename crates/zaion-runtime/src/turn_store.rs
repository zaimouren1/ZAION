use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration as StdDuration;

use chrono::{DateTime, Duration, SecondsFormat, TimeZone, Utc};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    AuthenticatedIngress, PartialLedgerTail, QuarantineEvent, TurnError, TurnExecution,
    TurnOutcome, TurnState, TurnTransitionError, VersionedTurnState,
};

mod dispatch;
mod dispatcher;

pub use dispatch::{OutboxCompletion, SigningValidatedOutbox};
pub use dispatcher::{
    InMemoryOutboxSignerResolver, OutboxDispatchFailure, OutboxDispatchFailureClass,
    OutboxDispatchFailureCode, OutboxDispatchPhase, OutboxDispatcher, OutboxDispatcherConfig,
    OutboxDispatcherError, OutboxDispatcherHealth, OutboxDispatcherLastError,
    OutboxDispatcherLifecycle, OutboxQuarantineRecord, OutboxSignerResolveError,
    OutboxSignerResolver,
};

pub const TURN_STORE_SCHEMA: &str = "zaion.turn_store.v2";
pub const TURN_OUTBOX_SCHEMA: &str = "zaion.turn_state_outbox.v2";

const MAX_ACTOR_COMPONENT_BYTES: usize = 1_024;
const MAX_LEASE_OWNER_BYTES: usize = 256;
const MAX_OUTBOX_ERROR_BYTES: usize = 4_096;
const MAX_OUTBOX_LEASE_SECONDS: i64 = 5 * 60;
const OUTBOX_ORDER_MIGRATION_ID: &str = "turn_outbox_commit_order_v1";
const OUTBOX_ORDER_MIGRATION_KIND: &str = "legacy_outbox_rowid_order_v1";
const OUTBOX_ORDER_GUARD_MIGRATION_ID: &str = "turn_outbox_commit_order_immutability_v1";
const OUTBOX_ORDER_GUARD_MIGRATION_KIND: &str = "commit_order_immutability_v1";
const OUTBOX_ORDER_TABLE: &str = "turn_outbox_commit_order_v2";
const OUTBOX_ORDER_INDEX: &str = "idx_turn_outbox_commit_order_v2_tenant";
const OUTBOX_REVISION_INDEX: &str = "ux_turn_outbox_v2_turn_revision";
const OUTBOX_ORDER_TRIGGER: &str = "turn_outbox_v2_assign_commit_order";
const OUTBOX_ORDER_UPDATE_GUARD: &str = "turn_outbox_commit_order_v2_no_update";
const OUTBOX_ORDER_DELETE_GUARD: &str = "turn_outbox_commit_order_v2_no_delete";
const OUTBOX_VERIFIED_COMMIT_MIGRATION_ID: &str = "turn_outbox_verified_commit_v1";
const OUTBOX_VERIFIED_COMMIT_MIGRATION_KIND: &str = "verified_event_commit_evidence_v1";
const OUTBOX_VERIFIED_COMMIT_TABLE: &str = "turn_outbox_verified_commit_v2";
const OUTBOX_VERIFIED_COMMIT_INSERT_GUARD: &str = "turn_outbox_verified_commit_v2_insert_state";
const OUTBOX_VERIFIED_COMMIT_UPDATE_GUARD: &str = "turn_outbox_verified_commit_v2_no_update";
const OUTBOX_VERIFIED_COMMIT_DELETE_GUARD: &str = "turn_outbox_verified_commit_v2_no_delete";
const OUTBOX_VERIFIED_DELIVERY_GUARD: &str = "turn_outbox_v2_verified_delivery_guard";

const CREATE_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS turn_actor_v2 (
    tenant_id TEXT NOT NULL,
    actor_key TEXT NOT NULL,
    principal_id TEXT NOT NULL,
    workspace_id TEXT NOT NULL,
    profile_id TEXT NOT NULL,
    channel_id TEXT NOT NULL,
    thread_id TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision >= 0),
    active_turn_id TEXT,
    lease_owner TEXT,
    lease_until_ms INTEGER,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    PRIMARY KEY (tenant_id, actor_key),
    CHECK (
        (active_turn_id IS NULL AND lease_owner IS NULL AND lease_until_ms IS NULL)
        OR
        (active_turn_id IS NOT NULL AND lease_owner IS NOT NULL AND lease_until_ms IS NOT NULL)
    )
);
CREATE INDEX IF NOT EXISTS idx_turn_actor_v2_recovery
    ON turn_actor_v2(tenant_id, lease_until_ms, actor_key);

CREATE TABLE IF NOT EXISTS turn_state_v2 (
    tenant_id TEXT NOT NULL,
    turn_id TEXT NOT NULL,
    actor_key TEXT NOT NULL,
    subject_id TEXT NOT NULL,
    principal_id TEXT NOT NULL,
    workspace_id TEXT NOT NULL,
    profile_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    source_surface TEXT NOT NULL,
    source_id TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    request_json TEXT NOT NULL,
    request_hash TEXT NOT NULL,
    authority_json TEXT NOT NULL,
    authority_hash TEXT NOT NULL,
    deadline_ms INTEGER NOT NULL,
    state TEXT NOT NULL CHECK (state IN (
        'accepted', 'routed', 'running', 'waiting_approval', 'tool_running',
        'completed', 'degraded', 'aborted', 'quarantined'
    )),
    revision INTEGER NOT NULL CHECK (revision >= 0),
    terminal_result_json TEXT,
    terminal_result_hash TEXT,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    PRIMARY KEY (tenant_id, turn_id),
    UNIQUE (tenant_id, idempotency_key),
    FOREIGN KEY (tenant_id, actor_key)
        REFERENCES turn_actor_v2(tenant_id, actor_key)
        ON DELETE RESTRICT,
    CHECK (
        (terminal_result_json IS NULL AND terminal_result_hash IS NULL)
        OR
        (terminal_result_json IS NOT NULL AND terminal_result_hash IS NOT NULL)
    )
);
CREATE INDEX IF NOT EXISTS idx_turn_state_v2_tenant_state
    ON turn_state_v2(tenant_id, state, updated_at_ms, turn_id);

CREATE TABLE IF NOT EXISTS turn_outbox_v2 (
    tenant_id TEXT NOT NULL,
    outbox_id TEXT NOT NULL,
    turn_id TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision >= 0),
    event_type TEXT NOT NULL,
    effect_kind TEXT NOT NULL CHECK (effect_kind = 'ledger_turn_state'),
    idempotency_mode TEXT NOT NULL CHECK (idempotency_mode = 'key_required'),
    payload_json TEXT NOT NULL,
    payload_hash TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('pending', 'leased', 'delivered')),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    available_at_ms INTEGER NOT NULL,
    lease_owner TEXT,
    lease_token TEXT,
    lease_until_ms INTEGER,
    delivered_at_ms INTEGER,
    ledger_event_id TEXT,
    last_error TEXT,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    PRIMARY KEY (tenant_id, outbox_id),
    UNIQUE (tenant_id, turn_id, revision, event_type),
    FOREIGN KEY (tenant_id, turn_id)
        REFERENCES turn_state_v2(tenant_id, turn_id)
        ON DELETE RESTRICT,
    CHECK (
        (status = 'pending' AND lease_owner IS NULL AND lease_token IS NULL
            AND lease_until_ms IS NULL
            AND delivered_at_ms IS NULL AND ledger_event_id IS NULL)
        OR
        (status = 'leased' AND lease_owner IS NOT NULL AND lease_token IS NOT NULL
            AND lease_until_ms IS NOT NULL
            AND delivered_at_ms IS NULL AND ledger_event_id IS NULL)
        OR
        (status = 'delivered' AND lease_owner IS NULL AND lease_token IS NULL
            AND lease_until_ms IS NULL
            AND delivered_at_ms IS NOT NULL AND ledger_event_id IS NOT NULL)
    )
);
CREATE INDEX IF NOT EXISTS idx_turn_outbox_v2_dispatch
    ON turn_outbox_v2(tenant_id, status, available_at_ms, lease_until_ms, created_at_ms);
"#;

const CREATE_ORDER_MIGRATION_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS turn_store_schema_migrations_v2 (
    migration_id TEXT PRIMARY KEY,
    migration_kind TEXT NOT NULL,
    source_row_count INTEGER NOT NULL CHECK (source_row_count >= 0),
    source_max_rowid INTEGER NOT NULL CHECK (source_max_rowid >= 0),
    source_digest TEXT NOT NULL,
    applied_at_ms INTEGER NOT NULL
);
"#;

const EXPECTED_ORDER_MIGRATION_TABLE: &str = r#"
CREATE TABLE turn_store_schema_migrations_v2 (
    migration_id TEXT PRIMARY KEY,
    migration_kind TEXT NOT NULL,
    source_row_count INTEGER NOT NULL CHECK (source_row_count >= 0),
    source_max_rowid INTEGER NOT NULL CHECK (source_max_rowid >= 0),
    source_digest TEXT NOT NULL,
    applied_at_ms INTEGER NOT NULL
);
"#;

const CREATE_ORDER_TABLE: &str = r#"
CREATE TABLE turn_outbox_commit_order_v2 (
    commit_ordinal INTEGER PRIMARY KEY AUTOINCREMENT,
    tenant_id TEXT NOT NULL,
    outbox_id TEXT NOT NULL,
    order_origin TEXT NOT NULL CHECK (
        order_origin IN ('legacy_rowid_backfill', 'transactional')
    ),
    legacy_source_rowid INTEGER,
    UNIQUE (tenant_id, outbox_id),
    FOREIGN KEY (tenant_id, outbox_id)
        REFERENCES turn_outbox_v2(tenant_id, outbox_id)
        ON DELETE RESTRICT,
    CHECK (
        (order_origin = 'legacy_rowid_backfill' AND legacy_source_rowid > 0)
        OR
        (order_origin = 'transactional' AND legacy_source_rowid IS NULL)
    )
);
"#;

const CREATE_ORDER_INDEX: &str = r#"
CREATE INDEX idx_turn_outbox_commit_order_v2_tenant
    ON turn_outbox_commit_order_v2(tenant_id, commit_ordinal);
"#;

const CREATE_REVISION_INDEX: &str = r#"
CREATE UNIQUE INDEX ux_turn_outbox_v2_turn_revision
    ON turn_outbox_v2(tenant_id, turn_id, revision);
"#;

const CREATE_ORDER_TRIGGER: &str = r#"
CREATE TRIGGER turn_outbox_v2_assign_commit_order
AFTER INSERT ON turn_outbox_v2
BEGIN
    INSERT INTO turn_outbox_commit_order_v2 (
        tenant_id, outbox_id, order_origin, legacy_source_rowid
    ) VALUES (
        NEW.tenant_id, NEW.outbox_id, 'transactional', NULL
    );
END;
"#;

const CREATE_ORDER_UPDATE_GUARD: &str = r#"
CREATE TRIGGER turn_outbox_commit_order_v2_no_update
BEFORE UPDATE ON turn_outbox_commit_order_v2
BEGIN
    SELECT RAISE(ABORT, 'turn outbox commit order is immutable');
END;
"#;

const CREATE_ORDER_DELETE_GUARD: &str = r#"
CREATE TRIGGER turn_outbox_commit_order_v2_no_delete
BEFORE DELETE ON turn_outbox_commit_order_v2
BEGIN
    SELECT RAISE(ABORT, 'turn outbox commit order is immutable');
END;
"#;

const CREATE_VERIFIED_COMMIT_TABLE: &str = r#"
CREATE TABLE turn_outbox_verified_commit_v2 (
    tenant_id TEXT NOT NULL,
    outbox_id TEXT NOT NULL,
    ledger_event_id TEXT NOT NULL CHECK (length(ledger_event_id) > 0),
    signer_public_key BLOB NOT NULL CHECK (
        typeof(signer_public_key) = 'blob' AND length(signer_public_key) = 32
    ),
    database_instance_id TEXT NOT NULL CHECK (length(database_instance_id) > 0),
    PRIMARY KEY (tenant_id, outbox_id),
    FOREIGN KEY (tenant_id, outbox_id)
        REFERENCES turn_outbox_v2(tenant_id, outbox_id)
        ON DELETE RESTRICT
);
"#;

const CREATE_VERIFIED_COMMIT_INSERT_GUARD: &str = r#"
CREATE TRIGGER turn_outbox_verified_commit_v2_insert_state
BEFORE INSERT ON turn_outbox_verified_commit_v2
WHEN NOT EXISTS (
    SELECT 1 FROM turn_outbox_v2 o
    WHERE o.tenant_id = NEW.tenant_id AND o.outbox_id = NEW.outbox_id
      AND (
          o.status = 'leased'
          OR (o.status = 'delivered' AND o.ledger_event_id = NEW.ledger_event_id)
      )
)
BEGIN
    SELECT RAISE(ABORT, 'verified outbox commit requires a leased or matching delivered row');
END;
"#;

const CREATE_VERIFIED_COMMIT_UPDATE_GUARD: &str = r#"
CREATE TRIGGER turn_outbox_verified_commit_v2_no_update
BEFORE UPDATE ON turn_outbox_verified_commit_v2
BEGIN
    SELECT RAISE(ABORT, 'verified outbox commit evidence is immutable');
END;
"#;

const CREATE_VERIFIED_COMMIT_DELETE_GUARD: &str = r#"
CREATE TRIGGER turn_outbox_verified_commit_v2_no_delete
BEFORE DELETE ON turn_outbox_verified_commit_v2
BEGIN
    SELECT RAISE(ABORT, 'verified outbox commit evidence is immutable');
END;
"#;

const CREATE_VERIFIED_DELIVERY_GUARD: &str = r#"
CREATE TRIGGER turn_outbox_v2_verified_delivery_guard
BEFORE UPDATE ON turn_outbox_v2
WHEN OLD.status = 'delivered'
   OR (
       OLD.status != 'delivered' AND NEW.status = 'delivered'
       AND NOT EXISTS (
           SELECT 1 FROM turn_outbox_verified_commit_v2 v
           WHERE v.tenant_id = NEW.tenant_id AND v.outbox_id = NEW.outbox_id
             AND v.ledger_event_id = NEW.ledger_event_id
       )
   )
BEGIN
    SELECT RAISE(ABORT, 'delivered outbox rows are immutable and require verified commit evidence');
END;
"#;

#[derive(Clone)]
pub struct DurableTurnStore {
    db_path: PathBuf,
    database_instance_id: String,
    conn: Arc<Mutex<Connection>>,
}

impl std::fmt::Debug for DurableTurnStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DurableTurnStore")
            .field("db_path", &self.db_path)
            .field("database_instance_id", &self.database_instance_id)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnActorIdentity {
    tenant_id: String,
    actor_key: String,
    principal_id: String,
    workspace_id: String,
    profile_id: String,
    channel_id: String,
    thread_id: String,
}

impl TurnActorIdentity {
    pub fn for_ingress(
        ingress: &AuthenticatedIngress,
        channel_id: impl Into<String>,
        thread_id: impl Into<String>,
    ) -> Result<Self, TurnStoreError> {
        let channel_id = channel_id.into();
        let thread_id = thread_id.into();
        validate_actor_component("channel_id", &channel_id)?;
        validate_actor_component("thread_id", &thread_id)?;
        let tenant_id = ingress.tenant_id().as_str().to_string();
        let principal_id = ingress.principal_id().as_str().to_string();
        let workspace_id = ingress.workspace_id().0.clone();
        let profile_id = ingress.profile_id().as_str().to_string();
        let actor_key = deterministic_actor_key(&[
            &tenant_id,
            &principal_id,
            &workspace_id,
            &profile_id,
            &channel_id,
            &thread_id,
        ]);
        Ok(Self {
            tenant_id,
            actor_key,
            principal_id,
            workspace_id,
            profile_id,
            channel_id,
            thread_id,
        })
    }

    pub fn actor_key(&self) -> &str {
        &self.actor_key
    }
}

#[derive(Debug, Clone)]
pub struct DurableTurnAdmission {
    actor: TurnActorIdentity,
    request: Value,
    lease_owner: String,
    approval_required: bool,
}

impl DurableTurnAdmission {
    pub fn new(
        actor: TurnActorIdentity,
        request: Value,
        lease_owner: impl Into<String>,
    ) -> Result<Self, TurnStoreError> {
        let lease_owner = lease_owner.into();
        validate_lease_identity("lease_owner", &lease_owner)?;
        Ok(Self {
            actor,
            request,
            lease_owner,
            approval_required: false,
        })
    }

    /// Mark the turn as requiring approval before execution: the durable
    /// turn starts in `WaitingApproval` instead of `Accepted`.
    pub fn with_approval_required(mut self, required: bool) -> Self {
        self.approval_required = required;
        self
    }

    pub fn actor(&self) -> &TurnActorIdentity {
        &self.actor
    }

    pub fn lease_owner(&self) -> &str {
        &self.lease_owner
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DurableTurnRecord {
    pub tenant_id: String,
    pub turn_id: String,
    pub actor_key: String,
    pub subject_id: String,
    pub principal_id: String,
    pub workspace_id: String,
    pub profile_id: String,
    pub session_id: String,
    pub source_surface: String,
    pub source_id: String,
    pub idempotency_key: String,
    pub request: Value,
    pub request_hash: String,
    pub authority: Value,
    pub authority_hash: String,
    pub deadline: DateTime<Utc>,
    pub state: VersionedTurnState,
    pub terminal_result: Option<Value>,
    pub terminal_result_hash: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnActorRecord {
    pub tenant_id: String,
    pub actor_key: String,
    pub principal_id: String,
    pub workspace_id: String,
    pub profile_id: String,
    pub channel_id: String,
    pub thread_id: String,
    pub revision: u64,
    pub active_turn_id: Option<String>,
    pub lease_owner: Option<String>,
    pub lease_until: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnOutboxStatus {
    Pending,
    Leased,
    Delivered,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TurnOutboxRecord {
    pub tenant_id: String,
    pub outbox_id: String,
    pub commit_ordinal: u64,
    pub order_origin: String,
    pub turn_id: String,
    pub revision: u64,
    pub event_type: String,
    pub effect_kind: String,
    pub idempotency_mode: String,
    pub payload: Value,
    pub payload_hash: String,
    pub status: TurnOutboxStatus,
    pub attempts: u64,
    pub available_at: DateTime<Utc>,
    pub lease_owner: Option<String>,
    pub lease_token: Option<String>,
    pub lease_until: Option<DateTime<Utc>>,
    pub delivered_at: Option<DateTime<Utc>>,
    pub ledger_event_id: Option<String>,
    pub verified_ledger_event_id: Option<String>,
    pub verified_signer_public_key: Option<Vec<u8>>,
    pub verified_database_instance_id: Option<String>,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BeginTurnResult {
    Created(DurableTurnRecord),
    Existing(DurableTurnRecord),
}

impl BeginTurnResult {
    pub fn record(&self) -> &DurableTurnRecord {
        match self {
            Self::Created(record) | Self::Existing(record) => record,
        }
    }

    pub const fn is_created(&self) -> bool {
        matches!(self, Self::Created(_))
    }
}

#[derive(Debug, Error)]
pub enum TurnStoreError {
    #[error("turn store I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("turn store SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("turn store JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("turn store connection mutex is poisoned")]
    MutexPoisoned,
    #[error("turn store row has unknown state: {0}")]
    CorruptState(String),
    #[error("turn store row has unknown outbox status: {0}")]
    CorruptOutboxStatus(String),
    #[error("turn store row has invalid revision: {0}")]
    CorruptRevision(i64),
    #[error("turn store row has invalid timestamp in {field}: {value}")]
    CorruptTimestamp { field: &'static str, value: i64 },
    #[error("invalid durable actor component {field}")]
    InvalidActorComponent { field: &'static str },
    #[error("invalid durable lease identity {field}")]
    InvalidLeaseIdentity { field: &'static str },
    #[error("actor identity does not match authenticated ingress")]
    ActorAuthorityMismatch,
    #[error("authenticated ingress deadline has expired")]
    DeadlineExpired,
    #[error("idempotency key is already bound to a different request or authority")]
    IdempotencyConflict,
    #[error("turn {turn_id} not found for approval")]
    UnknownTurn { turn_id: String },
    #[error("turn {turn_id} is not awaiting approval")]
    NotWaitingApproval { turn_id: String },
    #[error("actor {actor_key} already has active turn {active_turn_id} until {lease_until}")]
    ActorBusy {
        actor_key: String,
        active_turn_id: String,
        lease_until: DateTime<Utc>,
    },
    #[error("turn actor lease is not owned by {lease_owner}")]
    ActorLeaseLost { lease_owner: String },
    #[error("turn does not exist in tenant {tenant_id}: {turn_id}")]
    MissingTurn { tenant_id: String, turn_id: String },
    #[error(transparent)]
    Transition(#[from] TurnTransitionError),
    #[error("turn CAS lost after validation")]
    CasLost,
    #[error("terminal transition requires a persisted result")]
    MissingTerminalResult,
    #[error("terminal result may only be stored with a terminal state")]
    NonTerminalResult,
    #[error("terminal result resolves to {actual:?}, not requested state {expected:?}")]
    TerminalOutcomeMismatch {
        expected: TurnState,
        actual: TurnState,
    },
    #[error("turn store hash mismatch in {field}")]
    HashMismatch { field: &'static str },
    #[error("turn store row binding mismatch in {field}")]
    RecordBindingMismatch { field: &'static str },
    #[error("outbox record does not exist in tenant {tenant_id}: {outbox_id}")]
    MissingOutbox {
        tenant_id: String,
        outbox_id: String,
    },
    #[error("outbox {outbox_id} lease is not owned by {lease_owner}")]
    OutboxLeaseLost {
        outbox_id: String,
        lease_owner: String,
    },
    #[error("outbox {outbox_id} lease expired before verified completion")]
    OutboxLeaseExpired { outbox_id: String },
    #[error("outbox lease duration must be between 1 and {MAX_OUTBOX_LEASE_SECONDS} seconds")]
    InvalidOutboxLeaseDuration,
    #[error("outbox retry error exceeds {MAX_OUTBOX_ERROR_BYTES} bytes")]
    OutboxErrorTooLong,
    #[error("outbox retry availability cannot predate the retry decision")]
    OutboxRetryTimeInvalid,
    #[error("outbox retry attempts are exhausted")]
    OutboxAttemptsExhausted,
    #[error("outbox lease expiration is outside the supported timestamp range")]
    OutboxLeaseTimeOverflow,
    #[error("outbox commit ordinal is exhausted")]
    CommitOrdinalExhausted,
    #[error("outbox {outbox_id} is not the tenant commit-order head")]
    OutboxOrderConflict { outbox_id: String },
    #[error("outbox history for turn {turn_id} is incomplete or duplicated")]
    OutboxHistoryIncomplete { turn_id: String },
    #[error("outbox history revision mismatch: expected {expected}, actual {actual}")]
    OutboxHistoryRevision { expected: u64, actual: u64 },
    #[error("outbox history revision {revision} does not begin with Accepted")]
    OutboxHistoryGenesis { revision: u64 },
    #[error("outbox history revision {revision} has the wrong previous state")]
    OutboxHistoryPreviousState { revision: u64 },
    #[error("outbox history has an illegal transition at revision {revision}: {from:?} -> {to:?}")]
    OutboxHistoryIllegalTransition {
        revision: u64,
        from: TurnState,
        to: TurnState,
    },
    #[error("outbox history does not match durable turn {turn_id}")]
    OutboxHistoryCurrentTurn { turn_id: String },
    #[error("outbox revision {revision} is marked delivered without its verified predecessor")]
    OutboxDeliveredPrefix { revision: u64 },
    #[error("outbox revision {revision} references an unexpected ledger event")]
    OutboxLedgerEventMismatch { revision: u64 },
    #[error("verified outbox commit does not match outbox {outbox_id}")]
    OutboxCommitMismatch { outbox_id: String },
    #[error("outbox and event ledger do not resolve to the same database")]
    OutboxLedgerPathMismatch,
    #[error("outbox store and event ledger have different logical database instances")]
    OutboxLedgerInstanceMismatch,
    #[error("outbox principal does not match the supplied verification key")]
    OutboxPrincipalMismatch,
    #[error("delivered outbox {outbox_id} has no verified signer evidence")]
    OutboxSignerEvidenceMissing { outbox_id: String },
    #[error("outbox completion timestamp predates the committed outbox row")]
    OutboxCompletionTimeInvalid,
    #[error("turn state timestamp cannot move backwards")]
    NonMonotonicTimestamp,
    #[error("ledger verification failed: {0}")]
    Ledger(#[from] zaion_ledger::LedgerError),
    #[error("turn store schema integrity failure: {0}")]
    SchemaIntegrity(String),
    #[cfg(test)]
    #[error("injected failure after turn insert")]
    InjectedAfterTurnInsert,
    #[cfg(test)]
    #[error("injected failure after state update")]
    InjectedAfterStateUpdate,
    #[cfg(test)]
    #[error("injected failure after outbox insert")]
    InjectedAfterOutboxInsert,
    #[cfg(test)]
    #[error("injected failure after verified outbox commit evidence insert")]
    InjectedAfterVerifiedCommitEvidence,
}

impl DurableTurnStore {
    pub fn open(db_path: impl AsRef<Path>) -> Result<Self, TurnStoreError> {
        let db_path = db_path.as_ref().to_path_buf();
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let identity_ledger = zaion_ledger::EventLedger::new(&db_path);
        let database_instance_id = identity_ledger.database_instance_id()?;
        let mut conn = Connection::open(&db_path)?;
        conn.busy_timeout(StdDuration::from_secs(5))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=FULL;
             PRAGMA foreign_keys=ON;
             PRAGMA temp_store=MEMORY;",
        )?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(CREATE_SCHEMA)?;
        ensure_outbox_commit_order_schema(&tx, Utc::now())?;
        dispatcher::ensure_outbox_dispatcher_schema(&tx, Utc::now())?;
        ensure_outbox_verified_commit_schema(&tx, Utc::now())?;
        if zaion_ledger::validated_database_instance_id(&tx)? != database_instance_id {
            return Err(TurnStoreError::OutboxLedgerInstanceMismatch);
        }
        tx.commit()?;
        Ok(Self {
            db_path,
            database_instance_id,
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub fn begin_turn(
        &self,
        ingress: &AuthenticatedIngress,
        admission: &DurableTurnAdmission,
        now: DateTime<Utc>,
    ) -> Result<BeginTurnResult, TurnStoreError> {
        self.begin_turn_inner(ingress, admission, now, false, false)
    }

    fn begin_turn_inner(
        &self,
        ingress: &AuthenticatedIngress,
        admission: &DurableTurnAdmission,
        now: DateTime<Utc>,
        inject_after_turn_insert: bool,
        inject_after_outbox_insert: bool,
    ) -> Result<BeginTurnResult, TurnStoreError> {
        validate_actor_authority(ingress, &admission.actor)?;
        let request_json = canonical_json(&admission.request)?;
        let request_hash = sha256_text(&request_json);
        let authority = serde_json::to_value(ingress)?;
        let authority_json = canonical_json(&authority)?;
        let authority_hash = sha256_text(&authority_json);

        let mut conn = self.connection()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(existing) =
            load_by_idempotency(&tx, ingress.tenant_id().as_str(), ingress.idempotency_key())?
        {
            if existing.actor_key == admission.actor.actor_key
                && existing.request_hash == request_hash
                && existing.authority_hash == authority_hash
            {
                let existing = recover_duplicate_if_expired(&tx, &existing, now)?;
                tx.commit()?;
                return Ok(BeginTurnResult::Existing(existing));
            }
            return Err(TurnStoreError::IdempotencyConflict);
        }
        if ingress.deadline() <= now {
            return Err(TurnStoreError::DeadlineExpired);
        }

        ensure_actor_row(&tx, &admission.actor, now)?;
        recover_actor_for_admission(&tx, &admission.actor, now)?;

        let turn_id = deterministic_turn_id(
            ingress.tenant_id().as_str(),
            ingress.idempotency_key(),
            &request_hash,
        );
        let now_ms = timestamp_millis(now);
        let initial_state = if admission.approval_required {
            state_name(TurnState::WaitingApproval)
        } else {
            state_name(TurnState::Accepted)
        };
        tx.execute(
            &format!(
                "INSERT INTO turn_state_v2 (
                tenant_id, turn_id, actor_key, subject_id, principal_id,
                workspace_id, profile_id, session_id, source_surface, source_id,
                idempotency_key, request_json, request_hash, authority_json,
                authority_hash, deadline_ms, state, revision, terminal_result_json,
                terminal_result_hash, created_at_ms, updated_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11,
                       ?12, ?13, ?14, ?15, ?16, '{initial_state}', 0, NULL, NULL,
                       ?17, ?17)"
            ),
            params![
                ingress.tenant_id().as_str(),
                turn_id,
                admission.actor.actor_key,
                ingress.subject_id().as_str(),
                ingress.principal_id().as_str(),
                ingress.workspace_id().0.as_str(),
                ingress.profile_id().as_str(),
                ingress.session_id().0.as_str(),
                ingress.source().surface(),
                ingress.source().source_id(),
                ingress.idempotency_key(),
                request_json,
                request_hash,
                authority_json,
                authority_hash,
                timestamp_millis(ingress.deadline()),
                now_ms,
            ],
        )?;
        #[cfg(test)]
        if inject_after_turn_insert {
            return Err(TurnStoreError::InjectedAfterTurnInsert);
        }
        #[cfg(not(test))]
        let _ = inject_after_turn_insert;

        let actor_changed = tx.execute(
            "UPDATE turn_actor_v2
             SET active_turn_id = ?3, lease_owner = ?4, lease_until_ms = ?5,
                 revision = revision + 1, updated_at_ms = ?6
             WHERE tenant_id = ?1 AND actor_key = ?2 AND active_turn_id IS NULL",
            params![
                ingress.tenant_id().as_str(),
                admission.actor.actor_key,
                turn_id,
                admission.lease_owner,
                timestamp_millis(ingress.deadline()),
                now_ms,
            ],
        )?;
        if actor_changed != 1 {
            return Err(TurnStoreError::CasLost);
        }

        let record = load_turn(&tx, ingress.tenant_id().as_str(), &turn_id)?.ok_or_else(|| {
            TurnStoreError::MissingTurn {
                tenant_id: ingress.tenant_id().as_str().to_string(),
                turn_id: turn_id.clone(),
            }
        })?;
        insert_outbox(&tx, &record, None, None, now)?;
        #[cfg(test)]
        if inject_after_outbox_insert {
            return Err(TurnStoreError::InjectedAfterOutboxInsert);
        }
        #[cfg(not(test))]
        let _ = inject_after_outbox_insert;
        tx.commit()?;
        Ok(BeginTurnResult::Created(record))
    }

    pub fn load(
        &self,
        tenant_id: &str,
        turn_id: &str,
    ) -> Result<Option<DurableTurnRecord>, TurnStoreError> {
        let conn = self.connection()?;
        load_turn(&conn, tenant_id, turn_id)
    }

    pub fn load_by_idempotency_key(
        &self,
        tenant_id: &str,
        idempotency_key: &str,
    ) -> Result<Option<DurableTurnRecord>, TurnStoreError> {
        let conn = self.connection()?;
        load_by_idempotency(&conn, tenant_id, idempotency_key)
    }

    pub fn load_actor(
        &self,
        tenant_id: &str,
        actor_key: &str,
    ) -> Result<Option<TurnActorRecord>, TurnStoreError> {
        let conn = self.connection()?;
        load_actor(&conn, tenant_id, actor_key)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn compare_and_transition(
        &self,
        tenant_id: &str,
        turn_id: &str,
        lease_owner: &str,
        expected_state: TurnState,
        expected_revision: u64,
        next: TurnState,
        now: DateTime<Utc>,
    ) -> Result<DurableTurnRecord, TurnStoreError> {
        if next.is_terminal() {
            return Err(TurnStoreError::MissingTerminalResult);
        }
        self.transition_inner(
            tenant_id,
            turn_id,
            lease_owner,
            expected_state,
            expected_revision,
            next,
            None,
            now,
            false,
            false,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn compare_and_transition_with_result(
        &self,
        tenant_id: &str,
        turn_id: &str,
        lease_owner: &str,
        expected_state: TurnState,
        expected_revision: u64,
        next: TurnState,
        terminal_result: &TurnExecution,
        now: DateTime<Utc>,
    ) -> Result<DurableTurnRecord, TurnStoreError> {
        if !next.is_terminal() {
            return Err(TurnStoreError::NonTerminalResult);
        }
        let actual = terminal_result.terminal_state();
        if actual != next {
            return Err(TurnStoreError::TerminalOutcomeMismatch {
                expected: next,
                actual,
            });
        }
        self.transition_inner(
            tenant_id,
            turn_id,
            lease_owner,
            expected_state,
            expected_revision,
            next,
            Some(terminal_result),
            now,
            false,
            false,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn transition_inner(
        &self,
        tenant_id: &str,
        turn_id: &str,
        lease_owner: &str,
        expected_state: TurnState,
        expected_revision: u64,
        next: TurnState,
        terminal_result: Option<&TurnExecution>,
        now: DateTime<Utc>,
        inject_after_state_update: bool,
        inject_after_outbox_insert: bool,
    ) -> Result<DurableTurnRecord, TurnStoreError> {
        validate_lease_identity("lease_owner", lease_owner)?;
        let mut conn = self.connection()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current =
            load_turn(&tx, tenant_id, turn_id)?.ok_or_else(|| TurnStoreError::MissingTurn {
                tenant_id: tenant_id.to_string(),
                turn_id: turn_id.to_string(),
            })?;
        if now < current.updated_at {
            return Err(TurnStoreError::NonMonotonicTimestamp);
        }
        verify_actor_lease(&tx, &current, lease_owner, now)?;
        let next_state =
            current
                .state
                .compare_and_transition(expected_state, expected_revision, next)?;
        let terminal_json = terminal_result
            .map(serde_json::to_value)
            .transpose()?
            .as_ref()
            .map(canonical_json)
            .transpose()?;
        let terminal_hash = terminal_json.as_deref().map(sha256_text);
        let changed = tx.execute(
            "UPDATE turn_state_v2
             SET state = ?5, revision = ?6, terminal_result_json = ?7,
                 terminal_result_hash = ?8, updated_at_ms = ?9
             WHERE tenant_id = ?1 AND turn_id = ?2 AND state = ?3 AND revision = ?4",
            params![
                tenant_id,
                turn_id,
                state_name(expected_state),
                revision_to_i64(expected_revision)?,
                state_name(next),
                revision_to_i64(next_state.revision())?,
                terminal_json,
                terminal_hash,
                timestamp_millis(now),
            ],
        )?;
        if changed != 1 {
            return Err(TurnStoreError::CasLost);
        }
        #[cfg(test)]
        if inject_after_state_update {
            return Err(TurnStoreError::InjectedAfterStateUpdate);
        }
        #[cfg(not(test))]
        let _ = inject_after_state_update;

        update_actor_after_transition(&tx, &current, lease_owner, next, now)?;
        let record =
            load_turn(&tx, tenant_id, turn_id)?.ok_or_else(|| TurnStoreError::MissingTurn {
                tenant_id: tenant_id.to_string(),
                turn_id: turn_id.to_string(),
            })?;
        insert_outbox(
            &tx,
            &record,
            Some(expected_state),
            terminal_hash.as_deref(),
            now,
        )?;
        #[cfg(test)]
        if inject_after_outbox_insert {
            return Err(TurnStoreError::InjectedAfterOutboxInsert);
        }
        #[cfg(not(test))]
        let _ = inject_after_outbox_insert;
        tx.commit()?;
        Ok(record)
    }

    pub fn incomplete_turns(
        &self,
        tenant_id: &str,
        limit: usize,
    ) -> Result<Vec<DurableTurnRecord>, TurnStoreError> {
        let conn = self.connection()?;
        let mut statement = conn.prepare(&format!(
            "{TURN_SELECT}
             WHERE tenant_id = ?1
               AND state NOT IN ('completed', 'degraded', 'aborted', 'quarantined')
             ORDER BY updated_at_ms, turn_id
             LIMIT ?2"
        ))?;
        let rows =
            statement.query_map(params![tenant_id, bounded_limit(limit)], raw_turn_from_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .map(materialize_turn)
            .collect()
    }

    pub fn recover_expired_actor_leases(
        &self,
        tenant_id: &str,
        now: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<DurableTurnRecord>, TurnStoreError> {
        let mut conn = self.connection()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let actor_turns = {
            let mut statement = tx.prepare(
                "SELECT actor_key, active_turn_id
                 FROM turn_actor_v2
                 WHERE tenant_id = ?1 AND active_turn_id IS NOT NULL
                   AND (lease_until_ms IS NULL OR lease_until_ms <= ?2)
                 ORDER BY updated_at_ms, actor_key
                 LIMIT ?3",
            )?;
            let rows = statement.query_map(
                params![tenant_id, timestamp_millis(now), bounded_limit(limit)],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        let mut recovered = Vec::with_capacity(actor_turns.len());
        for (actor_key, turn_id) in actor_turns {
            let record = load_turn(&tx, tenant_id, &turn_id)?.ok_or_else(|| {
                TurnStoreError::MissingTurn {
                    tenant_id: tenant_id.to_string(),
                    turn_id: turn_id.clone(),
                }
            })?;
            if record.actor_key != actor_key {
                return Err(TurnStoreError::ActorAuthorityMismatch);
            }
            recovered.push(recover_turn_in_tx(&tx, &record, now)?);
        }
        tx.commit()?;
        Ok(recovered)
    }

    pub fn undelivered_outbox(
        &self,
        tenant_id: &str,
        limit: usize,
    ) -> Result<Vec<TurnOutboxRecord>, TurnStoreError> {
        let conn = self.connection()?;
        let _ = tenant_outbox_head(&conn, tenant_id)?;
        let mut statement = conn.prepare(&format!(
            "{OUTBOX_SELECT}
             WHERE o.tenant_id = ?1 AND o.status != 'delivered'
             ORDER BY c.commit_ordinal
             LIMIT ?2"
        ))?;
        let rows = statement.query_map(
            params![tenant_id, bounded_limit(limit)],
            raw_outbox_from_row,
        )?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .map(materialize_outbox)
            .collect()
    }

    pub fn claim_next_outbox(
        &self,
        tenant_id: &str,
        lease_owner: &str,
        now: DateTime<Utc>,
        lease_duration: Duration,
    ) -> Result<Option<TurnOutboxRecord>, TurnStoreError> {
        validate_lease_identity("lease_owner", lease_owner)?;
        if lease_duration < Duration::seconds(1)
            || lease_duration > Duration::seconds(MAX_OUTBOX_LEASE_SECONDS)
        {
            return Err(TurnStoreError::InvalidOutboxLeaseDuration);
        }
        let mut conn = self.connection()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        validate_no_extra_outbox_triggers(&tx)?;
        let now_ms = timestamp_millis(now);
        let candidate = tenant_outbox_head(&tx, tenant_id)?;
        let Some(outbox_id) = candidate else {
            tx.commit()?;
            return Ok(None);
        };
        let current = load_outbox(&tx, tenant_id, &outbox_id)?.ok_or_else(|| {
            TurnStoreError::MissingOutbox {
                tenant_id: tenant_id.to_string(),
                outbox_id: outbox_id.clone(),
            }
        })?;
        if dispatcher::outbox_is_quarantined(&tx, tenant_id, &outbox_id)? {
            tx.commit()?;
            return Ok(None);
        }
        let ready = current.available_at <= now
            && match current.status {
                TurnOutboxStatus::Pending => true,
                TurnOutboxStatus::Leased => current
                    .lease_until
                    .is_some_and(|lease_until| lease_until <= now),
                TurnOutboxStatus::Delivered => false,
            };
        if !ready {
            tx.commit()?;
            return Ok(None);
        }
        if current.attempts >= i64::MAX as u64 {
            return Err(TurnStoreError::OutboxAttemptsExhausted);
        }
        let lease_until = now
            .checked_add_signed(lease_duration)
            .ok_or(TurnStoreError::OutboxLeaseTimeOverflow)?;
        let lease_token = format!("outbox-lease-{}", uuid::Uuid::new_v4());
        let changed = tx.execute(
            "UPDATE turn_outbox_v2
             SET status = 'leased', lease_owner = ?3, lease_token = ?4,
                 lease_until_ms = ?5, attempts = attempts + 1,
                 last_error = NULL, updated_at_ms = ?6
             WHERE tenant_id = ?1 AND outbox_id = ?2
               AND (status = 'pending' OR (status = 'leased' AND lease_until_ms <= ?6))",
            params![
                tenant_id,
                outbox_id,
                lease_owner,
                lease_token,
                timestamp_millis(lease_until),
                now_ms,
            ],
        )?;
        if changed != 1 {
            return Err(TurnStoreError::CasLost);
        }
        let record = load_outbox(&tx, tenant_id, &outbox_id)?.ok_or_else(|| {
            TurnStoreError::MissingOutbox {
                tenant_id: tenant_id.to_string(),
                outbox_id: outbox_id.clone(),
            }
        })?;
        if record.status != TurnOutboxStatus::Leased
            || record.lease_owner.as_deref() != Some(lease_owner)
            || record.lease_token.as_deref() != Some(lease_token.as_str())
            || record.lease_until.map(timestamp_millis) != Some(timestamp_millis(lease_until))
            || record.attempts != current.attempts + 1
            || timestamp_millis(record.updated_at) != timestamp_millis(now)
            || tenant_outbox_head(&tx, tenant_id)?.as_deref() != Some(outbox_id.as_str())
        {
            return Err(TurnStoreError::SchemaIntegrity(
                "outbox claim was modified by unexpected database behavior".to_string(),
            ));
        }
        tx.commit()?;
        Ok(Some(record))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn release_outbox(
        &self,
        tenant_id: &str,
        outbox_id: &str,
        lease_owner: &str,
        lease_token: &str,
        now: DateTime<Utc>,
        available_at: DateTime<Utc>,
        error: &str,
    ) -> Result<(), TurnStoreError> {
        validate_lease_identity("lease_owner", lease_owner)?;
        validate_lease_identity("lease_token", lease_token)?;
        if error.len() > MAX_OUTBOX_ERROR_BYTES {
            return Err(TurnStoreError::OutboxErrorTooLong);
        }
        if available_at < now {
            return Err(TurnStoreError::OutboxRetryTimeInvalid);
        }
        let mut conn = self.connection()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        validate_no_extra_outbox_triggers(&tx)?;
        if tenant_outbox_head(&tx, tenant_id)?.as_deref() != Some(outbox_id) {
            return Err(TurnStoreError::OutboxOrderConflict {
                outbox_id: outbox_id.to_string(),
            });
        }
        let current = load_outbox(&tx, tenant_id, outbox_id)?.ok_or_else(|| {
            TurnStoreError::MissingOutbox {
                tenant_id: tenant_id.to_string(),
                outbox_id: outbox_id.to_string(),
            }
        })?;
        if now < current.updated_at {
            return Err(TurnStoreError::NonMonotonicTimestamp);
        }
        if current.status != TurnOutboxStatus::Leased
            || current.lease_owner.as_deref() != Some(lease_owner)
            || current.lease_token.as_deref() != Some(lease_token)
        {
            return Err(TurnStoreError::OutboxLeaseLost {
                outbox_id: outbox_id.to_string(),
                lease_owner: lease_owner.to_string(),
            });
        }
        if current
            .lease_until
            .is_none_or(|lease_until| lease_until <= now)
        {
            return Err(TurnStoreError::OutboxLeaseExpired {
                outbox_id: outbox_id.to_string(),
            });
        }
        let changed = tx.execute(
            "UPDATE turn_outbox_v2
             SET status = 'pending', lease_owner = NULL, lease_token = NULL,
                  lease_until_ms = NULL, available_at_ms = ?5,
                  last_error = ?6, updated_at_ms = ?7
             WHERE tenant_id = ?1 AND outbox_id = ?2 AND status = 'leased'
               AND lease_owner = ?3 AND lease_token = ?4
               AND lease_until_ms > ?7",
            params![
                tenant_id,
                outbox_id,
                lease_owner,
                lease_token,
                timestamp_millis(available_at),
                error,
                timestamp_millis(now),
            ],
        )?;
        if changed == 1 {
            let released = load_outbox(&tx, tenant_id, outbox_id)?.ok_or_else(|| {
                TurnStoreError::MissingOutbox {
                    tenant_id: tenant_id.to_string(),
                    outbox_id: outbox_id.to_string(),
                }
            })?;
            if released.status != TurnOutboxStatus::Pending
                || timestamp_millis(released.available_at) != timestamp_millis(available_at)
                || timestamp_millis(released.updated_at) != timestamp_millis(now)
                || released.last_error.as_deref() != Some(error)
                || tenant_outbox_head(&tx, tenant_id)?.as_deref() != Some(outbox_id)
            {
                return Err(TurnStoreError::SchemaIntegrity(
                    "outbox release was modified by unexpected database behavior".to_string(),
                ));
            }
            tx.commit()?;
            return Ok(());
        }
        Err(TurnStoreError::OutboxLeaseLost {
            outbox_id: outbox_id.to_string(),
            lease_owner: lease_owner.to_string(),
        })
    }

    fn connection(&self) -> Result<MutexGuard<'_, Connection>, TurnStoreError> {
        self.conn.lock().map_err(|_| TurnStoreError::MutexPoisoned)
    }

    #[cfg(test)]
    pub(crate) fn begin_turn_with_failpoint(
        &self,
        ingress: &AuthenticatedIngress,
        admission: &DurableTurnAdmission,
        now: DateTime<Utc>,
        after_turn_insert: bool,
        after_outbox_insert: bool,
    ) -> Result<BeginTurnResult, TurnStoreError> {
        self.begin_turn_inner(
            ingress,
            admission,
            now,
            after_turn_insert,
            after_outbox_insert,
        )
    }

    #[cfg(test)]
    pub(crate) fn transition_with_failpoint(
        &self,
        record: &DurableTurnRecord,
        lease_owner: &str,
        next: TurnState,
        now: DateTime<Utc>,
        after_state_update: bool,
        after_outbox_insert: bool,
    ) -> Result<DurableTurnRecord, TurnStoreError> {
        self.transition_inner(
            &record.tenant_id,
            &record.turn_id,
            lease_owner,
            record.state.state(),
            record.state.revision(),
            next,
            None,
            now,
            after_state_update,
            after_outbox_insert,
        )
    }
}

#[derive(Debug)]
struct OutboxOrderManifest {
    source_row_count: i64,
    source_max_rowid: i64,
    source_digest: String,
}

#[derive(Debug, Clone)]
struct LegacyOutboxOrderRow {
    source_rowid: i64,
    tenant_id: String,
    outbox_id: String,
    payload_hash: String,
}

fn ensure_outbox_commit_order_schema(
    tx: &Transaction<'_>,
    applied_at: DateTime<Utc>,
) -> Result<(), TurnStoreError> {
    let order_table_existed = schema_object_sql(tx, "table", OUTBOX_ORDER_TABLE)?.is_some();
    let order_index_existed = schema_object_sql(tx, "index", OUTBOX_ORDER_INDEX)?.is_some();
    let revision_index_existed = schema_object_sql(tx, "index", OUTBOX_REVISION_INDEX)?.is_some();
    let order_trigger_existed = schema_object_sql(tx, "trigger", OUTBOX_ORDER_TRIGGER)?.is_some();
    let update_guard_existed =
        schema_object_sql(tx, "trigger", OUTBOX_ORDER_UPDATE_GUARD)?.is_some();
    let delete_guard_existed =
        schema_object_sql(tx, "trigger", OUTBOX_ORDER_DELETE_GUARD)?.is_some();
    tx.execute_batch(CREATE_ORDER_MIGRATION_TABLE)?;

    let manifest = load_outbox_order_manifest(tx)?;
    if manifest.is_none() {
        if order_table_existed
            || order_index_existed
            || revision_index_existed
            || order_trigger_existed
            || update_guard_existed
            || delete_guard_existed
        {
            return Err(TurnStoreError::SchemaIntegrity(
                "outbox order objects exist without their migration marker".to_string(),
            ));
        }
        tx.execute_batch(CREATE_ORDER_TABLE)?;
        let source_rows = legacy_outbox_order_rows(tx)?;
        for source in &source_rows {
            tx.execute(
                "INSERT INTO turn_outbox_commit_order_v2 (
                    tenant_id, outbox_id, order_origin, legacy_source_rowid
                 ) VALUES (?1, ?2, 'legacy_rowid_backfill', ?3)",
                params![source.tenant_id, source.outbox_id, source.source_rowid],
            )?;
        }
        let source_row_count = i64::try_from(source_rows.len()).map_err(|_| {
            TurnStoreError::SchemaIntegrity(
                "legacy outbox row count exceeds SQLite integer range".to_string(),
            )
        })?;
        let source_max_rowid = source_rows.last().map_or(0, |source| source.source_rowid);
        let source_digest = legacy_outbox_order_digest(&source_rows);
        tx.execute(
            "INSERT INTO turn_store_schema_migrations_v2 (
                migration_id, migration_kind, source_row_count, source_max_rowid,
                source_digest, applied_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                OUTBOX_ORDER_MIGRATION_ID,
                OUTBOX_ORDER_MIGRATION_KIND,
                source_row_count,
                source_max_rowid,
                source_digest,
                timestamp_millis(applied_at),
            ],
        )?;
        tx.execute_batch(CREATE_ORDER_INDEX)?;
        tx.execute_batch(CREATE_REVISION_INDEX)?;
        tx.execute_batch(CREATE_ORDER_TRIGGER)?;
    } else if !order_table_existed
        || !order_index_existed
        || !revision_index_existed
        || !order_trigger_existed
    {
        return Err(TurnStoreError::SchemaIntegrity(
            "outbox order migration marker exists but a required schema object is missing"
                .to_string(),
        ));
    }

    let guard_marker = load_order_guard_marker(tx)?;
    match (
        guard_marker.is_some(),
        update_guard_existed,
        delete_guard_existed,
    ) {
        (false, false, false) => {
            tx.execute_batch(CREATE_ORDER_UPDATE_GUARD)?;
            tx.execute_batch(CREATE_ORDER_DELETE_GUARD)?;
            let (source_row_count, source_max_ordinal, source_digest) =
                order_guard_prefix_evidence(tx, None)?;
            tx.execute(
                "INSERT INTO turn_store_schema_migrations_v2 (
                    migration_id, migration_kind, source_row_count, source_max_rowid,
                    source_digest, applied_at_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    OUTBOX_ORDER_GUARD_MIGRATION_ID,
                    OUTBOX_ORDER_GUARD_MIGRATION_KIND,
                    source_row_count,
                    source_max_ordinal,
                    source_digest,
                    timestamp_millis(applied_at),
                ],
            )?;
        }
        (false, _, _) => {
            return Err(TurnStoreError::SchemaIntegrity(
                "outbox order guard objects exist without their migration marker".to_string(),
            ));
        }
        (true, true, true) => {}
        (true, _, _) => {
            return Err(TurnStoreError::SchemaIntegrity(
                "outbox order guard migration marker exists but a guard is missing".to_string(),
            ));
        }
    }

    validate_schema_object(tx, "table", OUTBOX_ORDER_TABLE, CREATE_ORDER_TABLE)?;
    validate_schema_object(
        tx,
        "table",
        "turn_store_schema_migrations_v2",
        EXPECTED_ORDER_MIGRATION_TABLE,
    )?;
    validate_schema_object(tx, "index", OUTBOX_ORDER_INDEX, CREATE_ORDER_INDEX)?;
    validate_schema_object(tx, "index", OUTBOX_REVISION_INDEX, CREATE_REVISION_INDEX)?;
    validate_schema_object(tx, "trigger", OUTBOX_ORDER_TRIGGER, CREATE_ORDER_TRIGGER)?;
    validate_schema_object(
        tx,
        "trigger",
        OUTBOX_ORDER_UPDATE_GUARD,
        CREATE_ORDER_UPDATE_GUARD,
    )?;
    validate_schema_object(
        tx,
        "trigger",
        OUTBOX_ORDER_DELETE_GUARD,
        CREATE_ORDER_DELETE_GUARD,
    )?;
    validate_order_trigger_schema(tx)?;
    validate_outbox_trigger_allowlist(tx)?;
    validate_order_guard_marker(tx)?;
    let manifest = load_outbox_order_manifest(tx)?.ok_or_else(|| {
        TurnStoreError::SchemaIntegrity("outbox order migration marker disappeared".to_string())
    })?;
    validate_outbox_order_mapping(tx, &manifest)
}

fn ensure_outbox_verified_commit_schema(
    tx: &Transaction<'_>,
    applied_at: DateTime<Utc>,
) -> Result<(), TurnStoreError> {
    let table_existed = schema_object_sql(tx, "table", OUTBOX_VERIFIED_COMMIT_TABLE)?.is_some();
    let insert_guard_existed =
        schema_object_sql(tx, "trigger", OUTBOX_VERIFIED_COMMIT_INSERT_GUARD)?.is_some();
    let update_guard_existed =
        schema_object_sql(tx, "trigger", OUTBOX_VERIFIED_COMMIT_UPDATE_GUARD)?.is_some();
    let delete_guard_existed =
        schema_object_sql(tx, "trigger", OUTBOX_VERIFIED_COMMIT_DELETE_GUARD)?.is_some();
    let delivery_guard_existed =
        schema_object_sql(tx, "trigger", OUTBOX_VERIFIED_DELIVERY_GUARD)?.is_some();
    let marker = load_verified_commit_marker(tx)?;

    match (
        marker.is_some(),
        table_existed,
        insert_guard_existed,
        update_guard_existed,
        delete_guard_existed,
        delivery_guard_existed,
    ) {
        (false, false, false, false, false, false) => {
            tx.execute_batch(CREATE_VERIFIED_COMMIT_TABLE)?;
            tx.execute_batch(CREATE_VERIFIED_COMMIT_INSERT_GUARD)?;
            tx.execute_batch(CREATE_VERIFIED_COMMIT_UPDATE_GUARD)?;
            tx.execute_batch(CREATE_VERIFIED_COMMIT_DELETE_GUARD)?;
            tx.execute_batch(CREATE_VERIFIED_DELIVERY_GUARD)?;
            let (source_row_count, source_max_ordinal, source_digest) =
                order_guard_prefix_evidence(tx, None)?;
            tx.execute(
                "INSERT INTO turn_store_schema_migrations_v2 (
                    migration_id, migration_kind, source_row_count, source_max_rowid,
                    source_digest, applied_at_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    OUTBOX_VERIFIED_COMMIT_MIGRATION_ID,
                    OUTBOX_VERIFIED_COMMIT_MIGRATION_KIND,
                    source_row_count,
                    source_max_ordinal,
                    source_digest,
                    timestamp_millis(applied_at),
                ],
            )?;
        }
        (false, _, _, _, _, _) => {
            return Err(TurnStoreError::SchemaIntegrity(
                "verified outbox commit objects exist without their migration marker".to_string(),
            ));
        }
        (true, true, true, true, true, true) => {}
        (true, _, _, _, _, _) => {
            return Err(TurnStoreError::SchemaIntegrity(
                "verified outbox commit migration marker exists but a required object is missing"
                    .to_string(),
            ));
        }
    }

    validate_schema_object(
        tx,
        "table",
        OUTBOX_VERIFIED_COMMIT_TABLE,
        CREATE_VERIFIED_COMMIT_TABLE,
    )?;
    validate_schema_object(
        tx,
        "trigger",
        OUTBOX_VERIFIED_COMMIT_INSERT_GUARD,
        CREATE_VERIFIED_COMMIT_INSERT_GUARD,
    )?;
    validate_schema_object(
        tx,
        "trigger",
        OUTBOX_VERIFIED_COMMIT_UPDATE_GUARD,
        CREATE_VERIFIED_COMMIT_UPDATE_GUARD,
    )?;
    validate_schema_object(
        tx,
        "trigger",
        OUTBOX_VERIFIED_COMMIT_DELETE_GUARD,
        CREATE_VERIFIED_COMMIT_DELETE_GUARD,
    )?;
    validate_schema_object(
        tx,
        "trigger",
        OUTBOX_VERIFIED_DELIVERY_GUARD,
        CREATE_VERIFIED_DELIVERY_GUARD,
    )?;
    validate_no_extra_outbox_triggers(tx)?;
    let marker = load_verified_commit_marker(tx)?.ok_or_else(|| {
        TurnStoreError::SchemaIntegrity(
            "verified outbox commit migration marker disappeared".to_string(),
        )
    })?;
    validate_verified_commit_marker(tx, &marker)?;
    validate_verified_commit_rows(tx, &marker)
}

fn schema_object_sql(
    tx: &Transaction<'_>,
    object_type: &str,
    name: &str,
) -> Result<Option<String>, TurnStoreError> {
    tx.query_row(
        "SELECT sql FROM sqlite_master WHERE type = ?1 AND name = ?2",
        params![object_type, name],
        |row| row.get(0),
    )
    .optional()
    .map_err(Into::into)
}

fn validate_schema_object(
    tx: &Transaction<'_>,
    object_type: &str,
    name: &str,
    expected_sql: &str,
) -> Result<(), TurnStoreError> {
    let actual = schema_object_sql(tx, object_type, name)?.ok_or_else(|| {
        TurnStoreError::SchemaIntegrity(format!("required {object_type} {name} is missing"))
    })?;
    if normalize_schema_sql(&actual) == normalize_schema_sql(expected_sql) {
        Ok(())
    } else {
        Err(TurnStoreError::SchemaIntegrity(format!(
            "required {object_type} {name} has an unexpected definition"
        )))
    }
}

fn normalize_schema_sql(sql: &str) -> String {
    sql.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_end_matches(';')
        .to_string()
}

fn validate_order_trigger_schema(tx: &Transaction<'_>) -> Result<(), TurnStoreError> {
    validate_schema_object(tx, "trigger", OUTBOX_ORDER_TRIGGER, CREATE_ORDER_TRIGGER)?;
    validate_schema_object(
        tx,
        "trigger",
        OUTBOX_ORDER_UPDATE_GUARD,
        CREATE_ORDER_UPDATE_GUARD,
    )?;
    validate_schema_object(
        tx,
        "trigger",
        OUTBOX_ORDER_DELETE_GUARD,
        CREATE_ORDER_DELETE_GUARD,
    )
}

fn validate_no_extra_outbox_triggers(tx: &Transaction<'_>) -> Result<(), TurnStoreError> {
    validate_order_trigger_schema(tx)?;
    validate_schema_object(
        tx,
        "trigger",
        OUTBOX_VERIFIED_COMMIT_INSERT_GUARD,
        CREATE_VERIFIED_COMMIT_INSERT_GUARD,
    )?;
    validate_schema_object(
        tx,
        "trigger",
        OUTBOX_VERIFIED_COMMIT_UPDATE_GUARD,
        CREATE_VERIFIED_COMMIT_UPDATE_GUARD,
    )?;
    validate_schema_object(
        tx,
        "trigger",
        OUTBOX_VERIFIED_COMMIT_DELETE_GUARD,
        CREATE_VERIFIED_COMMIT_DELETE_GUARD,
    )?;
    validate_schema_object(
        tx,
        "trigger",
        OUTBOX_VERIFIED_DELIVERY_GUARD,
        CREATE_VERIFIED_DELIVERY_GUARD,
    )?;
    dispatcher::validate_outbox_dispatcher_triggers(tx)?;
    validate_outbox_trigger_allowlist(tx)
}

fn validate_outbox_trigger_allowlist(tx: &Transaction<'_>) -> Result<(), TurnStoreError> {
    let extra: Option<String> = tx
        .query_row(
            "SELECT name FROM sqlite_master
             WHERE type = 'trigger'
               AND tbl_name IN (
                   'turn_actor_v2', 'turn_state_v2', 'turn_outbox_v2',
                   'turn_outbox_commit_order_v2',
                   'turn_outbox_verified_commit_v2',
                   'turn_outbox_quarantine_v2',
                   'turn_store_schema_migrations_v2'
                )
               AND name NOT IN (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ORDER BY name LIMIT 1",
            params![
                OUTBOX_ORDER_TRIGGER,
                OUTBOX_ORDER_UPDATE_GUARD,
                OUTBOX_ORDER_DELETE_GUARD,
                OUTBOX_VERIFIED_COMMIT_INSERT_GUARD,
                OUTBOX_VERIFIED_COMMIT_UPDATE_GUARD,
                OUTBOX_VERIFIED_COMMIT_DELETE_GUARD,
                OUTBOX_VERIFIED_DELIVERY_GUARD,
                dispatcher::OUTBOX_QUARANTINE_INSERT_GUARD,
                dispatcher::OUTBOX_QUARANTINE_UPDATE_GUARD,
                dispatcher::OUTBOX_QUARANTINE_DELETE_GUARD,
            ],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(name) = extra {
        Err(TurnStoreError::SchemaIntegrity(format!(
            "unexpected outbox trigger is installed: {name}"
        )))
    } else {
        Ok(())
    }
}

fn load_verified_commit_marker(
    tx: &Transaction<'_>,
) -> Result<Option<OutboxOrderManifest>, TurnStoreError> {
    tx.query_row(
        "SELECT migration_kind, source_row_count, source_max_rowid, source_digest
         FROM turn_store_schema_migrations_v2 WHERE migration_id = ?1",
        params![OUTBOX_VERIFIED_COMMIT_MIGRATION_ID],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
            ))
        },
    )
    .optional()?
    .map(
        |(kind, source_row_count, source_max_rowid, source_digest)| {
            if kind != OUTBOX_VERIFIED_COMMIT_MIGRATION_KIND
                || source_row_count < 0
                || source_max_rowid < 0
                || !source_digest.starts_with("sha256:")
            {
                return Err(TurnStoreError::SchemaIntegrity(
                    "verified outbox commit migration marker is malformed".to_string(),
                ));
            }
            Ok(OutboxOrderManifest {
                source_row_count,
                source_max_rowid,
                source_digest,
            })
        },
    )
    .transpose()
}

fn validate_verified_commit_marker(
    tx: &Transaction<'_>,
    marker: &OutboxOrderManifest,
) -> Result<(), TurnStoreError> {
    let (count, maximum, digest) = order_guard_prefix_evidence(tx, Some(marker.source_max_rowid))?;
    if count == marker.source_row_count
        && maximum == marker.source_max_rowid
        && digest == marker.source_digest
    {
        Ok(())
    } else {
        Err(TurnStoreError::SchemaIntegrity(
            "verified outbox commit migration evidence does not match its prefix".to_string(),
        ))
    }
}

fn validate_verified_commit_rows(
    tx: &Transaction<'_>,
    marker: &OutboxOrderManifest,
) -> Result<(), TurnStoreError> {
    let database_instance_id = zaion_ledger::validated_database_instance_id(tx)?;
    let invalid_evidence: i64 = tx.query_row(
        "SELECT COUNT(*)
         FROM turn_outbox_verified_commit_v2 v
         LEFT JOIN turn_outbox_v2 o
           ON o.tenant_id = v.tenant_id AND o.outbox_id = v.outbox_id
         WHERE o.outbox_id IS NULL OR o.status != 'delivered'
            OR o.ledger_event_id != v.ledger_event_id
            OR v.database_instance_id != ?1",
        params![database_instance_id],
        |row| row.get(0),
    )?;
    if invalid_evidence != 0 {
        return Err(TurnStoreError::SchemaIntegrity(
            "verified outbox commit evidence is orphaned or mismatched".to_string(),
        ));
    }

    let unverified_new_deliveries: i64 = tx.query_row(
        "SELECT COUNT(*)
         FROM turn_outbox_v2 o
         JOIN turn_outbox_commit_order_v2 c
           ON c.tenant_id = o.tenant_id AND c.outbox_id = o.outbox_id
         LEFT JOIN turn_outbox_verified_commit_v2 v
           ON v.tenant_id = o.tenant_id AND v.outbox_id = o.outbox_id
         WHERE o.status = 'delivered' AND v.outbox_id IS NULL
           AND c.commit_ordinal > ?1",
        params![marker.source_max_rowid],
        |row| row.get(0),
    )?;
    if unverified_new_deliveries != 0 {
        return Err(TurnStoreError::SchemaIntegrity(
            "post-migration delivered outbox row lacks verified commit evidence".to_string(),
        ));
    }
    Ok(())
}

fn load_order_guard_marker(
    tx: &Transaction<'_>,
) -> Result<Option<OutboxOrderManifest>, TurnStoreError> {
    tx.query_row(
        "SELECT migration_kind, source_row_count, source_max_rowid, source_digest
         FROM turn_store_schema_migrations_v2 WHERE migration_id = ?1",
        params![OUTBOX_ORDER_GUARD_MIGRATION_ID],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
            ))
        },
    )
    .optional()?
    .map(
        |(kind, source_row_count, source_max_rowid, source_digest)| {
            if kind != OUTBOX_ORDER_GUARD_MIGRATION_KIND
                || source_row_count < 0
                || source_max_rowid < 0
                || !source_digest.starts_with("sha256:")
            {
                return Err(TurnStoreError::SchemaIntegrity(
                    "outbox order guard migration marker is malformed".to_string(),
                ));
            }
            Ok(OutboxOrderManifest {
                source_row_count,
                source_max_rowid,
                source_digest,
            })
        },
    )
    .transpose()
}

fn order_guard_prefix_evidence(
    tx: &Transaction<'_>,
    maximum_ordinal: Option<i64>,
) -> Result<(i64, i64, String), TurnStoreError> {
    let rows = {
        let mut statement = tx.prepare(
            "SELECT c.commit_ordinal, c.tenant_id, c.outbox_id, c.order_origin,
                    c.legacy_source_rowid, o.payload_hash
             FROM turn_outbox_commit_order_v2 c
             JOIN turn_outbox_v2 o
               ON o.tenant_id = c.tenant_id AND o.outbox_id = c.outbox_id
             WHERE ?1 IS NULL OR c.commit_ordinal <= ?1
             ORDER BY c.commit_ordinal",
        )?;
        let rows = statement.query_map(params![maximum_ordinal], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    let mut hasher = Sha256::new();
    for (ordinal, tenant_id, outbox_id, origin, legacy_rowid, payload_hash) in &rows {
        hasher.update(ordinal.to_le_bytes());
        hasher.update(legacy_rowid.unwrap_or(-1).to_le_bytes());
        for part in [tenant_id, outbox_id, origin, payload_hash] {
            hasher.update((part.len() as u64).to_le_bytes());
            hasher.update(part.as_bytes());
            hasher.update([0x1f]);
        }
    }
    let count = i64::try_from(rows.len()).map_err(|_| {
        TurnStoreError::SchemaIntegrity(
            "outbox order guard prefix exceeds the SQLite integer range".to_string(),
        )
    })?;
    let maximum = rows.last().map_or(0, |row| row.0);
    Ok((
        count,
        maximum,
        format!("sha256:{}", hex::encode(hasher.finalize())),
    ))
}

fn validate_order_guard_marker(tx: &Transaction<'_>) -> Result<(), TurnStoreError> {
    let marker = load_order_guard_marker(tx)?.ok_or_else(|| {
        TurnStoreError::SchemaIntegrity(
            "outbox order guard migration marker is missing".to_string(),
        )
    })?;
    let (count, maximum, digest) = order_guard_prefix_evidence(tx, Some(marker.source_max_rowid))?;
    if count != marker.source_row_count
        || maximum != marker.source_max_rowid
        || digest != marker.source_digest
    {
        return Err(TurnStoreError::SchemaIntegrity(
            "outbox order guard migration evidence does not match its prefix".to_string(),
        ));
    }
    Ok(())
}

fn load_outbox_order_manifest(
    tx: &Transaction<'_>,
) -> Result<Option<OutboxOrderManifest>, TurnStoreError> {
    let manifest = tx
        .query_row(
            "SELECT migration_kind, source_row_count, source_max_rowid, source_digest
             FROM turn_store_schema_migrations_v2 WHERE migration_id = ?1",
            params![OUTBOX_ORDER_MIGRATION_ID],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?;
    let Some((migration_kind, source_row_count, source_max_rowid, source_digest)) = manifest else {
        return Ok(None);
    };
    if migration_kind != OUTBOX_ORDER_MIGRATION_KIND
        || source_row_count < 0
        || source_max_rowid < 0
        || !source_digest.starts_with("sha256:")
    {
        return Err(TurnStoreError::SchemaIntegrity(
            "outbox order migration marker is malformed".to_string(),
        ));
    }
    Ok(Some(OutboxOrderManifest {
        source_row_count,
        source_max_rowid,
        source_digest,
    }))
}

fn legacy_outbox_order_rows(
    tx: &Transaction<'_>,
) -> Result<Vec<LegacyOutboxOrderRow>, TurnStoreError> {
    let mut statement = tx.prepare(
        "SELECT rowid, tenant_id, outbox_id, payload_hash
         FROM turn_outbox_v2 ORDER BY rowid",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(LegacyOutboxOrderRow {
            source_rowid: row.get(0)?,
            tenant_id: row.get(1)?,
            outbox_id: row.get(2)?,
            payload_hash: row.get(3)?,
        })
    })?;
    let rows = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    if rows.iter().any(|row| row.source_rowid <= 0) {
        return Err(TurnStoreError::SchemaIntegrity(
            "legacy outbox contains a non-positive rowid".to_string(),
        ));
    }
    Ok(rows)
}

fn legacy_outbox_order_digest(rows: &[LegacyOutboxOrderRow]) -> String {
    let mut hasher = Sha256::new();
    for row in rows {
        for part in [
            row.source_rowid.to_string(),
            row.tenant_id.clone(),
            row.outbox_id.clone(),
            row.payload_hash.clone(),
        ] {
            hasher.update((part.len() as u64).to_le_bytes());
            hasher.update(part.as_bytes());
            hasher.update([0x1f]);
        }
    }
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

fn validate_outbox_order_mapping(
    tx: &Transaction<'_>,
    manifest: &OutboxOrderManifest,
) -> Result<(), TurnStoreError> {
    let missing_or_orphaned: i64 = tx.query_row(
        "SELECT
            (SELECT COUNT(*) FROM turn_outbox_v2 o
             LEFT JOIN turn_outbox_commit_order_v2 c
               ON c.tenant_id = o.tenant_id AND c.outbox_id = o.outbox_id
             WHERE c.outbox_id IS NULL)
          + (SELECT COUNT(*) FROM turn_outbox_commit_order_v2 c
             LEFT JOIN turn_outbox_v2 o
               ON o.tenant_id = c.tenant_id AND o.outbox_id = c.outbox_id
             WHERE o.outbox_id IS NULL)",
        [],
        |row| row.get(0),
    )?;
    if missing_or_orphaned != 0 {
        return Err(TurnStoreError::SchemaIntegrity(
            "outbox commit-order mapping is incomplete or orphaned".to_string(),
        ));
    }

    let foreign_key_violation = {
        let mut statement = tx.prepare("PRAGMA foreign_key_check")?;
        let violation = statement.query([])?.next()?.is_some();
        violation
    };
    if foreign_key_violation {
        return Err(TurnStoreError::SchemaIntegrity(
            "turn store contains a foreign-key violation".to_string(),
        ));
    }

    let legacy_rows = {
        let mut statement = tx.prepare(
            "SELECT c.legacy_source_rowid, c.tenant_id, c.outbox_id, o.payload_hash
             FROM turn_outbox_commit_order_v2 c
             JOIN turn_outbox_v2 o
               ON o.tenant_id = c.tenant_id AND o.outbox_id = c.outbox_id
             WHERE c.order_origin = 'legacy_rowid_backfill'
             ORDER BY c.commit_ordinal",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(LegacyOutboxOrderRow {
                source_rowid: row.get(0)?,
                tenant_id: row.get(1)?,
                outbox_id: row.get(2)?,
                payload_hash: row.get(3)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    let legacy_count = i64::try_from(legacy_rows.len()).map_err(|_| {
        TurnStoreError::SchemaIntegrity(
            "legacy outbox mapping count exceeds SQLite integer range".to_string(),
        )
    })?;
    if legacy_count != manifest.source_row_count
        || legacy_rows.last().map_or(0, |row| row.source_rowid) != manifest.source_max_rowid
        || legacy_outbox_order_digest(&legacy_rows) != manifest.source_digest
    {
        return Err(TurnStoreError::SchemaIntegrity(
            "legacy outbox order manifest does not match its mapping".to_string(),
        ));
    }
    let legacy_ordinal_bounds: (Option<i64>, Option<i64>) = tx.query_row(
        "SELECT MIN(commit_ordinal), MAX(commit_ordinal)
         FROM turn_outbox_commit_order_v2
         WHERE order_origin = 'legacy_rowid_backfill'",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if legacy_count > 0 && legacy_ordinal_bounds != (Some(1), Some(legacy_count)) {
        return Err(TurnStoreError::SchemaIntegrity(
            "legacy outbox order is not the initial contiguous prefix".to_string(),
        ));
    }

    let (min_ordinal, max_ordinal): (Option<i64>, Option<i64>) = tx.query_row(
        "SELECT MIN(commit_ordinal), MAX(commit_ordinal)
         FROM turn_outbox_commit_order_v2",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let sequence: Option<i64> = tx
        .query_row(
            "SELECT seq FROM sqlite_sequence WHERE name = ?1",
            params![OUTBOX_ORDER_TABLE],
            |row| row.get(0),
        )
        .optional()?;
    if sequence != max_ordinal {
        return Err(TurnStoreError::SchemaIntegrity(
            "outbox commit-order sequence does not match the persisted maximum".to_string(),
        ));
    }
    if min_ordinal.is_some_and(|ordinal| ordinal <= 0) {
        return Err(TurnStoreError::SchemaIntegrity(
            "outbox commit ordinals must be strictly positive".to_string(),
        ));
    }

    let revision_order_violation: Option<i64> = tx
        .query_row(
            "SELECT 1 FROM (
                 SELECT o.tenant_id, o.turn_id, o.revision, c.commit_ordinal,
                        LAG(c.commit_ordinal) OVER (
                            PARTITION BY o.tenant_id, o.turn_id ORDER BY o.revision
                        ) AS previous_ordinal
                 FROM turn_outbox_commit_order_v2 c
                 JOIN turn_outbox_v2 o
                   ON o.tenant_id = c.tenant_id AND o.outbox_id = c.outbox_id
             ) ordered
             WHERE previous_ordinal IS NOT NULL
               AND previous_ordinal >= commit_ordinal
             LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if revision_order_violation.is_some() {
        return Err(TurnStoreError::SchemaIntegrity(
            "turn revisions do not follow commit order".to_string(),
        ));
    }
    let first_transactional_ordinal: Option<i64> = tx.query_row(
        "SELECT MIN(commit_ordinal) FROM turn_outbox_commit_order_v2
         WHERE order_origin = 'transactional'",
        [],
        |row| row.get(0),
    )?;
    if let Some(first_transactional_ordinal) = first_transactional_ordinal {
        if first_transactional_ordinal <= legacy_count {
            return Err(TurnStoreError::SchemaIntegrity(
                "transactional outbox order overlaps the legacy prefix".to_string(),
            ));
        }
    }

    let delivered_hole: Option<i64> = tx
        .query_row(
            "SELECT 1 FROM (
                 SELECT o.tenant_id,
                        MIN(CASE WHEN o.status != 'delivered'
                                 THEN c.commit_ordinal END) AS first_undelivered,
                        MAX(CASE WHEN o.status = 'delivered'
                                 THEN c.commit_ordinal END) AS last_delivered
                 FROM turn_outbox_commit_order_v2 c
                 JOIN turn_outbox_v2 o
                   ON o.tenant_id = c.tenant_id AND o.outbox_id = c.outbox_id
                 GROUP BY o.tenant_id
             ) tenant_delivery
             WHERE first_undelivered IS NOT NULL AND last_delivered IS NOT NULL
               AND first_undelivered < last_delivered
             LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if delivered_hole.is_some() {
        return Err(TurnStoreError::SchemaIntegrity(
            "tenant outbox delivery history contains a committed-order hole".to_string(),
        ));
    }

    let incomplete_history: Option<i64> = tx
        .query_row(
            "SELECT 1
             FROM turn_state_v2 s
             LEFT JOIN (
                 SELECT tenant_id, turn_id, COUNT(*) AS row_count,
                        MIN(revision) AS min_revision, MAX(revision) AS max_revision
                 FROM turn_outbox_v2 GROUP BY tenant_id, turn_id
             ) o ON o.tenant_id = s.tenant_id AND o.turn_id = s.turn_id
             WHERE o.row_count IS NULL OR o.min_revision != 0
                OR o.max_revision != s.revision OR o.row_count - 1 != s.revision
             LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if incomplete_history.is_some() {
        return Err(TurnStoreError::SchemaIntegrity(
            "turn outbox revision history is incomplete".to_string(),
        ));
    }
    Ok(())
}

fn ensure_actor_row(
    tx: &Transaction<'_>,
    actor: &TurnActorIdentity,
    now: DateTime<Utc>,
) -> Result<(), TurnStoreError> {
    tx.execute(
        "INSERT INTO turn_actor_v2 (
            tenant_id, actor_key, principal_id, workspace_id, profile_id,
            channel_id, thread_id, revision, active_turn_id, lease_owner,
            lease_until_ms, created_at_ms, updated_at_ms
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, NULL, NULL, NULL, ?8, ?8)
         ON CONFLICT(tenant_id, actor_key) DO NOTHING",
        params![
            actor.tenant_id,
            actor.actor_key,
            actor.principal_id,
            actor.workspace_id,
            actor.profile_id,
            actor.channel_id,
            actor.thread_id,
            timestamp_millis(now),
        ],
    )?;
    let persisted = load_actor(tx, &actor.tenant_id, &actor.actor_key)?
        .ok_or_else(|| TurnStoreError::ActorAuthorityMismatch)?;
    if persisted.principal_id != actor.principal_id
        || persisted.workspace_id != actor.workspace_id
        || persisted.profile_id != actor.profile_id
        || persisted.channel_id != actor.channel_id
        || persisted.thread_id != actor.thread_id
    {
        return Err(TurnStoreError::ActorAuthorityMismatch);
    }
    Ok(())
}

fn recover_actor_for_admission(
    tx: &Transaction<'_>,
    actor: &TurnActorIdentity,
    now: DateTime<Utc>,
) -> Result<(), TurnStoreError> {
    let actor_record = load_actor(tx, &actor.tenant_id, &actor.actor_key)?
        .ok_or_else(|| TurnStoreError::ActorAuthorityMismatch)?;
    let Some(active_turn_id) = actor_record.active_turn_id else {
        return Ok(());
    };
    let lease_until = actor_record
        .lease_until
        .ok_or_else(|| TurnStoreError::ActorLeaseLost {
            lease_owner: "missing".to_string(),
        })?;
    if lease_until > now {
        return Err(TurnStoreError::ActorBusy {
            actor_key: actor.actor_key.clone(),
            active_turn_id,
            lease_until,
        });
    }
    let active = load_turn(tx, &actor.tenant_id, &active_turn_id)?.ok_or_else(|| {
        TurnStoreError::MissingTurn {
            tenant_id: actor.tenant_id.clone(),
            turn_id: active_turn_id,
        }
    })?;
    recover_turn_in_tx(tx, &active, now)?;
    Ok(())
}

fn recover_duplicate_if_expired(
    tx: &Transaction<'_>,
    record: &DurableTurnRecord,
    now: DateTime<Utc>,
) -> Result<DurableTurnRecord, TurnStoreError> {
    if record.state.state().is_terminal() {
        return Ok(record.clone());
    }
    let actor = load_actor(tx, &record.tenant_id, &record.actor_key)?.ok_or_else(|| {
        TurnStoreError::ActorLeaseLost {
            lease_owner: "missing".to_string(),
        }
    })?;
    if actor.active_turn_id.as_deref() != Some(record.turn_id.as_str()) {
        return Err(TurnStoreError::ActorLeaseLost {
            lease_owner: actor.lease_owner.unwrap_or_else(|| "missing".to_string()),
        });
    }
    if actor
        .lease_until
        .is_some_and(|lease_until| lease_until <= now)
    {
        recover_turn_in_tx(tx, record, now)
    } else {
        Ok(record.clone())
    }
}

fn recover_turn_in_tx(
    tx: &Transaction<'_>,
    record: &DurableTurnRecord,
    now: DateTime<Utc>,
) -> Result<DurableTurnRecord, TurnStoreError> {
    if now < record.updated_at {
        return Err(TurnStoreError::NonMonotonicTimestamp);
    }
    if record.state.state().is_terminal() {
        clear_actor(tx, record, now)?;
        return Ok(record.clone());
    }
    let (terminal_state, reason_code, terminal_result) = match record.state.state() {
        TurnState::Accepted | TurnState::Routed | TurnState::WaitingApproval => {
            let reason_code = "expired_actor_lease_before_uncertain_effect";
            let execution = TurnExecution::aborted(
                TurnError {
                    reason_code: reason_code.to_string(),
                    message: "durable turn lease expired before a recoverable continuation existed"
                        .to_string(),
                },
                PartialLedgerTail {
                    appended_event_ids: Vec::new(),
                    last_safe_parent_event_id: None,
                },
            );
            (
                TurnState::Aborted,
                reason_code,
                serde_json::to_value(execution)?,
            )
        }
        TurnState::Running | TurnState::ToolRunning => {
            let reason_code = "expired_actor_lease_with_uncertain_external_effect";
            let execution = TurnExecution::Finished {
                output: None,
                outcome: Box::new(TurnOutcome::Quarantined(QuarantineEvent {
                    level: 3,
                    reason_code: reason_code.to_string(),
                    diagnostic_scope: "durable_turn_recovery".to_string(),
                })),
            };
            (
                TurnState::Quarantined,
                reason_code,
                serde_json::to_value(execution)?,
            )
        }
        TurnState::Completed
        | TurnState::Degraded
        | TurnState::Aborted
        | TurnState::Quarantined => unreachable!("terminal state handled above"),
    };
    let next = record.state.compare_and_transition(
        record.state.state(),
        record.state.revision(),
        terminal_state,
    )?;
    let terminal_json = canonical_json(&terminal_result)?;
    let terminal_hash = sha256_text(&terminal_json);
    let changed = tx.execute(
        "UPDATE turn_state_v2
         SET state = ?3, revision = ?4, terminal_result_json = ?5,
             terminal_result_hash = ?6, updated_at_ms = ?7
         WHERE tenant_id = ?1 AND turn_id = ?2 AND state = ?8 AND revision = ?9",
        params![
            record.tenant_id,
            record.turn_id,
            state_name(terminal_state),
            revision_to_i64(next.revision())?,
            terminal_json,
            terminal_hash,
            timestamp_millis(now),
            state_name(record.state.state()),
            revision_to_i64(record.state.revision())?,
        ],
    )?;
    if changed != 1 {
        return Err(TurnStoreError::CasLost);
    }
    clear_actor(tx, record, now)?;
    let recovered = load_turn(tx, &record.tenant_id, &record.turn_id)?.ok_or_else(|| {
        TurnStoreError::MissingTurn {
            tenant_id: record.tenant_id.clone(),
            turn_id: record.turn_id.clone(),
        }
    })?;
    insert_outbox(
        tx,
        &recovered,
        Some(record.state.state()),
        Some(&terminal_hash),
        now,
    )?;
    debug_assert_eq!(
        recovered.terminal_result_hash.as_deref(),
        Some(terminal_hash.as_str()),
        "recovery result hash must remain bound to the outbox payload"
    );
    debug_assert!(!reason_code.is_empty());
    Ok(recovered)
}

fn verify_actor_lease(
    tx: &Transaction<'_>,
    record: &DurableTurnRecord,
    lease_owner: &str,
    now: DateTime<Utc>,
) -> Result<(), TurnStoreError> {
    let actor = load_actor(tx, &record.tenant_id, &record.actor_key)?.ok_or_else(|| {
        TurnStoreError::ActorLeaseLost {
            lease_owner: lease_owner.to_string(),
        }
    })?;
    if actor.active_turn_id.as_deref() != Some(record.turn_id.as_str())
        || actor.lease_owner.as_deref() != Some(lease_owner)
        || actor.lease_until.is_none_or(|deadline| deadline <= now)
    {
        return Err(TurnStoreError::ActorLeaseLost {
            lease_owner: lease_owner.to_string(),
        });
    }
    Ok(())
}

fn update_actor_after_transition(
    tx: &Transaction<'_>,
    record: &DurableTurnRecord,
    lease_owner: &str,
    next: TurnState,
    now: DateTime<Utc>,
) -> Result<(), TurnStoreError> {
    let (active_turn_id, owner, lease_until) = if next.is_terminal() {
        (None, None, None)
    } else {
        (
            Some(record.turn_id.as_str()),
            Some(lease_owner),
            Some(timestamp_millis(record.deadline)),
        )
    };
    let changed = tx.execute(
        "UPDATE turn_actor_v2
         SET active_turn_id = ?5, lease_owner = ?6, lease_until_ms = ?7,
             revision = revision + 1, updated_at_ms = ?8
         WHERE tenant_id = ?1 AND actor_key = ?2 AND active_turn_id = ?3
           AND lease_owner = ?4",
        params![
            record.tenant_id,
            record.actor_key,
            record.turn_id,
            lease_owner,
            active_turn_id,
            owner,
            lease_until,
            timestamp_millis(now),
        ],
    )?;
    if changed == 1 {
        Ok(())
    } else {
        Err(TurnStoreError::ActorLeaseLost {
            lease_owner: lease_owner.to_string(),
        })
    }
}

fn clear_actor(
    tx: &Transaction<'_>,
    record: &DurableTurnRecord,
    now: DateTime<Utc>,
) -> Result<(), TurnStoreError> {
    let changed = tx.execute(
        "UPDATE turn_actor_v2
         SET active_turn_id = NULL, lease_owner = NULL, lease_until_ms = NULL,
             revision = revision + 1, updated_at_ms = ?4
         WHERE tenant_id = ?1 AND actor_key = ?2 AND active_turn_id = ?3",
        params![
            record.tenant_id,
            record.actor_key,
            record.turn_id,
            timestamp_millis(now),
        ],
    )?;
    if changed == 1 {
        Ok(())
    } else {
        Err(TurnStoreError::CasLost)
    }
}

fn tenant_outbox_head(
    conn: &Connection,
    tenant_id: &str,
) -> Result<Option<String>, TurnStoreError> {
    let missing_mapping: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM turn_outbox_v2 o
             LEFT JOIN turn_outbox_commit_order_v2 c
               ON c.tenant_id = o.tenant_id AND c.outbox_id = o.outbox_id
             WHERE o.tenant_id = ?1 AND c.outbox_id IS NULL LIMIT 1",
            params![tenant_id],
            |row| row.get(0),
        )
        .optional()?;
    if missing_mapping.is_some() {
        return Err(TurnStoreError::SchemaIntegrity(
            "tenant outbox contains a record without commit order".to_string(),
        ));
    }
    let orphan_mapping: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM turn_outbox_commit_order_v2 c
             LEFT JOIN turn_outbox_v2 o
               ON o.tenant_id = c.tenant_id AND o.outbox_id = c.outbox_id
             WHERE c.tenant_id = ?1 AND o.outbox_id IS NULL LIMIT 1",
            params![tenant_id],
            |row| row.get(0),
        )
        .optional()?;
    if orphan_mapping.is_some() {
        return Err(TurnStoreError::SchemaIntegrity(
            "tenant commit order contains a record without an outbox row".to_string(),
        ));
    }
    let (first_undelivered, last_delivered): (Option<i64>, Option<i64>) = conn.query_row(
        "SELECT
                 MIN(CASE WHEN o.status != 'delivered' THEN c.commit_ordinal END),
                 MAX(CASE WHEN o.status = 'delivered' THEN c.commit_ordinal END)
             FROM turn_outbox_commit_order_v2 c
             JOIN turn_outbox_v2 o
               ON o.tenant_id = c.tenant_id AND o.outbox_id = c.outbox_id
             WHERE o.tenant_id = ?1",
        params![tenant_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if matches!((first_undelivered, last_delivered), (Some(first), Some(last)) if first < last) {
        return Err(TurnStoreError::SchemaIntegrity(
            "tenant outbox delivery history contains a committed-order hole".to_string(),
        ));
    }
    let Some(first_undelivered) = first_undelivered else {
        return Ok(None);
    };
    conn.query_row(
        "SELECT o.outbox_id
         FROM turn_outbox_commit_order_v2 c
         JOIN turn_outbox_v2 o
           ON o.tenant_id = c.tenant_id AND o.outbox_id = c.outbox_id
         WHERE o.tenant_id = ?1 AND c.commit_ordinal = ?2
           AND o.status != 'delivered'",
        params![tenant_id, first_undelivered],
        |row| row.get(0),
    )
    .optional()
    .map_err(Into::into)
}

fn next_commit_ordinal(tx: &Transaction<'_>) -> Result<i64, TurnStoreError> {
    let sequence: Option<i64> = tx
        .query_row(
            "SELECT seq FROM sqlite_sequence WHERE name = ?1",
            params![OUTBOX_ORDER_TABLE],
            |row| row.get(0),
        )
        .optional()?;
    let maximum: Option<i64> = tx.query_row(
        "SELECT MAX(commit_ordinal) FROM turn_outbox_commit_order_v2",
        [],
        |row| row.get(0),
    )?;
    if sequence != maximum {
        return Err(TurnStoreError::SchemaIntegrity(
            "outbox commit-order sequence does not match its maximum".to_string(),
        ));
    }
    maximum
        .unwrap_or(0)
        .checked_add(1)
        .ok_or(TurnStoreError::CommitOrdinalExhausted)
}

fn insert_outbox(
    tx: &Transaction<'_>,
    record: &DurableTurnRecord,
    previous_state: Option<TurnState>,
    terminal_result_hash: Option<&str>,
    now: DateTime<Utc>,
) -> Result<(), TurnStoreError> {
    validate_no_extra_outbox_triggers(tx)?;
    let expected_commit_ordinal = next_commit_ordinal(tx)?;
    let event_type = state_event_type(record.state.state());
    let outbox_id = deterministic_outbox_id(
        &record.tenant_id,
        &record.turn_id,
        record.state.revision(),
        &event_type,
    );
    let now_ms = timestamp_millis(now);
    let payload = serde_json::json!({
        "schema": TURN_OUTBOX_SCHEMA,
        "outbox_id": outbox_id,
        "tenant_id": record.tenant_id,
        "turn_id": record.turn_id,
        "actor_key": record.actor_key,
        "subject_id": record.subject_id,
        "principal_id": record.principal_id,
        "workspace_id": record.workspace_id,
        "profile_id": record.profile_id,
        "session_id": record.session_id,
        "source": {
            "surface": record.source_surface,
            "source_id": record.source_id,
        },
        "idempotency_key": record.idempotency_key,
        "request_hash": record.request_hash,
        "authority_hash": record.authority_hash,
        "previous_state": previous_state.map(state_name),
        "state": state_name(record.state.state()),
        "revision": record.state.revision(),
        "terminal": record.state.state().is_terminal(),
        "terminal_result_hash": terminal_result_hash,
        "occurred_at": parse_timestamp("outbox_created_at_ms", now_ms)?
            .to_rfc3339_opts(SecondsFormat::Millis, true),
    });
    let payload_json = canonical_json(&payload)?;
    let payload_hash = sha256_text(&payload_json);
    tx.execute(
        "INSERT INTO turn_outbox_v2 (
            tenant_id, outbox_id, turn_id, revision, event_type, effect_kind,
            idempotency_mode, payload_json, payload_hash, status, attempts,
            available_at_ms, lease_owner, lease_token, lease_until_ms,
            delivered_at_ms, ledger_event_id, last_error, created_at_ms,
            updated_at_ms
         ) VALUES (?1, ?2, ?3, ?4, ?5, 'ledger_turn_state', 'key_required',
                   ?6, ?7, 'pending', 0, ?8, NULL, NULL, NULL, NULL, NULL,
                   NULL, ?8, ?8)",
        params![
            record.tenant_id,
            outbox_id,
            record.turn_id,
            revision_to_i64(record.state.revision())?,
            event_type,
            payload_json,
            payload_hash,
            now_ms,
        ],
    )?;
    let assigned_order: Option<(i64, String, Option<i64>)> = tx
        .query_row(
            "SELECT commit_ordinal, order_origin, legacy_source_rowid
             FROM turn_outbox_commit_order_v2
             WHERE tenant_id = ?1 AND outbox_id = ?2",
            params![record.tenant_id, outbox_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    if assigned_order != Some((expected_commit_ordinal, "transactional".to_string(), None)) {
        return Err(TurnStoreError::SchemaIntegrity(
            "outbox trigger did not assign the expected transactional order".to_string(),
        ));
    }
    Ok(())
}

fn validate_actor_authority(
    ingress: &AuthenticatedIngress,
    actor: &TurnActorIdentity,
) -> Result<(), TurnStoreError> {
    if ingress.tenant_id().as_str() == actor.tenant_id
        && ingress.principal_id().as_str() == actor.principal_id
        && ingress.workspace_id().0 == actor.workspace_id
        && ingress.profile_id().as_str() == actor.profile_id
    {
        Ok(())
    } else {
        Err(TurnStoreError::ActorAuthorityMismatch)
    }
}

fn validate_actor_component(field: &'static str, value: &str) -> Result<(), TurnStoreError> {
    if !value.trim().is_empty() && value.len() <= MAX_ACTOR_COMPONENT_BYTES {
        Ok(())
    } else {
        Err(TurnStoreError::InvalidActorComponent { field })
    }
}

fn validate_lease_identity(field: &'static str, value: &str) -> Result<(), TurnStoreError> {
    let valid = !value.is_empty()
        && value.len() <= MAX_LEASE_OWNER_BYTES
        && value.trim() == value
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:-@".contains(&byte));
    if valid {
        Ok(())
    } else {
        Err(TurnStoreError::InvalidLeaseIdentity { field })
    }
}

fn deterministic_actor_key(parts: &[&str]) -> String {
    format!("actor-{}", &hash_parts(parts)[..40])
}

fn deterministic_turn_id(tenant_id: &str, idempotency_key: &str, request_hash: &str) -> String {
    format!(
        "turn-{}",
        &hash_parts(&[tenant_id, idempotency_key, request_hash])[..40]
    )
}

fn deterministic_outbox_id(
    tenant_id: &str,
    turn_id: &str,
    revision: u64,
    event_type: &str,
) -> String {
    let revision = revision.to_string();
    format!(
        "outbox-{}",
        &hash_parts(&[tenant_id, turn_id, &revision, event_type])[..40]
    )
}

fn hash_parts(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update((part.len() as u64).to_le_bytes());
        hasher.update(part.as_bytes());
        hasher.update([0x1f]);
    }
    hex::encode(hasher.finalize())
}

fn canonical_json(value: &Value) -> Result<String, TurnStoreError> {
    serde_json::to_string(value).map_err(Into::into)
}

fn sha256_text(value: &str) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(value.as_bytes())))
}

const TURN_SELECT: &str = "SELECT tenant_id, turn_id, actor_key, subject_id, principal_id,
            workspace_id, profile_id, session_id, source_surface, source_id,
            idempotency_key, request_json, request_hash, authority_json,
            authority_hash, deadline_ms, state, revision, terminal_result_json,
            terminal_result_hash, created_at_ms, updated_at_ms
     FROM turn_state_v2";

const ACTOR_SELECT: &str = "SELECT tenant_id, actor_key, principal_id, workspace_id, profile_id,
            channel_id, thread_id, revision, active_turn_id, lease_owner,
            lease_until_ms, created_at_ms, updated_at_ms
     FROM turn_actor_v2";

const OUTBOX_SELECT: &str = "SELECT o.tenant_id, o.outbox_id, c.commit_ordinal, c.order_origin,
            o.turn_id, o.revision, o.event_type, o.effect_kind,
            o.idempotency_mode, o.payload_json, o.payload_hash, o.status, o.attempts,
            o.available_at_ms, o.lease_owner, o.lease_token, o.lease_until_ms,
            o.delivered_at_ms, o.ledger_event_id, v.ledger_event_id,
            v.signer_public_key, v.database_instance_id, o.last_error,
            o.created_at_ms, o.updated_at_ms
     FROM turn_outbox_v2 o
     JOIN turn_outbox_commit_order_v2 c
       ON c.tenant_id = o.tenant_id AND c.outbox_id = o.outbox_id
     LEFT JOIN turn_outbox_verified_commit_v2 v
       ON v.tenant_id = o.tenant_id AND v.outbox_id = o.outbox_id";

fn load_by_idempotency(
    conn: &Connection,
    tenant_id: &str,
    idempotency_key: &str,
) -> Result<Option<DurableTurnRecord>, TurnStoreError> {
    conn.query_row(
        &format!("{TURN_SELECT} WHERE tenant_id = ?1 AND idempotency_key = ?2"),
        params![tenant_id, idempotency_key],
        raw_turn_from_row,
    )
    .optional()?
    .map(materialize_turn)
    .transpose()
}

fn load_turn(
    conn: &Connection,
    tenant_id: &str,
    turn_id: &str,
) -> Result<Option<DurableTurnRecord>, TurnStoreError> {
    conn.query_row(
        &format!("{TURN_SELECT} WHERE tenant_id = ?1 AND turn_id = ?2"),
        params![tenant_id, turn_id],
        raw_turn_from_row,
    )
    .optional()?
    .map(materialize_turn)
    .transpose()
}

fn load_actor(
    conn: &Connection,
    tenant_id: &str,
    actor_key: &str,
) -> Result<Option<TurnActorRecord>, TurnStoreError> {
    conn.query_row(
        &format!("{ACTOR_SELECT} WHERE tenant_id = ?1 AND actor_key = ?2"),
        params![tenant_id, actor_key],
        row_to_actor,
    )
    .optional()
    .map_err(Into::into)
}

fn load_outbox(
    conn: &Connection,
    tenant_id: &str,
    outbox_id: &str,
) -> Result<Option<TurnOutboxRecord>, TurnStoreError> {
    conn.query_row(
        &format!("{OUTBOX_SELECT} WHERE o.tenant_id = ?1 AND o.outbox_id = ?2"),
        params![tenant_id, outbox_id],
        raw_outbox_from_row,
    )
    .optional()?
    .map(materialize_outbox)
    .transpose()
}

struct RawTurnRow {
    tenant_id: String,
    turn_id: String,
    actor_key: String,
    subject_id: String,
    principal_id: String,
    workspace_id: String,
    profile_id: String,
    session_id: String,
    source_surface: String,
    source_id: String,
    idempotency_key: String,
    request_json: String,
    request_hash: String,
    authority_json: String,
    authority_hash: String,
    deadline_ms: i64,
    state: String,
    revision: i64,
    terminal_json: Option<String>,
    terminal_hash: Option<String>,
    created_at_ms: i64,
    updated_at_ms: i64,
}

fn raw_turn_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawTurnRow> {
    Ok(RawTurnRow {
        tenant_id: row.get(0)?,
        turn_id: row.get(1)?,
        actor_key: row.get(2)?,
        subject_id: row.get(3)?,
        principal_id: row.get(4)?,
        workspace_id: row.get(5)?,
        profile_id: row.get(6)?,
        session_id: row.get(7)?,
        source_surface: row.get(8)?,
        source_id: row.get(9)?,
        idempotency_key: row.get(10)?,
        request_json: row.get(11)?,
        request_hash: row.get(12)?,
        authority_json: row.get(13)?,
        authority_hash: row.get(14)?,
        deadline_ms: row.get(15)?,
        state: row.get(16)?,
        revision: row.get(17)?,
        terminal_json: row.get(18)?,
        terminal_hash: row.get(19)?,
        created_at_ms: row.get(20)?,
        updated_at_ms: row.get(21)?,
    })
}

fn materialize_turn(raw: RawTurnRow) -> Result<DurableTurnRecord, TurnStoreError> {
    verify_stored_hash("request_hash", &raw.request_json, &raw.request_hash)?;
    verify_stored_hash("authority_hash", &raw.authority_json, &raw.authority_hash)?;
    match (&raw.terminal_json, &raw.terminal_hash) {
        (Some(json), Some(hash)) => {
            verify_stored_hash("terminal_result_hash", json, hash)?;
        }
        (None, None) => {}
        _ => {
            return Err(TurnStoreError::HashMismatch {
                field: "terminal_result_hash",
            })
        }
    }
    let request = serde_json::from_str(&raw.request_json)?;
    let authority = serde_json::from_str(&raw.authority_json)?;
    verify_turn_bindings(&raw, &authority)?;
    let state = parse_state(&raw.state)?;
    let revision = checked_u64(raw.revision)?;
    let terminal_result = raw
        .terminal_json
        .as_ref()
        .map(|json| serde_json::from_str(json))
        .transpose()
        .map_err(TurnStoreError::from)?;
    validate_persisted_terminal(state, terminal_result.as_ref())?;
    Ok(DurableTurnRecord {
        tenant_id: raw.tenant_id,
        turn_id: raw.turn_id,
        actor_key: raw.actor_key,
        subject_id: raw.subject_id,
        principal_id: raw.principal_id,
        workspace_id: raw.workspace_id,
        profile_id: raw.profile_id,
        session_id: raw.session_id,
        source_surface: raw.source_surface,
        source_id: raw.source_id,
        idempotency_key: raw.idempotency_key,
        request,
        request_hash: raw.request_hash,
        authority,
        authority_hash: raw.authority_hash,
        deadline: parse_timestamp("deadline_ms", raw.deadline_ms)?,
        state: VersionedTurnState::restore(state, revision),
        terminal_result,
        terminal_result_hash: raw.terminal_hash,
        created_at: parse_timestamp("created_at_ms", raw.created_at_ms)?,
        updated_at: parse_timestamp("updated_at_ms", raw.updated_at_ms)?,
    })
}

fn row_to_actor(row: &rusqlite::Row<'_>) -> rusqlite::Result<TurnActorRecord> {
    let lease_until: Option<i64> = row.get(10)?;
    Ok(TurnActorRecord {
        tenant_id: row.get(0)?,
        actor_key: row.get(1)?,
        principal_id: row.get(2)?,
        workspace_id: row.get(3)?,
        profile_id: row.get(4)?,
        channel_id: row.get(5)?,
        thread_id: row.get(6)?,
        revision: checked_u64(row.get(7)?).map_err(to_sql_conversion_error)?,
        active_turn_id: row.get(8)?,
        lease_owner: row.get(9)?,
        lease_until: lease_until
            .map(|value| parse_timestamp("lease_until_ms", value))
            .transpose()
            .map_err(to_sql_conversion_error)?,
        created_at: parse_timestamp("created_at_ms", row.get(11)?)
            .map_err(to_sql_conversion_error)?,
        updated_at: parse_timestamp("updated_at_ms", row.get(12)?)
            .map_err(to_sql_conversion_error)?,
    })
}

struct RawOutboxRow {
    tenant_id: String,
    outbox_id: String,
    commit_ordinal: i64,
    order_origin: String,
    turn_id: String,
    revision: i64,
    event_type: String,
    effect_kind: String,
    idempotency_mode: String,
    payload_json: String,
    payload_hash: String,
    status: String,
    attempts: i64,
    available_at_ms: i64,
    lease_owner: Option<String>,
    lease_token: Option<String>,
    lease_until_ms: Option<i64>,
    delivered_at_ms: Option<i64>,
    ledger_event_id: Option<String>,
    verified_ledger_event_id: Option<String>,
    verified_signer_public_key: Option<Vec<u8>>,
    verified_database_instance_id: Option<String>,
    last_error: Option<String>,
    created_at_ms: i64,
    updated_at_ms: i64,
}

fn raw_outbox_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawOutboxRow> {
    Ok(RawOutboxRow {
        tenant_id: row.get(0)?,
        outbox_id: row.get(1)?,
        commit_ordinal: row.get(2)?,
        order_origin: row.get(3)?,
        turn_id: row.get(4)?,
        revision: row.get(5)?,
        event_type: row.get(6)?,
        effect_kind: row.get(7)?,
        idempotency_mode: row.get(8)?,
        payload_json: row.get(9)?,
        payload_hash: row.get(10)?,
        status: row.get(11)?,
        attempts: row.get(12)?,
        available_at_ms: row.get(13)?,
        lease_owner: row.get(14)?,
        lease_token: row.get(15)?,
        lease_until_ms: row.get(16)?,
        delivered_at_ms: row.get(17)?,
        ledger_event_id: row.get(18)?,
        verified_ledger_event_id: row.get(19)?,
        verified_signer_public_key: row.get(20)?,
        verified_database_instance_id: row.get(21)?,
        last_error: row.get(22)?,
        created_at_ms: row.get(23)?,
        updated_at_ms: row.get(24)?,
    })
}

fn materialize_outbox(raw: RawOutboxRow) -> Result<TurnOutboxRecord, TurnStoreError> {
    verify_stored_hash("outbox_payload_hash", &raw.payload_json, &raw.payload_hash)?;
    let payload: Value = serde_json::from_str(&raw.payload_json)?;
    let revision = checked_u64(raw.revision)?;
    verify_outbox_bindings(&raw, &payload, revision)?;
    let commit_ordinal = checked_u64(raw.commit_ordinal)?;
    if commit_ordinal == 0 {
        return Err(TurnStoreError::SchemaIntegrity(
            "outbox commit ordinal must be positive".to_string(),
        ));
    }
    let status = parse_outbox_status(&raw.status)?;
    validate_outbox_status_bindings(&raw, status)?;
    Ok(TurnOutboxRecord {
        tenant_id: raw.tenant_id,
        outbox_id: raw.outbox_id,
        commit_ordinal,
        order_origin: parse_order_origin(&raw.order_origin)?.to_string(),
        turn_id: raw.turn_id,
        revision,
        event_type: raw.event_type,
        effect_kind: raw.effect_kind,
        idempotency_mode: raw.idempotency_mode,
        payload,
        payload_hash: raw.payload_hash,
        status,
        attempts: checked_u64(raw.attempts)?,
        available_at: parse_timestamp("available_at_ms", raw.available_at_ms)?,
        lease_owner: raw.lease_owner,
        lease_token: raw.lease_token,
        lease_until: raw
            .lease_until_ms
            .map(|value| parse_timestamp("lease_until_ms", value))
            .transpose()?,
        delivered_at: raw
            .delivered_at_ms
            .map(|value| parse_timestamp("delivered_at_ms", value))
            .transpose()?,
        ledger_event_id: raw.ledger_event_id,
        verified_ledger_event_id: raw.verified_ledger_event_id,
        verified_signer_public_key: raw.verified_signer_public_key,
        verified_database_instance_id: raw.verified_database_instance_id,
        last_error: raw.last_error,
        created_at: parse_timestamp("created_at_ms", raw.created_at_ms)?,
        updated_at: parse_timestamp("updated_at_ms", raw.updated_at_ms)?,
    })
}

fn validate_outbox_status_bindings(
    raw: &RawOutboxRow,
    status: TurnOutboxStatus,
) -> Result<(), TurnStoreError> {
    let shape_matches = match status {
        TurnOutboxStatus::Pending => {
            raw.lease_owner.is_none()
                && raw.lease_token.is_none()
                && raw.lease_until_ms.is_none()
                && raw.delivered_at_ms.is_none()
                && raw.ledger_event_id.is_none()
        }
        TurnOutboxStatus::Leased => {
            raw.lease_owner.is_some()
                && raw.lease_token.is_some()
                && raw.lease_until_ms.is_some()
                && raw.delivered_at_ms.is_none()
                && raw.ledger_event_id.is_none()
        }
        TurnOutboxStatus::Delivered => {
            raw.lease_owner.is_none()
                && raw.lease_token.is_none()
                && raw.lease_until_ms.is_none()
                && raw.delivered_at_ms.is_some()
                && raw.ledger_event_id.is_some()
        }
    };
    if !shape_matches {
        return Err(TurnStoreError::RecordBindingMismatch {
            field: "outbox.status_fields",
        });
    }
    let evidence_shape_matches = match (
        raw.verified_ledger_event_id.as_deref(),
        raw.verified_signer_public_key.as_deref(),
        raw.verified_database_instance_id.as_deref(),
    ) {
        (None, None, None) => true,
        (Some(event_id), Some(public_key), Some(instance_id)) => {
            status == TurnOutboxStatus::Delivered
                && public_key.len() == 32
                && !instance_id.is_empty()
                && raw.ledger_event_id.as_deref() == Some(event_id)
        }
        _ => false,
    };
    if !evidence_shape_matches
        || (status != TurnOutboxStatus::Delivered && raw.verified_signer_public_key.is_some())
    {
        return Err(TurnStoreError::RecordBindingMismatch {
            field: "outbox.verified_commit",
        });
    }
    if raw.available_at_ms < raw.created_at_ms
        || raw.updated_at_ms < raw.created_at_ms
        || raw
            .lease_until_ms
            .is_some_and(|timestamp| timestamp < raw.created_at_ms)
        || raw
            .delivered_at_ms
            .is_some_and(|timestamp| timestamp < raw.created_at_ms)
    {
        return Err(TurnStoreError::RecordBindingMismatch {
            field: "outbox.timestamps",
        });
    }
    Ok(())
}

fn verify_turn_bindings(raw: &RawTurnRow, authority: &Value) -> Result<(), TurnStoreError> {
    for (field, pointer, actual) in [
        ("tenant_id", "/tenant_id", raw.tenant_id.as_str()),
        ("subject_id", "/subject_id", raw.subject_id.as_str()),
        ("principal_id", "/principal_id", raw.principal_id.as_str()),
        ("workspace_id", "/workspace_id", raw.workspace_id.as_str()),
        ("profile_id", "/profile_id", raw.profile_id.as_str()),
        ("session_id", "/session_id", raw.session_id.as_str()),
        (
            "source_surface",
            "/source/surface",
            raw.source_surface.as_str(),
        ),
        ("source_id", "/source/source_id", raw.source_id.as_str()),
        (
            "idempotency_key",
            "/idempotency_key",
            raw.idempotency_key.as_str(),
        ),
    ] {
        verify_json_text_binding(authority, pointer, actual, field)?;
    }
    let authority_deadline = authority
        .pointer("/deadline")
        .and_then(Value::as_str)
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc))
        .ok_or(TurnStoreError::RecordBindingMismatch {
            field: "deadline_ms",
        })?;
    if timestamp_millis(authority_deadline) != raw.deadline_ms {
        return Err(TurnStoreError::RecordBindingMismatch {
            field: "deadline_ms",
        });
    }
    if deterministic_turn_id(&raw.tenant_id, &raw.idempotency_key, &raw.request_hash) != raw.turn_id
    {
        return Err(TurnStoreError::RecordBindingMismatch { field: "turn_id" });
    }
    Ok(())
}

fn verify_outbox_bindings(
    raw: &RawOutboxRow,
    payload: &Value,
    revision: u64,
) -> Result<(), TurnStoreError> {
    verify_json_text_binding(payload, "/schema", TURN_OUTBOX_SCHEMA, "outbox.schema")?;
    verify_json_text_binding(payload, "/outbox_id", &raw.outbox_id, "outbox.outbox_id")?;
    verify_json_text_binding(payload, "/tenant_id", &raw.tenant_id, "outbox.tenant_id")?;
    verify_json_text_binding(payload, "/turn_id", &raw.turn_id, "outbox.turn_id")?;
    if payload.pointer("/revision").and_then(Value::as_u64) != Some(revision) {
        return Err(TurnStoreError::RecordBindingMismatch {
            field: "outbox.revision",
        });
    }
    let state = payload.pointer("/state").and_then(Value::as_str).ok_or(
        TurnStoreError::RecordBindingMismatch {
            field: "outbox.event_type",
        },
    )?;
    if !outbox_event_type_matches(&raw.event_type, state) {
        return Err(TurnStoreError::RecordBindingMismatch {
            field: "outbox.event_type",
        });
    }
    if raw.effect_kind != "ledger_turn_state" {
        return Err(TurnStoreError::RecordBindingMismatch {
            field: "outbox.effect_kind",
        });
    }
    if raw.idempotency_mode != "key_required" {
        return Err(TurnStoreError::RecordBindingMismatch {
            field: "outbox.idempotency_mode",
        });
    }
    if deterministic_outbox_id(&raw.tenant_id, &raw.turn_id, revision, &raw.event_type)
        != raw.outbox_id
    {
        return Err(TurnStoreError::RecordBindingMismatch {
            field: "outbox.outbox_id",
        });
    }
    Ok(())
}

fn verify_json_text_binding(
    value: &Value,
    pointer: &str,
    actual: &str,
    field: &'static str,
) -> Result<(), TurnStoreError> {
    if value.pointer(pointer).and_then(Value::as_str) == Some(actual) {
        Ok(())
    } else {
        Err(TurnStoreError::RecordBindingMismatch { field })
    }
}

fn timestamp_millis(value: DateTime<Utc>) -> i64 {
    value.timestamp_millis()
}

fn parse_timestamp(field: &'static str, value: i64) -> Result<DateTime<Utc>, TurnStoreError> {
    Utc.timestamp_millis_opt(value)
        .single()
        .ok_or(TurnStoreError::CorruptTimestamp { field, value })
}

fn checked_u64(value: i64) -> Result<u64, TurnStoreError> {
    u64::try_from(value).map_err(|_| TurnStoreError::CorruptRevision(value))
}

fn verify_stored_hash(
    field: &'static str,
    canonical_json: &str,
    stored_hash: &str,
) -> Result<(), TurnStoreError> {
    if sha256_text(canonical_json) == stored_hash {
        Ok(())
    } else {
        Err(TurnStoreError::HashMismatch { field })
    }
}

fn validate_persisted_terminal(
    state: TurnState,
    terminal_result: Option<&Value>,
) -> Result<(), TurnStoreError> {
    match (state.is_terminal(), terminal_result) {
        (false, None) => Ok(()),
        (false, Some(_)) => Err(TurnStoreError::NonTerminalResult),
        (true, None) => Err(TurnStoreError::MissingTerminalResult),
        (true, Some(result)) => {
            let execution: TurnExecution = serde_json::from_value(result.clone())?;
            let actual = execution.terminal_state();
            if actual == state {
                Ok(())
            } else {
                Err(TurnStoreError::TerminalOutcomeMismatch {
                    expected: state,
                    actual,
                })
            }
        }
    }
}

fn to_sql_conversion_error(error: TurnStoreError) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}

fn revision_to_i64(revision: u64) -> Result<i64, TurnStoreError> {
    i64::try_from(revision)
        .map_err(|_| TurnStoreError::Transition(TurnTransitionError::RevisionExhausted))
}

fn bounded_limit(limit: usize) -> i64 {
    i64::try_from(limit.clamp(1, 1_000)).unwrap_or(1_000)
}

fn state_name(state: TurnState) -> &'static str {
    match state {
        TurnState::Accepted => "accepted",
        TurnState::Routed => "routed",
        TurnState::Running => "running",
        TurnState::WaitingApproval => "waiting_approval",
        TurnState::ToolRunning => "tool_running",
        TurnState::Completed => "completed",
        TurnState::Degraded => "degraded",
        TurnState::Aborted => "aborted",
        TurnState::Quarantined => "quarantined",
    }
}

fn state_event_type(state: TurnState) -> String {
    format!("turn.state.{}", state_name(state))
}

fn outbox_event_type_matches(event_type: &str, state: &str) -> bool {
    event_type == format!("turn.state.{state}") || event_type == format!("turn.{state}")
}

fn parse_state(value: &str) -> Result<TurnState, TurnStoreError> {
    match value {
        "accepted" => Ok(TurnState::Accepted),
        "routed" => Ok(TurnState::Routed),
        "running" => Ok(TurnState::Running),
        "waiting_approval" => Ok(TurnState::WaitingApproval),
        "tool_running" => Ok(TurnState::ToolRunning),
        "completed" => Ok(TurnState::Completed),
        "degraded" => Ok(TurnState::Degraded),
        "aborted" => Ok(TurnState::Aborted),
        "quarantined" => Ok(TurnState::Quarantined),
        other => Err(TurnStoreError::CorruptState(other.to_string())),
    }
}

fn parse_outbox_status(value: &str) -> Result<TurnOutboxStatus, TurnStoreError> {
    match value {
        "pending" => Ok(TurnOutboxStatus::Pending),
        "leased" => Ok(TurnOutboxStatus::Leased),
        "delivered" => Ok(TurnOutboxStatus::Delivered),
        other => Err(TurnStoreError::CorruptOutboxStatus(other.to_string())),
    }
}

fn parse_order_origin(value: &str) -> Result<&'static str, TurnStoreError> {
    match value {
        "legacy_rowid_backfill" => Ok("legacy_rowid_backfill"),
        "transactional" => Ok("transactional"),
        other => Err(TurnStoreError::SchemaIntegrity(format!(
            "outbox row has unknown order origin: {other}"
        ))),
    }
}

#[cfg(test)]
#[path = "turn_store/tests.rs"]
mod tests;
