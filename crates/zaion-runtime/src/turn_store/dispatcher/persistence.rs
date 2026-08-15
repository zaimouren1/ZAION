use chrono::{DateTime, Duration, Utc};
use rusqlite::{params, OptionalExtension, Transaction};

use super::{
    OutboxDispatchFailure, OutboxQuarantineRecord, QueueSnapshot,
    CREATE_OUTBOX_QUARANTINE_DELETE_GUARD, CREATE_OUTBOX_QUARANTINE_INSERT_GUARD,
    CREATE_OUTBOX_QUARANTINE_TABLE, CREATE_OUTBOX_QUARANTINE_UPDATE_GUARD,
    OUTBOX_QUARANTINE_DELETE_GUARD, OUTBOX_QUARANTINE_INSERT_GUARD, OUTBOX_QUARANTINE_MIGRATION_ID,
    OUTBOX_QUARANTINE_MIGRATION_KIND, OUTBOX_QUARANTINE_TABLE, OUTBOX_QUARANTINE_UPDATE_GUARD,
};
use crate::turn_store;
use turn_store::{
    bounded_limit, load_outbox, order_guard_prefix_evidence, timestamp_millis,
    validate_no_extra_outbox_triggers, validate_schema_object, DurableTurnStore,
    OutboxOrderManifest, TurnOutboxRecord, TurnOutboxStatus, TurnStoreError,
    MAX_OUTBOX_LEASE_SECONDS,
};
use zaion_types::identity::PrincipalId;

pub(crate) fn ensure_outbox_dispatcher_schema(
    tx: &Transaction<'_>,
    applied_at: DateTime<Utc>,
) -> Result<(), TurnStoreError> {
    let table_existed =
        turn_store::schema_object_sql(tx, "table", OUTBOX_QUARANTINE_TABLE)?.is_some();
    let insert_guard_existed =
        turn_store::schema_object_sql(tx, "trigger", OUTBOX_QUARANTINE_INSERT_GUARD)?.is_some();
    let update_guard_existed =
        turn_store::schema_object_sql(tx, "trigger", OUTBOX_QUARANTINE_UPDATE_GUARD)?.is_some();
    let delete_guard_existed =
        turn_store::schema_object_sql(tx, "trigger", OUTBOX_QUARANTINE_DELETE_GUARD)?.is_some();
    let marker = load_quarantine_migration_marker(tx)?;

    match (
        marker.is_some(),
        table_existed,
        insert_guard_existed,
        update_guard_existed,
        delete_guard_existed,
    ) {
        (false, false, false, false, false) => {
            tx.execute_batch(CREATE_OUTBOX_QUARANTINE_TABLE)?;
            tx.execute_batch(CREATE_OUTBOX_QUARANTINE_INSERT_GUARD)?;
            tx.execute_batch(CREATE_OUTBOX_QUARANTINE_UPDATE_GUARD)?;
            tx.execute_batch(CREATE_OUTBOX_QUARANTINE_DELETE_GUARD)?;
            let (source_count, source_maximum, source_digest) =
                order_guard_prefix_evidence(tx, None)?;
            tx.execute(
                "INSERT INTO turn_store_schema_migrations_v2 (
                    migration_id, migration_kind, source_row_count, source_max_rowid,
                    source_digest, applied_at_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    OUTBOX_QUARANTINE_MIGRATION_ID,
                    OUTBOX_QUARANTINE_MIGRATION_KIND,
                    source_count,
                    source_maximum,
                    source_digest,
                    timestamp_millis(applied_at),
                ],
            )?;
        }
        (false, _, _, _, _) => {
            return Err(TurnStoreError::SchemaIntegrity(
                "outbox quarantine objects exist without their migration marker".to_string(),
            ));
        }
        (true, true, true, true, true) => {}
        (true, _, _, _, _) => {
            return Err(TurnStoreError::SchemaIntegrity(
                "outbox quarantine migration marker exists but a required object is missing"
                    .to_string(),
            ));
        }
    }

    validate_schema_object(
        tx,
        "table",
        OUTBOX_QUARANTINE_TABLE,
        CREATE_OUTBOX_QUARANTINE_TABLE,
    )?;
    validate_outbox_dispatcher_triggers(tx)?;
    let marker = load_quarantine_migration_marker(tx)?.ok_or_else(|| {
        TurnStoreError::SchemaIntegrity(
            "outbox quarantine migration marker disappeared".to_string(),
        )
    })?;
    validate_quarantine_migration_marker(tx, &marker)?;
    validate_quarantine_rows(tx)
}

