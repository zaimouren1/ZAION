# Runtime Batch Runner Execution Chain Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the runtime `batch_runner` mainline gap by replacing placeholder trajectory generation with an explicit, injectable prompt execution chain.

**Architecture:** `zaion-runtime` remains independent from `zaion-opd`; the runtime batch facade accepts a caller-supplied executor closure for real prompt execution. The default constructor becomes a safe boundary that refuses to run instead of producing fake ShareGPT data, while `with_executor` writes real execution results, token counts, tool usage, success/failure state, and checkpoints.

**Tech Stack:** Rust, serde/serde_json, chrono, cargo test/fmt/check, existing source-gate tests in `zaion-cli`.

---

### Task 1: Replace Placeholder Execution With Injected Executor

**Files:**
- Modify: `crates/zaion-runtime/src/batch_runner.rs`

- [ ] **Step 1: Write failing tests**

Add tests in the existing `#[cfg(test)]` module proving:

```rust
#[test]
fn default_runner_refuses_to_emit_placeholder_trajectories() {
    let dir = tempfile::tempdir().unwrap();
    let config = BatchConfig {
        num_workers: 1,
        checkpoint_path: dir.path().join("checkpoint.json"),
        output_path: dir.path().join("trajectories.jsonl"),
        prompts: vec!["hello".into()],
        toolset_distribution: vec![],
    };

    let runner = BatchRunner::new(config);
    let err = runner.run().expect_err("default runner should require an executor");
    assert!(err.contains("BatchRunner requires an explicit prompt executor"));
    assert!(!err.contains("placeholder"));
}

#[test]
fn injected_executor_produces_real_trajectory_and_checkpoint() {
    let dir = tempfile::tempdir().unwrap();
    let output_path = dir.path().join("trajectories.jsonl");
    let checkpoint_path = dir.path().join("checkpoint.json");
    let config = BatchConfig {
        num_workers: 1,
        checkpoint_path: checkpoint_path.clone(),
        output_path: output_path.clone(),
        prompts: vec!["summarize ledger".into()],
        toolset_distribution: vec![ToolsetSample {
            tools: vec!["ledger.read".into(), "memory.search".into()],
            weight: 1.0,
        }],
    };

    let runner = BatchRunner::with_executor(config, |request| {
        assert_eq!(request.index, 0);
        assert_eq!(request.prompt, "summarize ledger");
        assert_eq!(request.tools, vec!["ledger.read", "memory.search"]);
        Ok(BatchExecutionResult {
            assistant_message: "real executor response".into(),
            tools_used: vec!["ledger.read".into()],
            total_tokens: 42,
            success: true,
        })
    });

    let trajectories = runner.run().unwrap();
    assert_eq!(trajectories.len(), 1);
    assert_eq!(trajectories[0].messages[1].content, "real executor response");
    assert_eq!(trajectories[0].tools_used, vec!["ledger.read"]);
    assert_eq!(trajectories[0].total_tokens, 42);
    assert!(trajectories[0].success);

    let output = std::fs::read_to_string(output_path).unwrap();
    assert!(output.contains("real executor response"));
    assert!(!output.contains("EXPERIMENTAL placeholder response"));

    let checkpoint = std::fs::read_to_string(checkpoint_path).unwrap();
    assert!(checkpoint.contains("completed_indices"));
    assert!(checkpoint.contains("0"));
}
```

- [ ] **Step 2: Run red test**

Run: `cargo test -p zaion-runtime batch_runner -- --nocapture`
Expected: compile/test failure because `with_executor`, `BatchExecutionRequest`, and `BatchExecutionResult` do not exist and default runner still emits placeholder data.

- [ ] **Step 3: Implement minimal execution boundary**

In `batch_runner.rs`:
- Add `BatchExecutionRequest { prompt, index, tools }`.
- Add `BatchExecutionResult { assistant_message, tools_used, total_tokens, success }`.
- Store `executor: Arc<dyn Fn(BatchExecutionRequest) -> Result<BatchExecutionResult, String> + Send + Sync>` in `BatchRunner`.
- Make `BatchRunner::new` install a default executor that returns `BatchRunner requires an explicit prompt executor; use BatchRunner::with_executor(...) to run real LLM/tool execution`.
- Add `BatchRunner::with_executor`.
- Replace `process_prompt` placeholder with a call into the executor.
- Select tools from `toolset_distribution`, using the highest positive weight sample for deterministic runtime facade behavior.
- Only write checkpoint success when the executor returns `success: true`; failed executor results should produce a trajectory with `success: false` and record the failed index.

- [ ] **Step 4: Run green test**

Run: `cargo test -p zaion-runtime batch_runner -- --nocapture`
Expected: all `batch_runner` tests pass.

---

### Task 2: Add Source Gates And Ledger Truth

**Files:**
- Modify: `crates/zaion-cli/src/commands/system.rs`
- Modify: `crates/zaion-cli/tests/cli_stable_surface.rs`
- Modify: `MASTER_PLAN.md`
- Modify: `plans/openclaw_latest_gap_report.md`
- Modify: `plans/hermes_surpass_master_plan.md`

- [ ] **Step 1: Write failing source gate**

Add a stable-surface test that reads `crates/zaion-runtime/src/batch_runner.rs` and asserts:
- the old placeholder response string is absent,
- `BatchRunner::with_executor` exists,
- the explicit executor-required boundary exists,
- `BatchExecutionRequest` and `BatchExecutionResult` exist.

- [ ] **Step 2: Run red gate**

Run: `cargo test -p zaion-cli doctor_source_gate_locks_runtime_batch_runner_execution_chain --test cli_stable_surface -- --nocapture`
Expected: failure before the source gate implementation is mirrored in `system.rs`, or before code/doc strings are present.

- [ ] **Step 3: Implement source gate and docs**

Mirror the same checks in `doctor_source_gate_locks_architecture_truth_sources` or a nearby source-gate function in `system.rs`.
Add top ledger entries named `Runtime BatchRunner Execution Chain [SURPASSED]` explaining:
- runtime batch runner no longer emits placeholder assistant responses,
- real LLM/tool execution is caller-injected through `with_executor`,
- default construction refuses to run without explicit executor,
- OPD remains experimental and stable CLI promotion is still gated.

- [ ] **Step 4: Verify gates**

Run:
- `cargo test -p zaion-cli doctor_source_gate_locks_runtime_batch_runner_execution_chain --test cli_stable_surface -- --nocapture`
- `cargo test -p zaion-cli doctor_source_gate_locks_architecture_truth_documents --test cli_stable_surface -- --nocapture`

Expected: both pass.

---

### Task 3: Final Verification

**Files:**
- All modified files from Tasks 1-2

- [ ] **Step 1: Format check**

Run: `cargo fmt --package zaion-runtime --package zaion-cli --check`
Expected: exit 0.

- [ ] **Step 2: Compile checks**

Run:
- `cargo check -p zaion-runtime`
- `cargo check -p zaion-cli`

Expected: exit 0. Pre-existing warnings may remain, but no new errors.

- [ ] **Step 3: Diff hygiene**

Run: `git diff --check -- crates/zaion-runtime/src/batch_runner.rs crates/zaion-cli/src/commands/system.rs crates/zaion-cli/tests/cli_stable_surface.rs MASTER_PLAN.md plans/openclaw_latest_gap_report.md plans/hermes_surpass_master_plan.md`
Expected: exit 0 or only pre-existing CRLF warnings in ledger markdown files.
