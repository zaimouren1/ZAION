//! Integration-level tests for zaion-types.
//!
//! The types crate sits at the base of the workspace layer graph —
//! every ledger row, every memory namespace, every secret scope derives
//! from `MemoryNamespace::namespace_key` / `session_key` and every event
//! serializes `EventType::as_str`. A regression here silently
//! contaminates data across the entire system, so we treat these
//! invariants as immutable.

use zaion_types::envelope::{compute_source_hash, ingest, CanonicalEnvelope};
use zaion_types::event::EventType;
use zaion_types::identity::PrincipalId;
use zaion_types::session::{
    ChannelId, MemoryNamespace, ProjectId, RunId, SessionId, StyleLock, ThreadId, WorkspaceId,
};

// ── helpers ─────────────────────────────────────────────────────────────────

fn ns(
    principal: &str,
    workspace: &str,
    project: &str,
    channel: &str,
    thread: &str,
    session: &str,
) -> MemoryNamespace {
    MemoryNamespace {
        principal_id: PrincipalId(principal.to_string()),
        workspace_id: WorkspaceId(workspace.to_string()),
        project_id: ProjectId(project.to_string()),
        channel_id: ChannelId(channel.to_string()),
        thread_id: ThreadId(thread.to_string()),
        session_id: SessionId(session.to_string()),
        run_id: None,
        style_lock: StyleLock::default(),
    }
}

// ── namespace_key + session_key invariants ──────────────────────────────────

#[test]
fn namespace_key_is_stable_for_identical_inputs() {
    let a = ns("pid", "ws", "proj", "ch", "thr", "sess").namespace_key();
    let b = ns("pid", "ws", "proj", "ch", "thr", "sess").namespace_key();
    assert_eq!(
        a.0, b.0,
        "namespace_key must be a pure function of its inputs"
    );
}

#[test]
fn namespace_key_differs_when_any_field_changes() {
    let base = ns("pid", "ws", "proj", "ch", "thr", "sess").namespace_key();
    let cases = [
        ns("PID", "ws", "proj", "ch", "thr", "sess").namespace_key(),
        ns("pid", "WS", "proj", "ch", "thr", "sess").namespace_key(),
        ns("pid", "ws", "PROJ", "ch", "thr", "sess").namespace_key(),
        ns("pid", "ws", "proj", "CH", "thr", "sess").namespace_key(),
        ns("pid", "ws", "proj", "ch", "THR", "sess").namespace_key(),
    ];
    for (i, k) in cases.iter().enumerate() {
        assert_ne!(base.0, k.0, "case {} must differ from base", i);
    }
}

#[test]
fn namespace_key_session_id_is_ignored() {
    // session_id is intentionally *not* part of the namespace key —
    // it's the session_key that binds it. Regression-proofs that design.
    let a = ns("pid", "ws", "proj", "ch", "thr", "sess-A").namespace_key();
    let b = ns("pid", "ws", "proj", "ch", "thr", "sess-B").namespace_key();
    assert_eq!(a.0, b.0);
}

#[test]
fn session_key_extends_namespace_key_with_session_id() {
    let n = ns("pid", "ws", "proj", "ch", "thr", "sess-123");
    let sk = n.session_key().0;
    let nk = n.namespace_key().0;
    assert!(
        sk.starts_with(&nk),
        "session_key must be a strict extension of namespace_key (got {}, expected to start with {})",
        sk,
        nk,
    );
    assert_eq!(sk, format!("{}__sess-123", nk));
}

#[test]
fn session_key_differs_for_different_session_ids() {
    let n1 = ns("pid", "ws", "proj", "ch", "thr", "sess-A").session_key();
    let n2 = ns("pid", "ws", "proj", "ch", "thr", "sess-B").session_key();
    assert_ne!(n1.0, n2.0);
}

#[test]
fn namespace_key_sanitizes_special_characters() {
    // Any non-[alnum - _ .] char must be replaced with '-' to keep the
    // key file-system and SQL safe.
    let n = ns("p/id", "ws ace", "proj:x", "ch\"q", "thr?y", "s");
    let k = n.namespace_key().0;
    assert!(!k.contains('/'), "slash must be sanitized: {}", k);
    assert!(!k.contains(' '), "space must be sanitized: {}", k);
    assert!(!k.contains(':'), "colon must be sanitized: {}", k);
    assert!(!k.contains('"'), "double-quote must be sanitized: {}", k);
    assert!(!k.contains('?'), "question mark must be sanitized: {}", k);
    // Allowed punctuation survives:
    assert!(k.contains('-'), "'-' is allowed and should survive");
    assert!(k.contains('_'), "'_' is a field separator and survives");
}

#[test]
fn namespace_key_preserves_dot_hyphen_underscore() {
    let n = ns("a.b", "c-d", "e_f", "g.h", "i-j", "k_l");
    let k = n.namespace_key().0;
    assert!(k.contains("a.b"));
    assert!(k.contains("c-d"));
    assert!(k.contains("e_f"));
    assert!(k.contains("g.h"));
    assert!(k.contains("i-j"));
}

#[test]
fn namespace_key_run_id_does_not_affect_key() {
    // run_id is intentionally excluded from both keys — verify.
    let mut a = ns("pid", "ws", "proj", "ch", "thr", "sess");
    let mut b = a.clone();
    a.run_id = Some(RunId("run-A".into()));
    b.run_id = Some(RunId("run-B".into()));
    assert_eq!(a.namespace_key().0, b.namespace_key().0);
    assert_eq!(a.session_key().0, b.session_key().0);
}

