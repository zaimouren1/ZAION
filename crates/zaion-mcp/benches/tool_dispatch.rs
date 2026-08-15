//! Criterion benchmarks for the MCP tool hot path.
//!
//! Establishes baselines for the operations that run on every agentic-loop
//! tool invocation, deliberately excluding ledger I/O so the numbers reflect
//! CPU cost of the engine itself rather than disk/signing latency:
//!
//!   1. `register_builtin_tools` — one-time registry construction cost
//!   2. registry lookup (`get`) — per-call name resolution
//!   3. pure tool execution (`McpTool::call`) for representative tools
//!
//! Run with: `cargo bench -p zaion-mcp`

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use serde_json::json;
use zaion_mcp::{register_builtin_tools, McpToolRegistry};

fn build_registry() -> McpToolRegistry {
    let mut r = McpToolRegistry::new();
    register_builtin_tools(&mut r);
    r
}

/// One-time registry construction: how expensive is wiring all 66 tools?
fn bench_registry_build(c: &mut Criterion) {
    c.bench_function("register_builtin_tools", |b| {
        b.iter(|| {
            let r = build_registry();
            black_box(r.len());
        });
    });
}

/// Per-call lookup cost across a hit and a miss.
fn bench_registry_lookup(c: &mut Criterion) {
    let r = build_registry();
    let mut group = c.benchmark_group("registry_get");
    for name in ["fs_stat", "time_now", "json_query", "does_not_exist"] {
        group.bench_with_input(BenchmarkId::from_parameter(name), name, |b, name| {
            b.iter(|| black_box(r.get(black_box(name)).is_some()));
        });
    }
    group.finish();
}

/// Pure (no I/O) tool execution — the engine's per-call compute cost.
fn bench_tool_execution(c: &mut Criterion) {
    let r = build_registry();
    let mut group = c.benchmark_group("tool_call");

    let cases: &[(&str, serde_json::Value)] = &[
        ("time_now", json!({})),
        ("uuid_generate", json!({})),
        ("hash_text", json!({ "text": "the quick brown fox" })),
        (
            "base64_encode",
            json!({ "text": "the quick brown fox jumps over the lazy dog" }),
        ),
        (
            "json_query",
            json!({ "text": r#"{"items":[{"name":"a"},{"name":"b"}]}"#, "path": "items[1].name" }),
        ),
        (
            "text_regex_replace",
            json!({ "text": "a1b2c3d4e5", "pattern": "[0-9]", "replacement": "#" }),
        ),
        (
            "url_parse",
            json!({ "url": "https://example.com:8443/a/b?x=1&y=2" }),
        ),
    ];

    for (name, input) in cases {
        let tool = r.get(name).expect("benchmark tool must be registered");
        group.bench_with_input(BenchmarkId::from_parameter(name), input, |b, input| {
            b.iter(|| {
                let out = tool.call(black_box(input.clone()));
                black_box(out.is_ok());
            });
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_registry_build,
    bench_registry_lookup,
    bench_tool_execution
);
criterion_main!(benches);
