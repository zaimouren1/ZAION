use std::path::Path;
use std::sync::{Arc, Barrier};

use chrono::{Duration, TimeZone, Utc};
use tempfile::tempdir;
use zaion_crypto::ZaionKeypair;
use zaion_ledger::{EventLedger, IdempotentEventBinding, VerifiedEventCommit};
use zaion_types::event::EventId;
use zaion_types::identity::PrincipalId;
use zaion_types::session::{NamespaceKey, RunId, SessionId, WorkspaceId};

use super::*;
use crate::{
    AuthenticatedIngressInput, AuthenticatedSourceInput, IngressAttachmentInput, TurnExecution,
};

const TENANT: &str = "tenant-a";
const OWNER_A: &str = "worker-a";

fn clock() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 7, 15, 10, 0, 0)
        .single()
        .unwrap()
}

fn ingress(
    tenant_id: &str,
    subject_id: &str,
    idempotency_key: &str,
    now: DateTime<Utc>,
) -> AuthenticatedIngress {
    ingress_custom(
        tenant_id,
        subject_id,
        idempotency_key,
        now,
        now + Duration::minutes(5),
        vec!["turn:submit".to_string(), "tool:read".to_string()],
        Vec::new(),
    )
}

fn ingress_custom(
    tenant_id: &str,
    subject_id: &str,
    idempotency_key: &str,
    now: DateTime<Utc>,
    deadline: DateTime<Utc>,
    scopes: Vec<String>,
    attachments: Vec<IngressAttachmentInput>,
) -> AuthenticatedIngress {
    ingress_custom_with_principal(
        tenant_id,
        subject_id,
        idempotency_key,
        PrincipalId("did:key:turn-store-test".to_string()),
        now,
        deadline,
        scopes,
        attachments,
    )
}

#[allow(clippy::too_many_arguments)]
fn ingress_custom_with_principal(
    tenant_id: &str,
    subject_id: &str,
    idempotency_key: &str,
    principal_id: PrincipalId,
    now: DateTime<Utc>,
    deadline: DateTime<Utc>,
    scopes: Vec<String>,
    attachments: Vec<IngressAttachmentInput>,
) -> AuthenticatedIngress {
    AuthenticatedIngress::new(
        AuthenticatedIngressInput {
            tenant_id: tenant_id.to_string(),
            subject_id: subject_id.to_string(),
            principal_id,
            workspace_id: WorkspaceId("workspace-test".to_string()),
            profile_id: "default".to_string(),
            session_id: SessionId("session-test".to_string()),
            source: AuthenticatedSourceInput {
                surface: "cli".to_string(),
                source_id: "message-test".to_string(),
            },
            deadline,
            scopes,
            idempotency_key: idempotency_key.to_string(),
            attachments,
        },
        now,
    )
    .unwrap()
}

fn ingress_for_keypair(
    tenant_id: &str,
    subject_id: &str,
    idempotency_key: &str,
    now: DateTime<Utc>,
    keypair: &ZaionKeypair,
) -> AuthenticatedIngress {
    ingress_custom_with_principal(
        tenant_id,
        subject_id,
        idempotency_key,
        keypair.principal_id(),
        now,
        now + Duration::minutes(5),
        vec!["turn:submit".to_string(), "tool:read".to_string()],
        Vec::new(),
    )
}

fn admission(ingress: &AuthenticatedIngress, request: Value, owner: &str) -> DurableTurnAdmission {
    admission_on_thread(ingress, request, owner, "thread-main")
}

fn admission_on_thread(
    ingress: &AuthenticatedIngress,
    request: Value,
    owner: &str,
    thread_id: &str,
) -> DurableTurnAdmission {
    DurableTurnAdmission::new(
        TurnActorIdentity::for_ingress(ingress, "terminal", thread_id).unwrap(),
        request,
        owner,
    )
    .unwrap()
}

fn store() -> (tempfile::TempDir, DurableTurnStore) {
    let directory = tempdir().unwrap();
    let store = DurableTurnStore::open(directory.path().join("ledger.db")).unwrap();
    (directory, store)
}

fn append_claimed_outbox(
    store: &DurableTurnStore,
    claim: &TurnOutboxRecord,
    keypair: &ZaionKeypair,
    now: DateTime<Utc>,
) -> (EventLedger, VerifiedEventCommit) {
    let ledger = EventLedger::new(store.db_path());
    let validated = store
        .revalidate_outbox_for_signing(
            &claim.tenant_id,
            &claim.outbox_id,
            claim.lease_owner.as_deref().unwrap(),
            claim.lease_token.as_deref().unwrap(),
            now,
            &ledger,
            &keypair.public_key_bytes(),
        )
        .unwrap();
    let commit = ledger
        .append_verified_idempotent_event(keypair, validated.binding())
        .unwrap();
    (ledger, commit)
}

fn begin_keyed_turn(
    store: &DurableTurnStore,
    keypair: &ZaionKeypair,
    idempotency_key: &str,
    now: DateTime<Utc>,
) -> DurableTurnRecord {
    let ingress = ingress_for_keypair(TENANT, "subject-a", idempotency_key, now, keypair);
    store
        .begin_turn(
            &ingress,
            &admission(
                &ingress,
                serde_json::json!({"message": idempotency_key}),
                OWNER_A,
            ),
            now,
        )
        .unwrap()
        .record()
        .clone()
}

fn claim_head(store: &DurableTurnStore, now: DateTime<Utc>, owner: &str) -> TurnOutboxRecord {
    store
        .claim_next_outbox(TENANT, owner, now, Duration::seconds(30))
        .unwrap()
        .unwrap()
}

fn rewrite_outbox_payload(db_path: &Path, outbox_id: &str, mutate: impl FnOnce(&mut Value)) {
    let connection = rusqlite::Connection::open(db_path).unwrap();
    let payload_json: String = connection
        .query_row(
            "SELECT payload_json FROM turn_outbox_v2 WHERE outbox_id = ?1",
            rusqlite::params![outbox_id],
            |row| row.get(0),
        )
        .unwrap();
    let mut payload: Value = serde_json::from_str(&payload_json).unwrap();
    mutate(&mut payload);
    let payload_json = canonical_json(&payload).unwrap();
    let payload_hash = sha256_text(&payload_json);
    connection
        .execute(
            "UPDATE turn_outbox_v2
             SET payload_json = ?2, payload_hash = ?3 WHERE outbox_id = ?1",
            rusqlite::params![outbox_id, payload_json, payload_hash],
        )
        .unwrap();
}

fn drop_order_guards(connection: &rusqlite::Connection) {
    connection
        .execute_batch(&format!(
            "DROP TRIGGER {OUTBOX_ORDER_UPDATE_GUARD};
             DROP TRIGGER {OUTBOX_ORDER_DELETE_GUARD};"
        ))
        .unwrap();
}

fn restore_order_guards(connection: &rusqlite::Connection) {
    connection.execute_batch(CREATE_ORDER_UPDATE_GUARD).unwrap();
    connection.execute_batch(CREATE_ORDER_DELETE_GUARD).unwrap();
}

fn abort_execution(reason: &str) -> TurnExecution {
    TurnExecution::aborted(
        TurnError {
            reason_code: reason.to_string(),
            message: reason.to_string(),
        },
        PartialLedgerTail {
            appended_event_ids: Vec::new(),
            last_safe_parent_event_id: None,
        },
    )
}

fn assert_hash_mismatch(error: TurnStoreError, field: &str) {
    assert!(
        matches!(error, TurnStoreError::HashMismatch { field: actual } if actual == field),
        "expected typed hash mismatch for {field}, got {error:?}"
    );
}

fn assert_binding_mismatch(error: TurnStoreError, field: &str) {
    assert!(
        matches!(error, TurnStoreError::RecordBindingMismatch { field: actual } if actual == field),
        "expected typed row binding mismatch for {field}, got {error:?}"
    );
}

#[test]
fn begin_is_idempotent_and_persists_request_and_authority_hashes() {
    let now = clock();
    let (_directory, store) = store();
    let ingress = ingress(TENANT, "subject-a", "request-0001", now);
    let request = serde_json::json!({"message": "inspect the workspace"});
    let admission = admission(&ingress, request.clone(), OWNER_A);
    let created = store.begin_turn(&ingress, &admission, now).unwrap();
    assert!(created.is_created());
    let record = created.record().clone();
    assert_eq!(record.state, VersionedTurnState::accepted());
    assert_eq!(record.request, request);
    assert!(record.request_hash.starts_with("sha256:"));
    assert!(record.authority_hash.starts_with("sha256:"));
    assert_eq!(
        record.authority["deadline"],
        serde_json::to_value(ingress.deadline()).unwrap()
    );

    let duplicate = store.begin_turn(&ingress, &admission, now).unwrap();
    assert!(matches!(duplicate, BeginTurnResult::Existing(_)));
    assert_eq!(duplicate.record().turn_id, record.turn_id);
    assert_eq!(store.undelivered_outbox(TENANT, 10).unwrap().len(), 1);
    assert!(store.load("tenant-b", &record.turn_id).unwrap().is_none());
    assert!(store.incomplete_turns("tenant-b", 10).unwrap().is_empty());
    assert!(store.undelivered_outbox("tenant-b", 10).unwrap().is_empty());
}

#[test]
fn same_idempotency_key_rejects_changed_request_scope_deadline_and_attachment() {
    let now = clock();
    let (_directory, store) = store();
    let base = ingress(TENANT, "subject-a", "request-0002", now);
    let base_admission = admission(&base, serde_json::json!({"message": "original"}), OWNER_A);
    store.begin_turn(&base, &base_admission, now).unwrap();

    let changed_request = admission(&base, serde_json::json!({"message": "changed"}), "worker-b");
    assert!(matches!(
        store.begin_turn(&base, &changed_request, now),
        Err(TurnStoreError::IdempotencyConflict)
    ));

    let changed_scope = ingress_custom(
        TENANT,
        "subject-a",
        "request-0002",
        now,
        now + Duration::minutes(5),
        vec!["turn:submit".to_string(), "tool:write".to_string()],
        Vec::new(),
    );
    assert!(matches!(
        store.begin_turn(
            &changed_scope,
            &admission(
                &changed_scope,
                serde_json::json!({"message": "original"}),
                "worker-b"
            ),
            now
        ),
        Err(TurnStoreError::IdempotencyConflict)
    ));

    let changed_deadline = ingress_custom(
        TENANT,
        "subject-a",
        "request-0002",
        now,
        now + Duration::minutes(6),
        vec!["turn:submit".to_string(), "tool:read".to_string()],
        Vec::new(),
    );
    assert!(matches!(
        store.begin_turn(
            &changed_deadline,
            &admission(
                &changed_deadline,
                serde_json::json!({"message": "original"}),
                "worker-b"
            ),
            now
        ),
        Err(TurnStoreError::IdempotencyConflict)
    ));

    let changed_attachment = ingress_custom(
        TENANT,
        "subject-a",
        "request-0002",
        now,
        now + Duration::minutes(5),
        vec!["turn:submit".to_string(), "tool:read".to_string()],
        vec![IngressAttachmentInput {
            attachment_id: "attachment-1".to_string(),
            media_type: "text/plain".to_string(),
            byte_len: 5,
            sha256: format!("sha256:{}", "a".repeat(64)),
        }],
    );
    assert!(matches!(
        store.begin_turn(
            &changed_attachment,
            &admission(
                &changed_attachment,
                serde_json::json!({"message": "original"}),
                "worker-b"
            ),
            now
        ),
        Err(TurnStoreError::IdempotencyConflict)
    ));
}

