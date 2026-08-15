//! Integration tests for the Week 10 advanced built-in MCP tool set.
//!
//! These exercise the *public* surface — `McpToolRegistry` populated via
//! `register_builtin_tools`, then each tool invoked through `McpTool::call` —
//! so they verify real handler execution end to end, not just registration.
//!
//! Pure/deterministic tools (text/time/data/net-parse/sys-info) are asserted
//! on exact output. Filesystem tools write into `target/mcp-tests/` under the
//! crate root (which is the canonicalized workspace root during `cargo test`).
//! Read-only git tools are smoke-tested for shape, since their content depends
//! on repository state.

use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;
use zaion_mcp::{register_builtin_tools, McpToolRegistry};

/// Build a registry with all built-in tools registered.
fn registry() -> McpToolRegistry {
    let mut r = McpToolRegistry::new();
    register_builtin_tools(&mut r);
    r
}

/// Invoke a tool by name, expecting success; returns its JSON output.
fn call_ok(r: &McpToolRegistry, name: &str, input: serde_json::Value) -> serde_json::Value {
    let tool = r
        .get(name)
        .unwrap_or_else(|| panic!("tool '{name}' should be registered"));
    tool.call(input)
        .unwrap_or_else(|e| panic!("tool '{name}' should succeed, got error: {e}"))
}

/// Invoke a tool by name, expecting an error; returns the error string.
fn call_err(r: &McpToolRegistry, name: &str, input: serde_json::Value) -> String {
    let tool = r
        .get(name)
        .unwrap_or_else(|| panic!("tool '{name}' should be registered"));
    tool.call(input)
        .expect_err(&format!("tool '{name}' should have failed"))
}

/// Unique workspace-relative path under target/mcp-tests for fs tests.
fn temp_rel(name: &str) -> String {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    format!("target/mcp-tests/{name}-{}-{unique}", std::process::id())
}

// ── text tools ────────────────────────────────────────────────────────────

#[test]
fn base64_round_trips() {
    let r = registry();
    let enc = call_ok(&r, "base64_encode", json!({ "text": "hello, 世界" }));
    let encoded = enc["encoded"].as_str().unwrap().to_string();
    let dec = call_ok(&r, "base64_decode", json!({ "encoded": encoded }));
    assert_eq!(dec["text"], json!("hello, 世界"));
}

#[test]
fn base64_decode_rejects_garbage() {
    let r = registry();
    let err = call_err(
        &r,
        "base64_decode",
        json!({ "encoded": "!!!not base64!!!" }),
    );
    assert!(err.contains("base64 decode failed"), "got: {err}");
}

#[test]
fn url_encode_decode_round_trips() {
    let r = registry();
    let enc = call_ok(&r, "url_encode", json!({ "text": "a b&c=d/е" }));
    let encoded = enc["encoded"].as_str().unwrap();
    // Space and reserved chars must be percent-escaped.
    assert!(encoded.contains("%20"), "space should be %20: {encoded}");
    assert!(!encoded.contains(' '), "no raw spaces: {encoded}");
    let dec = call_ok(&r, "url_decode", json!({ "encoded": encoded }));
    assert_eq!(dec["text"], json!("a b&c=d/е"));
}

#[test]
fn text_regex_replace_counts_matches() {
    let r = registry();
    let out = call_ok(
        &r,
        "text_regex_replace",
        json!({ "text": "a1b2c3", "pattern": "[0-9]", "replacement": "#" }),
    );
    assert_eq!(out["result"], json!("a#b#c#"));
    assert_eq!(out["match_count"], json!(3));
}

#[test]
fn text_regex_replace_rejects_bad_pattern() {
    let r = registry();
    let err = call_err(
        &r,
        "text_regex_replace",
        json!({ "text": "x", "pattern": "(", "replacement": "y" }),
    );
    assert!(err.contains("invalid regex"), "got: {err}");
}

#[test]
fn text_diff_reports_changed_lines() {
    let r = registry();
    let out = call_ok(
        &r,
        "text_diff",
        json!({ "left": "one\ntwo\nthree", "right": "one\nTWO\nthree" }),
    );
    assert_eq!(out["changed"], json!(true));
    assert_eq!(out["added"].as_array().unwrap().len(), 1);
    assert_eq!(out["removed"].as_array().unwrap().len(), 1);
    assert_eq!(out["added"][0]["line"], json!(2));
}

#[test]
fn text_diff_identical_is_unchanged() {
    let r = registry();
    let out = call_ok(&r, "text_diff", json!({ "left": "same", "right": "same" }));
    assert_eq!(out["changed"], json!(false));
}

