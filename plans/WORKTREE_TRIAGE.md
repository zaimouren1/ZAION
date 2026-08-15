# Zaion Rust Worktree Triage

> Superseded historical cleanup record from 2026-05-01. Do not execute its
> website or hook cleanup/commit instructions as current guidance: the
> standalone `zaion-website/` project and repository-local `.claude/hooks/`
> were intentionally retired on 2026-07-13. Use `docs/PROJECT_STATUS.md`,
> `ROADMAP.md`, `git status --short`, and `git worktree list` for current state.
> In particular, never remove registered `.claude/worktrees/` or `zaion-data/`
> based on this historical record.

Date: 2026-05-01
Branch: `codex/worktree-triage`

## What Was Cleaned

This pass separated reproducible local artifacts from real source work.

Archived outside the repository:

- `temp_extract/`
- `cc-haha-main.zip`
- `hermes-agent-2026.4.8.zip`
- `.ralph/`
- `.ralphrc`
- `ralph-run.ps1`
- `review报告/`

Archive location:

`D:/zaion-reference/zaion-rust-cleanup-20260501/`

Removed as reproducible generated output:

- `target/`
- `zaion-website/node_modules/`
- `zaion-website/.next/`
- `zaion-website/.playwright-cli/`
- `zaion-website/output/`
- `zaion-website/tsconfig.tsbuildinfo`
- `zaion-website/next-env.d.ts`
- `zaion-website/.codex-dev.*.log`
- `.claude/hooks/trace.log`
- untracked `.claude/.session_injected/*` markers

Deleted from tracked source because they were zero-byte local session markers:

- `.claude/.session_injected/302b4c72-7e15-4177-8d79-e8a14ba0eac7`
- `.claude/.session_injected/8dc18c23-5213-4c86-9886-8a0025a6dbb1`

## What Was Not Cleaned

`zaion-data/` remains in place. It is ignored runtime data and contains local identity/ledger files, including `keypair.bin` and `ledger.db`. It should not be committed, but moving or deleting it could change local Zaion identity/runtime behavior.

The remaining dirty worktree is mostly real source work:

- Rust core/runtime/module changes under `crates/`
- new CLI commands and tests
- Phase 8 / maturity / reference plan documents
- website source rewrite and public image assets
- release/install metadata

## Hygiene Rules Added

Root `.gitignore` now ignores:

- `.claude/hooks/trace.log`
- `.claude/.session_injected/`
- `temp_extract/`
- `temp_extract*/`
- `zaion-website/.codex-dev.*.log`
- `zaion-website/.playwright-cli/`
- `zaion-website/output/`
- `review报告/`

Root `.gitattributes` now pins normal source files to LF and marks media/archive outputs as binary. This reduces Windows CRLF noise before publishing to GitHub.

## Recommended Commit Slices

1. `chore(repo): clean generated artifacts and normalize text rules`
   - `.gitignore`
   - `.gitattributes`
   - removal of tracked `.claude/.session_injected/*`
   - reference path updates in docs

2. `feat(runtime): stabilize identity, capability, provider, tool, and turn surfaces`
   - core Rust runtime/adapters/MCP/CLI changes
   - new Phase 8 command surfaces and tests

3. `docs(phase8): add evidence-backed roadmap and maturity plans`
   - `plans/phase8-b/`
   - `plans/reference-inventory/`
   - `plans/macro-maturity/`
   - Phase 8 docs

4. `feat(website): rebuild Zaion public site experience`
   - `zaion-website/app/**`
   - `zaion-website/components/**`
   - `zaion-website/lib/**`
   - `zaion-website/public/**`

5. `chore(release): update install and package metadata`
   - `install.sh`
   - `install.ps1`
   - `homebrew-formula.rb`
   - `winget-manifest.yaml`
   - CI workflow updates

## Safety Notes

Do not run broad `git clean -fdX` in this repository. It would delete ignored local runtime data such as `zaion-data/`.

Use targeted cleanup instead:

```powershell
Remove-Item -LiteralPath target -Recurse -Force
Remove-Item -LiteralPath zaion-website\node_modules -Recurse -Force
Remove-Item -LiteralPath zaion-website\.next -Recurse -Force
```
