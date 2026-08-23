# Changelog

All notable changes to Zaion are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versioning
follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-08-23

First tagged release: the stable first path.

### Added

- Local, auditable agent runtime in Rust (36-crate workspace).
- Multi-surface entry: chat-first terminal TUI, browser WebUI
  (`zaion dashboard`), background runtime (`zaion start`), HTTP gateway
  (`zaion gateway start`), and single-turn `zaion chat` / `zaion wake`.
- `zaion hero` mission mode with the core tool subset.
- Signed event ledger (Ed25519) with source-gate evidence and provenance
- Session management: profile / session / resume / fork / search / export.
- 7-layer memory system, traceable memory atoms, and context compression.
- 70+ built-in tools, MCP client/server, and skill store.
- Channels: Telegram, HTTP webhook, plus adapters for Discord/Slack/
  DingTalk/Feishu/Email/SMS/Signal and ACP/A2A.
- Unified gateway security: bearer auth, RBAC, TLS, audit, SSRF guard.
- Turn contract v2 (durable begin/outbox/approval) and a cancel chain
  (process-tree kill with cross-process cancellation).
- Evaluation: 300-task benchmark with dual-track scoring (sample + real
  LLM) and an honest capability-baseline report.
- Providers: Anthropic, OpenAI, Groq, Mistral, and Ollama (local).

### Notes

- Release binaries carry SHA-256 integrity sidecars; code signing is
  not yet configured (UNSIGNED).
- Experimental modules (Rollup/ZK, OPD, Singularity, Enclave) are not
  production security/ZK features.