#[test]
fn actor_allows_only_one_active_turn_and_releases_on_terminal_commit() {
    let now = clock();
    let (_directory, store) = store();
    let first_ingress = ingress(TENANT, "subject-a", "request-0003", now);
    let first_admission = admission(
        &first_ingress,
        serde_json::json!({"message": "first"}),
        OWNER_A,
    );
    let first = store
        .begin_turn(&first_ingress, &first_admission, now)
        .unwrap()
        .record()
        .clone();

    let second_ingress = ingress(TENANT, "subject-a", "request-0004", now);
    let second_admission = admission(
        &second_ingress,
        serde_json::json!({"message": "second"}),
        "worker-b",
    );
    assert!(matches!(
        store.begin_turn(&second_ingress, &second_admission, now),
        Err(TurnStoreError::ActorBusy { .. })
    ));

    let aborted = abort_execution("test_abort");
    store
        .compare_and_transition_with_result(
            TENANT,
            &first.turn_id,
            OWNER_A,
            TurnState::Accepted,
            0,
            TurnState::Aborted,
            &aborted,
            now + Duration::seconds(1),
        )
        .unwrap();
    let second = store
        .begin_turn(
            &second_ingress,
            &second_admission,
            now + Duration::seconds(2),
        )
        .unwrap();
    assert!(second.is_created());
    let actor = store
        .load_actor(TENANT, second_admission.actor().actor_key())
        .unwrap()
        .unwrap();
    assert_eq!(
        actor.active_turn_id.as_deref(),
        Some(second.record().turn_id.as_str())
    );
    assert_eq!(actor.lease_owner.as_deref(), Some("worker-b"));
}

#[test]
fn cas_state_terminal_result_actor_and_outbox_commit_together() {
    let now = clock();
    let (_directory, store) = store();
    let ingress = ingress(TENANT, "subject-a", "request-0005", now);
    let admission = admission(&ingress, serde_json::json!({"message": "run"}), OWNER_A);
    let accepted = store
        .begin_turn(&ingress, &admission, now)
        .unwrap()
        .record()
        .clone();
    let routed = store
        .compare_and_transition(
            TENANT,
            &accepted.turn_id,
            OWNER_A,
            TurnState::Accepted,
            0,
            TurnState::Routed,
            now + Duration::seconds(1),
        )
        .unwrap();
    let running = store
        .compare_and_transition(
            TENANT,
            &routed.turn_id,
            OWNER_A,
            TurnState::Routed,
            1,
            TurnState::Running,
            now + Duration::seconds(2),
        )
        .unwrap();
    let mismatched = TurnExecution::handled("not-an-abort");
    assert!(matches!(
        store.compare_and_transition_with_result(
            TENANT,
            &running.turn_id,
            OWNER_A,
            TurnState::Running,
            2,
            TurnState::Aborted,
            &mismatched,
            now + Duration::seconds(3),
        ),
        Err(TurnStoreError::TerminalOutcomeMismatch {
            expected: TurnState::Aborted,
            actual: TurnState::Completed,
        })
    ));
    let result = abort_execution("operator_cancelled");
    let aborted = store
        .compare_and_transition_with_result(
            TENANT,
            &running.turn_id,
            OWNER_A,
            TurnState::Running,
            2,
            TurnState::Aborted,
            &result,
            now + Duration::seconds(3),
        )
        .unwrap();
    assert_eq!(aborted.state.state(), TurnState::Aborted);
    assert_eq!(aborted.state.revision(), 3);
    assert_eq!(
        aborted.terminal_result,
        Some(serde_json::to_value(&result).unwrap())
    );
    assert!(aborted
        .terminal_result_hash
        .as_deref()
        .unwrap()
        .starts_with("sha256:"));
    assert!(store
        .load_actor(TENANT, &aborted.actor_key)
        .unwrap()
        .unwrap()
        .active_turn_id
        .is_none());

    let outbox = store.undelivered_outbox(TENANT, 10).unwrap();
    assert_eq!(outbox.len(), 4);
    assert_eq!(outbox[0].event_type, "turn.state.accepted");
    assert_eq!(outbox[3].event_type, "turn.state.aborted");
    assert_eq!(outbox[3].payload["outbox_id"], outbox[3].outbox_id);
    assert_eq!(outbox[3].payload["revision"], 3);
    assert_eq!(outbox[3].payload["terminal"], true);
    assert_eq!(
        outbox[3].payload["terminal_result_hash"],
        aborted.terminal_result_hash.as_deref().unwrap()
    );
}

#[test]
fn admission_and_transition_failpoints_roll_back_the_whole_transaction() {
    let now = clock();
    for (after_turn, after_outbox) in [(true, false), (false, true)] {
        let (_directory, store) = store();
        let ingress = ingress(TENANT, "subject-a", "request-0006", now);
        let admission = admission(&ingress, serde_json::json!({"message": "run"}), OWNER_A);
        assert!(store
            .begin_turn_with_failpoint(&ingress, &admission, now, after_turn, after_outbox)
            .is_err());
        assert!(store
            .load_actor(TENANT, admission.actor().actor_key())
            .unwrap()
            .is_none());
        assert!(store.incomplete_turns(TENANT, 10).unwrap().is_empty());
        assert!(store.undelivered_outbox(TENANT, 10).unwrap().is_empty());
    }

    for (after_state, after_outbox) in [(true, false), (false, true)] {
        let (_directory, store) = store();
        let ingress = ingress(TENANT, "subject-a", "request-0007", now);
        let admission = admission(&ingress, serde_json::json!({"message": "run"}), OWNER_A);
        let accepted = store
            .begin_turn(&ingress, &admission, now)
            .unwrap()
            .record()
            .clone();
        assert!(store
            .transition_with_failpoint(
                &accepted,
                OWNER_A,
                TurnState::Routed,
                now + Duration::seconds(1),
                after_state,
                after_outbox,
            )
            .is_err());
        assert_eq!(
            store
                .load(TENANT, &accepted.turn_id)
                .unwrap()
                .unwrap()
                .state,
            VersionedTurnState::accepted()
        );
        assert_eq!(store.undelivered_outbox(TENANT, 10).unwrap().len(), 1);
        assert_eq!(
            store
                .load_actor(TENANT, &accepted.actor_key)
                .unwrap()
                .unwrap()
                .revision,
            1
        );
    }
}

#[test]
fn expired_leases_abort_safe_states_and_quarantine_uncertain_states() {
    let now = clock();
    let (_directory, store) = store();
    let safe_ingress = ingress(TENANT, "subject-a", "request-0008", now);
    let safe_admission = admission(
        &safe_ingress,
        serde_json::json!({"message": "safe"}),
        OWNER_A,
    );
    let safe = store
        .begin_turn(&safe_ingress, &safe_admission, now)
        .unwrap()
        .record()
        .clone();
    let recovered = store
        .recover_expired_actor_leases(TENANT, safe.deadline + Duration::milliseconds(1), 10)
        .unwrap();
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].state.state(), TurnState::Aborted);
    let execution: TurnExecution =
        serde_json::from_value(recovered[0].terminal_result.clone().unwrap()).unwrap();
    assert!(matches!(
        execution,
        TurnExecution::Finished { outcome, .. }
            if matches!(outcome.as_ref(), TurnOutcome::Aborted(_, _))
    ));

    let uncertain_ingress = ingress(
        TENANT,
        "subject-a",
        "request-0009",
        now + Duration::seconds(1),
    );
    let uncertain_admission = admission(
        &uncertain_ingress,
        serde_json::json!({"message": "uncertain"}),
        "worker-b",
    );
    let accepted = store
        .begin_turn(
            &uncertain_ingress,
            &uncertain_admission,
            now + Duration::seconds(1),
        )
        .unwrap()
        .record()
        .clone();
    let routed = store
        .compare_and_transition(
            TENANT,
            &accepted.turn_id,
            "worker-b",
            TurnState::Accepted,
            0,
            TurnState::Routed,
            now + Duration::seconds(2),
        )
        .unwrap();
    let running = store
        .compare_and_transition(
            TENANT,
            &routed.turn_id,
            "worker-b",
            TurnState::Routed,
            1,
            TurnState::Running,
            now + Duration::seconds(3),
        )
        .unwrap();
    let recovered = store
        .recover_expired_actor_leases(TENANT, running.deadline + Duration::milliseconds(1), 10)
        .unwrap();
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].state.state(), TurnState::Quarantined);
    let execution: TurnExecution =
        serde_json::from_value(recovered[0].terminal_result.clone().unwrap()).unwrap();
    assert!(matches!(
        execution,
        TurnExecution::Finished { outcome, .. }
            if matches!(outcome.as_ref(), TurnOutcome::Quarantined(_))
    ));
}

#[test]
fn duplicate_admission_recovers_its_expired_actor_before_returning() {
    let now = clock();
    let (_directory, store) = store();
    let ingress = ingress(TENANT, "subject-a", "request-0014", now);
    let admission = admission(
        &ingress,
        serde_json::json!({"message": "retry me"}),
        OWNER_A,
    );
    let accepted = store
        .begin_turn(&ingress, &admission, now)
        .unwrap()
        .record()
        .clone();

    let duplicate = store
        .begin_turn(
            &ingress,
            &admission,
            accepted.deadline + Duration::milliseconds(1),
        )
        .unwrap();
    assert!(matches!(duplicate, BeginTurnResult::Existing(_)));
    assert_eq!(duplicate.record().state.state(), TurnState::Aborted);
    assert!(store
        .load_actor(TENANT, &accepted.actor_key)
        .unwrap()
        .unwrap()
        .active_turn_id
        .is_none());
}

#[test]
fn separate_connections_have_one_cas_winner() {
    let now = clock();
    let directory = tempdir().unwrap();
    let path = directory.path().join("ledger.db");
    let setup = DurableTurnStore::open(&path).unwrap();
    let ingress = ingress(TENANT, "subject-a", "request-0010", now);
    let admission = admission(&ingress, serde_json::json!({"message": "run"}), OWNER_A);
    let accepted = setup
        .begin_turn(&ingress, &admission, now)
        .unwrap()
        .record()
        .clone();
    drop(setup);

    let barrier = Arc::new(Barrier::new(3));
    let mut handles = Vec::new();
    for _ in 0..2 {
        let store = DurableTurnStore::open(&path).unwrap();
        let barrier = Arc::clone(&barrier);
        let accepted = accepted.clone();
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            store.compare_and_transition(
                TENANT,
                &accepted.turn_id,
                OWNER_A,
                TurnState::Accepted,
                0,
                TurnState::Routed,
                now + Duration::seconds(1),
            )
        }));
    }
    barrier.wait();
    let results = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);
    let store = DurableTurnStore::open(&path).unwrap();
    assert_eq!(store.undelivered_outbox(TENANT, 10).unwrap().len(), 2);
}

#[test]
fn outbox_claim_is_atomic_leased_and_idempotently_completed() {
    let now = clock();
    let keypair = ZaionKeypair::generate();
    let directory = tempdir().unwrap();
    let path = directory.path().join("ledger.db");
    let setup = DurableTurnStore::open(&path).unwrap();
    let ingress = ingress_for_keypair(TENANT, "subject-a", "request-0011", now, &keypair);
    let admission = admission(&ingress, serde_json::json!({"message": "run"}), OWNER_A);
    setup.begin_turn(&ingress, &admission, now).unwrap();
    drop(setup);

    let barrier = Arc::new(Barrier::new(3));
    let mut handles = Vec::new();
    for owner in ["dispatcher-a", "dispatcher-b"] {
        let store = DurableTurnStore::open(&path).unwrap();
        let barrier = Arc::clone(&barrier);
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            store.claim_next_outbox(TENANT, owner, now, Duration::seconds(30))
        }));
    }
    barrier.wait();
    let claims = handles
        .into_iter()
        .map(|handle| handle.join().unwrap().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(claims.iter().filter(|claim| claim.is_some()).count(), 1);
    let claim = claims.into_iter().flatten().next().unwrap();
    assert_eq!(claim.status, TurnOutboxStatus::Leased);
    assert_eq!(claim.attempts, 1);
    assert_eq!(claim.idempotency_mode, "key_required");
    let owner = claim.lease_owner.as_deref().unwrap();
    let lease_token = claim.lease_token.as_deref().unwrap();

    let store = DurableTurnStore::open(&path).unwrap();
    let (ledger, commit) = append_claimed_outbox(&store, &claim, &keypair, now);
    assert_eq!(
        store
            .complete_outbox(
                TENANT,
                &claim.outbox_id,
                owner,
                lease_token,
                &commit,
                now + Duration::seconds(1),
                &ledger,
            )
            .unwrap(),
        OutboxCompletion::Delivered
    );
    assert_eq!(
        store
            .complete_outbox(
                TENANT,
                &claim.outbox_id,
                owner,
                lease_token,
                &commit,
                now + Duration::seconds(2),
                &ledger,
            )
            .unwrap(),
        OutboxCompletion::AlreadyDelivered
    );
    assert!(store.undelivered_outbox(TENANT, 10).unwrap().is_empty());
}

