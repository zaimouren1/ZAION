//! End-to-end tests for zaion-gitledger.
//!
//! Exercises the three public modules (shadow, rollback, diff) against a
//! fresh git repo in a `tempdir`. No network, no external binaries, no git
//! CLI shelling — everything uses libgit2 via `git2`.

use std::fs;
use std::path::Path;

use git2::{Repository, Signature};
use tempfile::tempdir;

use zaion_crypto::ZaionKeypair;
use zaion_gitledger::{
    diff_refs, diff_workdir, parse_event_id_from_msg, parse_event_type_from_msg, RollbackEngine,
    ShadowEngine, SHADOW_BRANCH_PREFIX,
};
use zaion_ledger::EventLedger;
use zaion_types::session::NamespaceKey;

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Initialize a minimal repo with a single `HEAD` commit that contains
/// `README.md`. Returns `(repo_dir, ledger_dir, ledger, keypair, ns_key)`.
///
/// The ledger database lives OUTSIDE the repo tree so `git reset --hard`
/// never tries to delete it while SQLite still has a handle open (which
/// fails on Windows).
fn init_test_repo() -> (
    tempfile::TempDir,
    tempfile::TempDir,
    EventLedger,
    ZaionKeypair,
    NamespaceKey,
) {
    let repo_dir = tempdir().expect("repo tempdir");
    let ledger_dir = tempdir().expect("ledger tempdir");
    let repo = Repository::init(repo_dir.path()).expect("git init");

    // Configure a user so libgit2 can sign commits.
    let mut cfg = repo.config().unwrap();
    cfg.set_str("user.name", "test-user").unwrap();
    cfg.set_str("user.email", "test@zaion.local").unwrap();

    // Seed HEAD with a single commit.
    fs::write(repo_dir.path().join("README.md"), "initial\n").unwrap();
    {
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("README.md")).unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let sig = Signature::now("test-user", "test@zaion.local").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "initial commit", &tree, &[])
            .unwrap();
    }

    // Build a ledger outside the repo tree so rollbacks / diffs don't try to
    // delete it.
    let ledger_path = ledger_dir.path().join("ledger.db");
    let ledger = EventLedger::new(ledger_path);
    let kp = ZaionKeypair::generate();
    let ns = NamespaceKey(kp.principal_id().as_str().to_string());
    (repo_dir, ledger_dir, ledger, kp, ns)
}

// ─── commit-message parser tests ────────────────────────────────────────────

#[test]
fn parse_event_id_from_canonical_message() {
    let msg = "zaion: channel.received [event_id: evt-abcdef12]\nprincipal: did:zaion:...";
    assert_eq!(parse_event_id_from_msg(msg), Some("evt-abcdef12".into()));
}

#[test]
fn parse_event_id_returns_none_for_plain_commits() {
    assert!(parse_event_id_from_msg("fix: unrelated change").is_none());
}

#[test]
fn parse_event_type_from_canonical_message() {
    let msg = "zaion: tool.executed [event_id: evt-1234]";
    assert_eq!(parse_event_type_from_msg(msg), Some("tool.executed".into()));
}

#[test]
fn parse_event_type_returns_none_when_prefix_missing() {
    assert!(parse_event_type_from_msg("no-zaion-prefix").is_none());
}

// ─── ShadowEngine tests ─────────────────────────────────────────────────────

#[test]
fn shadow_engine_opens_branch_with_pid_prefix() {
    let (dir, _ldir, ledger, kp, ns) = init_test_repo();
    let pid = kp.principal_id().as_str().to_string();
    let engine = ShadowEngine::open(dir.path(), kp, ledger, ns).unwrap();

    let name = engine.branch_name();
    assert!(name.starts_with(&format!("{}/", SHADOW_BRANCH_PREFIX)));
    // Suffix is the first up-to-12 chars of the principal_id.
    let suffix = &name[SHADOW_BRANCH_PREFIX.len() + 1..];
    assert!(suffix.len() <= 12);
    assert!(pid.starts_with(suffix));
}

#[test]
fn shadow_engine_stages_and_commits_with_event_metadata() {
    let (dir, _ldir, ledger, kp, ns) = init_test_repo();
    let engine = ShadowEngine::open(dir.path(), kp, ledger, ns).unwrap();

    // Introduce a new file so there's something to stage.
    fs::write(dir.path().join("new.txt"), "hello shadow\n").unwrap();

    let shadow = engine
        .stage_all_and_commit("agent.patch_applied", "evt-s1")
        .unwrap();

    assert_eq!(shadow.event_id, "evt-s1");
    assert_eq!(shadow.event_type, "agent.patch_applied");
    assert_eq!(shadow.oid.len(), 40, "git oid should be 40 hex chars");
    assert!(shadow.message.contains("[event_id: evt-s1]"));
    assert_eq!(engine.shadow_tip().as_deref(), Some(shadow.oid.as_str()));
}

