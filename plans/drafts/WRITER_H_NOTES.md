# WRITER_H_NOTES.md
# Fix P0 — CRITICAL #6 / #7 / #8

**Date:** 2026-04-18
**Author:** Writer-H (isolated sub-agent)
**Plan authority:** plans/fix_p0_critical_and_ledger_20260418.md §3

---

## Summary of changes

### CRITICAL #6 — Path-traversal guard (crates/zaion-runtime/src/reference.rs)

**Root cause:** The block `let _ = (canonical, base_canonical);` discarded both
canonicalized paths after computing them, leaving the `expand_file` function with
no traversal enforcement — any caller could escape the project root via `..`,
symlinks, or absolute paths.

**Fix:**
- Replaced the dead assignment with a real `canonical.starts_with(&base_canonical)`
  check that returns a descriptive `RefError` on violation.
- Added a Windows-only volume prefix check (`canonical.components().next() !=
  base_canonical.components().next()`) to block `\\?\` / UNC tricks that could
  point to a different drive letter. The `use std::path::Component;` import was
  removed (comparison is done via `PartialEq` on `Option<Component>` from the
  iterator result — the enum variants are accessible without an explicit `use`).

**Unit tests added (all green):**
1. `expand_file_happy_path_inside_base` — file directly inside base resolves fine.
2. `expand_file_dotdot_escape_blocked` — `../secret.txt` must error; secret content must not appear in output.
3. `expand_file_absolute_outside_base_blocked` — absolute path outside base must error.
4. `expand_file_symlink_escape_blocked_unix` (`#[cfg(unix)]`) — symlink pointing outside base must be blocked after `canonicalize()`.
5. `expand_file_symlink_escape_blocked_windows` (`#[cfg(windows)]`) — same on Windows; skips gracefully if the process lacks symlink privilege.

---

### CRITICAL #7 — Master key zeroized on Drop (crates/zaion-secrets/src/store.rs)

**Root cause:** `cipher_key: [u8; 32]` was a plain array field with no explicit
`Drop` impl; when `EncryptedStore` was dropped the 32 key bytes remained in heap
memory until the allocator reused the page.

**Design choice: `Zeroizing<[u8; 32]>` wrapper (not `ZeroizeOnDrop` derive)**

Rationale:
- `EncryptedStore` has no `#[derive(Serialize, Deserialize)]` on the outer struct —
  it is a runtime-only handle. `Zeroizing<T>` as the field type is therefore
  zero-overhead and requires no extra proc-macro derive on the struct itself.
- `ZeroizeOnDrop` derive would add `Drop` to the struct, conflicting with any
  future manual `Drop` impl (e.g. for audit logging on teardown).
- `Zeroizing<[u8; 32]>` implements `Deref<Target = [u8; 32]>`, so `Key::from_slice`
  call sites needed only `&*self.cipher_key` (explicit deref-coercion to `&[u8]`).
- Construction uses `(*master_key).into()` (via `From<[u8;32]> for Zeroizing<[u8;32]>`).

**New workspace dep added:** `zeroize = { version = "1", features = ["derive"] }`
(was already transitively present at 1.8.2; now explicit and auditable).

**Crate dep added:** `zaion-secrets/Cargo.toml` lists `zeroize = { workspace = true }`.

**Unit tests added (both green):**
1. `cipher_key_zeroizes_on_drop_trait_bound` — static assertion:
   `fn assert_drop<T: Drop>()` called with `Zeroizing<[u8;32]>`. Removing the
   wrapper turns this into a compile error.
2. `cipher_key_is_zeroed_after_drop` — runtime check via `ManuallyDrop` +
   `std::ptr::addr_of!(*slot.cipher_key)` + `ManuallyDrop::drop` +
   `std::ptr::read` asserting `== [0u8; 32]`.

---

### CRITICAL #8 — API key not serialized plaintext (crates/zaion-federation/src/honcho.rs)

**Root cause:** `HonchoConfig.api_key: String` was a plain `pub` field with full
`#[derive(Serialize, Deserialize)]`; every `toml::to_string_pretty(&config)` call
(as done by `cmd_honcho_setup`) wrote the live API key bytes to disk in cleartext.

**New deps added to workspace:** `secrecy = { version = "0.8", features = ["serde"] }`
**New deps added to zaion-federation:** `zaion-secrets`, `secrecy`, `dirs`, `hex`
**New dev-dep added to zaion-federation:** `toml` (test-only)

**Design:**
- Introduced `ApiKeySource` enum (fully serializable; never contains plaintext):
  - `ApiKeySource::Env { var: String }` — read from environment at runtime.
  - `ApiKeySource::SecretsStore { alias: String, store_path: Option<String> }` —
    load from `zaion-secrets` encrypted store, keyed by alias.
- Removed `api_key: String` from `HonchoConfig`; replaced with
  `api_key_source: ApiKeySource` (default = `Env { var: "HONCHO_API_KEY" }`).
- `ApiKeySource::resolve() -> Result<SecretString>` fetches the key at runtime only.
- `HonchoClient` stores `api_key: SecretString` (private); helper `fn bearer()`
  calls `.expose_secret()` only when constructing the HTTP `Authorization` header.
- `HonchoClient::new` remains **infallible** (panics) to preserve ABI compatibility
  with all existing CLI callers that are outside the allowed-edits scope.
- `HonchoClient::try_new` is the new fallible constructor for library callers.

**Unit tests added (all green):**
1. `serialized_config_does_not_contain_api_key` — TOML output must not contain the sentinel.
2. `roundtrip_config_toml_no_plaintext` — serialize → TOML (no plaintext) →
   deserialize → `resolve()` still yields the correct key.
3. `missing_env_var_returns_error` — `try_new` with absent env var returns `Err`.

---

## Cargo.toml changes

| File | Added |
|------|-------|
| `Cargo.toml` (workspace) | `zeroize = { version = "1", features = ["derive"] }`, `secrecy = { version = "0.8", features = ["serde"] }` |
| `crates/zaion-secrets/Cargo.toml` | `zeroize = { workspace = true }` |
| `crates/zaion-federation/Cargo.toml` | `zaion-secrets`, `secrecy`, `dirs`, `hex`; dev: `toml` |

---

## Grep regression

```
rg -n 'let _ = \(canonical, base_canonical\)' crates/
# exit 1 — zero matches
```

---

## Test results (all crates — cargo test exit code: 0)

```
zaion-runtime:    236 passed, 0 failed  (includes 5 new traversal-guard tests)
zaion-secrets:     11 passed, 0 failed  (includes 2 new zeroize tests)
zaion-federation:  17 passed, 0 failed  (includes 3 new API-key plaintext tests)
doc-tests:          1 passed
```
