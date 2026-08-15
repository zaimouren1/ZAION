//! End-to-end tests for zaion-codex codegen and diff modules.
//!
//! These are the two formerly-untested public modules of the crate — the
//! AST / index / semantic_search layers are exercised by the in-lib unit
//! tests in `src/lib.rs`.

use std::fs;
use std::path::PathBuf;

use tempfile::tempdir;

use zaion_codex::{diff_files, ChangeKind, CodegenBuilder, CodegenKind, CodexError, DiffSummary};

// ─── codegen tests ──────────────────────────────────────────────────────────

fn write_src(path: &PathBuf, content: &str) {
    fs::write(path, content).expect("write fixture");
}

#[test]
fn codegen_replace_substitutes_named_function_body() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("main.rs");
    write_src(
        &path,
        "pub fn keep() {}\n\npub fn target() {\n    println!(\"old\");\n}\n\npub fn other() {}\n",
    );

    CodegenBuilder::new(&path, "target")
        .content("pub fn target() {\n    println!(\"new\");\n}")
        .kind(CodegenKind::Replace)
        .build()
        .apply()
        .unwrap();

    let out = fs::read_to_string(&path).unwrap();
    assert!(
        out.contains("println!(\"new\")"),
        "replacement body missing:\n{}",
        out
    );
    assert!(
        !out.contains("println!(\"old\")"),
        "old body should be gone:\n{}",
        out
    );
    assert!(out.contains("pub fn keep()"), "keep() must survive");
    assert!(out.contains("pub fn other()"), "other() must survive");
}

#[test]
fn codegen_insert_after_appends_following_target_block() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("m.rs");
    write_src(&path, "pub fn a() {\n    let _ = 1;\n}\n\npub fn b() {}\n");

    CodegenBuilder::new(&path, "a")
        .content("pub fn new_after() { /* injected */ }")
        .kind(CodegenKind::InsertAfter)
        .build()
        .apply()
        .unwrap();

    let out = fs::read_to_string(&path).unwrap();
    let pos_a = out.find("pub fn a()").expect("a present");
    let pos_new = out.find("pub fn new_after()").expect("injection present");
    let pos_b = out.find("pub fn b()").expect("b present");
    assert!(pos_a < pos_new, "new_after should come after a()");
    assert!(pos_new < pos_b, "new_after should come before b()");
}

#[test]
fn codegen_insert_before_prepends_target_block() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("m.rs");
    write_src(&path, "pub fn target() {}\n");

    CodegenBuilder::new(&path, "target")
        .content("pub fn before_target() { /* prefix */ }")
        .kind(CodegenKind::InsertBefore)
        .build()
        .apply()
        .unwrap();

    let out = fs::read_to_string(&path).unwrap();
    let pos_prefix = out.find("pub fn before_target()").expect("prefix present");
    let pos_target = out.find("pub fn target()").expect("target present");
    assert!(pos_prefix < pos_target, "insertion must precede target");
}

#[test]
fn codegen_replace_matches_struct_not_just_fn() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("m.rs");
    write_src(&path, "pub struct Widget {\n    pub name: String,\n}\n");

    CodegenBuilder::new(&path, "Widget")
        .content("pub struct Widget {\n    pub id: u64,\n    pub name: String,\n}")
        .kind(CodegenKind::Replace)
        .build()
        .apply()
        .unwrap();

    let out = fs::read_to_string(&path).unwrap();
    assert!(
        out.contains("pub id: u64"),
        "new field must appear:\n{}",
        out
    );
    assert!(
        out.contains("pub name: String"),
        "existing field must survive:\n{}",
        out
    );
}

#[test]
fn codegen_no_match_leaves_file_semantically_unchanged() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("m.rs");
    let original = "pub fn only() {}\n";
    write_src(&path, original);

    CodegenBuilder::new(&path, "does_not_exist")
        .content("pub fn injected() {}")
        .kind(CodegenKind::Replace)
        .build()
        .apply()
        .unwrap();

    let out = fs::read_to_string(&path).unwrap();
    // Normalise trailing newlines for line-join round-trip.
    assert_eq!(out.trim(), original.trim());
    assert!(
        !out.contains("injected"),
        "no symbol matched ⇒ nothing injected"
    );
}

#[test]
fn codegen_errors_on_missing_file() {
    let edit = CodegenBuilder::new("nonexistent/file.rs", "x")
        .content("")
        .build();
    let err = edit.apply().unwrap_err();
    match err {
        CodexError::Io(_) => {}
        other => panic!("expected CodexError::Io, got {:?}", other),
    }
}

#[test]
fn codegen_builder_defaults_to_replace_kind() {
    let edit = CodegenBuilder::new("/tmp/whatever.rs", "x")
        .content("body")
        .build();
    assert_eq!(edit.kind, CodegenKind::Replace);
    assert_eq!(edit.symbol_name, "x");
    assert_eq!(edit.new_content, "body");
}

