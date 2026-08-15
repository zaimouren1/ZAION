# P2 · P0 Compile Fixes + 8 CRITICAL Security Fixes + Ledger Truth Calibration (A+D)

- Date: 2026-04-18
- Reviewer / Drafter: main context (Cascade, opus[1m])
- Scope: Option A (P0 compile + 8 CRITICAL) + Option D (ledger truth rollback)
- Writers: four independent subagent contexts (E / F / G / H), strictly separated from Reviewer
- Approval status: **PENDING** — requires user written "批准 / approve" before Writer dispatch
- Source: `review报告/CODE_REVIEW_REPORT.md` + live `cargo check --workspace` output + grep verification
- Rollback anchor: a git snapshot commit will be created BEFORE any writer dispatches

---

## 0. Ground truth calibration vs. review report

Before the writers move, I verified each claim against the live tree. Two corrections to the review report:

1. **Compile error locus was mis-stated.** The report blames `execute_code_uds.rs:333` (undefined `tool_name`) and `execute_code_js.rs:227` (missing format placeholder). Live `cargo check --workspace` at 2026-04-18 actually reports:
   - `E0063` at `crates/zaion-cli/src/commands/slash_integration.rs:48` — `SlashCommandContext` missing three fields (`current_session_id`, `display_config`, `session_brancher`).
   - `E0308` at `crates/zaion-cli/src/commands/slash_integration.rs:54` — passing `&SlashCommandContext` where `&mut` is required by `execute_slash_command` in `zaion-runtime/src/slash_commands.rs:118`.
   - `execute_code_uds.rs` and `execute_code_js.rs` now pass rustc — those errors were already fixed independently. HIGH #15/#16 in the report should be marked resolved during the ledger pass.

2. **Three CRITICAL items re-confirmed verbatim** (grep proof):
   - CRITICAL #4: `mcp_bridge.rs:336-337` still has `"principal_placeholder".to_string()` / `"signature_placeholder".to_string()`.
   - CRITICAL #5: `unified_agent_runtime.rs:303-305` still defines `fn sign_turn(...) -> String { format!("sig_{}_{}", ...) }`.
   - CRITICAL #6: `reference.rs:131-134` still has `let _ = (canonical, base_canonical); // both computed, traversal check skipped for now`.