pub(crate) fn validate_outbox_dispatcher_triggers(
    tx: &Transaction<'_>,
) -> Result<(), TurnStoreError> {
    validate_schema_object(
        tx,
        "trigger",
        OUTBOX_QUARANTINE_INSERT_GUARD,
        CREATE_OUTBOX_QUARANTINE_INSERT_GUARD,
    )?;
    validate_schema_object(
        tx,
        "trigger",
        OUTBOX_QUARANTINE_UPDATE_GUARD,
        CREATE_OUTBOX_QUARANTINE_UPDATE_GUARD,
    )?;
    validate_schema_object(
        tx,
        "trigger",
        OUTBOX_QUARANTINE_DELETE_GUARD,
        CREATE_OUTBOX_QUARANTINE_DELETE_GUARD,
    )
}

fn load_quarantine_migration_marker(
    tx: &Transaction<'_>,
) -> Result<Option<OutboxOrderManifest>, TurnStoreError> {
    tx.query_row(
        "SELECT migration_kind, source_row_count, source_max_rowid, source_digest
         FROM turn_store_schema_migrations_v2 WHERE migration_id = ?1",
        params![OUTBOX_QUARANTINE_MIGRATION_ID],
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
            if kind != OUTBOX_QUARANTINE_MIGRATION_KIND
                || source_row_count < 0
                || source_max_rowid < 0
                || !source_digest.starts_with("sha256:")
            {
                return Err(TurnStoreError::SchemaIntegrity(
                    "outbox quarantine migration marker is malformed".to_string(),
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

fn validate_quarantine_migration_marker(
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
            "outbox quarantine migration evidence does not match its prefix".to_string(),
        ))
    }
}

fn validate_quarantine_rows(tx: &Transaction<'_>) -> Result<(), TurnStoreError> {
    let invalid: i64 = tx.query_row(
        "SELECT COUNT(*)
         FROM turn_outbox_quarantine_v2 q
         LEFT JOIN turn_outbox_v2 o
           ON o.tenant_id = q.tenant_id AND o.outbox_id = q.outbox_id
         LEFT JOIN turn_outbox_commit_order_v2 c
           ON c.tenant_id = q.tenant_id AND c.outbox_id = q.outbox_id
         LEFT JOIN turn_state_v2 s
           ON s.tenant_id = o.tenant_id AND s.turn_id = o.turn_id
         WHERE o.outbox_id IS NULL OR c.outbox_id IS NULL OR s.turn_id IS NULL
            OR o.status = 'delivered'
            OR c.commit_ordinal != q.commit_ordinal
            OR o.payload_hash != q.payload_hash
            OR o.attempts < q.attempts
            OR s.principal_id != q.principal_id",
        [],
        |row| row.get(0),
    )?;
    if invalid == 0 {
        Ok(())
    } else {
        Err(TurnStoreError::SchemaIntegrity(
            "outbox quarantine evidence is orphaned or mismatched".to_string(),
        ))
    }
}

pub(crate) fn outbox_is_quarantined(
    conn: &rusqlite::Connection,
    tenant_id: &str,
    outbox_id: &str,
) -> Result<bool, TurnStoreError> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM turn_outbox_quarantine_v2
             WHERE tenant_id = ?1 AND outbox_id = ?2",
            params![tenant_id, outbox_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .is_some())
}

impl DurableTurnStore {
    pub fn renew_outbox_lease(
        &self,
        tenant_id: &str,
        outbox_id: &str,
        lease_owner: &str,
        lease_token: &str,
        now: DateTime<Utc>,
        lease_duration: Duration,
    ) -> Result<TurnOutboxRecord, TurnStoreError> {
        turn_store::validate_lease_identity("lease_owner", lease_owner)?;
        turn_store::validate_lease_identity("lease_token", lease_token)?;
        if lease_duration < Duration::seconds(1)
            || lease_duration > Duration::seconds(MAX_OUTBOX_LEASE_SECONDS)
        {
            return Err(TurnStoreError::InvalidOutboxLeaseDuration);
        }
        let lease_until = now
            .checked_add_signed(lease_duration)
            .ok_or(TurnStoreError::OutboxLeaseTimeOverflow)?;
        let mut conn = self.connection()?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        validate_no_extra_outbox_triggers(&tx)?;
        if turn_store::tenant_outbox_head(&tx, tenant_id)?.as_deref() != Some(outbox_id) {
            return Err(TurnStoreError::OutboxOrderConflict {
                outbox_id: outbox_id.to_string(),
            });
        }
        if outbox_is_quarantined(&tx, tenant_id, outbox_id)? {
            return Err(TurnStoreError::OutboxLeaseLost {
                outbox_id: outbox_id.to_string(),
                lease_owner: lease_owner.to_string(),
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
        if current.lease_until.is_none_or(|deadline| deadline <= now) {
            return Err(TurnStoreError::OutboxLeaseExpired {
                outbox_id: outbox_id.to_string(),
            });
        }
        let changed = tx.execute(
            "UPDATE turn_outbox_v2
             SET lease_until_ms = ?5, updated_at_ms = ?6
             WHERE tenant_id = ?1 AND outbox_id = ?2 AND status = 'leased'
               AND lease_owner = ?3 AND lease_token = ?4
               AND lease_until_ms > ?6",
            params![
                tenant_id,
                outbox_id,
                lease_owner,
                lease_token,
                timestamp_millis(lease_until),
                timestamp_millis(now),
            ],
        )?;
        if changed != 1 {
            return Err(TurnStoreError::OutboxLeaseLost {
                outbox_id: outbox_id.to_string(),
                lease_owner: lease_owner.to_string(),
            });
        }
        let renewed = load_outbox(&tx, tenant_id, outbox_id)?.ok_or_else(|| {
            TurnStoreError::MissingOutbox {
                tenant_id: tenant_id.to_string(),
                outbox_id: outbox_id.to_string(),
            }
        })?;
        if renewed.lease_until.map(timestamp_millis) != Some(timestamp_millis(lease_until))
            || timestamp_millis(renewed.updated_at) != timestamp_millis(now)
            || renewed.attempts != current.attempts
        {
            return Err(TurnStoreError::SchemaIntegrity(
                "outbox lease renewal was modified by unexpected database behavior".to_string(),
            ));
        }
        tx.commit()?;
        Ok(renewed)
    }

    pub fn load_outbox_quarantine(
        &self,
        tenant_id: &str,
        outbox_id: &str,
    ) -> Result<Option<OutboxQuarantineRecord>, TurnStoreError> {
        let conn = self.connection()?;
        load_quarantine_record(&conn, tenant_id, outbox_id)
    }

    pub fn list_outbox_quarantines(
        &self,
        limit: usize,
    ) -> Result<Vec<OutboxQuarantineRecord>, TurnStoreError> {
        let conn = self.connection()?;
        let mut statement = conn.prepare(
            "SELECT tenant_id, outbox_id, commit_ordinal, principal_id,
                    failure_class, failure_code, failure_phase, error_message,
                    attempts, lease_owner, lease_token, payload_hash,
                    quarantined_at_ms
             FROM turn_outbox_quarantine_v2
             ORDER BY commit_ordinal LIMIT ?1",
        )?;
        let rows = statement.query_map(params![bounded_limit(limit)], raw_quarantine_from_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .map(materialize_quarantine)
            .collect()
    }

    pub(super) fn dispatch_queue_snapshot(
        &self,
        now: DateTime<Utc>,
        tenant_limit: usize,
    ) -> Result<QueueSnapshot, TurnStoreError> {
        let conn = self.connection()?;
        let now_ms = timestamp_millis(now);
        let mut statement = conn.prepare(
            "WITH tenant_heads AS (
                 SELECT o.tenant_id, MIN(c.commit_ordinal) AS head_ordinal
                 FROM turn_outbox_v2 o
                 JOIN turn_outbox_commit_order_v2 c
                   ON c.tenant_id = o.tenant_id AND c.outbox_id = o.outbox_id
                 WHERE o.status != 'delivered'
                 GROUP BY o.tenant_id
             )
             SELECT o.tenant_id
             FROM tenant_heads h
             JOIN turn_outbox_commit_order_v2 c
               ON c.tenant_id = h.tenant_id AND c.commit_ordinal = h.head_ordinal
             JOIN turn_outbox_v2 o
               ON o.tenant_id = c.tenant_id AND o.outbox_id = c.outbox_id
             LEFT JOIN turn_outbox_quarantine_v2 q
               ON q.tenant_id = o.tenant_id AND q.outbox_id = o.outbox_id
             WHERE q.outbox_id IS NULL AND o.available_at_ms <= ?1
               AND (
                   o.status = 'pending'
                   OR (o.status = 'leased' AND o.lease_until_ms <= ?1)
               )
             ORDER BY c.commit_ordinal
             LIMIT ?2",
        )?;
        let ready_tenants: Vec<String> = statement
            .query_map(params![now_ms, bounded_limit(tenant_limit)], |row| {
                row.get(0)
            })?
            .collect::<rusqlite::Result<Vec<String>>>()?;
        drop(statement);

        let (queue_depth, leased_count, oldest_created_at_ms): (i64, i64, Option<i64>) = conn
            .query_row(
                "SELECT COUNT(*),
                        COALESCE(SUM(CASE WHEN o.status = 'leased' THEN 1 ELSE 0 END), 0),
                        MIN(o.created_at_ms)
                 FROM turn_outbox_v2 o
                 LEFT JOIN turn_outbox_quarantine_v2 q
                   ON q.tenant_id = o.tenant_id AND q.outbox_id = o.outbox_id
                 WHERE o.status != 'delivered' AND q.outbox_id IS NULL",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )?;
        let dead_letters: i64 = conn.query_row(
            "SELECT COUNT(*) FROM turn_outbox_quarantine_v2",
            [],
            |row| row.get(0),
        )?;
        Ok(QueueSnapshot {
            ready_tenants,
            queue_depth: checked_nonnegative_u64("queue_depth", queue_depth)?,
            leased_count: checked_nonnegative_u64("leased_count", leased_count)?,
            oldest_created_at: oldest_created_at_ms
                .map(|value| turn_store::parse_timestamp("created_at_ms", value))
                .transpose()?,
            dead_letters: checked_nonnegative_u64("dead_letters", dead_letters)?,
        })
    }

    pub(super) fn quarantine_outbox(
        &self,
        claim: &TurnOutboxRecord,
        failure: &OutboxDispatchFailure,
        exhausted: bool,
        now: DateTime<Utc>,
    ) -> Result<OutboxQuarantineRecord, TurnStoreError> {
        let lease_owner =
            claim
                .lease_owner
                .as_deref()
                .ok_or_else(|| TurnStoreError::OutboxLeaseLost {
                    outbox_id: claim.outbox_id.clone(),
                    lease_owner: "missing".to_string(),
                })?;
        let lease_token =
            claim
                .lease_token
                .as_deref()
                .ok_or_else(|| TurnStoreError::OutboxLeaseLost {
                    outbox_id: claim.outbox_id.clone(),
                    lease_owner: lease_owner.to_string(),
                })?;
        let principal_id = claimed_principal_id(claim)?;
        let mut conn = self.connection()?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        validate_no_extra_outbox_triggers(&tx)?;
        if turn_store::tenant_outbox_head(&tx, &claim.tenant_id)?.as_deref()
            != Some(claim.outbox_id.as_str())
        {
            return Err(TurnStoreError::OutboxOrderConflict {
                outbox_id: claim.outbox_id.clone(),
            });
        }
        let current = load_outbox(&tx, &claim.tenant_id, &claim.outbox_id)?.ok_or_else(|| {
            TurnStoreError::MissingOutbox {
                tenant_id: claim.tenant_id.clone(),
                outbox_id: claim.outbox_id.clone(),
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
                outbox_id: claim.outbox_id.clone(),
                lease_owner: lease_owner.to_string(),
            });
        }
        if current
            .lease_until
            .is_none_or(|lease_until| lease_until <= now)
        {
            return Err(TurnStoreError::OutboxLeaseExpired {
                outbox_id: claim.outbox_id.clone(),
            });
        }
        let failure_class = if exhausted {
            "retry_exhausted"
        } else {
            "permanent"
        };
        tx.execute(
            "INSERT INTO turn_outbox_quarantine_v2 (
                tenant_id, outbox_id, commit_ordinal, principal_id,
                failure_class, failure_code, failure_phase, error_message,
                attempts, lease_owner, lease_token, payload_hash,
                quarantined_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                current.tenant_id,
                current.outbox_id,
                i64::try_from(current.commit_ordinal)
                    .map_err(|_| { TurnStoreError::CommitOrdinalExhausted })?,
                principal_id.as_str(),
                failure_class,
                failure.code.as_str(),
                failure.phase.as_str(),
                failure.message,
                i64::try_from(current.attempts)
                    .map_err(|_| { TurnStoreError::OutboxAttemptsExhausted })?,
                lease_owner,
                lease_token,
                current.payload_hash,
                timestamp_millis(now),
            ],
        )?;
        let changed = tx.execute(
            "UPDATE turn_outbox_v2
             SET status = 'pending', lease_owner = NULL, lease_token = NULL,
                 lease_until_ms = NULL, available_at_ms = ?5,
                 last_error = ?6, updated_at_ms = ?5
             WHERE tenant_id = ?1 AND outbox_id = ?2 AND status = 'leased'
               AND lease_owner = ?3 AND lease_token = ?4
               AND lease_until_ms > ?5",
            params![
                claim.tenant_id,
                claim.outbox_id,
                lease_owner,
                lease_token,
                timestamp_millis(now),
                failure.message,
            ],
        )?;
        if changed != 1 {
            return Err(TurnStoreError::OutboxLeaseLost {
                outbox_id: claim.outbox_id.clone(),
                lease_owner: lease_owner.to_string(),
            });
        }
        let quarantined = load_quarantine_record(&tx, &claim.tenant_id, &claim.outbox_id)?
            .ok_or_else(|| {
                TurnStoreError::SchemaIntegrity(
                    "outbox quarantine evidence disappeared before commit".to_string(),
                )
            })?;
        let released = load_outbox(&tx, &claim.tenant_id, &claim.outbox_id)?.ok_or_else(|| {
            TurnStoreError::MissingOutbox {
                tenant_id: claim.tenant_id.clone(),
                outbox_id: claim.outbox_id.clone(),
            }
        })?;
        if released.status != TurnOutboxStatus::Pending
            || released.lease_owner.is_some()
            || released.lease_token.is_some()
            || released.lease_until.is_some()
            || released.last_error.as_deref() != Some(failure.message.as_str())
            || !outbox_is_quarantined(&tx, &claim.tenant_id, &claim.outbox_id)?
        {
            return Err(TurnStoreError::SchemaIntegrity(
                "outbox quarantine did not atomically fence future claims".to_string(),
            ));
        }
        tx.commit()?;
        Ok(quarantined)
    }
}

pub(crate) fn claimed_principal_id(
    claim: &TurnOutboxRecord,
) -> Result<PrincipalId, TurnStoreError> {
    claim
        .payload
        .pointer("/principal_id")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .map(|value| PrincipalId(value.to_string()))
        .ok_or(TurnStoreError::RecordBindingMismatch {
            field: "outbox.principal_id",
        })
}

fn load_quarantine_record(
    conn: &rusqlite::Connection,
    tenant_id: &str,
    outbox_id: &str,
) -> Result<Option<OutboxQuarantineRecord>, TurnStoreError> {
    conn.query_row(
        "SELECT tenant_id, outbox_id, commit_ordinal, principal_id,
                failure_class, failure_code, failure_phase, error_message,
                attempts, lease_owner, lease_token, payload_hash,
                quarantined_at_ms
         FROM turn_outbox_quarantine_v2
         WHERE tenant_id = ?1 AND outbox_id = ?2",
        params![tenant_id, outbox_id],
        raw_quarantine_from_row,
    )
    .optional()?
    .map(materialize_quarantine)
    .transpose()
}

#[derive(Debug)]
struct RawQuarantineRecord {
    tenant_id: String,
    outbox_id: String,
    commit_ordinal: i64,
    principal_id: String,
    failure_class: String,
    failure_code: String,
    failure_phase: String,
    error_message: String,
    attempts: i64,
    lease_owner: String,
    lease_token: String,
    payload_hash: String,
    quarantined_at_ms: i64,
}

fn raw_quarantine_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawQuarantineRecord> {
    Ok(RawQuarantineRecord {
        tenant_id: row.get(0)?,
        outbox_id: row.get(1)?,
        commit_ordinal: row.get(2)?,
        principal_id: row.get(3)?,
        failure_class: row.get(4)?,
        failure_code: row.get(5)?,
        failure_phase: row.get(6)?,
        error_message: row.get(7)?,
        attempts: row.get(8)?,
        lease_owner: row.get(9)?,
        lease_token: row.get(10)?,
        payload_hash: row.get(11)?,
        quarantined_at_ms: row.get(12)?,
    })
}

fn materialize_quarantine(
    raw: RawQuarantineRecord,
) -> Result<OutboxQuarantineRecord, TurnStoreError> {
    if !matches!(raw.failure_class.as_str(), "permanent" | "retry_exhausted")
        || raw.failure_code.is_empty()
        || raw.failure_phase.is_empty()
        || raw.error_message.is_empty()
        || raw.principal_id.is_empty()
    {
        return Err(TurnStoreError::SchemaIntegrity(
            "outbox quarantine row contains an invalid classification".to_string(),
        ));
    }
    Ok(OutboxQuarantineRecord {
        tenant_id: raw.tenant_id,
        outbox_id: raw.outbox_id,
        commit_ordinal: checked_positive_u64("commit_ordinal", raw.commit_ordinal)?,
        principal_id: raw.principal_id,
        failure_class: raw.failure_class,
        failure_code: raw.failure_code,
        failure_phase: raw.failure_phase,
        error_message: raw.error_message,
        attempts: checked_positive_u64("attempts", raw.attempts)?,
        lease_owner: raw.lease_owner,
        lease_token: raw.lease_token,
        payload_hash: raw.payload_hash,
        quarantined_at: turn_store::parse_timestamp("quarantined_at_ms", raw.quarantined_at_ms)?,
    })
}

fn checked_nonnegative_u64(field: &'static str, value: i64) -> Result<u64, TurnStoreError> {
    u64::try_from(value).map_err(|_| {
        TurnStoreError::SchemaIntegrity(format!("outbox dispatcher metric {field} is negative"))
    })
}

fn checked_positive_u64(field: &'static str, value: i64) -> Result<u64, TurnStoreError> {
    let value = checked_nonnegative_u64(field, value)?;
    if value == 0 {
        Err(TurnStoreError::SchemaIntegrity(format!(
            "outbox quarantine field {field} must be positive"
        )))
    } else {
        Ok(value)
    }
}