#[test]
fn expired_outbox_claim_uses_a_fencing_token_even_for_the_same_owner() {
    let now = clock();
    let (_directory, store) = store();
    let keypair = ZaionKeypair::generate();
    let ingress = ingress_for_keypair(TENANT, "subject-a", "request-0012", now, &keypair);
    let admission = admission(&ingress, serde_json::json!({"message": "run"}), OWNER_A);
    store.begin_turn(&ingress, &admission, now).unwrap();
    assert!(matches!(
        store.claim_next_outbox(TENANT, "dispatcher-a", now, Duration::milliseconds(999)),
        Err(TurnStoreError::InvalidOutboxLeaseDuration)
    ));
    let first = store
        .claim_next_outbox(TENANT, "dispatcher-a", now, Duration::seconds(30))
        .unwrap()
        .unwrap();
    let (ledger, commit) = append_claimed_outbox(&store, &first, &keypair, now);
    assert!(matches!(
        store.complete_outbox(
            TENANT,
            &first.outbox_id,
            "dispatcher-b",
            first.lease_token.as_deref().unwrap(),
            &commit,
            now,
            &ledger,
        ),
        Err(TurnStoreError::OutboxLeaseLost { .. })
    ));
    let reclaimed = store
        .claim_next_outbox(
            TENANT,
            "dispatcher-a",
            now + Duration::seconds(31),
            Duration::seconds(30),
        )
        .unwrap()
        .unwrap();
    assert_eq!(reclaimed.outbox_id, first.outbox_id);
    assert_eq!(reclaimed.attempts, 2);
    assert_ne!(reclaimed.lease_token, first.lease_token);
    assert!(matches!(
        store.complete_outbox(
            TENANT,
            &first.outbox_id,
            "dispatcher-a",
            first.lease_token.as_deref().unwrap(),
            &commit,
            now + Duration::seconds(32),
            &ledger,
        ),
        Err(TurnStoreError::OutboxLeaseLost { .. })
    ));
    assert!(matches!(
        store.release_outbox(
            TENANT,
            &reclaimed.outbox_id,
            "dispatcher-a",
            reclaimed.lease_token.as_deref().unwrap(),
            now + Duration::seconds(32),
            now + Duration::seconds(40),
            &"x".repeat(MAX_OUTBOX_ERROR_BYTES + 1),
        ),
        Err(TurnStoreError::OutboxErrorTooLong)
    ));
    store
        .release_outbox(
            TENANT,
            &reclaimed.outbox_id,
            "dispatcher-a",
            reclaimed.lease_token.as_deref().unwrap(),
            now + Duration::seconds(32),
            now + Duration::seconds(40),
            "ledger temporarily unavailable",
        )
        .unwrap();
    let queued = store.undelivered_outbox(TENANT, 10).unwrap();
    assert_eq!(queued[0].status, TurnOutboxStatus::Pending);
    assert_eq!(
        queued[0].last_error.as_deref(),
        Some("ledger temporarily unavailable")
    );
    assert_eq!(queued[0].updated_at, now + Duration::seconds(32));
    assert_eq!(queued[0].available_at, now + Duration::seconds(40));
}

#[test]
fn cross_turn_claims_follow_commit_order_and_head_backoff_blocks_followers() {
    let now = clock();
    let (_directory, store) = store();
    let keypair = ZaionKeypair::generate();
    let first_ingress =
        ingress_for_keypair(TENANT, "subject-a", "request-order-first", now, &keypair);
    let first = store
        .begin_turn(
            &first_ingress,
            &admission_on_thread(
                &first_ingress,
                serde_json::json!({"message": "first"}),
                OWNER_A,
                "thread-first",
            ),
            now + Duration::seconds(10),
        )
        .unwrap()
        .record()
        .clone();
    let second_ingress =
        ingress_for_keypair(TENANT, "subject-a", "request-order-second", now, &keypair);
    let second = store
        .begin_turn(
            &second_ingress,
            &admission_on_thread(
                &second_ingress,
                serde_json::json!({"message": "second"}),
                "worker-b",
                "thread-second",
            ),
            now,
        )
        .unwrap()
        .record()
        .clone();

    let ordered = store.undelivered_outbox(TENANT, 10).unwrap();
    assert_eq!(ordered.len(), 2);
    assert_eq!(ordered[0].turn_id, first.turn_id);
    assert_eq!(ordered[1].turn_id, second.turn_id);
    assert!(ordered[0].commit_ordinal < ordered[1].commit_ordinal);
    assert!(ordered
        .iter()
        .all(|record| record.order_origin == "transactional"));

    let first_claim = store
        .claim_next_outbox(
            TENANT,
            "dispatcher-a",
            now + Duration::seconds(11),
            Duration::seconds(30),
        )
        .unwrap()
        .unwrap();
    assert_eq!(first_claim.turn_id, first.turn_id);
    assert!(store
        .claim_next_outbox(
            TENANT,
            "dispatcher-b",
            now + Duration::seconds(12),
            Duration::seconds(30),
        )
        .unwrap()
        .is_none());
    store
        .release_outbox(
            TENANT,
            &first_claim.outbox_id,
            "dispatcher-a",
            first_claim.lease_token.as_deref().unwrap(),
            now + Duration::seconds(12),
            now + Duration::seconds(20),
            "retry later",
        )
        .unwrap();
    assert!(store
        .claim_next_outbox(
            TENANT,
            "dispatcher-b",
            now + Duration::seconds(19),
            Duration::seconds(30),
        )
        .unwrap()
        .is_none());
    let reclaimed = store
        .claim_next_outbox(
            TENANT,
            "dispatcher-b",
            now + Duration::seconds(20),
            Duration::seconds(30),
        )
        .unwrap()
        .unwrap();
    assert_eq!(reclaimed.outbox_id, first_claim.outbox_id);
    let (ledger, commit) =
        append_claimed_outbox(&store, &reclaimed, &keypair, now + Duration::seconds(20));
    store
        .complete_outbox(
            TENANT,
            &reclaimed.outbox_id,
            "dispatcher-b",
            reclaimed.lease_token.as_deref().unwrap(),
            &commit,
            now + Duration::seconds(21),
            &ledger,
        )
        .unwrap();
    let next = store
        .claim_next_outbox(
            TENANT,
            "dispatcher-b",
            now + Duration::seconds(21),
            Duration::seconds(30),
        )
        .unwrap()
        .unwrap();
    assert_eq!(next.turn_id, second.turn_id);
}

#[test]
fn completion_rejects_a_leased_record_after_the_tenant_head() {
    let now = clock();
    let (_directory, store) = store();
    let keypair = ZaionKeypair::generate();
    for (key, thread) in [
        ("request-complete-head", "thread-head"),
        ("request-complete-later", "thread-later"),
    ] {
        let ingress = ingress_for_keypair(TENANT, "subject-a", key, now, &keypair);
        store
            .begin_turn(
                &ingress,
                &admission_on_thread(
                    &ingress,
                    serde_json::json!({"message": key}),
                    OWNER_A,
                    thread,
                ),
                now,
            )
            .unwrap();
    }
    let outbox = store.undelivered_outbox(TENANT, 10).unwrap();
    let later = &outbox[1];
    let head = store
        .claim_next_outbox(TENANT, "dispatcher-head", now, Duration::seconds(30))
        .unwrap()
        .unwrap();
    let (ledger, commit) = append_claimed_outbox(&store, &head, &keypair, now);
    rusqlite::Connection::open(store.db_path())
        .unwrap()
        .execute(
            "UPDATE turn_outbox_v2
             SET status = 'leased', lease_owner = 'dispatcher-old',
                 lease_token = 'lease-old', lease_until_ms = ?3, attempts = 1
             WHERE tenant_id = ?1 AND outbox_id = ?2",
            rusqlite::params![
                TENANT,
                later.outbox_id,
                timestamp_millis(now + Duration::seconds(30))
            ],
        )
        .unwrap();
    assert!(matches!(
        store.complete_outbox(
            TENANT,
            &later.outbox_id,
            "dispatcher-old",
            "lease-old",
            &commit,
            now + Duration::seconds(1),
            &ledger,
        ),
        Err(TurnStoreError::OutboxOrderConflict { .. })
    ));
}

#[test]
fn legacy_rows_are_backfilled_by_rowid_once_and_new_writes_use_the_trigger() {
    let now = clock();
    let directory = tempdir().unwrap();
    let path = directory.path().join("ledger.db");
    let store = DurableTurnStore::open(&path).unwrap();
    let legacy_ingress = ingress(TENANT, "subject-a", "request-legacy-order", now);
    let accepted = store
        .begin_turn(
            &legacy_ingress,
            &admission(
                &legacy_ingress,
                serde_json::json!({"message": "legacy"}),
                OWNER_A,
            ),
            now,
        )
        .unwrap()
        .record()
        .clone();
    store
        .compare_and_transition(
            TENANT,
            &accepted.turn_id,
            OWNER_A,
            TurnState::Accepted,
            0,
            TurnState::Routed,
            now + Duration::seconds(1),
        )
        .unwrap();
    drop(store);

    let legacy = rusqlite::Connection::open(&path).unwrap();
    legacy
        .execute_batch(
            "DROP TRIGGER turn_outbox_v2_assign_commit_order;
             DROP INDEX ux_turn_outbox_v2_turn_revision;
             DROP TABLE turn_outbox_commit_order_v2;
             DELETE FROM turn_store_schema_migrations_v2
             WHERE migration_id IN (
                 'turn_outbox_commit_order_v1',
                 'turn_outbox_commit_order_immutability_v1'
             );",
        )
        .unwrap();
    drop(legacy);

    let migrated = DurableTurnStore::open(&path).unwrap();
    let backfilled = migrated.undelivered_outbox(TENANT, 10).unwrap();
    assert_eq!(backfilled.len(), 2);
    assert_eq!(backfilled[0].commit_ordinal, 1);
    assert_eq!(backfilled[1].commit_ordinal, 2);
    assert!(backfilled
        .iter()
        .all(|record| record.order_origin == "legacy_rowid_backfill"));
    drop(migrated);

    let reopened = DurableTurnStore::open(&path).unwrap();
    let stable = reopened.undelivered_outbox(TENANT, 10).unwrap();
    assert_eq!(
        stable
            .iter()
            .map(|record| record.commit_ordinal)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    let next_ingress = ingress(TENANT, "subject-a", "request-after-migration", now);
    reopened
        .begin_turn(
            &next_ingress,
            &admission_on_thread(
                &next_ingress,
                serde_json::json!({"message": "new"}),
                "worker-b",
                "thread-after-migration",
            ),
            now + Duration::seconds(2),
        )
        .unwrap();
    let with_new = reopened.undelivered_outbox(TENANT, 10).unwrap();
    assert_eq!(with_new[2].commit_ordinal, 3);
    assert_eq!(with_new[2].order_origin, "transactional");
}

#[test]
fn concurrent_turn_writers_receive_unique_commit_ordinals() {
    let now = clock();
    let directory = tempdir().unwrap();
    let path = directory.path().join("ledger.db");
    drop(DurableTurnStore::open(&path).unwrap());
    let barrier = Arc::new(Barrier::new(3));
    let mut handles = Vec::new();
    for (key, thread_id) in [
        ("request-writer-a", "thread-writer-a"),
        ("request-writer-b", "thread-writer-b"),
    ] {
        let store = DurableTurnStore::open(&path).unwrap();
        let barrier = Arc::clone(&barrier);
        let ingress = ingress(TENANT, "subject-a", key, now);
        let admission = admission_on_thread(
            &ingress,
            serde_json::json!({"message": key}),
            OWNER_A,
            thread_id,
        );
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            store.begin_turn(&ingress, &admission, now)
        }));
    }
    barrier.wait();
    for handle in handles {
        assert!(handle.join().unwrap().is_ok());
    }
    let store = DurableTurnStore::open(&path).unwrap();
    let ordinals = store
        .undelivered_outbox(TENANT, 10)
        .unwrap()
        .into_iter()
        .map(|record| record.commit_ordinal)
        .collect::<Vec<_>>();
    assert_eq!(ordinals, vec![1, 2]);
}

