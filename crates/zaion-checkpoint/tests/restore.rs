//! Integration tests for zaion-checkpoint::restore — the "undo my work"
//! path that had zero coverage prior to this suite.

use std::fs;

use tempfile::tempdir;
use zaion_checkpoint::{CheckpointId, CheckpointManager};

/// Create a manager whose shadow-repo root is a tempdir so tests don't
/// touch the user's home directory.
fn make_manager() -> (CheckpointManager, tempfile::TempDir) {
    let root = tempdir().expect("tempdir");
    let mgr = CheckpointManager::new(root.path().to_path_buf());
    (mgr, root)
}

#[test]
fn restore_with_empty_sentinel_is_a_noop() {
    let (mgr, _root) = make_manager();
    let watched = tempdir().unwrap();
    fs::write(watched.path().join("a.txt"), "unchanged\n").unwrap();
    // "empty" is the sentinel ID returned when snapshot() sees nothing to commit.
    let result = mgr.restore(watched.path(), &CheckpointId("empty".into()));
    assert!(result.is_ok(), "restore of 'empty' sentinel must succeed");
    // Contents must be untouched.
    let actual = fs::read_to_string(watched.path().join("a.txt")).unwrap();
    assert_eq!(actual.replace("\r\n", "\n"), "unchanged\n");
}

#[test]
fn restore_unknown_oid_errors_not_found() {
    let (mgr, _root) = make_manager();
    let watched = tempdir().unwrap();
    fs::write(watched.path().join("f.txt"), "v1\n").unwrap();
    // Snapshot so that the shadow repo exists but does NOT contain this OID.
    let _ = mgr.snapshot(watched.path(), "initial").unwrap();
    let bogus = CheckpointId("0000000000000000000000000000000000000000".into());
    let err = mgr.restore(watched.path(), &bogus).unwrap_err();
    let msg = format!("{}", err);
    assert!(
        msg.contains("0000"),
        "error message should mention the missing OID: {}",
        msg
    );
}

#[test]
fn restore_roundtrip_recovers_original_content() {
    let (mgr, _root) = make_manager();
    let watched = tempdir().unwrap();
    fs::write(watched.path().join("x.txt"), "v1\n").unwrap();
    let id_v1 = mgr.snapshot(watched.path(), "v1").unwrap();

    // Mutate.
    fs::write(watched.path().join("x.txt"), "v2\n").unwrap();
    let _id_v2 = mgr.snapshot(watched.path(), "v2").unwrap();

    // Roll back to v1.
    mgr.restore(watched.path(), &id_v1).unwrap();
    let actual = fs::read_to_string(watched.path().join("x.txt")).unwrap();
    assert_eq!(actual.replace("\r\n", "\n"), "v1\n");
}

#[test]
fn snapshot_on_empty_dir_returns_empty_sentinel() {
    let (mgr, _root) = make_manager();
    let watched = tempdir().unwrap();
    // Directory is empty apart from implicit dot-files — snapshot should
    // either return "empty" or succeed without files. Either way it must
    // not crash and must produce a well-formed ID.
    let id = mgr.snapshot(watched.path(), "nothing here").unwrap();
    assert!(!id.0.is_empty(), "snapshot must return a non-empty ID");
}

#[test]
fn list_checkpoints_is_newest_first() {
    let (mgr, _root) = make_manager();
    let watched = tempdir().unwrap();
    fs::write(watched.path().join("f.txt"), "one\n").unwrap();
    let id1 = mgr.snapshot(watched.path(), "first").unwrap();
    fs::write(watched.path().join("f.txt"), "two\n").unwrap();
    let id2 = mgr.snapshot(watched.path(), "second").unwrap();

    let list = mgr.list_checkpoints(watched.path()).unwrap();
    assert!(list.len() >= 2);
    // Newest first
    assert_eq!(list[0].id.0, id2.0);
    assert_eq!(list[1].id.0, id1.0);
}

#[test]
fn list_checkpoints_on_unsnapshotted_dir_returns_empty() {
    let (mgr, _root) = make_manager();
    let watched = tempdir().unwrap();
    let list = mgr.list_checkpoints(watched.path()).unwrap();
    assert!(list.is_empty(), "unsnapshotted dir must return empty list");
}
