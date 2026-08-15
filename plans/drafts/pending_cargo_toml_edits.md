# Pending Cargo.toml Edits — Writer-F

**Date:** 2026-04-18

## Already Applied

| Crate | Change | Status |
|-------|--------|--------|
| `crates/zaion-opd/Cargo.toml` | Added `shell-words = "1.1"` to `[dependencies]` | ✅ Done |

## No Further Workspace Changes Required

- `crates/zaion-shadow/Cargo.toml` — `command_spec.rs` uses only `tokio` (already present) and `std`; no new deps.
- `crates/zaion-evolve/Cargo.toml` — `cargo metadata` is invoked via `std::process::Command`; no new deps.
- Root `Cargo.toml` — no workspace-level changes needed; `shell-words` is crate-local to `zaion-opd`.
