use std::path::PathBuf;

use zaion_crypto::principal_id_from_public_key;
use zaion_ledger::{EventLedger, IdempotentEventBinding, VerifiedEventCommit};
use zaion_types::{
    identity::{PrincipalId, PublicKeyBytes},
    session::{NamespaceKey, RunId},
};

use super::*;

/// A leased outbox row whose complete durable history was revalidated for
/// signing. Fields remain private so callers cannot manufacture this state.
#[derive(Debug, Clone)]
pub struct SigningValidatedOutbox {
    outbox: TurnOutboxRecord,
    binding: IdempotentEventBinding,
}

impl SigningValidatedOutbox {
    pub fn outbox(&self) -> &TurnOutboxRecord {
        &self.outbox
    }

    pub fn binding(&self) -> &IdempotentEventBinding {
        &self.binding
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutboxCompletion {
    Delivered,
    AlreadyDelivered,
}

#[derive(Debug, Clone)]
struct ValidatedHistoryEntry {
    outbox: TurnOutboxRecord,
    state: TurnState,
    binding: IdempotentEventBinding,
}

#[derive(Debug, Clone)]
struct DispatchSnapshot {
    history: Vec<ValidatedHistoryEntry>,
    target_index: usize,
    tenant_prefix: Vec<VerifiedPrefixEntry>,
    database_path: PathBuf,
    database_instance_id: String,
}

#[derive(Debug, Clone)]
struct VerifiedPrefixEntry {
    revision: u64,
    binding: IdempotentEventBinding,
    ledger_event_id: String,
    public_key: PublicKeyBytes,
}

impl DispatchSnapshot {
    fn target(&self) -> &ValidatedHistoryEntry {
        &self.history[self.target_index]
    }
}

impl DurableTurnStore {
    /// Revalidate the tenant head, active fencing lease, immutable turn
    /// history, signer principal, and every already delivered parent before a
    /// caller signs the current state event.
    #[allow(clippy::too_many_arguments)]
    pub fn revalidate_outbox_for_signing(
        &self,
        tenant_id: &str,
        outbox_id: &str,
        lease_owner: &str,
        lease_token: &str,
        now: DateTime<Utc>,
        ledger: &EventLedger,
        public_key: &PublicKeyBytes,
    ) -> Result<SigningValidatedOutbox, TurnStoreError> {
        validate_lease_identity("lease_owner", lease_owner)?;
        validate_lease_identity("lease_token", lease_token)?;
        let snapshot =
            self.dispatch_snapshot(tenant_id, outbox_id, lease_owner, lease_token, now, false)?;
        verify_ledger_scope(&snapshot, ledger, public_key)?;
        verify_tenant_prefix(&snapshot, ledger)?;
        let target = snapshot.target();
        Ok(SigningValidatedOutbox {
            outbox: target.outbox.clone(),
            binding: target.binding.clone(),
        })
    }

    /// Complete a leased outbox row only when a sealed Ledger token still
    /// matches the live signed event and the durable store remains unchanged.
    #[allow(clippy::too_many_arguments)]
    pub fn complete_outbox(
        &self,
        tenant_id: &str,
        outbox_id: &str,
        lease_owner: &str,
        lease_token: &str,
        commit: &VerifiedEventCommit,
        now: DateTime<Utc>,
        ledger: &EventLedger,
    ) -> Result<OutboxCompletion, TurnStoreError> {
        self.complete_outbox_inner(
            tenant_id,
            outbox_id,
            lease_owner,
            lease_token,
            commit,
            now,
            ledger,
            false,
        )
    }

    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn complete_outbox_with_evidence_failpoint(
        &self,
        tenant_id: &str,
        outbox_id: &str,
        lease_owner: &str,
        lease_token: &str,
        commit: &VerifiedEventCommit,
        now: DateTime<Utc>,
        ledger: &EventLedger,
    ) -> Result<OutboxCompletion, TurnStoreError> {
        self.complete_outbox_inner(
            tenant_id,
            outbox_id,
            lease_owner,
            lease_token,
            commit,
            now,
            ledger,
            true,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn complete_outbox_inner(
        &self,
        tenant_id: &str,
        outbox_id: &str,
        lease_owner: &str,
        lease_token: &str,
        commit: &VerifiedEventCommit,
        now: DateTime<Utc>,
        ledger: &EventLedger,
        inject_after_evidence_insert: bool,
    ) -> Result<OutboxCompletion, TurnStoreError> {
        validate_lease_identity("lease_owner", lease_owner)?;
        validate_lease_identity("lease_token", lease_token)?;
        let commit_public_key = commit.public_key_bytes();

        let snapshot =
            self.dispatch_snapshot(tenant_id, outbox_id, lease_owner, lease_token, now, true)?;
        verify_ledger_scope(&snapshot, ledger, &commit_public_key)?;
        verify_tenant_prefix(&snapshot, ledger)?;
        if !commit.matches_binding(ledger, &snapshot.target().binding)? {
            return Err(TurnStoreError::OutboxCommitMismatch {
                outbox_id: outbox_id.to_string(),
            });
        }

        let mut conn = self.connection()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        validate_no_extra_outbox_triggers(&tx)?;
        let current = load_dispatch_snapshot(
            &tx,
            tenant_id,
            outbox_id,
            lease_owner,
            lease_token,
            now,
            true,
        )?;
        verify_tenant_prefix_in_connection(&current, &tx)?;
        let commit_matches =
            commit.matches_binding_in_connection(&tx, &current.target().binding)?;
        if current.database_path != snapshot.database_path
            || current.database_path != commit.canonical_ledger_path()
            || current.database_instance_id != snapshot.database_instance_id
            || current.database_instance_id != commit.database_instance_id()
            || !bindings_match(&snapshot.target().binding, &current.target().binding)
            || !prefixes_match(&snapshot.tenant_prefix, &current.tenant_prefix)
            || !commit_matches
        {
            return Err(TurnStoreError::OutboxCommitMismatch {
                outbox_id: outbox_id.to_string(),
            });
        }
        let target = current.target();
        let inserted_evidence =
            ensure_verified_commit_evidence(&tx, &target.outbox, commit, &commit_public_key)?;
        #[cfg(test)]
        if inject_after_evidence_insert && inserted_evidence {
            return Err(TurnStoreError::InjectedAfterVerifiedCommitEvidence);
        }
        #[cfg(not(test))]
        let _ = (inject_after_evidence_insert, inserted_evidence);
        if target.outbox.status == TurnOutboxStatus::Delivered {
            if target.outbox.ledger_event_id.as_deref() != Some(commit.event_id()) {
                return Err(TurnStoreError::OutboxCommitMismatch {
                    outbox_id: outbox_id.to_string(),
                });
            }
            let repaired = load_outbox(&tx, tenant_id, outbox_id)?.ok_or_else(|| {
                TurnStoreError::MissingOutbox {
                    tenant_id: tenant_id.to_string(),
                    outbox_id: outbox_id.to_string(),
                }
            })?;
            verify_outbox_commit_evidence(&repaired, commit, &commit_public_key)?;
            tx.commit()?;
            return Ok(OutboxCompletion::AlreadyDelivered);
        }
        if now < target.outbox.updated_at {
            return Err(TurnStoreError::OutboxCompletionTimeInvalid);
        }

        let changed = tx.execute(
            "UPDATE turn_outbox_v2
             SET status = 'delivered', lease_owner = NULL, lease_token = NULL,
                 lease_until_ms = NULL, delivered_at_ms = ?6,
                 ledger_event_id = ?5, updated_at_ms = ?6
             WHERE tenant_id = ?1 AND outbox_id = ?2 AND status = 'leased'
               AND lease_owner = ?3 AND lease_token = ?4
               AND lease_until_ms > ?6",
            params![
                tenant_id,
                outbox_id,
                lease_owner,
                lease_token,
                commit.event_id(),
                timestamp_millis(now),
            ],
        )?;
        if changed != 1 {
            return Err(TurnStoreError::OutboxLeaseLost {
                outbox_id: outbox_id.to_string(),
                lease_owner: lease_owner.to_string(),
            });
        }
        let delivered = load_outbox(&tx, tenant_id, outbox_id)?.ok_or_else(|| {
            TurnStoreError::MissingOutbox {
                tenant_id: tenant_id.to_string(),
                outbox_id: outbox_id.to_string(),
            }
        })?;
        if delivered.status != TurnOutboxStatus::Delivered
            || delivered.ledger_event_id.as_deref() != Some(commit.event_id())
            || delivered.delivered_at.map(timestamp_millis) != Some(timestamp_millis(now))
            || timestamp_millis(delivered.updated_at) != timestamp_millis(now)
            || delivered.last_error.is_some()
        {
            return Err(TurnStoreError::SchemaIntegrity(
                "verified outbox completion was modified by unexpected database behavior"
                    .to_string(),
            ));
        }
        verify_outbox_commit_evidence(&delivered, commit, &commit_public_key)?;
        let _ = tenant_outbox_head(&tx, tenant_id)?;
        tx.commit()?;
        Ok(OutboxCompletion::Delivered)
    }

    fn dispatch_snapshot(
        &self,
        tenant_id: &str,
        outbox_id: &str,
        lease_owner: &str,
        lease_token: &str,
        now: DateTime<Utc>,
        allow_delivered: bool,
    ) -> Result<DispatchSnapshot, TurnStoreError> {
        let mut conn = self.connection()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        validate_no_extra_outbox_triggers(&tx)?;
        let snapshot = load_dispatch_snapshot(
            &tx,
            tenant_id,
            outbox_id,
            lease_owner,
            lease_token,
            now,
            allow_delivered,
        )?;
        if snapshot.database_instance_id != self.database_instance_id {
            return Err(TurnStoreError::OutboxLedgerInstanceMismatch);
        }
        tx.commit()?;
        Ok(snapshot)
    }
}

#[allow(clippy::too_many_arguments)]
fn load_dispatch_snapshot(
    conn: &Connection,
    tenant_id: &str,
    outbox_id: &str,
    lease_owner: &str,
    lease_token: &str,
    now: DateTime<Utc>,
    allow_delivered: bool,
) -> Result<DispatchSnapshot, TurnStoreError> {
    let target =
        load_outbox(conn, tenant_id, outbox_id)?.ok_or_else(|| TurnStoreError::MissingOutbox {
            tenant_id: tenant_id.to_string(),
            outbox_id: outbox_id.to_string(),
        })?;
    let turn = load_turn(conn, tenant_id, &target.turn_id)?.ok_or_else(|| {
        TurnStoreError::MissingTurn {
            tenant_id: tenant_id.to_string(),
            turn_id: target.turn_id.clone(),
        }
    })?;
    let history = validate_turn_history(conn, &turn)?;
    let target_index = history
        .iter()
        .position(|entry| entry.outbox.outbox_id == outbox_id)
        .ok_or_else(|| TurnStoreError::OutboxHistoryIncomplete {
            turn_id: turn.turn_id.clone(),
        })?;
    let target = &history[target_index].outbox;
    let tenant_head = tenant_outbox_head(conn, tenant_id)?;
    if target.status != TurnOutboxStatus::Delivered {
        if tenant_head.as_deref() != Some(outbox_id) {
            return Err(TurnStoreError::OutboxOrderConflict {
                outbox_id: outbox_id.to_string(),
            });
        }
        validate_active_lease(target, lease_owner, lease_token, now)?;
    } else if !allow_delivered {
        return Err(TurnStoreError::OutboxLeaseLost {
            outbox_id: outbox_id.to_string(),
            lease_owner: lease_owner.to_string(),
        });
    }

    let database_path = zaion_ledger::validated_database_path(conn)?;
    let database_instance_id = zaion_ledger::validated_database_instance_id(conn)?;
    let tenant_prefix = load_tenant_verified_prefix(
        conn,
        tenant_id,
        target.commit_ordinal,
        &turn.turn_id,
        &history,
        &database_instance_id,
    )?;

    Ok(DispatchSnapshot {
        history,
        target_index,
        tenant_prefix,
        database_path,
        database_instance_id,
    })
}

fn load_tenant_verified_prefix(
    conn: &Connection,
    tenant_id: &str,
    target_ordinal: u64,
    target_turn_id: &str,
    target_history: &[ValidatedHistoryEntry],
    database_instance_id: &str,
) -> Result<Vec<VerifiedPrefixEntry>, TurnStoreError> {
    let target_ordinal_i64 = i64::try_from(target_ordinal).map_err(|_| {
        TurnStoreError::SchemaIntegrity(
            "outbox commit ordinal exceeds the SQLite integer range".to_string(),
        )
    })?;
    let turn_ids = {
        let mut statement = conn.prepare(
            "SELECT DISTINCT o.turn_id
             FROM turn_outbox_commit_order_v2 c
             JOIN turn_outbox_v2 o
               ON o.tenant_id = c.tenant_id AND o.outbox_id = c.outbox_id
             WHERE o.tenant_id = ?1 AND c.commit_ordinal < ?2
               AND o.status = 'delivered'
             ORDER BY o.turn_id",
        )?;
        let rows = statement.query_map(params![tenant_id, target_ordinal_i64], |row| row.get(0))?;
        rows.collect::<rusqlite::Result<Vec<String>>>()?
    };

    let mut prefix = Vec::new();
    for turn_id in turn_ids {
        let turn =
            load_turn(conn, tenant_id, &turn_id)?.ok_or_else(|| TurnStoreError::MissingTurn {
                tenant_id: tenant_id.to_string(),
                turn_id: turn_id.clone(),
            })?;
        let owned_history;
        let history = if turn_id == target_turn_id {
            target_history
        } else {
            owned_history = validate_turn_history(conn, &turn)?;
            &owned_history
        };
        for entry in history.iter().filter(|entry| {
            entry.outbox.commit_ordinal < target_ordinal
                && entry.outbox.status == TurnOutboxStatus::Delivered
        }) {
            let ledger_event_id = entry.outbox.ledger_event_id.clone().ok_or(
                TurnStoreError::OutboxDeliveredPrefix {
                    revision: entry.outbox.revision,
                },
            )?;
            let public_key =
                PublicKeyBytes(entry.outbox.verified_signer_public_key.clone().ok_or_else(
                    || TurnStoreError::OutboxSignerEvidenceMissing {
                        outbox_id: entry.outbox.outbox_id.clone(),
                    },
                )?);
            if entry.outbox.verified_ledger_event_id.as_deref() != Some(ledger_event_id.as_str()) {
                return Err(TurnStoreError::OutboxLedgerEventMismatch {
                    revision: entry.outbox.revision,
                });
            }
            if entry.outbox.verified_database_instance_id.as_deref() != Some(database_instance_id) {
                return Err(TurnStoreError::OutboxLedgerInstanceMismatch);
            }
            if principal_id_from_public_key(&public_key).as_str() != turn.principal_id {
                return Err(TurnStoreError::OutboxPrincipalMismatch);
            }
            prefix.push(VerifiedPrefixEntry {
                revision: entry.outbox.revision,
                binding: entry.binding.clone(),
                ledger_event_id,
                public_key: public_key.clone(),
            });
        }
    }
    prefix.sort_by_key(|entry| entry.binding.expected_event_id().0.clone());
    Ok(prefix)
}

fn validate_active_lease(
    target: &TurnOutboxRecord,
    lease_owner: &str,
    lease_token: &str,
    now: DateTime<Utc>,
) -> Result<(), TurnStoreError> {
    if now < target.updated_at {
        return Err(TurnStoreError::NonMonotonicTimestamp);
    }
    if target.status != TurnOutboxStatus::Leased
        || target.lease_owner.as_deref() != Some(lease_owner)
        || target.lease_token.as_deref() != Some(lease_token)
    {
        return Err(TurnStoreError::OutboxLeaseLost {
            outbox_id: target.outbox_id.clone(),
            lease_owner: lease_owner.to_string(),
        });
    }
    if target
        .lease_until
        .is_none_or(|lease_until| lease_until <= now)
    {
        return Err(TurnStoreError::OutboxLeaseExpired {
            outbox_id: target.outbox_id.clone(),
        });
    }
    Ok(())
}

fn validate_turn_history(
    conn: &Connection,
    turn: &DurableTurnRecord,
) -> Result<Vec<ValidatedHistoryEntry>, TurnStoreError> {
    let raw_rows = {
        let mut statement = conn.prepare(&format!(
            "{OUTBOX_SELECT}
             WHERE o.tenant_id = ?1 AND o.turn_id = ?2
             ORDER BY o.revision"
        ))?;
        let rows =
            statement.query_map(params![turn.tenant_id, turn.turn_id], raw_outbox_from_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    let outbox = raw_rows
        .into_iter()
        .map(materialize_outbox)
        .collect::<Result<Vec<_>, _>>()?;
    let actual_count =
        u64::try_from(outbox.len()).map_err(|_| TurnStoreError::OutboxHistoryIncomplete {
            turn_id: turn.turn_id.clone(),
        })?;
    let expected_count = turn.state.revision().checked_add(1).ok_or_else(|| {
        TurnStoreError::OutboxHistoryIncomplete {
            turn_id: turn.turn_id.clone(),
        }
    })?;
    if actual_count != expected_count {
        return Err(TurnStoreError::OutboxHistoryIncomplete {
            turn_id: turn.turn_id.clone(),
        });
    }

    let mut validated: Vec<ValidatedHistoryEntry> = Vec::with_capacity(outbox.len());
    let mut undelivered_seen = false;
    for (index, record) in outbox.into_iter().enumerate() {
        let expected_revision =
            u64::try_from(index).map_err(|_| TurnStoreError::OutboxHistoryIncomplete {
                turn_id: turn.turn_id.clone(),
            })?;
        if record.revision != expected_revision {
            return Err(TurnStoreError::OutboxHistoryRevision {
                expected: expected_revision,
                actual: record.revision,
            });
        }
        if let Some(previous) = validated.last() {
            if previous.outbox.commit_ordinal >= record.commit_ordinal {
                return Err(TurnStoreError::OutboxHistoryIncomplete {
                    turn_id: turn.turn_id.clone(),
                });
            }
            if previous.outbox.created_at > record.created_at {
                return Err(TurnStoreError::NonMonotonicTimestamp);
            }
        }
        validate_outbox_identity(&record, turn)?;
        let state = outbox_payload_state(&record)?;
        validate_history_transition(&record, state, validated.last())?;
        validate_terminal_binding(&record, state, turn)?;

        let parent_event_id = validated
            .last()
            .map(|previous| previous.binding.expected_event_id());
        let binding = IdempotentEventBinding::new(
            record.outbox_id.clone(),
            PrincipalId(turn.principal_id.clone()),
            NamespaceKey(turn.session_id.clone()),
            Some(RunId(turn.turn_id.clone())),
            // Legacy rows retain their exact `turn.*` projection. New writes
            // use `turn.state.*`; changing a persisted type during dispatch
            // would break crash-retry idempotency.
            record.event_type.clone(),
            record.payload.clone(),
            parent_event_id,
        )?;

        match record.status {
            TurnOutboxStatus::Delivered => {
                if undelivered_seen {
                    return Err(TurnStoreError::OutboxDeliveredPrefix {
                        revision: record.revision,
                    });
                }
                if record.ledger_event_id.as_deref() != Some(binding.expected_event_id().0.as_str())
                {
                    return Err(TurnStoreError::OutboxLedgerEventMismatch {
                        revision: record.revision,
                    });
                }
            }
            TurnOutboxStatus::Pending | TurnOutboxStatus::Leased => undelivered_seen = true,
        }
        validated.push(ValidatedHistoryEntry {
            outbox: record,
            state,
            binding,
        });
    }

    let first = validated
        .first()
        .ok_or_else(|| TurnStoreError::OutboxHistoryIncomplete {
            turn_id: turn.turn_id.clone(),
        })?;
    let last = validated
        .last()
        .ok_or_else(|| TurnStoreError::OutboxHistoryIncomplete {
            turn_id: turn.turn_id.clone(),
        })?;
    if first.state != TurnState::Accepted
        || first.outbox.created_at != turn.created_at
        || last.state != turn.state.state()
        || last.outbox.revision != turn.state.revision()
        || last.outbox.created_at != turn.updated_at
    {
        return Err(TurnStoreError::OutboxHistoryCurrentTurn {
            turn_id: turn.turn_id.clone(),
        });
    }
    Ok(validated)
}

fn validate_outbox_identity(
    record: &TurnOutboxRecord,
    turn: &DurableTurnRecord,
) -> Result<(), TurnStoreError> {
    validate_outbox_payload_shape(&record.payload)?;
    for (field, pointer, expected) in [
        ("outbox.actor_key", "/actor_key", turn.actor_key.as_str()),
        ("outbox.subject_id", "/subject_id", turn.subject_id.as_str()),
        (
            "outbox.principal_id",
            "/principal_id",
            turn.principal_id.as_str(),
        ),
        (
            "outbox.workspace_id",
            "/workspace_id",
            turn.workspace_id.as_str(),
        ),
        ("outbox.profile_id", "/profile_id", turn.profile_id.as_str()),
        ("outbox.session_id", "/session_id", turn.session_id.as_str()),
        (
            "outbox.source_surface",
            "/source/surface",
            turn.source_surface.as_str(),
        ),
        (
            "outbox.source_id",
            "/source/source_id",
            turn.source_id.as_str(),
        ),
        (
            "outbox.idempotency_key",
            "/idempotency_key",
            turn.idempotency_key.as_str(),
        ),
        (
            "outbox.request_hash",
            "/request_hash",
            turn.request_hash.as_str(),
        ),
        (
            "outbox.authority_hash",
            "/authority_hash",
            turn.authority_hash.as_str(),
        ),
    ] {
        verify_json_text_binding(&record.payload, pointer, expected, field)?;
    }
    if record.effect_kind != "ledger_turn_state" {
        return Err(TurnStoreError::RecordBindingMismatch {
            field: "outbox.effect_kind",
        });
    }
    if record.idempotency_mode != "key_required" {
        return Err(TurnStoreError::RecordBindingMismatch {
            field: "outbox.idempotency_mode",
        });
    }
    let occurred_at_text = record
        .payload
        .pointer("/occurred_at")
        .and_then(Value::as_str)
        .ok_or(TurnStoreError::RecordBindingMismatch {
            field: "outbox.occurred_at",
        })?;
    let occurred_at = DateTime::parse_from_rfc3339(occurred_at_text)
        .ok()
        .map(|value| value.with_timezone(&Utc))
        .ok_or(TurnStoreError::RecordBindingMismatch {
            field: "outbox.occurred_at",
        })?;
    if timestamp_millis(occurred_at) != timestamp_millis(record.created_at) {
        return Err(TurnStoreError::RecordBindingMismatch {
            field: "outbox.occurred_at",
        });
    }
    if record.event_type.starts_with("turn.state.")
        && occurred_at_text
            != record
                .created_at
                .to_rfc3339_opts(SecondsFormat::Millis, true)
    {
        return Err(TurnStoreError::RecordBindingMismatch {
            field: "outbox.occurred_at",
        });
    }
    Ok(())
}

fn validate_outbox_payload_shape(payload: &Value) -> Result<(), TurnStoreError> {
    const TOP_LEVEL_FIELDS: [&str; 20] = [
        "schema",
        "outbox_id",
        "tenant_id",
        "turn_id",
        "actor_key",
        "subject_id",
        "principal_id",
        "workspace_id",
        "profile_id",
        "session_id",
        "source",
        "idempotency_key",
        "request_hash",
        "authority_hash",
        "previous_state",
        "state",
        "revision",
        "terminal",
        "terminal_result_hash",
        "occurred_at",
    ];
    let object = payload
        .as_object()
        .ok_or(TurnStoreError::RecordBindingMismatch {
            field: "outbox.payload_shape",
        })?;
    if object.len() != TOP_LEVEL_FIELDS.len()
        || TOP_LEVEL_FIELDS
            .iter()
            .any(|field| !object.contains_key(*field))
    {
        return Err(TurnStoreError::RecordBindingMismatch {
            field: "outbox.payload_shape",
        });
    }
    let source = object.get("source").and_then(Value::as_object).ok_or(
        TurnStoreError::RecordBindingMismatch {
            field: "outbox.source_shape",
        },
    )?;
    if source.len() != 2 || !source.contains_key("surface") || !source.contains_key("source_id") {
        return Err(TurnStoreError::RecordBindingMismatch {
            field: "outbox.source_shape",
        });
    }
    Ok(())
}

fn outbox_payload_state(record: &TurnOutboxRecord) -> Result<TurnState, TurnStoreError> {
    let state = record
        .payload
        .pointer("/state")
        .and_then(Value::as_str)
        .ok_or(TurnStoreError::RecordBindingMismatch {
            field: "outbox.state",
        })?;
    parse_state(state)
}

fn validate_history_transition(
    record: &TurnOutboxRecord,
    state: TurnState,
    previous: Option<&ValidatedHistoryEntry>,
) -> Result<(), TurnStoreError> {
    match previous {
        None => {
            if record.revision != 0 || state != TurnState::Accepted {
                return Err(TurnStoreError::OutboxHistoryGenesis {
                    revision: record.revision,
                });
            }
            if record.payload.get("previous_state") != Some(&Value::Null) {
                return Err(TurnStoreError::OutboxHistoryPreviousState {
                    revision: record.revision,
                });
            }
        }
        Some(previous) => {
            if record
                .payload
                .pointer("/previous_state")
                .and_then(Value::as_str)
                != Some(state_name(previous.state))
            {
                return Err(TurnStoreError::OutboxHistoryPreviousState {
                    revision: record.revision,
                });
            }
            if !previous.state.can_transition_to(state) {
                return Err(TurnStoreError::OutboxHistoryIllegalTransition {
                    revision: record.revision,
                    from: previous.state,
                    to: state,
                });
            }
        }
    }
    Ok(())
}

fn validate_terminal_binding(
    record: &TurnOutboxRecord,
    state: TurnState,
    turn: &DurableTurnRecord,
) -> Result<(), TurnStoreError> {
    if record.payload.pointer("/terminal").and_then(Value::as_bool) != Some(state.is_terminal()) {
        return Err(TurnStoreError::RecordBindingMismatch {
            field: "outbox.terminal",
        });
    }
    let terminal_hash = record.payload.get("terminal_result_hash").ok_or(
        TurnStoreError::RecordBindingMismatch {
            field: "outbox.terminal_result_hash",
        },
    )?;
    if state.is_terminal() {
        if record.revision != turn.state.revision()
            || terminal_hash.as_str() != turn.terminal_result_hash.as_deref()
        {
            return Err(TurnStoreError::RecordBindingMismatch {
                field: "outbox.terminal_result_hash",
            });
        }
    } else if !terminal_hash.is_null() {
        return Err(TurnStoreError::RecordBindingMismatch {
            field: "outbox.terminal_result_hash",
        });
    }
    Ok(())
}

fn verify_ledger_scope(
    snapshot: &DispatchSnapshot,
    ledger: &EventLedger,
    public_key: &PublicKeyBytes,
) -> Result<(), TurnStoreError> {
    if ledger.canonical_database_path()? != snapshot.database_path {
        return Err(TurnStoreError::OutboxLedgerPathMismatch);
    }
    if ledger.database_instance_id()? != snapshot.database_instance_id {
        return Err(TurnStoreError::OutboxLedgerInstanceMismatch);
    }
    let expected_principal = snapshot.target().binding.principal_id();
    if &principal_id_from_public_key(public_key) != expected_principal {
        return Err(TurnStoreError::OutboxPrincipalMismatch);
    }
    Ok(())
}

fn ensure_verified_commit_evidence(
    conn: &Connection,
    outbox: &TurnOutboxRecord,
    commit: &VerifiedEventCommit,
    public_key: &PublicKeyBytes,
) -> Result<bool, TurnStoreError> {
    match (
        outbox.verified_ledger_event_id.as_deref(),
        outbox.verified_signer_public_key.as_deref(),
        outbox.verified_database_instance_id.as_deref(),
    ) {
        (None, None, None) => {
            let changed = conn.execute(
                "INSERT INTO turn_outbox_verified_commit_v2 (
                    tenant_id, outbox_id, ledger_event_id, signer_public_key,
                    database_instance_id
                 ) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    outbox.tenant_id,
                    outbox.outbox_id,
                    commit.event_id(),
                    public_key.0,
                    commit.database_instance_id(),
                ],
            )?;
            if changed == 1 {
                Ok(true)
            } else {
                Err(TurnStoreError::OutboxCommitMismatch {
                    outbox_id: outbox.outbox_id.clone(),
                })
            }
        }
        (Some(event_id), Some(stored_key), Some(instance_id))
            if event_id == commit.event_id()
                && stored_key == public_key.0.as_slice()
                && instance_id == commit.database_instance_id() =>
        {
            Ok(false)
        }
        _ => Err(TurnStoreError::OutboxCommitMismatch {
            outbox_id: outbox.outbox_id.clone(),
        }),
    }
}

fn verify_outbox_commit_evidence(
    outbox: &TurnOutboxRecord,
    commit: &VerifiedEventCommit,
    public_key: &PublicKeyBytes,
) -> Result<(), TurnStoreError> {
    if outbox.verified_ledger_event_id.as_deref() == Some(commit.event_id())
        && outbox.verified_signer_public_key.as_deref() == Some(public_key.0.as_slice())
        && outbox.verified_database_instance_id.as_deref() == Some(commit.database_instance_id())
    {
        Ok(())
    } else {
        Err(TurnStoreError::OutboxCommitMismatch {
            outbox_id: outbox.outbox_id.clone(),
        })
    }
}

fn verify_tenant_prefix(
    snapshot: &DispatchSnapshot,
    ledger: &EventLedger,
) -> Result<(), TurnStoreError> {
    for entry in &snapshot.tenant_prefix {
        let verified =
            ledger.verify_existing_idempotent_event(&entry.public_key, &entry.binding)?;
        if verified.event_id() != entry.ledger_event_id
            || verified.canonical_ledger_path() != snapshot.database_path
            || verified.database_instance_id() != snapshot.database_instance_id
        {
            return Err(TurnStoreError::OutboxLedgerEventMismatch {
                revision: entry.revision,
            });
        }
    }
    Ok(())
}

fn verify_tenant_prefix_in_connection(
    snapshot: &DispatchSnapshot,
    conn: &Connection,
) -> Result<(), TurnStoreError> {
    for entry in &snapshot.tenant_prefix {
        let verified = zaion_ledger::verify_existing_idempotent_event_in_connection(
            conn,
            &entry.public_key,
            &entry.binding,
        )?;
        if verified.event_id() != entry.ledger_event_id
            || verified.canonical_ledger_path() != snapshot.database_path
            || verified.database_instance_id() != snapshot.database_instance_id
        {
            return Err(TurnStoreError::OutboxLedgerEventMismatch {
                revision: entry.revision,
            });
        }
    }
    Ok(())
}

fn bindings_match(left: &IdempotentEventBinding, right: &IdempotentEventBinding) -> bool {
    left.idempotency_key() == right.idempotency_key()
        && left.principal_id() == right.principal_id()
        && left.namespace_key() == right.namespace_key()
        && left.run_id() == right.run_id()
        && left.event_type() == right.event_type()
        && left.payload() == right.payload()
        && left.parent_event_id().map(|event| event.0.as_str())
            == right.parent_event_id().map(|event| event.0.as_str())
}

fn prefixes_match(left: &[VerifiedPrefixEntry], right: &[VerifiedPrefixEntry]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            left.revision == right.revision
                && left.ledger_event_id == right.ledger_event_id
                && left.public_key.0 == right.public_key.0
                && bindings_match(&left.binding, &right.binding)
        })
}