#[test]
fn reopen_fails_closed_on_partial_or_tampered_order_schema() {
    for mutation in [
        "DELETE FROM turn_store_schema_migrations_v2
         WHERE migration_id = 'turn_outbox_commit_order_v1'",
        "DROP TRIGGER turn_outbox_v2_assign_commit_order;
         CREATE TRIGGER turn_outbox_v2_assign_commit_order
         AFTER INSERT ON turn_outbox_v2 BEGIN SELECT 1; END;",
        "DROP TRIGGER turn_outbox_commit_order_v2_no_delete;
         DELETE FROM turn_outbox_commit_order_v2",
    ] {
        let directory = tempdir().unwrap();
        let path = directory.path().join("ledger.db");
        let store = DurableTurnStore::open(&path).unwrap();
        let ingress = ingress(TENANT, "subject-a", "request-schema-tamper", clock());
        store
            .begin_turn(
                &ingress,
                &admission(&ingress, serde_json::json!({"message": "test"}), OWNER_A),
                clock(),
            )
            .unwrap();
        drop(store);
        rusqlite::Connection::open(&path)
            .unwrap()
            .execute_batch(mutation)
            .unwrap();
        assert!(matches!(
            DurableTurnStore::open(&path),
            Err(TurnStoreError::SchemaIntegrity(_))
        ));
    }
}

#[test]
fn ordinal_attempt_and_lease_time_overflow_fail_without_partial_changes() {
    let now = clock();
    let (_directory, ordinal_store) = store();
    let ordinal_ingress = ingress(TENANT, "subject-a", "request-ordinal-overflow", now);
    let accepted = ordinal_store
        .begin_turn(
            &ordinal_ingress,
            &admission(
                &ordinal_ingress,
                serde_json::json!({"message": "run"}),
                OWNER_A,
            ),
            now,
        )
        .unwrap()
        .record()
        .clone();
    let tamper = rusqlite::Connection::open(ordinal_store.db_path()).unwrap();
    tamper
        .execute(
            "UPDATE sqlite_sequence SET seq = ?2 WHERE name = ?1",
            rusqlite::params![OUTBOX_ORDER_TABLE, i64::MAX],
        )
        .unwrap();
    assert!(matches!(
        ordinal_store.compare_and_transition(
            TENANT,
            &accepted.turn_id,
            OWNER_A,
            TurnState::Accepted,
            0,
            TurnState::Routed,
            now + Duration::seconds(1),
        ),
        Err(TurnStoreError::SchemaIntegrity(_))
    ));
    tamper
        .execute(
            "UPDATE turn_outbox_commit_order_v2 SET commit_ordinal = ?1",
            rusqlite::params![i64::MAX],
        )
        .unwrap_err();
    drop_order_guards(&tamper);
    tamper
        .execute(
            "UPDATE turn_outbox_commit_order_v2 SET commit_ordinal = ?1",
            rusqlite::params![i64::MAX],
        )
        .unwrap();
    restore_order_guards(&tamper);
    assert!(matches!(
        ordinal_store.compare_and_transition(
            TENANT,
            &accepted.turn_id,
            OWNER_A,
            TurnState::Accepted,
            0,
            TurnState::Routed,
            now + Duration::seconds(1),
        ),
        Err(TurnStoreError::CommitOrdinalExhausted)
    ));
    assert_eq!(
        ordinal_store
            .load(TENANT, &accepted.turn_id)
            .unwrap()
            .unwrap()
            .state,
        VersionedTurnState::accepted()
    );
    assert_eq!(
        ordinal_store.undelivered_outbox(TENANT, 10).unwrap().len(),
        1
    );

    let (_directory, attempts_store) = store();
    let attempts_ingress = ingress(TENANT, "subject-a", "request-attempt-overflow", now);
    attempts_store
        .begin_turn(
            &attempts_ingress,
            &admission(
                &attempts_ingress,
                serde_json::json!({"message": "run"}),
                OWNER_A,
            ),
            now,
        )
        .unwrap();
    rusqlite::Connection::open(attempts_store.db_path())
        .unwrap()
        .execute(
            "UPDATE turn_outbox_v2 SET attempts = ?2 WHERE tenant_id = ?1",
            rusqlite::params![TENANT, i64::MAX],
        )
        .unwrap();
    assert!(matches!(
        attempts_store.claim_next_outbox(TENANT, "dispatcher-a", now, Duration::seconds(30)),
        Err(TurnStoreError::OutboxAttemptsExhausted)
    ));

    let (_directory, time_store) = store();
    let time_ingress = ingress(TENANT, "subject-a", "request-time-overflow", now);
    time_store
        .begin_turn(
            &time_ingress,
            &admission(
                &time_ingress,
                serde_json::json!({"message": "run"}),
                OWNER_A,
            ),
            now,
        )
        .unwrap();
    assert!(matches!(
        time_store.claim_next_outbox(
            TENANT,
            "dispatcher-a",
            DateTime::<Utc>::MAX_UTC,
            Duration::seconds(1),
        ),
        Err(TurnStoreError::OutboxLeaseTimeOverflow)
    ));
}

#[test]
fn persisted_request_authority_terminal_and_outbox_hashes_fail_closed_on_tamper() {
    let now = clock();
    for (idempotency_key, column, expected_field) in [
        ("request-0015", "request_json", "request_hash"),
        ("request-0016", "authority_json", "authority_hash"),
    ] {
        let (_directory, store) = store();
        let ingress = ingress(TENANT, "subject-a", idempotency_key, now);
        let admission = admission(
            &ingress,
            serde_json::json!({"message": idempotency_key}),
            OWNER_A,
        );
        let record = store
            .begin_turn(&ingress, &admission, now)
            .unwrap()
            .record()
            .clone();
        let sql = format!(
            "UPDATE turn_state_v2 SET {column} = '{{\"tampered\":true}}'
             WHERE tenant_id = ?1 AND turn_id = ?2"
        );
        rusqlite::Connection::open(store.db_path())
            .unwrap()
            .execute(&sql, rusqlite::params![TENANT, record.turn_id])
            .unwrap();
        assert_hash_mismatch(
            store.load(TENANT, &record.turn_id).unwrap_err(),
            expected_field,
        );
    }

    let (_directory, terminal_store) = store();
    let terminal_ingress = ingress(TENANT, "subject-a", "request-0017", now);
    let terminal_admission = admission(
        &terminal_ingress,
        serde_json::json!({"message": "terminal"}),
        OWNER_A,
    );
    let accepted = terminal_store
        .begin_turn(&terminal_ingress, &terminal_admission, now)
        .unwrap()
        .record()
        .clone();
    let terminal = abort_execution("test_terminal");
    let aborted = terminal_store
        .compare_and_transition_with_result(
            TENANT,
            &accepted.turn_id,
            OWNER_A,
            TurnState::Accepted,
            0,
            TurnState::Aborted,
            &terminal,
            now + Duration::seconds(1),
        )
        .unwrap();
    rusqlite::Connection::open(terminal_store.db_path())
        .unwrap()
        .execute(
            "UPDATE turn_state_v2 SET terminal_result_json = 'null'
             WHERE tenant_id = ?1 AND turn_id = ?2",
            rusqlite::params![TENANT, aborted.turn_id],
        )
        .unwrap();
    assert_hash_mismatch(
        terminal_store.load(TENANT, &aborted.turn_id).unwrap_err(),
        "terminal_result_hash",
    );

    let (_directory, outbox_store) = store();
    let outbox_ingress = ingress(TENANT, "subject-a", "request-0018", now);
    let outbox_admission = admission(
        &outbox_ingress,
        serde_json::json!({"message": "outbox"}),
        OWNER_A,
    );
    outbox_store
        .begin_turn(&outbox_ingress, &outbox_admission, now)
        .unwrap();
    rusqlite::Connection::open(outbox_store.db_path())
        .unwrap()
        .execute(
            "UPDATE turn_outbox_v2 SET payload_json = '{\"tampered\":true}'
             WHERE tenant_id = ?1",
            rusqlite::params![TENANT],
        )
        .unwrap();
    assert_hash_mismatch(
        outbox_store.undelivered_outbox(TENANT, 10).unwrap_err(),
        "outbox_payload_hash",
    );
}

#[test]
fn persisted_authority_columns_and_deterministic_ids_fail_closed_on_tamper() {
    let now = clock();
    for (idempotency_key, mutation, expected_field) in [
        (
            "request-0020",
            "subject_id = 'subject-tampered'",
            "subject_id",
        ),
        (
            "request-0021",
            "principal_id = 'did:key:tampered'",
            "principal_id",
        ),
        (
            "request-0022",
            "workspace_id = 'workspace-tampered'",
            "workspace_id",
        ),
        (
            "request-0023",
            "profile_id = 'profile-tampered'",
            "profile_id",
        ),
        (
            "request-0024",
            "session_id = 'session-tampered'",
            "session_id",
        ),
        (
            "request-0025",
            "source_surface = 'internal-queue'",
            "source_surface",
        ),
        (
            "request-0026",
            "source_id = 'message-tampered'",
            "source_id",
        ),
        (
            "request-0027",
            "idempotency_key = 'request-tampered'",
            "idempotency_key",
        ),
        (
            "request-0028",
            "deadline_ms = deadline_ms + 1000",
            "deadline_ms",
        ),
    ] {
        let (_directory, store) = store();
        let ingress = ingress(TENANT, "subject-a", idempotency_key, now);
        let admission = admission(
            &ingress,
            serde_json::json!({"message": idempotency_key}),
            OWNER_A,
        );
        let record = store
            .begin_turn(&ingress, &admission, now)
            .unwrap()
            .record()
            .clone();
        rusqlite::Connection::open(store.db_path())
            .unwrap()
            .execute(
                &format!(
                    "UPDATE turn_state_v2 SET {mutation} WHERE tenant_id = ?1 AND turn_id = ?2"
                ),
                rusqlite::params![TENANT, record.turn_id],
            )
            .unwrap();
        assert_binding_mismatch(
            store.load(TENANT, &record.turn_id).unwrap_err(),
            expected_field,
        );
    }

    let (_directory, request_store) = store();
    let request_ingress = ingress(TENANT, "subject-a", "request-0029", now);
    let request_admission = admission(
        &request_ingress,
        serde_json::json!({"message": "original"}),
        OWNER_A,
    );
    let record = request_store
        .begin_turn(&request_ingress, &request_admission, now)
        .unwrap()
        .record()
        .clone();
    let tampered_request = "{\"message\":\"tampered\"}";
    let tampered_hash = sha256_text(tampered_request);
    rusqlite::Connection::open(request_store.db_path())
        .unwrap()
        .execute(
            "UPDATE turn_state_v2 SET request_json = ?3, request_hash = ?4
             WHERE tenant_id = ?1 AND turn_id = ?2",
            rusqlite::params![TENANT, record.turn_id, tampered_request, tampered_hash],
        )
        .unwrap();
    assert_binding_mismatch(
        request_store.load(TENANT, &record.turn_id).unwrap_err(),
        "turn_id",
    );

    let (_directory, outbox_store) = store();
    let outbox_ingress = ingress(TENANT, "subject-a", "request-0030", now);
    let outbox_admission = admission(
        &outbox_ingress,
        serde_json::json!({"message": "outbox binding"}),
        OWNER_A,
    );
    outbox_store
        .begin_turn(&outbox_ingress, &outbox_admission, now)
        .unwrap();
    rusqlite::Connection::open(outbox_store.db_path())
        .unwrap()
        .execute(
            "UPDATE turn_outbox_v2 SET event_type = 'turn.running' WHERE tenant_id = ?1",
            rusqlite::params![TENANT],
        )
        .unwrap();
    assert_binding_mismatch(
        outbox_store.undelivered_outbox(TENANT, 10).unwrap_err(),
        "outbox.event_type",
    );
}

