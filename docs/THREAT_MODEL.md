# Zaion Product Threat Model

Status: M0 governance baseline

This document defines the product threats that can block parity, surpass, and
release claims. It is not a security certification and does not claim that the
listed controls are complete. Current implementation facts remain in source,
tests, and `docs/PROJECT_STATUS.md`.

## Scope

In scope:

- local identity, private keys, secrets, signed ledger, memory, and session data;
- CLI, TUI, browser UI, HTTP/WebSocket gateway, ACP, MCP, webhooks, and channels;
- provider requests and responses;
- local and remote tools, code execution, result storage, and environments;
- context assembly, compression, evidence graphs, turn proofs, and receipts;
- installers, updates, packages, containers, services, skills, and plugins;
- benchmark manifests, result artifacts, and published product scores.

Out of scope for this M0 document:

- security claims for experimental ZK, enclave, OPD, or self-evolution modules;
- infrastructure not controlled or configured by the Zaion project;
- model-provider internals beyond Zaion's request, redaction, and retention
  boundary.

## Protected Assets

1. Ed25519 principal keys and continuity.
2. Provider, channel, MCP, webhook, and relay credentials.
3. Signed event order, hashes, receipts, and proof lineage.
4. User messages, memory atoms, files, tool output, and session state.
5. Tool authority, approval decisions, environment identity, and cancellation.
6. Release artifacts, update metadata, skill packages, and evaluation evidence.

## Trust Boundaries

| Boundary | Untrusted side | Trusted side | Required control |
| --- | --- | --- | --- |
| User input | Prompt, pasted files, terminal input | Canonical ingress request | Size limits, canonicalization, provenance |
| Network ingress | Gateway, webhook, channel payload | Session/runtime dispatch | Authentication, replay defense, allowlists, rate limits |
| Provider | Remote model and response stream | Runtime state | Redaction, timeout, cancellation, schema validation |
| Tool/MCP | Tool description, arguments, output, subprocess | Turn kernel | Capability policy, approval, sandbox, receipt |
| Environment | Docker/SSH/cloud sandbox filesystem and process | Host and principal state | Strong environment identity, path isolation, cleanup |
| Persistence | Database/files that may be stale or tampered | Rehydrated session/memory | Signature, chain, hash, migration validation |
| UI protocol | stdio/WebSocket frames | TUI state | Framing validation, session binding, protocol recovery |
| Supply chain | Dependency, skill, installer, update artifact | Installed Zaion | Pinning, signature, checksum, SBOM, rollback |
| Evaluation | Plans, source claims, benchmark output | Published score | Pinned baseline, immutable evidence, anti-inflation rules |

## Threat Register