// ── EventType::as_str wire-format invariants ────────────────────────────────

#[test]
fn event_type_as_str_uses_dot_notation() {
    // These exact strings are serialized to disk on every ledger row;
    // any change breaks existing ledgers. Treat as a wire format.
    let cases: &[(EventType, &str)] = &[
        (EventType::ProcessCreated, "process.created"),
        (EventType::ProcessMigrated, "process.migrated"),
        (EventType::ChannelReceived, "channel.received"),
        (EventType::ChannelSent, "channel.sent"),
        (EventType::TaskStarted, "task.started"),
        (EventType::TaskCompleted, "task.completed"),
        (EventType::TaskFailed, "task.failed"),
        (EventType::ToolCalled, "tool.called"),
        (EventType::ToolResult, "tool.result"),
        (EventType::ProviderInvoked, "provider.invoked"),
        (EventType::ProviderResponded, "provider.responded"),
        (EventType::SkillDistilled, "skill.distilled"),
        (EventType::RuleApplied, "rule.applied"),
        (EventType::CheckpointWritten, "checkpoint.written"),
        (EventType::CheckpointRestored, "checkpoint.restored"),
        (EventType::IdentityVerified, "identity.verified"),
        (EventType::OmniRoute, "omni.route"),
        (EventType::AnswerTrace, "answer.trace"),
        (EventType::TurnProof, "turn.proof"),
        (EventType::ToolReceipt, "tool.receipt"),
        (EventType::ToolReceiptProofJoin, "tool.receipt.proof_join"),
        (EventType::OperationEvent, "operation.event"),
    ];
    for (ev, expected) in cases {
        assert_eq!(
            ev.as_str(),
            *expected,
            "wire format must not change for {:?}",
            ev
        );
    }
}

#[test]
fn event_type_custom_passes_through_verbatim() {
    let ev = EventType::Custom("my.custom.event".into());
    assert_eq!(ev.as_str(), "my.custom.event");
}

#[test]
fn event_type_as_str_does_not_allocate() {
    // as_str returns &str borrowed from self; ensures no accidental to_string().
    let ev = EventType::ChannelReceived;
    let s: &str = ev.as_str();
    assert_eq!(s, "channel.received");
}

// ── PrincipalId round-trip invariants ───────────────────────────────────────

#[test]
fn principal_id_as_str_roundtrip() {
    let pid = PrincipalId("did:zaion:abcdef1234".into());
    assert_eq!(pid.as_str(), "did:zaion:abcdef1234");
    let pid2 = PrincipalId(pid.as_str().to_string());
    assert_eq!(pid, pid2);
}

#[test]
fn canonical_envelope_builds_valid_channel_received_payload() {
    let source_hash = compute_source_hash("cli", "pid-1", "terminal", "default", "m1", "hello");
    let envelope = CanonicalEnvelope::new(
        "cli",
        PrincipalId("pid-1".into()),
        ChannelId("terminal".into()),
        ThreadId("default".into()),
        "m1",
        "hello",
        Some(source_hash.clone()),
    )
    .expect("valid canonical envelope");

    let payload = envelope.to_channel_received_payload();
    assert_eq!(payload["schema"], "zaion.canonical_envelope.v1");
    assert_eq!(payload["principal_id"], "pid-1");
    assert_eq!(payload["channel_id"], "terminal");
    assert_eq!(payload["source_hash"], source_hash);
    assert_eq!(payload["content"], "hello");
    assert_eq!(payload["message"], "hello");
}

#[test]
fn canonical_envelope_ingest_validates_and_clones_the_single_ingress_shape() {
    let envelope = CanonicalEnvelope::new(
        "cli",
        PrincipalId("pid-1".into()),
        ChannelId("terminal".into()),
        ThreadId("default".into()),
        "m1",
        "hello",
        None,
    )
    .expect("valid canonical envelope");

    let ingested = ingest(&envelope).expect("ingest validates valid envelope");
    assert_eq!(ingested.source_hash, envelope.source_hash);
    assert_eq!(
        ingested.to_channel_received_payload()["schema"],
        "zaion.canonical_envelope.v1"
    );
}

#[test]
fn canonical_envelope_rejects_dummy_principal_and_missing_hash() {
    let err = CanonicalEnvelope::new(
        "cli",
        PrincipalId("default_principal".into()),
        ChannelId("terminal".into()),
        ThreadId("default".into()),
        "m1",
        "hello",
        None,
    )
    .expect_err("dummy principal must be rejected");
    assert!(err.to_string().contains("not production-safe"));

    let err = CanonicalEnvelope::new(
        "cli",
        PrincipalId("pid-1".into()),
        ChannelId("terminal".into()),
        ThreadId("default".into()),
        "m1",
        "hello",
        Some(String::new()),
    )
    .expect_err("empty source hash must be rejected");
    assert!(err.to_string().contains("source_hash is empty"));
}

#[test]
fn canonical_envelope_rejects_hash_that_does_not_bind_body() {
    let source_hash = compute_source_hash("cli", "pid-1", "terminal", "default", "m1", "hello");
    let err = CanonicalEnvelope::new(
        "cli",
        PrincipalId("pid-1".into()),
        ChannelId("terminal".into()),
        ThreadId("default".into()),
        "m1",
        "tampered",
        Some(source_hash),
    )
    .expect_err("source_hash must bind the actual body");
    assert!(err.to_string().contains("does not match"));
}
