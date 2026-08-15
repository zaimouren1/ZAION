# Operation Stream Panel Consumers Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Telegram and TUI render wake-produced `operation.event` records with the same explicit, user-visible tool-call line.

**Architecture:** Add a small shared panel renderer for `OperationEvent` so every live panel consumes the runtime-owned event instead of formatting its own guess. Telegram keeps its existing `collect_wake_reply` transcript path, and TUI keeps its existing `StreamEvent::Operation` drain path, but both call the same renderer for `ToolCallVisible`, `ToolProgress`, `ToolReceiptProduced`, degraded, aborted, and quarantine events.

**Tech Stack:** Rust, `zaion-runtime::operation_stream`, `zaion-cli` Telegram/TUI consumers, doctor source gates, Markdown architecture ledgers.

---

### Task 1: Shared Operation Panel Renderer

**Files:**
- Create: `crates/zaion-cli/src/commands/panel_render.rs`
- Modify: `crates/zaion-cli/src/commands/mod.rs`
- Modify: `crates/zaion-cli/src/commands/network/telegram_panel.rs`
- Test: `crates/zaion-cli/src/commands/panel_render.rs`

- [ ] **Step 1: Write the failing test**

Add unit tests that build a `ToolCallVisible` operation event with:

```rust
serde_json::json!({
    "tool_name": "database_query",
    "input_preview": {"sql": "SELECT region, revenue FROM sales WHERE quarter = 'Q2'"}
})
```