#[test]
fn shadow_engine_log_is_newest_first_and_limit_respected() {
    let (dir, _ldir, ledger, kp, ns) = init_test_repo();
    let engine = ShadowEngine::open(dir.path(), kp, ledger, ns).unwrap();

    for i in 0..3 {
        fs::write(dir.path().join(format!("f{}.txt", i)), format!("{}\n", i)).unwrap();
        engine
            .stage_all_and_commit("test.commit", &format!("evt-{}", i))
            .unwrap();
    }

    let all = engine.log(10).unwrap();
    assert_eq!(all.len(), 3);
    // Newest-first ⇒ evt-2 at index 0.
    assert_eq!(all[0].event_id, "evt-2");
    assert_eq!(all[1].event_id, "evt-1");
    assert_eq!(all[2].event_id, "evt-0");

    let limited = engine.log(2).unwrap();
    assert_eq!(limited.len(), 2);
    assert_eq!(limited[0].event_id, "evt-2");
}

#[test]
fn shadow_engine_log_empty_when_no_commits_yet() {
    let (dir, _ldir, ledger, kp, ns) = init_test_repo();
    let engine = ShadowEngine::open(dir.path(), kp, ledger, ns).unwrap();
    assert!(engine.log(10).unwrap().is_empty());
    assert!(engine.shadow_tip().is_none());
}

#[test]
fn shadow_commit_records_ledger_event_with_signature() {
    let (dir, ldir, ledger, kp, ns) = init_test_repo();
    // Keep a clone of the namespace key so we can query the ledger after the
    // engine takes ownership.
    let ns_clone = ns.clone();
    let principal = kp.principal_id();
    let engine = ShadowEngine::open(dir.path(), kp, ledger, ns).unwrap();

    fs::write(dir.path().join("ledger-check.txt"), "probe\n").unwrap();
    engine
        .stage_all_and_commit("agent.probe", "evt-ledger")
        .unwrap();

    // Re-open the ledger from the external ledger_dir and verify the event.
    let ledger2 = EventLedger::new(ldir.path().join("ledger.db"));
    let sk = zaion_types::session::SessionKey(ns_clone.0.clone());
    let events = ledger2.list_events(&sk, None, 20).unwrap();
    let found = events.iter().find(|e| e.event_type == "git.shadow_commit");
    assert!(
        found.is_some(),
        "git.shadow_commit event should be recorded"
    );
    let evt = found.unwrap();
    assert_eq!(evt.principal_id, principal);
    assert!(evt.signature.is_some(), "ledger event must be signed");
    let sig = evt.signature.as_ref().unwrap();
    assert!(!sig.0.is_empty(), "signature must be non-empty");
    assert_eq!(evt.payload["event_id"].as_str(), Some("evt-ledger"));
    assert_eq!(evt.payload["event_type"].as_str(), Some("agent.probe"));
}

// ─── RollbackEngine tests ───────────────────────────────────────────────────

#[test]
fn rollback_to_event_resets_workdir_to_shadow_commit() {
    let (dir, ldir, ledger, kp, ns) = init_test_repo();
    // Clone the keypair bytes so both engines can own a ZaionKeypair.
    let kp_bytes = kp.to_bytes();
    let ns_clone = ns.clone();
    let shadow_engine = ShadowEngine::open(dir.path(), kp, ledger, ns).unwrap();
    let branch = shadow_engine.branch_name().to_string();

    fs::write(dir.path().join("state.txt"), "v1\n").unwrap();
    shadow_engine
        .stage_all_and_commit("state.v1", "evt-v1")
        .unwrap();
    fs::write(dir.path().join("state.txt"), "v2\n").unwrap();
    shadow_engine
        .stage_all_and_commit("state.v2", "evt-v2")
        .unwrap();

    // Current tip is v2. Normalise CRLF for Windows autocrlf compatibility.
    let v2_actual = fs::read_to_string(dir.path().join("state.txt")).unwrap();
    assert_eq!(v2_actual.replace("\r\n", "\n"), "v2\n");

    // Drop the shadow engine so the Repository handle is released before
    // the rollback engine re-opens it (safer on Windows).
    drop(shadow_engine);

    // Roll back to v1.
    let kp2 = ZaionKeypair::from_bytes(&kp_bytes).unwrap();
    let ledger2 = EventLedger::new(ldir.path().join("ledger.db"));
    let rb = RollbackEngine::open(dir.path(), kp2, ledger2, ns_clone, branch).unwrap();
    let result = rb.rollback_to_event("evt-v1", None).unwrap();

    assert_eq!(result.event_id, "evt-v1");
    assert!(result.verify_passed.is_none());
    // After a hard reset the working tree is at v1. Windows git may apply
    // autocrlf and rewrite \n → \r\n, so compare after normalising.
    let actual = fs::read_to_string(dir.path().join("state.txt")).unwrap();
    assert_eq!(actual.replace("\r\n", "\n"), "v1\n");
}