#[test]
fn uuid_generate_is_unique_and_well_formed() {
    let r = registry();
    let a = call_ok(&r, "uuid_generate", json!({}));
    let b = call_ok(&r, "uuid_generate", json!({}));
    let ua = a["uuid"].as_str().unwrap();
    let ub = b["uuid"].as_str().unwrap();
    assert_ne!(ua, ub, "two UUIDs must differ");
    assert_eq!(ua.len(), 36, "UUID v4 string is 36 chars");
    assert_eq!(ua.matches('-').count(), 4);
}

#[test]
fn json_query_traverses_objects_and_arrays() {
    let r = registry();
    let doc = r#"{"items":[{"name":"first"},{"name":"second"}]}"#;
    let out = call_ok(
        &r,
        "json_query",
        json!({ "text": doc, "path": "items[1].name" }),
    );
    assert_eq!(out["value"], json!("second"));
}

#[test]
fn json_query_reports_missing_key() {
    let r = registry();
    let err = call_err(
        &r,
        "json_query",
        json!({ "text": r#"{"a":1}"#, "path": "b" }),
    );
    assert!(err.contains("not found"), "got: {err}");
}

// ── time tools ────────────────────────────────────────────────────────────

#[test]
fn time_now_exposes_consistent_fields() {
    let r = registry();
    let out = call_ok(&r, "time_now", json!({}));
    assert!(out["rfc3339"].as_str().unwrap().contains('T'));
    assert!(out["unix_secs"].as_i64().unwrap() > 1_700_000_000);
    // unix_millis must be consistent with unix_secs (same instant, ms precision).
    let secs = out["unix_secs"].as_i64().unwrap();
    let millis = out["unix_millis"].as_i64().unwrap();
    assert_eq!(millis / 1000, secs);
}

#[test]
fn time_parse_accepts_unix_and_rfc3339() {
    let r = registry();
    let from_unix = call_ok(&r, "time_parse", json!({ "text": "0" }));
    assert_eq!(from_unix["unix_secs"], json!(0));
    assert_eq!(from_unix["rfc3339"], json!("1970-01-01T00:00:00+00:00"));

    let from_rfc = call_ok(
        &r,
        "time_parse",
        json!({ "text": "1970-01-01T00:00:00+00:00" }),
    );
    assert_eq!(from_rfc["unix_secs"], json!(0));
}

#[test]
fn time_parse_rejects_garbage() {
    let r = registry();
    let err = call_err(&r, "time_parse", json!({ "text": "not-a-time" }));
    assert!(err.contains("could not parse"), "got: {err}");
}

#[test]
fn time_diff_computes_signed_delta() {
    let r = registry();
    let out = call_ok(&r, "time_diff", json!({ "from": "0", "to": "3600" }));
    assert_eq!(out["seconds"], json!(3600));
    assert_eq!(out["minutes"], json!(60));
    assert_eq!(out["hours"], json!(1));

    // Reversed order yields a negative delta.
    let neg = call_ok(&r, "time_diff", json!({ "from": "3600", "to": "0" }));
    assert_eq!(neg["seconds"], json!(-3600));
}

// ── data tools ────────────────────────────────────────────────────────────

#[test]
fn hash_text_is_stable_sha256() {
    let r = registry();
    let out = call_ok(&r, "hash_text", json!({ "text": "abc" }));
    // Known SHA-256 of "abc".
    assert_eq!(
        out["sha256"],
        json!("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
    );
    assert_eq!(out["bytes"], json!(3));
}

#[test]
fn csv_parse_with_header_yields_objects() {
    let r = registry();
    let out = call_ok(
        &r,
        "csv_parse",
        json!({ "text": "name,age\nalice,30\nbob,25" }),
    );
    assert_eq!(out["row_count"], json!(2));
    assert_eq!(out["rows"][0]["name"], json!("alice"));
    assert_eq!(out["rows"][1]["age"], json!("25"));
}

#[test]
fn csv_parse_without_header_yields_arrays() {
    let r = registry();
    let out = call_ok(
        &r,
        "csv_parse",
        json!({ "text": "a;b;c\n1;2;3", "has_header": false, "delimiter": ";" }),
    );
    assert_eq!(out["row_count"], json!(2));
    assert_eq!(out["rows"][0], json!(["a", "b", "c"]));
    assert_eq!(out["rows"][1], json!(["1", "2", "3"]));
}

#[test]
fn json_format_pretty_prints_valid_json() {
    let r = registry();
    let out = call_ok(&r, "json_format", json!({ "text": "{\"a\":1,\"b\":2}" }));
    assert_eq!(out["valid"], json!(true));
    let formatted = out["formatted"].as_str().unwrap();
    assert!(formatted.contains('\n'), "pretty output is multi-line");
    assert!(formatted.contains("\"a\": 1"));
}

#[test]
fn json_format_rejects_invalid_json() {
    let r = registry();
    let err = call_err(&r, "json_format", json!({ "text": "{not json}" }));
    assert!(err.contains("invalid json"), "got: {err}");
}

#[test]
fn random_hex_respects_byte_count() {
    let r = registry();
    let out = call_ok(&r, "random_hex", json!({ "bytes": 8 }));
    assert_eq!(out["bytes"], json!(8));
    // 8 bytes => 16 hex chars.
    assert_eq!(out["hex"].as_str().unwrap().len(), 16);
}

// ── sys tools (host info; assert shape, not exact values) ───────────────────

#[test]
fn sys_os_reports_known_consts() {
    let r = registry();
    let out = call_ok(&r, "sys_os", json!({}));
    // std::env::consts are always populated.
    assert_eq!(out["os"], json!(std::env::consts::OS));
    assert_eq!(out["arch"], json!(std::env::consts::ARCH));
    assert_eq!(out["family"], json!(std::env::consts::FAMILY));
}

#[test]
fn sys_hostname_returns_non_empty() {
    let r = registry();
    let out = call_ok(&r, "sys_hostname", json!({}));
    assert!(
        !out["hostname"].as_str().unwrap().is_empty(),
        "hostname should be non-empty"
    );
}

// ── net parse tools (pure; no network I/O) ──────────────────────────────────

#[test]
fn url_parse_splits_components() {
    let r = registry();
    let out = call_ok(
        &r,
        "url_parse",
        json!({ "url": "https://example.com:8443/path/to?x=1&y=2" }),
    );
    assert_eq!(out["scheme"], json!("https"));
    assert_eq!(out["host"], json!("example.com"));
    assert_eq!(out["port"], json!(8443));
    assert_eq!(out["path"], json!("/path/to"));
    assert_eq!(out["query"], json!("x=1&y=2"));
}

#[test]
fn url_parse_defaults_path_and_omits_port() {
    let r = registry();
    let out = call_ok(&r, "url_parse", json!({ "url": "http://localhost" }));
    assert_eq!(out["host"], json!("localhost"));
    assert_eq!(out["port"], serde_json::Value::Null);
    assert_eq!(out["path"], json!("/"));
    assert_eq!(out["query"], serde_json::Value::Null);
}

#[test]
fn url_parse_rejects_schemeless_url() {
    let r = registry();
    let err = call_err(&r, "url_parse", json!({ "url": "example.com/x" }));
    assert!(err.contains("missing scheme"), "got: {err}");
}

// ── git tools (read-only; smoke-test shape against this repo) ────────────────

#[test]
fn git_status_returns_branch_and_entries() {
    let r = registry();
    let out = call_ok(&r, "git_status", json!({}));
    // Shape only: branch is a string, entries is an array, count matches.
    assert!(out["branch"].is_string());
    let entries = out["entries"].as_array().expect("entries is an array");
    assert_eq!(
        out["changed_count"].as_u64().unwrap() as usize,
        entries.len()
    );
}

#[test]
fn git_log_respects_limit() {
    let r = registry();
    let out = call_ok(&r, "git_log", json!({ "limit": 3 }));
    let commits = out["commits"].as_array().expect("commits is an array");
    assert!(commits.len() <= 3, "limit must be honored");
    if let Some(first) = commits.first() {
        assert!(first["hash"].as_str().unwrap().len() >= 7);
        assert!(first["subject"].is_string());
    }
}

#[test]
fn git_branch_identifies_current() {
    let r = registry();
    let out = call_ok(&r, "git_branch", json!({}));
    assert!(out["branches"].is_array());
    assert!(out["current"].is_string());
}

// ── filesystem tools (write under target/mcp-tests) ─────────────────────────

#[test]
fn fs_stat_reports_file_metadata() {
    let r = registry();
    let rel = temp_rel("stat.txt");
    std::fs::write(&rel, "0123456789").expect("seed file");

    let out = call_ok(&r, "fs_stat", json!({ "path": rel.clone() }));
    assert_eq!(out["is_file"], json!(true));
    assert_eq!(out["is_dir"], json!(false));
    assert_eq!(out["size_bytes"], json!(10));

    let _ = std::fs::remove_file(&rel);
}

#[test]
fn fs_append_creates_and_extends() {
    let r = registry();
    let rel = temp_rel("append.txt");
    let _ = std::fs::remove_file(&rel);

    call_ok(
        &r,
        "fs_append",
        json!({ "path": rel.clone(), "content": "first\n" }),
    );
    call_ok(
        &r,
        "fs_append",
        json!({ "path": rel.clone(), "content": "second\n" }),
    );

    let contents = std::fs::read_to_string(&rel).expect("read appended file");
    assert_eq!(contents, "first\nsecond\n");

    let _ = std::fs::remove_file(&rel);
}

#[test]
fn fs_read_lines_returns_inclusive_range() {
    let r = registry();
    let rel = temp_rel("lines.txt");
    std::fs::write(&rel, "l1\nl2\nl3\nl4\nl5").expect("seed file");

    let out = call_ok(
        &r,
        "fs_read_lines",
        json!({ "path": rel.clone(), "start": 2, "end": 4 }),
    );
    assert_eq!(out["content"], json!("l2\nl3\nl4"));
    assert_eq!(out["total_lines"], json!(5));

    let _ = std::fs::remove_file(&rel);
}

#[test]
fn fs_glob_finds_matching_files() {
    let r = registry();
    // Seed two uniquely-named files so the glob can target them.
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir_rel = format!("target/mcp-tests/glob-{}-{}", std::process::id(), stamp);
    std::fs::create_dir_all(&dir_rel).expect("create glob dir");
    std::fs::write(format!("{dir_rel}/a.toml"), "x").unwrap();
    std::fs::write(format!("{dir_rel}/b.txt"), "y").unwrap();

    let out = call_ok(
        &r,
        "fs_glob",
        json!({ "pattern": "**/*.toml", "path": dir_rel.clone() }),
    );
    let matches = out["matches"].as_array().unwrap();
    assert!(
        matches
            .iter()
            .any(|m| m.as_str().unwrap().ends_with("a.toml")),
        "should match the .toml file: {matches:?}"
    );
    assert!(
        !matches
            .iter()
            .any(|m| m.as_str().unwrap().ends_with("b.txt")),
        "should not match the .txt file"
    );

    let _ = std::fs::remove_dir_all(&dir_rel);
}

#[test]
fn fs_find_matches_name_regex() {
    let r = registry();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir_rel = format!("target/mcp-tests/find-{}-{}", std::process::id(), stamp);
    std::fs::create_dir_all(&dir_rel).expect("create find dir");
    std::fs::write(format!("{dir_rel}/config.json"), "{}").unwrap();
    std::fs::write(format!("{dir_rel}/readme.md"), "#").unwrap();

    let out = call_ok(
        &r,
        "fs_find",
        json!({ "name_pattern": r"\.json$", "path": dir_rel.clone() }),
    );
    let matches = out["matches"].as_array().unwrap();
    assert!(matches
        .iter()
        .any(|m| m.as_str().unwrap().ends_with("config.json")));
    assert!(!matches
        .iter()
        .any(|m| m.as_str().unwrap().ends_with("readme.md")));

    let _ = std::fs::remove_dir_all(&dir_rel);
}

#[test]
fn fs_tree_builds_nested_structure() {
    let r = registry();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir_rel = format!("target/mcp-tests/tree-{}-{}", std::process::id(), stamp);
    std::fs::create_dir_all(format!("{dir_rel}/sub")).expect("create tree dirs");
    std::fs::write(format!("{dir_rel}/top.txt"), "x").unwrap();
    std::fs::write(format!("{dir_rel}/sub/leaf.txt"), "y").unwrap();

    let out = call_ok(
        &r,
        "fs_tree",
        json!({ "path": dir_rel.clone(), "max_depth": 3 }),
    );
    let tree = &out["tree"];
    assert_eq!(tree["type"], json!("dir"));
    let children = tree["children"].as_array().unwrap();
    assert!(!children.is_empty(), "tree root should have children");

    let _ = std::fs::remove_dir_all(&dir_rel);
}

#[test]
fn fs_tools_reject_path_escape() {
    let r = registry();
    // Absolute and parent-escaping paths must be refused by the workspace guard.
    let abs_err = call_err(&r, "fs_stat", json!({ "path": "/etc/passwd" }));
    assert!(
        abs_err.contains("absolute") || abs_err.contains("escapes"),
        "got: {abs_err}"
    );

    let parent_err = call_err(&r, "fs_stat", json!({ "path": "../../../secret" }));
    assert!(parent_err.contains("escapes"), "got: {parent_err}");
}

// ── registry-wide sanity ────────────────────────────────────────────────────

#[test]
fn registry_exposes_full_tool_count() {
    let r = registry();
    assert_eq!(r.len(), 66, "Week 10 brings the built-in tool count to 66");
}