The remaining five CRITICAL items (#1-3, #7-8) are taken on the report's word for now; each writer verifies their target before editing.

---

## 1. Goals

1. `cargo check --workspace` green (currently 2 errors, 69 warnings — we only gate on errors for this P2).
2. Every one of the 8 CRITICAL items either:
   - **fixed**, with new unit/integration tests proving the fix, or
   - **gated behind a `cfg(feature = "unsafe_shell_stub")`** off by default, with a compile-time warning if the feature is on, **only** if a proper fix is out of scope of this P2 and explicit user approval is granted to defer it.
3. `plans/openclaw_latest_gap_report.md` SURPASSED/PARTIAL/TODO flags realigned so downstream Phase planning starts from truth, not from placeholder code claiming to be Ed25519.
4. `MASTER_PLAN.md §8.1` mirror updated with `[LEDGER-CALIBRATED-2026-04-18]` governance event.

---

## 2. Issue inventory (re-verified)

### P0 compile errors (Writer-E)

| ID | File:Line | Problem | Proposed fix direction |
|----|-----------|---------|-----------------------|
| E0063 | `crates/zaion-cli/src/commands/slash_integration.rs:48` | `SlashCommandContext` literal missing `current_session_id`, `display_config`, `session_brancher` | Before editing, Writer-E reads the canonical `SlashCommandContext` definition in `zaion-runtime/src/slash_commands.rs` and fills the three fields with semantically correct defaults drawn from the caller's existing state; no placeholder values. |
| E0308 | `crates/zaion-cli/src/commands/slash_integration.rs:54` | `&ctx` where `&mut ctx` required | Change the local binding to `let mut ctx = ...;` and pass `&mut ctx`. If `execute_slash_command` mutates long-lived state, Writer-E verifies the mutation is safe (no aliasing) and documents it in WRITER_E_NOTES. |

### CRITICAL list (Writers F / G / H)

| # | Title | File | Owner |
|---|-------|------|-------|
| 1 | Shell command injection — `ShadowTask.command` unsandboxed | `crates/zaion-shadow/src/executor.rs:470-499` | Writer-F |
| 2 | Shell injection — `execute_terminal` passes user string to `sh -c` | `crates/zaion-opd/src/tool_executor.rs:182-186` | Writer-F |
| 3 | Arbitrary code execution — `cargo check` after patch allows malicious `build.rs` | `crates/zaion-evolve/src/applier.rs:57-70` | Writer-F |
| 4 | Fake Ed25519 — `"principal_placeholder"` / `"signature_placeholder"` | `crates/zaion-runtime/src/mcp_bridge.rs:336-337` | Writer-G |
| 5 | Fake signing — `sign_turn` returns `format!("sig_{}_{}")` | `crates/zaion-runtime/src/unified_agent_runtime.rs:303-305` | Writer-G |
| 6 | Path-traversal guard disabled — `let _ = (canonical, base_canonical);` | `crates/zaion-runtime/src/reference.rs:131-134` | Writer-H |
| 7 | Master key not zeroized on Drop | `crates/zaion-secrets/src/store.rs:38-41` | Writer-H |
| 8 | API key serialized plaintext to disk | `crates/zaion-federation/src/honcho.rs:32` | Writer-H |

---

## 3. Team split (four writers in parallel, one reviewer)

### Writer-E — build doctor (P0 compile fix)

- Model: sonnet
- Isolation: independent subagent; touches only `crates/zaion-cli/src/commands/slash_integration.rs` (plus a test file under `crates/zaion-cli/tests/` if needed)
- Hard requirements:
  1. Read `crates/zaion-runtime/src/slash_commands.rs` to understand the `SlashCommandContext` contract (all fields, lifetimes, mutability).
  2. Supply **real** values for `current_session_id`, `display_config`, `session_brancher` drawn from the caller's state or a documented default. No `Default::default()` on complex types without reading what default actually means.
  3. Fix the mutability — either `let mut ctx = ...;` or refactor to pass a builder.
  4. Add one integration test that constructs a `SlashCommandContext` and round-trips it through `execute_slash_command` with a no-op command. Test **must** fail without the fix.
  5. `cargo check --workspace` MUST be green after the change. Paste full output into WRITER_E_NOTES.md.
- Forbidden: touching any other crate; adding `#[allow(...)]`; commenting out callers to make it compile.
- Deliverable: the edited file + `tests/slash_integration_smoke.rs` + `plans/drafts/WRITER_E_NOTES.md`.

### Writer-F — sandbox the shell (CRITICAL #1 / #2 / #3)

- Model: sonnet
- Isolation: independent subagent; touches `zaion-shadow`, `zaion-opd`, `zaion-evolve` only
- Hard requirements:
  1. **CRITICAL #1** `ShadowTask.command`:
     - Introduce `CommandSpec { program: String, args: Vec<String>, env: BTreeMap<String,String>, cwd: PathBuf }`; **NEVER** concatenate into a shell string.
     - Replace `sh -c "..."` with `std::process::Command::new(program).args(args)...`.
     - Reject any program not in an explicit allow-list (configurable via `ShadowConfig.allowed_programs`, default empty → fail closed).
     - Unit tests: allow-list hit, allow-list miss, env/cwd isolation, shell metacharacter in args remains literal.
  2. **CRITICAL #2** `execute_terminal`:
     - Same `CommandSpec` treatment; no `sh -c`.
     - Add argv-level parse (prefer `shell-words` crate already in workspace or pull it) + allow-list.
     - Unit test: `"rm -rf /"` in user input stays inert because the program `rm` is not in the allow-list (or, if allow-listed, args are passed literally and the test asserts the fake FS under `tempdir()` was untouched).
  3. **CRITICAL #3** `cargo check` in `applier.rs`:
     - Either (a) run `cargo check` inside a subprocess with `--offline --locked --frozen` plus `CARGO_TARGET_DIR` pinned to a temp dir AND environment stripped of network credentials, or (b) switch the gate to `cargo metadata --format-version 1 --no-deps --offline` which does NOT compile `build.rs`.
     - Default to (b); (a) only if syntax-only is insufficient. Document choice in WRITER_F_NOTES.
     - Unit test: a synthetic `build.rs` that writes a sentinel file must NOT create the file during the gate.
- Forbidden: editing unrelated files; silencing warnings; stubbing out the feature with `unimplemented!`.
- Deliverable: edited files + tests + `plans/drafts/WRITER_F_NOTES.md`.

### Writer-G — real signatures (CRITICAL #4 / #5)

- Model: sonnet
- Isolation: independent subagent; touches `zaion-runtime` (specifically `mcp_bridge.rs`, `unified_agent_runtime.rs`) and `zaion-crypto` if an API gap is found
- Hard requirements:
  1. **CRITICAL #4** `McpProvenance`:
     - Inject an `Arc<SigningKey>` (ed25519-dalek v2) into the runtime at construction; pulled from `zaion-secrets::KeyStore` at boot.
     - `principal_id` = DID or base64url(verifying_key).
     - `ed25519_signature` = base64url(signing_key.sign(canonical_bytes)) where `canonical_bytes` is a deterministic serialization of (tool, input, output, timestamp). Use `ciborium` canonical or a hash-then-sign over SHA-256.
     - Add a `verify_provenance(&self, verifying_key: &VerifyingKey) -> Result<(), McpError>` method.
     - Unit tests: sign→verify roundtrip, tamper detection on each field, missing key fails closed.
  2. **CRITICAL #5** `sign_turn`:
     - Same pattern. Replace the length-concat string with ed25519 signature over SHA-256(user_message || 0x1F || response || 0x1F || turn_id || 0x1F || timestamp).
     - Return a typed `TurnSignature { scheme: "ed25519-sha256-v1", signature: Vec<u8>, signing_key_id: String }`, not `String`.
     - Add `verify_turn` companion; wire into existing `unified_agent_runtime` tests.
  3. Re-emit a trace-log entry every time a signature is produced (so Reviewer can sanity-check production).
- Forbidden: touching `zaion-cli` / `zaion-evolve` / `zaion-shadow`; introducing new blocking reqwest; committing any `todo!()`.
- Deliverable: edited files + tests + `plans/drafts/WRITER_G_NOTES.md`.

### Writer-H — paths and secrets (CRITICAL #6 / #7 / #8)

- Model: sonnet
- Isolation: independent subagent; touches `zaion-runtime/src/reference.rs`, `zaion-secrets/src/store.rs`, `zaion-federation/src/honcho.rs`
- Hard requirements:
  1. **CRITICAL #6** path traversal:
     - Replace `let _ = (canonical, base_canonical);` with an actual `canonical.starts_with(&base_canonical)` check, returning `Err(ReferenceError::EscapesBase { requested, base })`.
     - On Windows: also verify same volume (`Path::components().next()` disk letter match) to block `\\?\...` tricks.
     - Unit tests: `..` escape, symlink escape (create via `std::os::windows::fs::symlink_dir` / `std::os::unix::fs::symlink` guarded by `cfg`), happy path.
  2. **CRITICAL #7** master key zeroize:
     - Add `zeroize = { workspace = true, features = ["derive"] }` (if not already workspace-dep); derive `ZeroizeOnDrop` on `EncryptedStore` (or wrap `cipher_key` in `Zeroizing<[u8; 32]>`).
     - Unit test: after dropping, a best-effort inspection of the freed region returns zeros (use a `Vec<u8>` wrapper and check post-drop via a follow-up allocation pattern; OR assert `Zeroizing` bound in code via type-level assertion).
  3. **CRITICAL #8** API key plaintext:
     - Remove `#[serde(flatten)]` / `serialize` on `api_key`. Instead, `api_key: SecretString` (use `secrecy` crate, workspace dep if present; else add).
     - Replace the on-disk representation with a pointer: `HonchoConfig.api_key_source: ApiKeySource::SecretsStore { alias: String }` and load at runtime via `zaion-secrets`.
     - Unit test: round-trip serialize/deserialize must NOT contain the plaintext; a probe `fs::read_to_string(config_path).unwrap().contains(api_key)` must be false.
- Forbidden: editing any other crate; hard-coding a key; adding an `#[allow(dead_code)]` just to make it compile.
- Deliverable: edited files + tests + `plans/drafts/WRITER_H_NOTES.md`.

---

## 4. Reviewer acceptance matrix (main context — me)

After all four writers deliver, Reviewer runs:

1. `cargo check --workspace 2>&1 | tee /tmp/cargo_check.log` — MUST exit 0, 0 errors. Warning delta OK if non-increasing.
2. `cargo test -p zaion-cli -p zaion-shadow -p zaion-opd -p zaion-evolve -p zaion-runtime -p zaion-secrets -p zaion-federation 2>&1 | tee /tmp/cargo_test.log` — all new tests green, no prior test regressed.
3. Grep regressions — every one of these greps MUST return 0 matches:
   - `rg -n 'principal_placeholder\|signature_placeholder' crates/`
   - `rg -n 'format!\("sig_\{' crates/`
   - `rg -n 'let _ = \(canonical, base_canonical\)' crates/`
   - `rg -n 'sh -c' crates/zaion-shadow crates/zaion-opd`
   - `rg -n 'cargo check' crates/zaion-evolve/src/applier.rs` (expect zero, or guarded by offline+locked)
4. Grey-box end-to-end:
   - Submit an MCP tool call → confirm `McpProvenance.ed25519_signature` verifies against `VerifyingKey` from `zaion-secrets`.
   - Submit a unified agent turn → confirm `TurnSignature` verifies.
   - Attempt to reference `../../../../../etc/passwd` → MUST return `EscapesBase`.
5. Ledger calibration (Option D):
   - Open `plans/openclaw_latest_gap_report.md`.
   - For every entry that referenced "signed provenance / Ed25519 turn ledger / path-traversal reference safety" as SURPASSED or COMPLETED, append a calibration note: `[CALIBRATED 2026-04-18] previously claimed SURPASSED under placeholder implementation; now [SURPASSED|PARTIAL|TODO] after WRITER_G / WRITER_H landing`.
   - Also record HIGH #15 / #16 as resolved if our own grep proves the `execute_code_uds.rs:333` / `execute_code_js.rs:227` errors no longer exist.
   - Mirror one-paragraph summary into `MASTER_PLAN.md §8.1` as `[LEDGER-CALIBRATED-2026-04-18]`.

Any failure → bounce to the responsible writer. Reviewer MUST NOT hand-edit writer code.

---

## 5. Risk and rollback

| Risk | Mitigation |
|------|-----------|
| R-1 Fixing shell sandboxing breaks existing `ShadowTask` callers | Writer-F adds a migration doc + keeps old signature behind `#[deprecated]` for one release, returning an error at runtime if used. |
| R-2 Adding ed25519 signing breaks persisted state | Writer-G bumps a `schema_version` field in the ledger/provenance record; reads tolerate legacy unsigned records but mark them `Unverified`. |
| R-3 Path traversal guard breaks legitimate relative paths | Writer-H adds explicit `ReferenceBuilder::allow_symlinks(bool)` opt-in; default false. |
| R-4 Zeroize derive bumps MSRV | zeroize 1.8 supports stable 1.72+; check workspace MSRV first. |
| R-5 Writer collisions on `Cargo.toml` / workspace deps | Writers E/F/G/H each write to `plans/drafts/pending_cargo_toml_edits.md` with their requested deps; Reviewer applies them sequentially at acceptance time to avoid merge conflicts. |
| R-6 Four parallel writers consume context faster than expected | Use haiku for Writer-E (mechanical fix), sonnet for F/G/H (security reasoning). |

**Rollback anchor**: Reviewer runs `git add -A && git commit -m "snapshot: pre-P2-p0-critical-20260418"` BEFORE dispatching. Any writer disaster is recovered with `git reset --hard HEAD^` (guarded by pre-tool-guard.sh — Reviewer must be explicit when running reset).

---

## 6. Timeline

1. **T+0**: This plan written.
2. **T+∈**: User "批准" or "approve" → Reviewer creates git snapshot.
3. **T+0..60min**: Writers E / F / G / H dispatched in parallel; each writes notes + tests + code.
4. **T+60..90min**: Reviewer acceptance matrix (§4). If pass → step 5. If fail → bounce.
5. **T+90..120min**: Ledger calibration (Option D) landing.
6. **T+done**: Final summary to user, including the rotated-token reminder repeated.

---

## 7. Out-of-scope acknowledgments

This P2 does NOT attempt:

- HIGH / MEDIUM / LOW items from the review report (except HIGH #15/#16 which happen to be already fixed and will just be ledger-aligned).
- Splitting `zaion-runtime` (14,000+ lines) — separate P2 needed.
- Fixing `zaion-core → zaion-runtime` reverse dependency — separate P2.
- Rotating the plaintext `ANTHROPIC_AUTH_TOKEN` in `~/.claude/settings.json` — user-level action, reminder only.
- Global plugin hook C-0 — cross-repo.

---

## 8. Approval line

User please respond with exactly one of:

- `批准 A+D 原案` — dispatch as specified.
- `批准 A+D，改点：...` — I incorporate the change, re-confirm, then dispatch.
- `驳回，理由：...` — I rewrite.

-- END --