#[test]
fn signing_revalidation_rejects_rehashed_payload_identity_and_policy_tampering() {
    let now = clock();
    let cases = [
        ("/actor_key", "tampered-actor", "outbox.actor_key"),
        ("/subject_id", "tampered-subject", "outbox.subject_id"),
        ("/principal_id", "did:key:tampered", "outbox.principal_id"),
        ("/workspace_id", "tampered-workspace", "outbox.workspace_id"),
        ("/profile_id", "tampered-profile", "outbox.profile_id"),
        ("/session_id", "tampered-session", "outbox.session_id"),
        (
            "/source/surface",
            "tampered-surface",
            "outbox.source_surface",
        ),
        ("/source/source_id", "tampered-source", "outbox.source_id"),
        (
            "/idempotency_key",
            "tampered-idempotency",
            "outbox.idempotency_key",
        ),
        (
            "/request_hash",
            "sha256:tampered-request",
            "outbox.request_hash",
        ),
        (
            "/authority_hash",
            "sha256:tampered-authority",
            "outbox.authority_hash",
        ),
        (
            "/occurred_at",
            "2026-07-15T18:00:00+08:00",
            "outbox.occurred_at",
        ),
    ];

    for (index, (pointer, replacement, expected_field)) in cases.into_iter().enumerate() {
        let directory = tempdir().unwrap();
        let store = DurableTurnStore::open(directory.path().join("ledger.db")).unwrap();
        let keypair = ZaionKeypair::generate();
        begin_keyed_turn(&store, &keypair, &format!("request-rehashed-{index}"), now);
        let claim = claim_head(&store, now, "dispatcher-rehashed");
        rewrite_outbox_payload(store.db_path(), &claim.outbox_id, |payload| {
            *payload.pointer_mut(pointer).unwrap() = Value::String(replacement.to_string());
        });
        let ledger = EventLedger::new(store.db_path());
        assert_binding_mismatch(
            store
                .revalidate_outbox_for_signing(
                    TENANT,
                    &claim.outbox_id,
                    "dispatcher-rehashed",
                    claim.lease_token.as_deref().unwrap(),
                    now,
                    &ledger,
                    &keypair.public_key_bytes(),
                )
                .unwrap_err(),
            expected_field,
        );
    }

    let directory = tempdir().unwrap();
    let store = DurableTurnStore::open(directory.path().join("extra-field.db")).unwrap();
    let keypair = ZaionKeypair::generate();
    begin_keyed_turn(&store, &keypair, "request-extra-field", now);
    let claim = claim_head(&store, now, "dispatcher-extra-field");
    rewrite_outbox_payload(store.db_path(), &claim.outbox_id, |payload| {
        payload["approval_granted"] = Value::Bool(true);
    });
    let ledger = EventLedger::new(store.db_path());
    assert_binding_mismatch(
        store
            .revalidate_outbox_for_signing(
                TENANT,
                &claim.outbox_id,
                "dispatcher-extra-field",
                claim.lease_token.as_deref().unwrap(),
                now,
                &ledger,
                &keypair.public_key_bytes(),
            )
            .unwrap_err(),
        "outbox.payload_shape",
    );

    for (index, (column, expected_field)) in [
        ("effect_kind", "outbox.effect_kind"),
        ("idempotency_mode", "outbox.idempotency_mode"),
    ]
    .into_iter()
    .enumerate()
    {
        let directory = tempdir().unwrap();
        let store = DurableTurnStore::open(directory.path().join("ledger.db")).unwrap();
        let keypair = ZaionKeypair::generate();
        begin_keyed_turn(&store, &keypair, &format!("request-policy-{index}"), now);
        let claim = claim_head(&store, now, "dispatcher-policy");
        let connection = rusqlite::Connection::open(store.db_path()).unwrap();
        connection
            .execute_batch("PRAGMA ignore_check_constraints=ON")
            .unwrap();
        connection
            .execute(
                &format!("UPDATE turn_outbox_v2 SET {column} = 'tampered' WHERE outbox_id = ?1"),
                rusqlite::params![claim.outbox_id],
            )
            .unwrap();
        let ledger = EventLedger::new(store.db_path());
        assert_binding_mismatch(
            store
                .revalidate_outbox_for_signing(
                    TENANT,
                    &claim.outbox_id,
                    "dispatcher-policy",
                    claim.lease_token.as_deref().unwrap(),
                    now,
                    &ledger,
                    &keypair.public_key_bytes(),
                )
                .unwrap_err(),
            expected_field,
        );
    }
}