The rendered text must contain `database_query (执行中...)`, the SQL preview, and must not contain raw English status text like `(running)`.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p zaion-cli panel_render -- --nocapture`

Expected: FAIL because `panel_render` does not exist yet.

- [ ] **Step 3: Implement minimal renderer**

Create `render_operation_panel_event(&OperationEvent) -> String`.

Rules:
- `ToolCallVisible`: `🛠️ <tool_name> (执行中...)\n│ → <preview>`
- If `input_preview` is an object with one key, render only the value after the arrow, so SQL appears as `SELECT ...` instead of a JSON object dump.
- If `input_preview` is an object with multiple keys, render pretty JSON.
- `ToolProgress`: `🛠️ <display_text> (进行中...)`
- `ToolReceiptProduced`: `✅ <display_text> (已完成)`
- `TurnDegraded`: `⚠️ <display_text> (降级)`
- `TurnAborted`: `⛔ <display_text> (已中止)`
- `Quarantined`: `🔒 <display_text> (隔离)`
- Other events: return `display_text`.

- [ ] **Step 4: Keep Telegram wrapper stable**

Make `render_telegram_operation_event()` delegate to the shared renderer. Do not remove the public Telegram helper yet; existing callers and doctor gates expect it.

- [ ] **Step 5: Verify task**

Run: `cargo test -p zaion-cli panel_render -- --nocapture`

Expected: PASS.

### Task 2: Telegram Consumer Contract

**Files:**
- Modify: `crates/zaion-cli/src/commands/network/telegram.rs`
- Modify: `crates/zaion-cli/src/commands/network/telegram_panel.rs`
- Test: `crates/zaion-cli/src/commands/network/telegram.rs`
- Test: `crates/zaion-cli/src/commands/network/telegram_panel.rs`

- [ ] **Step 1: Write the failing test**

Add a Telegram transcript test that sends `StreamEvent::Operation(ToolCallVisible)` through a local channel, calls `collect_wake_reply`, and asserts the visible reply contains:

```text
🛠️ database_query (执行中...)
│ → SELECT region, revenue FROM sales WHERE quarter = 'Q2'
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p zaion-cli telegram_operation_event -- --nocapture`

Expected: FAIL before the shared renderer is wired into Telegram's transcript path.

- [ ] **Step 3: Wire Telegram to shared renderer**

Keep the existing `StreamEvent::Operation(event)` match arm, but ensure it calls the shared renderer through `render_telegram_operation_event()` and pushes the rendered operation line into `WakeTranscript.notices`.

- [ ] **Step 4: Verify task**

Run: `cargo test -p zaion-cli telegram_operation_event -- --nocapture`

Expected: PASS.

### Task 3: TUI Consumer Contract

**Files:**
- Modify: `crates/zaion-cli/src/commands/process/tui/app.rs`
- Test: `crates/zaion-cli/src/commands/process/tui/app.rs`

- [ ] **Step 1: Write the failing test**

Add a TUI unit test that creates an `AppState`, attaches a local receiver containing a `StreamEvent::Operation(ToolCallVisible)`, drains events, and asserts the latest tool message content contains:

```text
🛠️ database_query (执行中...)
│ → SELECT region, revenue FROM sales WHERE quarter = 'Q2'
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p zaion-cli tui_operation_event -- --nocapture`

Expected: FAIL because TUI currently uses raw `event.display_text` for operation tool messages.

- [ ] **Step 3: Wire TUI to shared renderer**

In the `StreamEvent::Operation(event)` arm, compute `let rendered = render_operation_panel_event(&event)` and push the rendered text for tool/degraded/aborted/quarantine events. Keep `status_text` as the original concise `event.display_text`.

- [ ] **Step 4: Verify task**

Run: `cargo test -p zaion-cli tui_operation_event -- --nocapture`

Expected: PASS.

### Task 4: Source Gates And Truth Ledgers

**Files:**
- Modify: `crates/zaion-cli/src/commands/system.rs`
- Modify: `crates/zaion-cli/tests/cli_stable_surface.rs`
- Modify: `plans/openclaw_latest_gap_report.md`
- Modify: `plans/hermes_surpass_master_plan.md`
- Modify: `MASTER_PLAN.md`
- Modify: `plans/ZAION_ARCHITECTURE_SOURCE_AUDIT.md`

- [ ] **Step 1: Add source gates**

Extend the architecture contract doctor gate to require:
- `render_operation_panel_event`
- `执行中`
- `StreamEvent::Operation(event)` in Telegram and TUI consumers
- TUI calls `render_operation_panel_event(&event)`

- [ ] **Step 2: Update truth documents**

Record this phase as `Operation Stream Panel Consumers [PARTIAL-SURPASSED]`.

Closed scope:
- Telegram and TUI consume wake-produced operation events through one shared renderer.
- Visible tool calls show tool name, execution status, and safe input preview.

Remaining scope:
- ACP/MCP/webhook direct producer migration.
- Cross-process persisted operation stream storage.
- Full live WebSocket or long-poll endpoint.
- Global ledger operation-backlog replay.

- [ ] **Step 3: Verify source gates**

Run:

```powershell
cargo test -p zaion-cli doctor_source_gate_locks_architecture_contract_implementation_plan --test cli_stable_surface -- --nocapture
```

Expected: PASS.

### Task 5: Phase Verification

**Files:**
- No new edits unless verification exposes a defect.

- [ ] **Step 1: Format check**

Run: `cargo fmt --package zaion-runtime --package zaion-cli --check`

- [ ] **Step 2: Runtime operation tests**

Run: `cargo test -p zaion-runtime operation_stream -- --nocapture`

- [ ] **Step 3: Panel renderer tests**

Run: `cargo test -p zaion-cli panel_render -- --nocapture`

- [ ] **Step 4: Telegram panel tests**

Run: `cargo test -p zaion-cli telegram_panel -- --nocapture`

- [ ] **Step 5: Telegram consumer tests**

Run: `cargo test -p zaion-cli telegram_operation_event -- --nocapture`

- [ ] **Step 6: TUI consumer tests**

Run: `cargo test -p zaion-cli tui_operation_event -- --nocapture`

- [ ] **Step 7: Doctor gates**

Run: `cargo test -p zaion-cli doctor_source_gate_locks_architecture_contract_implementation_plan --test cli_stable_surface -- --nocapture`

- [ ] **Step 8: Build check**

Run: `cargo check -p zaion-cli`

- [ ] **Step 9: Diff whitespace check**

Run: `git diff --check`

---

## Self-Review

- Spec coverage: covers the approved user-facing panel experience for explicit tool visibility in Telegram and TUI using the same operation event renderer.
- Placeholder scan: no TBD/TODO/later placeholders are used.
- Type consistency: the plan uses existing `StreamEvent::Operation`, `OperationEventKind::ToolCallVisible`, `render_telegram_operation_event`, and TUI `drain_events` names.
