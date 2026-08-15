# Writer-F Notes — P0 Security Fixes (§3)

**Date:** 2026-04-18  
**Scope:** `plans/fix_p0_critical_and_ledger_20260418.md §3`  
**Crates touched:** `zaion-shadow`, `zaion-opd`, `zaion-evolve`

---

## Summary of Completed Work

### CRITICAL #1 — `zaion-shadow`: Allow-list for process spawning

**File created:** `crates/zaion-shadow/src/command_spec.rs`  
**Files modified:** `crates/zaion-shadow/src/executor.rs`, `crates/zaion-shadow/src/lib.rs`, `crates/zaion-shadow/src/tests.rs`

**Problem:** `ShadowTask.command` was passed directly to process spawn with no validation.  
**Fix:**
- New `CommandSpec { program, args, env: BTreeMap, cwd: Option<PathBuf> }` struct.
- New `AllowList` (wraps `BTreeSet<String>`) with `is_allowed(&str) -> bool`.
- New `ProgramNotAllowed { program: String }` error type.
- `CommandSpec::into_tokio_command(&AllowList)` — rejects unlisted programs, uses `tokio::process::Command::new(prog).args(rest)`, never `sh -c`.
- `ExecutorConfig.allowed_programs: Vec<String>` added, default `Vec::new()` (fail-closed).
- `run_task_aci_gated` / `execute_inner` thread `&[String]` allow-list down the call chain.
- `lib.rs` exports: `CommandSpec`, `AllowList`, `ProgramNotAllowed`.

**Tests added (command_spec.rs):**
- `allowlist_hit_returns_ok`
- `allowlist_miss_returns_err`
- `shell_metacharacter_in_args_is_literal`
- `env_and_cwd_are_applied`
- `default_allowlist_is_fail_closed`

**Tests added (tests.rs):**
- `allowlist_miss_fails_task` — end-to-end: empty allow-list → task fails
- `shell_metacharacter_in_args_is_literal_exec` — `echo 'hello; rm -rf /'` passes literal arg

---

### CRITICAL #2 — `zaion-opd`: Remove `sh -c` from `execute_terminal`

**File modified:** `crates/zaion-opd/src/tool_executor.rs`  
**Cargo.toml modified:** `crates/zaion-opd/Cargo.toml` — added `shell-words = "1.1"`

**Problem:** `execute_terminal` passed the user-supplied command string to `Command::new("sh").arg("-c").arg(command)` — a direct shell-injection vector.  
**Fix:**
- `ToolExecutorFn` type changed to `fn(&str, &Value, &HashSet<String>) -> Result<String>`.
- `ToolExecutor` gains `allowed_programs: HashSet<String>` field (default empty = fail-closed).
- Builder method `with_allowed_programs(impl IntoIterator)` for ergonomic configuration.
- `execute_terminal` rewired:
  1. `shell_words::split(command)` — safe argv splitting, no shell expansion.
  2. Check `program` against `allowed_programs` — reject with descriptive error if absent.
  3. `Command::new(program).args(rest).output()` — direct exec, no shell.
- `execute_read_file` / `execute_write_file` gain `_allowed: &HashSet<String>` param (ignored).
- `execute()` dispatch passes `&self.allowed_programs`.
- Temp file paths fixed from Unix `/tmp/` to `std::env::temp_dir()` for Windows compatibility.

**Tests added:**
- `execute_terminal_blocks_unlisted_program` — empty allow-list → `Err` mentioning "allow-list"
- `execute_terminal_shell_metacharacters_are_literal` — `echo 'hello; rm -rf /'` → literal in stdout
- `with_allowed_programs_builder_sets_list` — verifies `echo`, `cat` allowed; `sh`, `bash` denied

---

### CRITICAL #3 — `zaion-evolve`: Replace `cargo check` with `cargo metadata`

**File modified:** `crates/zaion-evolve/src/applier.rs`

**Problem:** Post-apply `cargo check --quiet` compiles `build.rs` scripts, allowing a malicious patch to execute arbitrary code at validation time.  
**Fix:**
- New method `PatchApplier::cargo_metadata_gate(workspace_root: &Path) -> Result<(), String>`:
  - Runs `cargo metadata --format-version 1 --no-deps --offline`
  - No `build.rs` is compiled; only manifest TOML is parsed.
- Old `cargo_check` converted to deprecated shim that delegates to `cargo_metadata_gate`:
  ```rust
  #[deprecated(since = "0.1.0", note = "use cargo_metadata_gate")]
  pub fn cargo_check(workspace_root: &Path) -> Result<(), String> {
      Self::cargo_metadata_gate(workspace_root)
  }
  ```
- `apply_one` error message: `"cargo check failed:"` → `"metadata gate failed:"`.
- `apply_pending` string check: `"cargo check failed"` → `"metadata gate failed"`.
- Module doc updated with security rationale.
- Test `apply_with_check_reverts_on_bad_patch` rewritten: patches `Cargo.toml` with invalid TOML
  (detected by `cargo metadata`) rather than relying on type-checking.

**Tests added:**
- `cargo_metadata_gate_passes_for_valid_manifest` — valid `Cargo.toml` → `Ok`
- `cargo_metadata_gate_fails_for_missing_manifest` — no `Cargo.toml` → `Err`
- `cargo_metadata_gate_does_not_execute_build_rs` — sentinel file proves `build.rs` never compiled

---

## Verification Results

```
cargo check -p zaion-opd -p zaion-evolve -p zaion-shadow --offline
→ Finished (9 pre-existing warnings, none from our changes)

cargo test -p zaion-opd -p zaion-shadow
→ test result: ok. 71 passed; 0 failed

cargo test -p zaion-evolve
→ test result: ok. 40 passed; 0 failed

rg -n "sh -c" crates/zaion-shadow crates/zaion-opd crates/zaion-evolve
→ comments/docs only — no live sh -c calls

rg -n "cargo check" crates/zaion-evolve/src/applier.rs
→ comments/docs + deprecated shim definition only — no live cargo check calls
```

---

## Gap Report Status

All three CRITICAL items from §3 are **SURPASSED**:
- Fail-closed allow-lists (default empty, explicit opt-in required)
- No shell injection surface (`shell_words` + direct `Command::new`)
- No `build.rs` execution during patch validation (`cargo metadata` gate)
- Unit tests proving each security property