#[test]
fn rollback_unknown_event_id_errors_not_found() {
    let (dir, ldir, ledger, kp, ns) = init_test_repo();
    let kp_bytes = kp.to_bytes();
    let ns_clone = ns.clone();
    let shadow_engine = ShadowEngine::open(dir.path(), kp, ledger, ns).unwrap();
    let branch = shadow_engine.branch_name().to_string();
    fs::write(dir.path().join("any.txt"), "x\n").unwrap();
    shadow_engine
        .stage_all_and_commit("t", "evt-exists")
        .unwrap();
    drop(shadow_engine);

    let kp2 = ZaionKeypair::from_bytes(&kp_bytes).unwrap();
    let ledger2 = EventLedger::new(ldir.path().join("ledger.db"));
    let rb = RollbackEngine::open(dir.path(), kp2, ledger2, ns_clone, branch).unwrap();
    let err = rb.rollback_to_event("evt-missing", None).unwrap_err();
    assert!(format!("{}", err).contains("evt-missing"));
}

#[test]
fn rollback_with_successful_verify_records_passing_result() {
    let (dir, ldir, ledger, kp, ns) = init_test_repo();
    let kp_bytes = kp.to_bytes();
    let ns_clone = ns.clone();
    let shadow_engine = ShadowEngine::open(dir.path(), kp, ledger, ns).unwrap();
    let branch = shadow_engine.branch_name().to_string();
    fs::write(dir.path().join("a.txt"), "x\n").unwrap();
    shadow_engine.stage_all_and_commit("t", "evt-ok").unwrap();
    drop(shadow_engine);

    let kp2 = ZaionKeypair::from_bytes(&kp_bytes).unwrap();
    let ledger2 = EventLedger::new(ldir.path().join("ledger.db"));
    let rb = RollbackEngine::open(dir.path(), kp2, ledger2, ns_clone, branch).unwrap();

    // Cross-platform no-op command that should exit 0.
    let ok_cmd = if cfg!(windows) {
        "cmd /C exit 0"
    } else {
        "true"
    };
    let result = rb.rollback_to_event("evt-ok", Some(ok_cmd)).unwrap();
    assert_eq!(result.verify_passed, Some(true));
}

// ─── diff tests ─────────────────────────────────────────────────────────────

#[test]
fn diff_workdir_detects_uncommitted_changes() {
    let (dir, _ldir, _ledger, _kp, _ns) = init_test_repo();
    fs::write(dir.path().join("new.txt"), "fresh content\n").unwrap();
    // Stage the new file so libgit2's diff_tree_to_workdir_with_index sees it.
    {
        let repo = Repository::open(dir.path()).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("new.txt")).unwrap();
        index.write().unwrap();
    }
    let summary = diff_workdir(dir.path(), None).unwrap();
    assert!(summary.files_changed >= 1);
    assert!(summary.insertions >= 1);
    assert!(summary.unified.contains("fresh content"));
    let matches = summary
        .file_stats
        .iter()
        .any(|(p, _, _)| p.ends_with("new.txt"));
    assert!(matches, "file_stats should include new.txt");
}

#[test]
fn diff_workdir_clean_repo_reports_no_changes() {
    let (dir, _ldir, _ledger, _kp, _ns) = init_test_repo();
    let summary = diff_workdir(dir.path(), None).unwrap();
    assert_eq!(summary.files_changed, 0);
    assert_eq!(summary.insertions, 0);
    assert_eq!(summary.deletions, 0);
}

#[test]
fn diff_refs_compares_two_shadow_commits() {
    let (dir, _ldir, ledger, kp, ns) = init_test_repo();
    let shadow_engine = ShadowEngine::open(dir.path(), kp, ledger, ns).unwrap();

    fs::write(dir.path().join("s.txt"), "alpha\n").unwrap();
    let a = shadow_engine.stage_all_and_commit("t", "evt-a").unwrap();
    fs::write(dir.path().join("s.txt"), "alpha\nbeta\n").unwrap();
    let b = shadow_engine.stage_all_and_commit("t", "evt-b").unwrap();

    let summary = diff_refs(dir.path(), &a.oid, &b.oid).unwrap();
    assert!(
        summary.insertions >= 1,
        "adding a line should yield insertions"
    );
    assert!(summary.unified.contains("beta"));
}

#[test]
fn diff_refs_errors_for_unknown_reference() {
    let (dir, _ldir, _ledger, _kp, _ns) = init_test_repo();
    let err = diff_refs(dir.path(), "HEAD", "does-not-exist").unwrap_err();
    // Either a git revparse error or a NotFound — we just check it errors.
    let msg = format!("{}", err);
    assert!(!msg.is_empty());
}