// ─── diff tests ─────────────────────────────────────────────────────────────

#[test]
fn diff_files_detects_pure_additions() {
    let dir = tempdir().unwrap();
    let old_path = dir.path().join("a.old.rs");
    let new_path = dir.path().join("a.new.rs");
    fs::write(&old_path, "line1\nline2\n").unwrap();
    fs::write(&new_path, "line1\nline2\nline3\nline4\n").unwrap();

    let summary = diff_files(&old_path, &new_path).unwrap();
    assert_eq!(summary.deletions, 0);
    assert_eq!(summary.additions, 2);
    let added: Vec<_> = summary
        .changes
        .iter()
        .filter(|c| c.kind == ChangeKind::Added)
        .map(|c| c.content.as_str())
        .collect();
    assert_eq!(added, vec!["line3", "line4"]);
}

#[test]
fn diff_files_detects_pure_deletions() {
    let dir = tempdir().unwrap();
    let old_path = dir.path().join("a.old.rs");
    let new_path = dir.path().join("a.new.rs");
    fs::write(&old_path, "keep\ndrop_me\n").unwrap();
    fs::write(&new_path, "keep\n").unwrap();

    let summary = diff_files(&old_path, &new_path).unwrap();
    assert_eq!(summary.additions, 0);
    assert_eq!(summary.deletions, 1);
    assert!(summary
        .changes
        .iter()
        .any(|c| c.kind == ChangeKind::Deleted && c.content == "drop_me"));
}

#[test]
fn diff_files_treats_in_place_edit_as_delete_plus_add() {
    let dir = tempdir().unwrap();
    let old_path = dir.path().join("a.old.rs");
    let new_path = dir.path().join("a.new.rs");
    fs::write(&old_path, "pub fn x() { 1 }\n").unwrap();
    fs::write(&new_path, "pub fn x() { 2 }\n").unwrap();

    let summary = diff_files(&old_path, &new_path).unwrap();
    assert_eq!(summary.additions, 1);
    assert_eq!(summary.deletions, 1);
    assert!(summary
        .changes
        .iter()
        .any(|c| c.kind == ChangeKind::Deleted && c.content.contains("{ 1 }")));
    assert!(summary
        .changes
        .iter()
        .any(|c| c.kind == ChangeKind::Added && c.content.contains("{ 2 }")));
}

#[test]
fn diff_files_missing_old_path_treats_all_as_additions() {
    let dir = tempdir().unwrap();
    let new_path = dir.path().join("new-only.rs");
    fs::write(&new_path, "one\ntwo\nthree\n").unwrap();
    // diff_files silently treats missing old_path as empty content.
    let summary = diff_files(&dir.path().join("does-not-exist.rs"), &new_path).unwrap();
    assert_eq!(summary.additions, 3);
    assert_eq!(summary.deletions, 0);
}

#[test]
fn diff_summary_parse_unified_diff_extracts_counts() {
    let diff_text = "\
@@ -1,2 +1,3 @@
 unchanged-line
-removed-line
+added-line-1
+added-line-2
";
    let summary = DiffSummary::parse(std::path::Path::new("foo.rs"), diff_text).unwrap();
    assert_eq!(summary.additions, 2);
    assert_eq!(summary.deletions, 1);
    assert_eq!(summary.changes.len(), 3);
    assert!(summary
        .changes
        .iter()
        .any(|c| c.kind == ChangeKind::Added && c.content == "added-line-1"));
    assert!(summary
        .changes
        .iter()
        .any(|c| c.kind == ChangeKind::Deleted && c.content == "removed-line"));
}

#[test]
fn diff_summary_parse_ignores_file_header_lines() {
    let diff_text = "\
--- a/old
+++ b/new
@@ -1,1 +1,1 @@
-old
+new
";
    let summary = DiffSummary::parse(std::path::Path::new("x.rs"), diff_text).unwrap();
    // The '---' and '+++' header lines must NOT be counted as changes.
    assert_eq!(summary.additions, 1);
    assert_eq!(summary.deletions, 1);
}

#[test]
fn diff_summary_human_readable_summary_contains_counts() {
    let s = DiffSummary {
        file_path: "path/to/file.rs".into(),
        additions: 7,
        deletions: 4,
        changes: vec![],
    };
    let rendered = s.summary();
    assert!(rendered.contains("+7"));
    assert!(rendered.contains("-4"));
    assert!(rendered.contains("file.rs"));
}

#[test]
fn diff_summary_parse_empty_input_returns_zero_counts() {
    let summary = DiffSummary::parse(std::path::Path::new("x.rs"), "").unwrap();
    assert_eq!(summary.additions, 0);
    assert_eq!(summary.deletions, 0);
    assert!(summary.changes.is_empty());
}
