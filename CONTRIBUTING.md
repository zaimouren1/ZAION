# Contributing to Zaion

Start with [docs/PROJECT_MAP.md](docs/PROJECT_MAP.md) and the dated
[docs/PROJECT_STATUS.md](docs/PROJECT_STATUS.md). The repository is frequently
used with multiple dirty worktrees, so scope and verification matter more than
broad cleanup.

## Development setup

Zaion is a Cargo workspace. The current dependency graph requires Rust 1.92 or
newer; the container build uses Rust 1.93.

```powershell
cargo metadata --format-version 1 --locked
cargo check -p zaion-types -p zaion-paths --all-targets --locked
```

Use `ZAION_HOME` to isolate local runtime state while developing:

```powershell
$env:ZAION_HOME = Join-Path $env:TEMP "zaion-dev"
```

Never commit keys, local ledger databases, provider credentials, or files from
`zaion-data/`.

## Before editing

1. Run `git status --short` and identify the exact files owned by your change.
2. Inspect registered worktrees with `git worktree list` before cleaning
   `.claude/worktrees/`.
3. Do not restore, delete, or reformat unrelated user changes.
4. Keep generated files under `target/`; do not add test run output to Git.

## Verification

Run the narrowest relevant checks first. A typical Rust change uses:

```powershell
cargo test -p <crate> <focused_test> --locked -- --nocapture
cargo check -p <crate> --all-targets --locked
cargo clippy -p <crate> --all-targets --locked -- -D warnings
```

Release gates are:

```powershell
cargo check --workspace --all-targets --locked
cargo test --workspace --locked -j1 -- --test-threads=1
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
bash scripts/check-release-assets.sh
```

The workspace currently has pre-existing rustfmt drift. Do not mix a bulk
formatter rewrite with a functional change; isolate it so review can distinguish
mechanical and behavioral edits.

Run the read-only repository audit when changing structure or documentation:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/project-audit.ps1
```

## Architecture rules

- Product surfaces should parse input and render output; reusable turn policy
  belongs in `zaion-runtime`.
- Identity, signatures, provenance, and append-only evidence are compatibility
  boundaries.
- Mark capabilities `SURPASSED`, `PARTIAL`, or `OPEN` only with named source
  evidence and verification.
- Stable, beta, and experimental surfaces must remain visibly distinct.
- Generated comparison JSON belongs to an evidence pipeline, not hand edits.

## Documentation

- Update public entry behavior in `README.md`.
- Update stable navigation in `docs/PROJECT_MAP.md`.
- Update dated blockers and verification in `docs/PROJECT_STATUS.md`.
- Add new documentation to `docs/README.md` or plans/evidence to
  `plans/README.md`.
- Follow `docs/AGENTS.md` for progress-ledger updates.

Historical plans and completion reports are evidence. Do not silently rewrite
them to make current claims look cleaner; add a current correction or recover
damaged text from an intact source.