#[test]
fn signing_revalidation_rejects_history_gaps_illegal_transitions_and_stale_current_state() {
    let now = clock();

    let directory = tempdir().unwrap();
    let gap_store = DurableTurnStore::open(directory.path().join("gap.db")).unwrap();
    let gap_keypair = ZaionKeypair::generate();
    let accepted = begin_keyed_turn(&gap_store, &gap_keypair, "request-history-gap", now);
    let routed = gap_store
        .compare_and_transition(
            TENANT,
            &accepted.turn_id,
            OWNER_A,
            TurnState::Accepted,
            0,
            TurnState::Routed,
            now + Duration::seconds(1),
        )
        .unwrap();
    gap_store
        .compare_and_transition(
            TENANT,
            &accepted.turn_id,
            OWNER_A,
            TurnState::Routed,
            1,
            TurnState::Running,
            now + Duration::seconds(2),
        )
        .unwrap();
    let claim = claim_head(&gap_store, now + Duration::seconds(3), "dispatcher-gap");
    let connection = rusqlite::Connection::open(gap_store.db_path()).unwrap();
    connection.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
    drop_order_guards(&connection);
    let middle_id: String = connection
        .query_row(
            "SELECT outbox_id FROM turn_outbox_v2
             WHERE tenant_id = ?1 AND turn_id = ?2 AND revision = 1",
            rusqlite::params![TENANT, routed.turn_id],
            |row| row.get(0),
        )
        .unwrap();
    connection
        .execute(
            "DELETE FROM turn_outbox_commit_order_v2 WHERE outbox_id = ?1",
            rusqlite::params![middle_id],
        )
        .unwrap();
    connection
        .execute(
            "DELETE FROM turn_outbox_v2 WHERE outbox_id = ?1",
            rusqlite::params![middle_id],
        )
        .unwrap();
    restore_order_guards(&connection);
    let ledger = EventLedger::new(gap_store.db_path());
    assert!(matches!(
        gap_store.revalidate_outbox_for_signing(
            TENANT,
            &claim.outbox_id,
            "dispatcher-gap",
            claim.lease_token.as_deref().unwrap(),
            now + Duration::seconds(3),
            &ledger,
            &gap_keypair.public_key_bytes(),
        ),
        Err(TurnStoreError::OutboxHistoryIncomplete { .. })
    ));

    let directory = tempdir().unwrap();
    let illegal_store = DurableTurnStore::open(directory.path().join("illegal.db")).unwrap();
    let illegal_keypair = ZaionKeypair::generate();
    let accepted = begin_keyed_turn(
        &illegal_store,
        &illegal_keypair,
        "request-illegal-transition",
        now,
    );
    illegal_store
        .compare_and_transition(
            TENANT,
            &accepted.turn_id,
            OWNER_A,
            TurnState::Accepted,
            0,
            TurnState::Routed,
            now + Duration::seconds(1),
        )
        .unwrap();
    illegal_store
        .compare_and_transition(
            TENANT,
            &accepted.turn_id,
            OWNER_A,
            TurnState::Routed,
            1,
            TurnState::Running,
            now + Duration::seconds(2),
        )
        .unwrap();
    let claim = claim_head(
        &illegal_store,
        now + Duration::seconds(3),
        "dispatcher-illegal",
    );
    let connection = rusqlite::Connection::open(illegal_store.db_path()).unwrap();
    connection.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
    drop_order_guards(&connection);
    let (old_id, payload_json): (String, String) = connection
        .query_row(
            "SELECT outbox_id, payload_json FROM turn_outbox_v2
             WHERE tenant_id = ?1 AND turn_id = ?2 AND revision = 2",
            rusqlite::params![TENANT, accepted.turn_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let event_type = "turn.state.accepted";
    let new_id = deterministic_outbox_id(TENANT, &accepted.turn_id, 2, event_type);
    let mut payload: Value = serde_json::from_str(&payload_json).unwrap();
    payload["outbox_id"] = Value::String(new_id.clone());
    payload["state"] = Value::String("accepted".to_string());
    let payload_json = canonical_json(&payload).unwrap();
    let payload_hash = sha256_text(&payload_json);
    connection
        .execute(
            "UPDATE turn_outbox_v2
             SET outbox_id = ?2, event_type = ?3, payload_json = ?4, payload_hash = ?5
             WHERE outbox_id = ?1",
            rusqlite::params![old_id, new_id, event_type, payload_json, payload_hash],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE turn_outbox_commit_order_v2 SET outbox_id = ?2 WHERE outbox_id = ?1",
            rusqlite::params![old_id, new_id],
        )
        .unwrap();
    restore_order_guards(&connection);
    let ledger = EventLedger::new(illegal_store.db_path());
    assert!(matches!(
        illegal_store.revalidate_outbox_for_signing(
            TENANT,
            &claim.outbox_id,
            "dispatcher-illegal",
            claim.lease_token.as_deref().unwrap(),
            now + Duration::seconds(3),
            &ledger,
            &illegal_keypair.public_key_bytes(),
        ),
        Err(TurnStoreError::OutboxHistoryIllegalTransition {
            from: TurnState::Routed,
            to: TurnState::Accepted,
            ..
        })
    ));

    let directory = tempdir().unwrap();
    let stale_store = DurableTurnStore::open(directory.path().join("stale.db")).unwrap();
    let stale_keypair = ZaionKeypair::generate();
    let accepted = begin_keyed_turn(&stale_store, &stale_keypair, "request-stale-current", now);
    stale_store
        .compare_and_transition(
            TENANT,
            &accepted.turn_id,
            OWNER_A,
            TurnState::Accepted,
            0,
            TurnState::Routed,
            now + Duration::seconds(1),
        )
        .unwrap();
    let claim = claim_head(&stale_store, now + Duration::seconds(2), "dispatcher-stale");
    rusqlite::Connection::open(stale_store.db_path())
        .unwrap()
        .execute(
            "UPDATE turn_state_v2 SET state = 'running'
             WHERE tenant_id = ?1 AND turn_id = ?2",
            rusqlite::params![TENANT, accepted.turn_id],
        )
        .unwrap();
    let ledger = EventLedger::new(stale_store.db_path());
    assert!(matches!(
        stale_store.revalidate_outbox_for_signing(
            TENANT,
            &claim.outbox_id,
            "dispatcher-stale",
            claim.lease_token.as_deref().unwrap(),
            now + Duration::seconds(2),
            &ledger,
            &stale_keypair.public_key_bytes(),
        ),
        Err(TurnStoreError::OutboxHistoryCurrentTurn { .. })
    ));
}

#[test]
fn signing_requires_the_verified_delivered_parent_event() {
    let now = clock();
    let (_directory, store) = store();
    let keypair = ZaionKeypair::generate();
    let accepted = begin_keyed_turn(&store, &keypair, "request-parent-required", now);
    store
        .compare_and_transition(
            TENANT,
            &accepted.turn_id,
            OWNER_A,
            TurnState::Accepted,
            0,
            TurnState::Routed,
            now + Duration::seconds(1),
        )
        .unwrap();
    let first = claim_head(&store, now + Duration::seconds(2), "dispatcher-parent");
    let (ledger, first_commit) =
        append_claimed_outbox(&store, &first, &keypair, now + Duration::seconds(2));
    assert_eq!(
        store
            .complete_outbox(
                TENANT,
                &first.outbox_id,
                "dispatcher-parent",
                first.lease_token.as_deref().unwrap(),
                &first_commit,
                now + Duration::seconds(3),
                &ledger,
            )
            .unwrap(),
        OutboxCompletion::Delivered
    );
    let second = claim_head(&store, now + Duration::seconds(3), "dispatcher-parent");
    let validated = store
        .revalidate_outbox_for_signing(
            TENANT,
            &second.outbox_id,
            "dispatcher-parent",
            second.lease_token.as_deref().unwrap(),
            now + Duration::seconds(3),
            &ledger,
            &keypair.public_key_bytes(),
        )
        .unwrap();
    assert_eq!(
        validated
            .binding()
            .parent_event_id()
            .map(|event| event.0.as_str()),
        Some(first_commit.event_id())
    );
    drop(ledger);
    rusqlite::Connection::open(store.db_path())
        .unwrap()
        .execute(
            "DELETE FROM events WHERE event_id = ?1",
            rusqlite::params![first_commit.event_id()],
        )
        .unwrap();
    let ledger = EventLedger::new(store.db_path());
    assert!(matches!(
        store.revalidate_outbox_for_signing(
            TENANT,
            &second.outbox_id,
            "dispatcher-parent",
            second.lease_token.as_deref().unwrap(),
            now + Duration::seconds(3),
            &ledger,
            &keypair.public_key_bytes(),
        ),
        Err(TurnStoreError::Ledger(zaion_ledger::LedgerError::NotFound(
            _
        )))
    ));
}

#[test]
fn append_crash_reclaim_is_idempotent_and_completes_once() {
    let now = clock();
    let (_directory, store) = store();
    let keypair = ZaionKeypair::generate();
    begin_keyed_turn(&store, &keypair, "request-append-crash", now);
    let first = claim_head(&store, now, "dispatcher-crash");
    let (ledger, first_commit) = append_claimed_outbox(&store, &first, &keypair, now);

    let reclaimed = claim_head(&store, now + Duration::seconds(31), "dispatcher-reclaim");
    assert_ne!(first.lease_token, reclaimed.lease_token);
    let validated = store
        .revalidate_outbox_for_signing(
            TENANT,
            &reclaimed.outbox_id,
            "dispatcher-reclaim",
            reclaimed.lease_token.as_deref().unwrap(),
            now + Duration::seconds(31),
            &ledger,
            &keypair.public_key_bytes(),
        )
        .unwrap();
    let retry_commit = ledger
        .append_verified_idempotent_event(&keypair, validated.binding())
        .unwrap();
    assert_eq!(first_commit, retry_commit);
    assert_eq!(
        store
            .complete_outbox(
                TENANT,
                &reclaimed.outbox_id,
                "dispatcher-reclaim",
                reclaimed.lease_token.as_deref().unwrap(),
                &retry_commit,
                now + Duration::seconds(32),
                &ledger,
            )
            .unwrap(),
        OutboxCompletion::Delivered
    );
    assert_eq!(
        store
            .complete_outbox(
                TENANT,
                &reclaimed.outbox_id,
                "dispatcher-reclaim",
                reclaimed.lease_token.as_deref().unwrap(),
                &retry_commit,
                now + Duration::seconds(33),
                &ledger,
            )
            .unwrap(),
        OutboxCompletion::AlreadyDelivered
    );
    assert_eq!(
        ledger
            .list_principal_events(&keypair.principal_id(), 10)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn legacy_state_event_type_remains_exact_across_append_crash_retry() {
    let now = clock();
    let (_directory, store) = store();
    let keypair = ZaionKeypair::generate();
    begin_keyed_turn(&store, &keypair, "request-legacy-type", now);
    let original = claim_head(&store, now, "dispatcher-legacy-type");
    let old_event_type = "turn.accepted";
    let old_outbox_id =
        deterministic_outbox_id(TENANT, &original.turn_id, original.revision, old_event_type);
    let connection = rusqlite::Connection::open(store.db_path()).unwrap();
    connection.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
    drop_order_guards(&connection);
    let mut payload = original.payload.clone();
    payload["outbox_id"] = Value::String(old_outbox_id.clone());
    let payload_json = canonical_json(&payload).unwrap();
    let payload_hash = sha256_text(&payload_json);
    connection
        .execute(
            "UPDATE turn_outbox_v2
             SET outbox_id = ?2, event_type = ?3, payload_json = ?4, payload_hash = ?5
             WHERE outbox_id = ?1",
            rusqlite::params![
                original.outbox_id,
                old_outbox_id,
                old_event_type,
                payload_json,
                payload_hash
            ],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE turn_outbox_commit_order_v2 SET outbox_id = ?2 WHERE outbox_id = ?1",
            rusqlite::params![original.outbox_id, old_outbox_id],
        )
        .unwrap();
    restore_order_guards(&connection);

    let legacy = store.undelivered_outbox(TENANT, 10).unwrap()[0].clone();
    let ledger = EventLedger::new(store.db_path());
    let validated = store
        .revalidate_outbox_for_signing(
            TENANT,
            &legacy.outbox_id,
            "dispatcher-legacy-type",
            legacy.lease_token.as_deref().unwrap(),
            now,
            &ledger,
            &keypair.public_key_bytes(),
        )
        .unwrap();
    assert_eq!(validated.binding().event_type(), old_event_type);
    let first_commit = ledger
        .append_verified_idempotent_event(&keypair, validated.binding())
        .unwrap();
    let reclaimed = claim_head(
        &store,
        now + Duration::seconds(31),
        "dispatcher-legacy-retry",
    );
    let validated = store
        .revalidate_outbox_for_signing(
            TENANT,
            &reclaimed.outbox_id,
            "dispatcher-legacy-retry",
            reclaimed.lease_token.as_deref().unwrap(),
            now + Duration::seconds(31),
            &ledger,
            &keypair.public_key_bytes(),
        )
        .unwrap();
    assert_eq!(validated.binding().event_type(), old_event_type);
    let retry_commit = ledger
        .append_verified_idempotent_event(&keypair, validated.binding())
        .unwrap();
    assert_eq!(first_commit, retry_commit);
    assert_eq!(
        store
            .complete_outbox(
                TENANT,
                &reclaimed.outbox_id,
                "dispatcher-legacy-retry",
                reclaimed.lease_token.as_deref().unwrap(),
                &retry_commit,
                now + Duration::seconds(32),
                &ledger,
            )
            .unwrap(),
        OutboxCompletion::Delivered
    );
}

#[test]
fn concurrent_verified_completion_has_one_delivery_and_one_idempotent_replay() {
    let now = clock();
    let directory = tempdir().unwrap();
    let path = directory.path().join("concurrent-complete.db");
    let setup = DurableTurnStore::open(&path).unwrap();
    let keypair = ZaionKeypair::generate();
    begin_keyed_turn(&setup, &keypair, "request-concurrent-complete", now);
    let claim = claim_head(&setup, now, "dispatcher-concurrent");
    let (ledger, commit) = append_claimed_outbox(&setup, &claim, &keypair, now);
    drop(setup);

    let ledger = Arc::new(ledger);
    let barrier = Arc::new(Barrier::new(3));
    let mut handles = Vec::new();
    for _ in 0..2 {
        let store = DurableTurnStore::open(&path).unwrap();
        let ledger = Arc::clone(&ledger);
        let barrier = Arc::clone(&barrier);
        let commit = commit.clone();
        let claim = claim.clone();
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            store.complete_outbox(
                TENANT,
                &claim.outbox_id,
                "dispatcher-concurrent",
                claim.lease_token.as_deref().unwrap(),
                &commit,
                now + Duration::seconds(1),
                &ledger,
            )
        }));
    }
    barrier.wait();
    let results = handles
        .into_iter()
        .map(|handle| handle.join().unwrap().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        results
            .iter()
            .filter(|result| **result == OutboxCompletion::Delivered)
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|result| **result == OutboxCompletion::AlreadyDelivered)
            .count(),
        1
    );
}

#[test]
fn verified_completion_rejects_wrong_binding_path_and_tampered_signature() {
    let now = clock();
    for case in [
        "namespace",
        "run",
        "event_type",
        "payload",
        "parent",
        "principal",
        "idempotency_key",
    ] {
        let directory = tempdir().unwrap();
        let store = DurableTurnStore::open(directory.path().join(format!("{case}.db"))).unwrap();
        let keypair = ZaionKeypair::generate();
        begin_keyed_turn(&store, &keypair, &format!("request-wrong-{case}"), now);
        let claim = claim_head(&store, now, "dispatcher-wrong");
        let ledger = EventLedger::new(store.db_path());
        let correct = store
            .revalidate_outbox_for_signing(
                TENANT,
                &claim.outbox_id,
                "dispatcher-wrong",
                claim.lease_token.as_deref().unwrap(),
                now,
                &ledger,
                &keypair.public_key_bytes(),
            )
            .unwrap();
        let alternate = ZaionKeypair::generate();
        let signer = if case == "principal" {
            &alternate
        } else {
            &keypair
        };
        let binding = IdempotentEventBinding::new(
            if case == "idempotency_key" {
                "outbox-wrong-key".to_string()
            } else {
                correct.binding().idempotency_key().to_string()
            },
            if case == "principal" {
                alternate.principal_id()
            } else {
                correct.binding().principal_id().clone()
            },
            if case == "namespace" {
                NamespaceKey("session-wrong".to_string())
            } else {
                correct.binding().namespace_key().clone()
            },
            if case == "run" {
                Some(RunId("turn-wrong".to_string()))
            } else {
                correct.binding().run_id().cloned()
            },
            if case == "event_type" {
                "turn.state.running"
            } else {
                correct.binding().event_type()
            },
            if case == "payload" {
                serde_json::json!({"tampered": true})
            } else {
                correct.binding().payload().clone()
            },
            if case == "parent" {
                Some(EventId("evt-parent-wrong".to_string()))
            } else {
                correct.binding().parent_event_id().cloned()
            },
        )
        .unwrap();
        let wrong_commit = ledger
            .append_verified_idempotent_event(signer, &binding)
            .unwrap();
        let error = store
            .complete_outbox(
                TENANT,
                &claim.outbox_id,
                "dispatcher-wrong",
                claim.lease_token.as_deref().unwrap(),
                &wrong_commit,
                now + Duration::seconds(1),
                &ledger,
            )
            .unwrap_err();
        if case == "principal" {
            assert!(matches!(error, TurnStoreError::OutboxPrincipalMismatch));
        } else {
            assert!(matches!(error, TurnStoreError::OutboxCommitMismatch { .. }));
        }
    }

    let directory = tempdir().unwrap();
    let store = DurableTurnStore::open(directory.path().join("path.db")).unwrap();
    let keypair = ZaionKeypair::generate();
    begin_keyed_turn(&store, &keypair, "request-wrong-path", now);
    let claim = claim_head(&store, now, "dispatcher-path");
    let store_ledger = EventLedger::new(store.db_path());
    let validated = store
        .revalidate_outbox_for_signing(
            TENANT,
            &claim.outbox_id,
            "dispatcher-path",
            claim.lease_token.as_deref().unwrap(),
            now,
            &store_ledger,
            &keypair.public_key_bytes(),
        )
        .unwrap();
    let other_ledger = EventLedger::new(directory.path().join("other.db"));
    let wrong_path_commit = other_ledger
        .append_verified_idempotent_event(&keypair, validated.binding())
        .unwrap();
    assert!(matches!(
        store.complete_outbox(
            TENANT,
            &claim.outbox_id,
            "dispatcher-path",
            claim.lease_token.as_deref().unwrap(),
            &wrong_path_commit,
            now + Duration::seconds(1),
            &other_ledger,
        ),
        Err(TurnStoreError::OutboxLedgerPathMismatch)
    ));

    let directory = tempdir().unwrap();
    let store = DurableTurnStore::open(directory.path().join("signature.db")).unwrap();
    let keypair = ZaionKeypair::generate();
    begin_keyed_turn(&store, &keypair, "request-bad-signature", now);
    let claim = claim_head(&store, now, "dispatcher-signature");
    let (ledger, commit) = append_claimed_outbox(&store, &claim, &keypair, now);
    rusqlite::Connection::open(store.db_path())
        .unwrap()
        .execute(
            "UPDATE events SET signature_hex = ?2 WHERE event_id = ?1",
            rusqlite::params![commit.event_id(), "00".repeat(64)],
        )
        .unwrap();
    assert!(matches!(
        store.complete_outbox(
            TENANT,
            &claim.outbox_id,
            "dispatcher-signature",
            claim.lease_token.as_deref().unwrap(),
            &commit,
            now + Duration::seconds(1),
            &ledger,
        ),
        Err(TurnStoreError::Ledger(
            zaion_ledger::LedgerError::EventBindingSignatureInvalid
        ))
    ));
    assert_eq!(
        store.undelivered_outbox(TENANT, 10).unwrap()[0].status,
        TurnOutboxStatus::Leased
    );
}