| ID | Severity | Threat | Required evidence before closure | Current disposition |
| --- | --- | --- | --- | --- |
| TM-01 | Critical | Principal key theft, replacement, or cross-profile reuse | Encrypted-at-rest test, import/export continuity test, permission audit, cross-profile negative test | Open gate |
| TM-02 | Critical | Ledger truncation, reordering, rollback, forged event, or proof/receipt mismatch | Tamper matrix covering signature, sequence, parent, hash, receipt join, and namespace transition | Partial controls; gate open |
| TM-03 | Critical | Prompt injection causes secret disclosure or unauthorized action | Direct and indirect injection corpus across web, files, memory, MCP descriptions, and channels | Open gate |
| TM-04 | Critical | Tool permission bypass, shell escalation, path escape, or unsafe code execution | Allow/deny matrix, read-before-edit checks, sandbox breakout suite, typed terminal outcomes | Partial controls; gate open |
| TM-05 | High | SSRF, DNS rebinding, redirect escape, unsafe download, or webhook target abuse | Resolver-time IP checks, redirect revalidation, private-range negatives, size and timeout limits | Open gate |
| TM-06 | High | Channel spoofing, replay, wrong topic/user routing, or stale reply delivery | Signed source binding, idempotency, allowlist/topic matrix, stale-completion tests | Partial controls; gate open |
| TM-07 | High | Cancellation races allow stale provider/tool work to mutate state or reply | Provider-stream and tool-loop cancellation tests, bounded unwind, generation ownership | Open gate |
| TM-08 | Critical | Cross-principal/session memory or context leakage | Multi-principal isolation suite across CLI, TUI, gateway, channels, ACP, and compression child sessions | Open gate |
| TM-09 | High | Memory poisoning, stale fact dominance, or invalidated fact reuse | Source-required writes, conflict/expiry tests, invalidation propagation, recall-quality benchmark | Partial controls; gate open |
| TM-10 | High | Compression drops critical state, breaks tool pairs, or forges lineage | Long-session corpus, forced/automatic split tests, tool-pair integrity, signed transition verification | Partial controls; gate open |
| TM-11 | Critical | Malicious MCP/skill/plugin gains ambient credentials or shadows trusted tools | Filtered environment, name-collision tests, description scanning, per-server policy, signed package provenance | Open gate |
| TM-12 | High | Gateway exposed externally without authentication, safe CORS, or write audit | Bind/auth/CORS integration matrix and unauthorized write negatives | Partial controls: non-loopback token, same-origin policy, request bounds, and black-box negatives landed; unified server, RBAC, TLS, and complete write audit remain open |
| TM-13 | Critical | Compromised installer, update, dependency, or release artifact | Signed artifacts, checksums, SBOM, reproducible build record, rollback drill | Partial controls: checksum filename binding, placeholder rejection, and non-root container landed; signatures, SBOM verification, reproducibility, and rollback drill remain open |
| TM-14 | High | Benchmark gaming, stale competitor baseline, fabricated completion, or evidence reuse | Commit-pinned source, unique task IDs, immutable result artifacts, independent rerun | M0 validator added; execution evidence absent |
| TM-15 | High | Self-evolution promotes unreviewed or unsafe code | Signed promotion chain, isolated evaluation, rollback, human authorization, release tests | Experimental; release-blocking |

"Partial controls" means relevant code or tests exist, not that the threat is
closed. Closure requires the evidence named in the same row.

## Security Invariants

The following failures block a release-grade score regardless of weighted
average:

1. A principal can read or mutate another principal's protected state.
2. A tool executes outside its granted capability or without an attributable
   decision and receipt.
3. A successful turn proof is emitted with a broken ledger, missing evidence,
   invalid signature, or mismatched receipt join.
4. A cancelled or superseded turn can deliver stale output or mutate durable
   state.
5. An externally reachable gateway accepts unauthenticated writes.
6. A secret appears in model-visible tool output, logs, receipts, or benchmark
   artifacts.
7. An update or promoted change cannot be authenticated and rolled back.
8. A score of 10 is published without release-grade evidence for every
   represented task slot.

## Abuse Cases

Minimum adversarial scenarios for the executable suite:

- a web page instructs the model to exfiltrate provider keys;
- an MCP server advertises a tool name that collides with a built-in tool;
- an MCP subprocess attempts to inherit unrelated environment credentials;
- a symlink or traversal path escapes the workspace during write/download;
- DNS resolves publicly during validation and privately during connection;
- a Telegram retry delivers an old answer after a newer turn owns the session;
- compression moves a turn to a child session with mismatched signed lineage;
- a copied ledger omits or reorders a receipt/proof join;
- a profile loads another profile's memory, session, or MCP configuration;
- a benchmark task is marked verified using source inspection only.

Each case must eventually become an executable benchmark or security test with
an artifact retained under a declared evidence path.

## Review Triggers

Review this model when any of the following changes:

- a new stable ingress surface, channel, provider, tool, or environment;
- gateway binding, authentication, CORS, or external deployment defaults;
- key storage, ledger schema, proof topology, memory isolation, or compression;
- MCP/ACP protocol versions, skill installation, or self-evolution promotion;
- release distribution, update ownership, or competitor baseline;
- benchmark schema, category weights, scoring rules, or evidence grades.
