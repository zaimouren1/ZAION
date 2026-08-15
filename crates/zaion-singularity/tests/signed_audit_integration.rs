//! End-to-end verification of the Ed25519-signed audit trail through the
//! unified SingularityRuntime (Systems I–V).
//!
//! The core Zaion vision is that critical operations are auditable via an
//! Ed25519 signature on an append-only, hash-chained ledger. System II
//! (Autonomic) appends an `autonomic.reflex_triggered` event whenever a
//! registered reflex fires. These tests:
//!   1. Register a reflex via the runtime's public `register_reflex`.
//!   2. Fire it through `check_reflexes`, producing a signed ledger event.
//!   3. Read the event back from the ledger `Arc` the test still holds.
//!   4. Verify the Ed25519 signature (`verify_event_signature`) and the
//!      hash chain (`verify_chain`).
//!
//! This closes the gap where the signed cross-system audit trail had no
//! end-to-end coverage through the orchestrating runtime.

use std::sync::Arc;

use serde_json::json;
use tempfile::TempDir;
use zaion_autonomic::{AutonomicReflex, ReflexAction, ReflexTrigger};
use zaion_crypto::keypair::ZaionKeypair;
use zaion_ledger::{verify_event_signature, EventLedger, EventSignatureMode};
use zaion_singularity::SingularityRuntime;
use zaion_types::session::SessionKey;
use zaion_types::NamespaceKey;

/// Build a runtime, retaining the ledger `Arc`, the signing keypair, and the
/// namespace so the test can read and verify the audit trail end-to-end.
struct Harness {
    runtime: SingularityRuntime,
    ledger: Arc<EventLedger>,
    keypair: Arc<ZaionKeypair>,
    namespace: String,
    _temp: TempDir,
}

fn harness(namespace: &str) -> Harness {
    let temp = TempDir::new().unwrap();
    let keypair = Arc::new(ZaionKeypair::generate());
    let ns_key = NamespaceKey(namespace.to_string());
    let ledger = Arc::new(EventLedger::new(temp.path().join("ledger.db")));
    ledger.ensure().unwrap();

    let runtime = SingularityRuntime::new(
        temp.path(),
        Arc::clone(&ledger),
        Arc::clone(&keypair),
        ns_key,
    )
    .unwrap();

    Harness {
        runtime,
        ledger,
        keypair,
        namespace: namespace.to_string(),
        _temp: temp,
    }
}

/// A reflex that fires whenever its trigger type is seen and the value clears
/// the threshold.
fn test_reflex(id: &str, trigger_type: &str, threshold: f64) -> AutonomicReflex {
    AutonomicReflex {
        id: id.to_string(),
        name: format!("reflex-{id}"),
        trigger: ReflexTrigger {
            trigger_type: trigger_type.to_string(),
            pattern: None,
            threshold: Some(threshold),
        },
        action: ReflexAction {
            action_type: "log_event".to_string(),
            parameters: json!({ "note": "audit-test" }),
        },
        enabled: true,
    }
}

#[tokio::test]
async fn reflex_fire_appends_verifiable_signed_event() {
    let mut h = harness("singularity-audit-fire");
    h.runtime
        .register_reflex(test_reflex("r1", "memory_pressure", 0.8));
    assert_eq!(h.runtime.reflex_count(), 1);

    // Fire the reflex: value 0.9 >= threshold 0.8.
    let actions = h
        .runtime
        .check_reflexes("memory_pressure", 0.9)
        .await
        .unwrap();
    assert_eq!(actions, vec!["log_event".to_string()]);

    // Read the audited event back from the ledger the test still holds.
    let session = SessionKey(h.namespace.clone());
    let events = h
        .ledger
        .list_events(&session, Some("autonomic.reflex_triggered"), 10)
        .expect("list reflex events");
    assert_eq!(events.len(), 1, "one reflex fire -> one audited event");

    let event = &events[0];
    assert_eq!(event.payload["trigger_type"], json!("memory_pressure"));
    assert_eq!(event.payload["value"], json!(0.9));
    assert_eq!(event.payload["action"], json!("log_event"));

    // The event must carry a signature that verifies against the runtime's key.
    let mode = verify_event_signature(&h.keypair.public_key_bytes(), event)
        .expect("reflex event signature must verify");
    assert_eq!(mode, EventSignatureMode::CanonicalEnvelope);
}

#[tokio::test]
async fn reflex_below_threshold_appends_nothing() {
    let mut h = harness("singularity-audit-nofire");
    h.runtime
        .register_reflex(test_reflex("r1", "memory_pressure", 0.8));

    // Value below threshold: must not fire, must not audit.
    let actions = h
        .runtime
        .check_reflexes("memory_pressure", 0.5)
        .await
        .unwrap();
    assert!(actions.is_empty(), "below-threshold reflex must not fire");

    let session = SessionKey(h.namespace.clone());
    let events = h
        .ledger
        .list_events(&session, Some("autonomic.reflex_triggered"), 10)
        .expect("list reflex events");
    assert!(events.is_empty(), "no fire -> no audit event");
}

#[tokio::test]
async fn tampered_reflex_event_fails_verification() {
    let mut h = harness("singularity-audit-tamper");
    h.runtime.register_reflex(test_reflex("r1", "idle", 0.0));
    h.runtime.check_reflexes("idle", 1.0).await.unwrap();

    let session = SessionKey(h.namespace.clone());
    let mut event = h
        .ledger
        .list_events(&session, Some("autonomic.reflex_triggered"), 1)
        .expect("list event")
        .into_iter()
        .next()
        .expect("one event");

    let public_key = h.keypair.public_key_bytes();
    verify_event_signature(&public_key, &event).expect("pristine event verifies");

    // Mutating the audited payload must break the signature.
    event.payload["action"] = json!("exfiltrate");
    assert!(
        verify_event_signature(&public_key, &event).is_err(),
        "tampered reflex payload must fail verification"
    );
}

#[tokio::test]
async fn audit_chain_intact_across_multiple_reflex_fires() {
    let mut h = harness("singularity-audit-chain");
    h.runtime.register_reflex(test_reflex("r1", "tick", 0.0));

    // Fire the reflex several times; each fire appends one chained event.
    for _ in 0..5 {
        let actions = h.runtime.check_reflexes("tick", 1.0).await.unwrap();
        assert_eq!(actions.len(), 1);
    }

    let report = h
        .ledger
        .verify_chain(&h.keypair.principal_id())
        .expect("verify_chain succeeds");
    assert_eq!(report.total, 5, "5 reflex fires -> 5 chained events");
    assert_eq!(report.verified, report.total, "every link verifies");
    assert_eq!(report.broken_at, None, "chain must be unbroken");
}

#[tokio::test]
async fn multiple_reflexes_each_audited() {
    let mut h = harness("singularity-audit-multi");
    // Two reflexes on the same trigger both fire for one check.
    h.runtime.register_reflex(test_reflex("r1", "spike", 0.5));
    h.runtime.register_reflex(test_reflex("r2", "spike", 0.5));
    assert_eq!(h.runtime.reflex_count(), 2);

    let actions = h.runtime.check_reflexes("spike", 0.9).await.unwrap();
    assert_eq!(actions.len(), 2, "both reflexes fire");

    let session = SessionKey(h.namespace.clone());
    let events = h
        .ledger
        .list_events(&session, Some("autonomic.reflex_triggered"), 10)
        .expect("list events");
    assert_eq!(events.len(), 2, "each fire is individually audited");

    let public_key = h.keypair.public_key_bytes();
    for event in &events {
        verify_event_signature(&public_key, event)
            .expect("every reflex event must carry a verifiable signature");
    }
}