#[test]
fn expired_lease_cannot_sign_complete_or_release() {
    let now = clock();
    let (_directory, store) = store();
    let keypair = ZaionKeypair::generate();
    begin_keyed_turn(&store, &keypair, "request-expired-dispatch", now);
    let claim = claim_head(&store, now, "dispatcher-expired");
    let (ledger, commit) = append_claimed_outbox(&store, &claim, &keypair, now);
    let expired_at = now + Duration::seconds(30);
    assert!(matches!(
        store.revalidate_outbox_for_signing(
            TENANT,
            &claim.outbox_id,
            "dispatcher-expired",
            claim.lease_token.as_deref().unwrap(),
            expired_at,
            &ledger,
            &keypair.public_key_bytes(),
        ),
        Err(TurnStoreError::OutboxLeaseExpired { .. })
    ));
    assert!(matches!(
        store.complete_outbox(
            TENANT,
            &claim.outbox_id,
            "dispatcher-expired",
            claim.lease_token.as_deref().unwrap(),
            &commit,
            expired_at,
            &ledger,
        ),
        Err(TurnStoreError::OutboxLeaseExpired { .. })
    ));
    assert!(matches!(
        store.release_outbox(
            TENANT,
            &claim.outbox_id,
            "dispatcher-expired",
            claim.lease_token.as_deref().unwrap(),
            expired_at,
            expired_at + Duration::seconds(5),
            "too late",
        ),
        Err(TurnStoreError::OutboxLeaseExpired { .. })
    ));
}

#[test]
fn verified_completion_rejects_live_trigger_injection_before_update() {
    let now = clock();
    let (_directory, store) = store();
    let keypair = ZaionKeypair::generate();
    begin_keyed_turn(&store, &keypair, "request-trigger-injection", now);
    let claim = claim_head(&store, now, "dispatcher-trigger");
    let (ledger, commit) = append_claimed_outbox(&store, &claim, &keypair, now);
    let connection = rusqlite::Connection::open(store.db_path()).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER injected_outbox_update
             AFTER UPDATE ON turn_outbox_v2
             BEGIN
                 UPDATE turn_outbox_v2 SET ledger_event_id = 'evt-injected'
                 WHERE outbox_id = NEW.outbox_id;
             END;",
        )
        .unwrap();
    assert!(matches!(
        store.complete_outbox(
            TENANT,
            &claim.outbox_id,
            "dispatcher-trigger",
            claim.lease_token.as_deref().unwrap(),
            &commit,
            now + Duration::seconds(1),
            &ledger,
        ),
        Err(TurnStoreError::SchemaIntegrity(_))
    ));
    assert_eq!(
        store.undelivered_outbox(TENANT, 10).unwrap()[0].status,
        TurnOutboxStatus::Leased
    );
}

#[test]
fn verified_commit_evidence_is_atomic_persistent_and_immutable() {
    let now = clock();
    let (directory, store) = store();
    let keypair = ZaionKeypair::generate();
    begin_keyed_turn(&store, &keypair, "request-evidence-atomic", now);
    let claim = claim_head(&store, now, "dispatcher-evidence");
    let (ledger, commit) = append_claimed_outbox(&store, &claim, &keypair, now);

    assert!(matches!(
        store.complete_outbox_with_evidence_failpoint(
            TENANT,
            &claim.outbox_id,
            "dispatcher-evidence",
            claim.lease_token.as_deref().unwrap(),
            &commit,
            now + Duration::seconds(1),
            &ledger,
        ),
        Err(TurnStoreError::InjectedAfterVerifiedCommitEvidence)
    ));
    let connection = rusqlite::Connection::open(store.db_path()).unwrap();
    let evidence_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM turn_outbox_verified_commit_v2 WHERE outbox_id = ?1",
            rusqlite::params![claim.outbox_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(evidence_count, 0);
    assert_eq!(
        store.undelivered_outbox(TENANT, 10).unwrap()[0].status,
        TurnOutboxStatus::Leased
    );

    assert_eq!(
        store
            .complete_outbox(
                TENANT,
                &claim.outbox_id,
                "dispatcher-evidence",
                claim.lease_token.as_deref().unwrap(),
                &commit,
                now + Duration::seconds(1),
                &ledger,
            )
            .unwrap(),
        OutboxCompletion::Delivered
    );
    let stored: (String, Vec<u8>, String) = connection
        .query_row(
            "SELECT ledger_event_id, signer_public_key, database_instance_id
             FROM turn_outbox_verified_commit_v2 WHERE outbox_id = ?1",
            rusqlite::params![claim.outbox_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(stored.0, commit.event_id());
    assert_eq!(stored.1, keypair.public_key_bytes().0);
    assert_eq!(stored.2, commit.database_instance_id());
    assert!(connection
        .execute(
            "UPDATE turn_outbox_verified_commit_v2 SET signer_public_key = ?2
             WHERE outbox_id = ?1",
            rusqlite::params![claim.outbox_id, vec![9_u8; 32]],
        )
        .is_err());
    assert!(connection
        .execute(
            "DELETE FROM turn_outbox_verified_commit_v2 WHERE outbox_id = ?1",
            rusqlite::params![claim.outbox_id],
        )
        .is_err());
    assert!(connection
        .execute(
            "UPDATE turn_outbox_v2 SET updated_at_ms = updated_at_ms + 1
             WHERE outbox_id = ?1",
            rusqlite::params![claim.outbox_id],
        )
        .is_err());

    drop(connection);
    drop(store);
    let reopened = DurableTurnStore::open(directory.path().join("ledger.db")).unwrap();
    let ingress = ingress_for_keypair(
        TENANT,
        "subject-a",
        "request-evidence-reopen",
        now + Duration::seconds(2),
        &keypair,
    );
    reopened
        .begin_turn(
            &ingress,
            &admission_on_thread(
                &ingress,
                serde_json::json!({"message": "reopen"}),
                OWNER_A,
                "thread-evidence-reopen",
            ),
            now + Duration::seconds(2),
        )
        .unwrap();
    let next = claim_head(
        &reopened,
        now + Duration::seconds(2),
        "dispatcher-evidence-reopen",
    );
    reopened
        .revalidate_outbox_for_signing(
            TENANT,
            &next.outbox_id,
            "dispatcher-evidence-reopen",
            next.lease_token.as_deref().unwrap(),
            now + Duration::seconds(2),
            &EventLedger::new(reopened.db_path()),
            &keypair.public_key_bytes(),
        )
        .unwrap();
}

#[test]
fn tenant_prefix_uses_each_delivered_principals_own_signer_key() {
    let now = clock();
    let (_directory, store) = store();
    let first_keypair = ZaionKeypair::generate();
    let second_keypair = ZaionKeypair::generate();
    for (key, thread, keypair) in [
        ("request-principal-a", "thread-principal-a", &first_keypair),
        ("request-principal-b", "thread-principal-b", &second_keypair),
    ] {
        let ingress = ingress_for_keypair(TENANT, "subject-a", key, now, keypair);
        store
            .begin_turn(
                &ingress,
                &admission_on_thread(
                    &ingress,
                    serde_json::json!({"message": key}),
                    OWNER_A,
                    thread,
                ),
                now,
            )
            .unwrap();
    }

    let first = claim_head(&store, now, "dispatcher-multi-principal");
    let (ledger, first_commit) = append_claimed_outbox(&store, &first, &first_keypair, now);
    store
        .complete_outbox(
            TENANT,
            &first.outbox_id,
            "dispatcher-multi-principal",
            first.lease_token.as_deref().unwrap(),
            &first_commit,
            now + Duration::seconds(1),
            &ledger,
        )
        .unwrap();

    let second = claim_head(
        &store,
        now + Duration::seconds(1),
        "dispatcher-multi-principal",
    );
    let validated = store
        .revalidate_outbox_for_signing(
            TENANT,
            &second.outbox_id,
            "dispatcher-multi-principal",
            second.lease_token.as_deref().unwrap(),
            now + Duration::seconds(1),
            &ledger,
            &second_keypair.public_key_bytes(),
        )
        .unwrap();
    assert_eq!(
        validated.binding().principal_id(),
        &second_keypair.principal_id()
    );
}

#[test]
fn missing_signer_evidence_blocks_followers_and_sealed_commit_repairs_it() {
    let now = clock();
    let (_directory, store) = store();
    let keypair = ZaionKeypair::generate();
    for (key, thread) in [
        ("request-repair-first", "thread-repair-first"),
        ("request-repair-second", "thread-repair-second"),
    ] {
        let ingress = ingress_for_keypair(TENANT, "subject-a", key, now, &keypair);
        store
            .begin_turn(
                &ingress,
                &admission_on_thread(
                    &ingress,
                    serde_json::json!({"message": key}),
                    OWNER_A,
                    thread,
                ),
                now,
            )
            .unwrap();
    }
    let first = claim_head(&store, now, "dispatcher-repair");
    let (ledger, first_commit) = append_claimed_outbox(&store, &first, &keypair, now);
    store
        .complete_outbox(
            TENANT,
            &first.outbox_id,
            "dispatcher-repair",
            first.lease_token.as_deref().unwrap(),
            &first_commit,
            now + Duration::seconds(1),
            &ledger,
        )
        .unwrap();
    let second = claim_head(&store, now + Duration::seconds(1), "dispatcher-repair");

    let connection = rusqlite::Connection::open(store.db_path()).unwrap();
    connection
        .execute_batch(&format!(
            "DROP TRIGGER {OUTBOX_VERIFIED_COMMIT_DELETE_GUARD};"
        ))
        .unwrap();
    connection
        .execute(
            "DELETE FROM turn_outbox_verified_commit_v2 WHERE outbox_id = ?1",
            rusqlite::params![first.outbox_id],
        )
        .unwrap();
    connection
        .execute_batch(CREATE_VERIFIED_COMMIT_DELETE_GUARD)
        .unwrap();
    assert!(matches!(
        store.revalidate_outbox_for_signing(
            TENANT,
            &second.outbox_id,
            "dispatcher-repair",
            second.lease_token.as_deref().unwrap(),
            now + Duration::seconds(1),
            &ledger,
            &keypair.public_key_bytes(),
        ),
        Err(TurnStoreError::OutboxSignerEvidenceMissing { .. })
    ));

    assert_eq!(
        store
            .complete_outbox(
                TENANT,
                &first.outbox_id,
                "repair-evidence",
                "repair-evidence-token",
                &first_commit,
                now + Duration::seconds(1),
                &ledger,
            )
            .unwrap(),
        OutboxCompletion::AlreadyDelivered
    );
    store
        .revalidate_outbox_for_signing(
            TENANT,
            &second.outbox_id,
            "dispatcher-repair",
            second.lease_token.as_deref().unwrap(),
            now + Duration::seconds(1),
            &ledger,
            &keypair.public_key_bytes(),
        )
        .unwrap();

    let alternate = ZaionKeypair::generate();
    connection
        .execute_batch(&format!(
            "DROP TRIGGER {OUTBOX_VERIFIED_COMMIT_UPDATE_GUARD};"
        ))
        .unwrap();
    connection
        .execute(
            "UPDATE turn_outbox_verified_commit_v2 SET signer_public_key = ?2
             WHERE outbox_id = ?1",
            rusqlite::params![first.outbox_id, alternate.public_key_bytes().0],
        )
        .unwrap();
    connection
        .execute_batch(CREATE_VERIFIED_COMMIT_UPDATE_GUARD)
        .unwrap();
    assert!(matches!(
        store.revalidate_outbox_for_signing(
            TENANT,
            &second.outbox_id,
            "dispatcher-repair",
            second.lease_token.as_deref().unwrap(),
            now + Duration::seconds(1),
            &ledger,
            &keypair.public_key_bytes(),
        ),
        Err(TurnStoreError::OutboxPrincipalMismatch)
    ));
}

#[test]
fn verified_commit_schema_migrates_once_and_rejects_partial_or_bypassed_state() {
    let now = clock();
    let directory = tempdir().unwrap();
    let path = directory.path().join("verified-commit-migration.db");
    let store = DurableTurnStore::open(&path).unwrap();
    let ingress = ingress(TENANT, "subject-a", "request-verified-migration", now);
    store
        .begin_turn(
            &ingress,
            &admission(&ingress, serde_json::json!({"message": "legacy"}), OWNER_A),
            now,
        )
        .unwrap();
    drop(store);
    let legacy = rusqlite::Connection::open(&path).unwrap();
    legacy
        .execute_batch(&format!(
            "DROP TRIGGER {OUTBOX_VERIFIED_DELIVERY_GUARD};
             DROP TABLE {OUTBOX_VERIFIED_COMMIT_TABLE};
             DELETE FROM turn_store_schema_migrations_v2
             WHERE migration_id = '{OUTBOX_VERIFIED_COMMIT_MIGRATION_ID}';"
        ))
        .unwrap();
    drop(legacy);
    let migrated = DurableTurnStore::open(&path).unwrap();
    assert_eq!(migrated.undelivered_outbox(TENANT, 10).unwrap().len(), 1);
    drop(migrated);
    DurableTurnStore::open(&path).unwrap();

    for mutation in [
        format!(
            "DELETE FROM turn_store_schema_migrations_v2
             WHERE migration_id = '{OUTBOX_VERIFIED_COMMIT_MIGRATION_ID}'"
        ),
        format!("DROP TRIGGER {OUTBOX_VERIFIED_COMMIT_DELETE_GUARD}"),
        format!(
            "DROP TRIGGER {OUTBOX_VERIFIED_DELIVERY_GUARD};
             CREATE TRIGGER {OUTBOX_VERIFIED_DELIVERY_GUARD}
             BEFORE UPDATE ON turn_outbox_v2 BEGIN SELECT 1; END;"
        ),
    ] {
        let directory = tempdir().unwrap();
        let path = directory.path().join("verified-schema-tamper.db");
        drop(DurableTurnStore::open(&path).unwrap());
        rusqlite::Connection::open(&path)
            .unwrap()
            .execute_batch(&mutation)
            .unwrap();
        assert!(matches!(
            DurableTurnStore::open(&path),
            Err(TurnStoreError::SchemaIntegrity(_))
        ));
    }

    let directory = tempdir().unwrap();
    let path = directory.path().join("verified-delivery-guard.db");
    let store = DurableTurnStore::open(&path).unwrap();
    let keypair = ZaionKeypair::generate();
    begin_keyed_turn(&store, &keypair, "request-delivery-guard", now);
    let claim = claim_head(&store, now, "dispatcher-delivery-guard");
    let connection = rusqlite::Connection::open(&path).unwrap();
    assert!(connection
        .execute(
            "UPDATE turn_outbox_v2
             SET status = 'delivered', delivered_at_ms = ?2,
                 ledger_event_id = 'evt-unverified', lease_owner = NULL,
                 lease_token = NULL, lease_until_ms = NULL
             WHERE outbox_id = ?1",
            rusqlite::params![claim.outbox_id, timestamp_millis(now)],
        )
        .is_err());
    assert!(connection
        .execute(
            "INSERT INTO turn_outbox_verified_commit_v2 (
                tenant_id, outbox_id, ledger_event_id, signer_public_key,
                database_instance_id
             ) VALUES (?1, ?2, 'evt-malformed', ?3, ?4)",
            rusqlite::params![
                TENANT,
                claim.outbox_id,
                vec![1_u8; 31],
                EventLedger::new(&path).database_instance_id().unwrap(),
            ],
        )
        .is_err());
}

#[test]
fn unsigned_delivered_head_cannot_authorize_the_next_turn() {
    let now = clock();
    let (_directory, store) = store();
    let keypair = ZaionKeypair::generate();
    for (key, thread) in [
        ("request-prefix-first", "thread-prefix-first"),
        ("request-prefix-second", "thread-prefix-second"),
    ] {
        let ingress = ingress_for_keypair(TENANT, "subject-a", key, now, &keypair);
        store
            .begin_turn(
                &ingress,
                &admission_on_thread(
                    &ingress,
                    serde_json::json!({"message": key}),
                    OWNER_A,
                    thread,
                ),
                now,
            )
            .unwrap();
    }
    let outbox = store.undelivered_outbox(TENANT, 10).unwrap();
    let first = claim_head(&store, now, "dispatcher-forged-prefix");
    assert_eq!(first.outbox_id, outbox[0].outbox_id);
    let first_binding = IdempotentEventBinding::new(
        first.outbox_id.clone(),
        keypair.principal_id(),
        NamespaceKey("session-test".to_string()),
        Some(RunId(first.turn_id.clone())),
        first.event_type.clone(),
        first.payload.clone(),
        None,
    )
    .unwrap();
    let ledger = EventLedger::new(store.db_path());
    let connection = rusqlite::Connection::open(store.db_path()).unwrap();
    connection
        .execute(
            "INSERT INTO turn_outbox_verified_commit_v2 (
                tenant_id, outbox_id, ledger_event_id, signer_public_key,
                database_instance_id
             ) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                TENANT,
                first.outbox_id,
                first_binding.expected_event_id().0,
                keypair.public_key_bytes().0,
                ledger.database_instance_id().unwrap(),
            ],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE turn_outbox_v2
             SET status = 'delivered', delivered_at_ms = ?2,
                 ledger_event_id = ?3, updated_at_ms = ?2,
                 lease_owner = NULL, lease_token = NULL, lease_until_ms = NULL
             WHERE outbox_id = ?1",
            rusqlite::params![
                first.outbox_id,
                timestamp_millis(now),
                first_binding.expected_event_id().0
            ],
        )
        .unwrap();
    let second = claim_head(&store, now, "dispatcher-prefix");
    assert_eq!(second.outbox_id, outbox[1].outbox_id);
    assert!(matches!(
        store.revalidate_outbox_for_signing(
            TENANT,
            &second.outbox_id,
            "dispatcher-prefix",
            second.lease_token.as_deref().unwrap(),
            now,
            &ledger,
            &keypair.public_key_bytes(),
        ),
        Err(TurnStoreError::Ledger(zaion_ledger::LedgerError::NotFound(
            _
        )))
    ));
}

