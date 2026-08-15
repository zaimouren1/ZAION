//! End-to-end verification of the Ed25519-signed audit trail.
//!
//! The core Zaion vision is that *every* critical operation is auditable via
//! an Ed25519 signature on an append-only, hash-chained ledger. The MCP
//! dispatcher is the guaranteed-append path: every `dispatch` writes two
//! signed events — `mcp.tool_called` and `tool.receipt`.
//!
//! These tests drive real dispatches through the public `McpDispatcher` API,
//! read the audit trail back via `dispatcher.ledger()`, and assert that:
//!   1. The expected signed events were appended.
//!   2. Each event's Ed25519 signature verifies against the dispatcher's
//!      public key (`verify_event_signature`).
//!   3. The hash chain is intact (`verify_chain` reports no `broken_at`).
//!
//! This closes the gap where signature/chain integrity was tested only at the
//! ledger unit level, never end-to-end through the engine that produces them.

use serde_json::json;
use zaion_crypto::keypair::ZaionKeypair;
use zaion_ledger::{verify_event_signature, EventLedger, EventSignatureMode};
use zaion_mcp::{McpCall, McpDispatcher, McpToolRegistry};
use zaion_types::session::{NamespaceKey, SessionKey};

/// Build a dispatcher backed by an in-memory ledger and a fresh keypair.
fn dispatcher(ns: &str) -> McpDispatcher {
    let registry = McpToolRegistry::new();
    let ledger = EventLedger::new(":memory:");
    let keypair = ZaionKeypair::generate();
    let ns_key = NamespaceKey(ns.to_string());
    // `new` registers all built-in tools internally.
    McpDispatcher::new(registry, ledger, keypair, ns_key)
}

#[test]
fn dispatch_appends_two_signed_audit_events() {
    let ns = "audit-two-events";
    let mut d = dispatcher(ns);

    let result = d.dispatch(McpCall::new("time_now", json!({})));
    assert!(result.success, "time_now should succeed");

    // Both audit event types must be present for this single dispatch.
    let session = SessionKey(ns.to_string());
    let called = d
        .ledger()
        .list_events(&session, Some("mcp.tool_called"), 10)
        .expect("list mcp.tool_called");
    let receipts = d
        .ledger()
        .list_events(&session, Some("tool.receipt"), 10)
        .expect("list tool.receipt");

    assert_eq!(called.len(), 1, "exactly one mcp.tool_called event");
    assert_eq!(receipts.len(), 1, "exactly one tool.receipt event");
    assert_eq!(called[0].payload["tool_name"], json!("time_now"));
    assert_eq!(receipts[0].payload["tool_name"], json!("time_now"));
    assert_eq!(receipts[0].payload["success"], json!(true));
}

#[test]
fn audit_events_carry_verifiable_signatures() {
    let ns = "audit-signatures";
    let mut d = dispatcher(ns);

    d.dispatch(McpCall::new(
        "hash_text",
        json!({ "text": "the quick brown fox" }),
    ));

    let public_key = d.public_key_bytes();
    let session = SessionKey(ns.to_string());
    let events = d
        .ledger()
        .list_events(&session, None, 10)
        .expect("list all audit events");

    assert!(!events.is_empty(), "dispatch must have appended events");

    for event in &events {
        let mode = verify_event_signature(&public_key, event)
            .unwrap_or_else(|e| panic!("signature must verify for {}: {e}", event.event_type));
        // Fresh dispatcher events use the canonical envelope signing scheme.
        assert_eq!(
            mode,
            EventSignatureMode::CanonicalEnvelope,
            "event '{}' should use canonical-envelope signing",
            event.event_type
        );
    }
}

#[test]
fn tampered_payload_fails_signature_verification() {
    let ns = "audit-tamper";
    let mut d = dispatcher(ns);

    d.dispatch(McpCall::new("uuid_generate", json!({})));

    let public_key = d.public_key_bytes();
    let session = SessionKey(ns.to_string());
    let mut event = d
        .ledger()
        .list_events(&session, Some("tool.receipt"), 1)
        .expect("list receipt")
        .into_iter()
        .next()
        .expect("one receipt event");

    // A pristine event verifies.
    verify_event_signature(&public_key, &event).expect("pristine event verifies");

    // Mutating the payload must break verification — the signature covers it.
    event.payload["tool_name"] = json!("forged_tool");
    let tampered = verify_event_signature(&public_key, &event);
    assert!(
        tampered.is_err(),
        "tampered payload must fail signature verification, got: {tampered:?}"
    );
}

#[test]
fn wrong_key_fails_signature_verification() {
    let ns = "audit-wrong-key";
    let mut d = dispatcher(ns);

    d.dispatch(McpCall::new("uuid_generate", json!({})));

    let session = SessionKey(ns.to_string());
    let event = d
        .ledger()
        .list_events(&session, None, 1)
        .expect("list event")
        .into_iter()
        .next()
        .expect("one event");

    // A different keypair's public key must not verify this dispatcher's events.
    let attacker = ZaionKeypair::generate();
    let result = verify_event_signature(&attacker.public_key_bytes(), &event);
    assert!(
        result.is_err(),
        "foreign key must fail verification, got: {result:?}"
    );
}

#[test]
fn hash_chain_stays_intact_across_many_dispatches() {
    let ns = "audit-chain";
    let mut d = dispatcher(ns);

    // Drive a mix of successful and failing dispatches; both append audit events.
    d.dispatch(McpCall::new("time_now", json!({})));
    d.dispatch(McpCall::new("uuid_generate", json!({})));
    d.dispatch(McpCall::new("hash_text", json!({ "text": "abc" })));
    d.dispatch(McpCall::new("does_not_exist", json!({}))); // failing dispatch still audits

    let principal = d.principal_id();
    let report = d
        .ledger()
        .verify_chain(&principal)
        .expect("verify_chain succeeds");

    // 4 dispatches × 2 events each = 8 chained events, all verified, no break.
    assert_eq!(report.total, 8, "4 dispatches append 8 audit events");
    assert_eq!(report.verified, report.total, "every link must verify");
    assert_eq!(report.broken_at, None, "chain must be unbroken");
}

#[test]
fn failed_dispatch_is_still_audited_with_signature() {
    let ns = "audit-failure";
    let mut d = dispatcher(ns);

    let result = d.dispatch(McpCall::new("does_not_exist", json!({})));
    assert!(!result.success, "unknown tool must fail");

    let public_key = d.public_key_bytes();
    let session = SessionKey(ns.to_string());
    let receipts = d
        .ledger()
        .list_events(&session, Some("tool.receipt"), 10)
        .expect("list receipts");

    assert_eq!(receipts.len(), 1, "failure is still audited");
    let receipt = &receipts[0];
    assert_eq!(receipt.payload["success"], json!(false));
    assert_eq!(receipt.payload["receipt_status"], json!("failed"));
    // Even a failure receipt must carry a verifiable signature.
    verify_event_signature(&public_key, receipt).expect("failure receipt is signed");
}