#[test]
fn order_integrity_fails_closed_for_live_holes_zero_and_reordering() {
    let now = clock();

    let directory = tempdir().unwrap();
    let path = directory.path().join("delivered-hole.db");
    let store = DurableTurnStore::open(&path).unwrap();
    for (key, thread) in [
        ("request-hole-first", "thread-hole-first"),
        ("request-hole-second", "thread-hole-second"),
    ] {
        let ingress = ingress(TENANT, "subject-a", key, now);
        store
            .begin_turn(
                &ingress,
                &admission_on_thread(
                    &ingress,
                    serde_json::json!({"message": key}),
                    OWNER_A,
                    thread,
                ),
                now,
            )
            .unwrap();
    }
    let later = store.undelivered_outbox(TENANT, 10).unwrap()[1].clone();
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE turn_outbox_v2
             SET status = 'leased', lease_owner = 'tamper-hole',
                 lease_token = 'tamper-hole-token', lease_until_ms = ?2
             WHERE outbox_id = ?1",
            rusqlite::params![
                later.outbox_id,
                timestamp_millis(now + Duration::seconds(30)),
            ],
        )
        .unwrap();
    let ledger = EventLedger::new(&path);
    connection
        .execute(
            "INSERT INTO turn_outbox_verified_commit_v2 (
                tenant_id, outbox_id, ledger_event_id, signer_public_key,
                database_instance_id
             ) VALUES (?1, ?2, 'evt-fake-delivered', ?3, ?4)",
            rusqlite::params![
                TENANT,
                later.outbox_id,
                vec![7_u8; 32],
                ledger.database_instance_id().unwrap(),
            ],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE turn_outbox_v2
             SET status = 'delivered', delivered_at_ms = ?2,
                 ledger_event_id = 'evt-fake-delivered',
                 lease_owner = NULL, lease_token = NULL, lease_until_ms = NULL
             WHERE outbox_id = ?1",
            rusqlite::params![later.outbox_id, timestamp_millis(now)],
        )
        .unwrap();
    assert!(matches!(
        store.undelivered_outbox(TENANT, 10),
        Err(TurnStoreError::SchemaIntegrity(_))
    ));
    drop(store);
    assert!(matches!(
        DurableTurnStore::open(&path),
        Err(TurnStoreError::SchemaIntegrity(_))
    ));

    let directory = tempdir().unwrap();
    let path = directory.path().join("missing-mapping.db");
    let store = DurableTurnStore::open(&path).unwrap();
    let missing_ingress = ingress(TENANT, "subject-a", "request-missing-map", now);
    store
        .begin_turn(
            &missing_ingress,
            &admission(
                &missing_ingress,
                serde_json::json!({"message": "map"}),
                OWNER_A,
            ),
            now,
        )
        .unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    drop_order_guards(&connection);
    connection
        .execute("DELETE FROM turn_outbox_commit_order_v2", [])
        .unwrap();
    assert!(matches!(
        store.undelivered_outbox(TENANT, 10),
        Err(TurnStoreError::SchemaIntegrity(_))
    ));

    for mutation in [
        "UPDATE turn_outbox_commit_order_v2 SET commit_ordinal = 0
         WHERE commit_ordinal = 1",
        "UPDATE turn_outbox_commit_order_v2
         SET commit_ordinal = CASE commit_ordinal WHEN 1 THEN -1 WHEN 2 THEN 1 END;
         UPDATE turn_outbox_commit_order_v2 SET commit_ordinal = 2 WHERE commit_ordinal = -1",
    ] {
        let directory = tempdir().unwrap();
        let path = directory.path().join("order-tamper.db");
        let store = DurableTurnStore::open(&path).unwrap();
        for (key, thread) in [
            ("request-order-a", "thread-order-a"),
            ("request-order-b", "thread-order-b"),
        ] {
            let ingress = ingress(TENANT, "subject-a", key, now);
            store
                .begin_turn(
                    &ingress,
                    &admission_on_thread(
                        &ingress,
                        serde_json::json!({"message": key}),
                        OWNER_A,
                        thread,
                    ),
                    now,
                )
                .unwrap();
        }
        let connection = rusqlite::Connection::open(&path).unwrap();
        assert!(connection.execute_batch(mutation).is_err());
        drop_order_guards(&connection);
        connection.execute_batch(mutation).unwrap();
        drop(connection);
        drop(store);
        assert!(matches!(
            DurableTurnStore::open(&path),
            Err(TurnStoreError::SchemaIntegrity(_))
        ));
    }
}

#[test]
fn actor_key_is_stable_for_same_scope_and_changes_with_thread() {
    let now = clock();
    let ingress = ingress(TENANT, "subject-a", "request-0013", now);
    let first = TurnActorIdentity::for_ingress(&ingress, "terminal", "thread-main").unwrap();
    let same = TurnActorIdentity::for_ingress(&ingress, "terminal", "thread-main").unwrap();
    let other = TurnActorIdentity::for_ingress(&ingress, "terminal", "thread-other").unwrap();
    assert_eq!(first.actor_key(), same.actor_key());
    assert_ne!(first.actor_key(), other.actor_key());
}
