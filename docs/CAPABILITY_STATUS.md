# Capability Status

Zaion's codebase contains stable product paths and macro-module prototypes. The
stable path is the default user path.

| Area | Status | User path | Boundary |
| --- | --- | --- | --- |
| CLI golden path | Stable | `help`, `onboard`, `doctor`, `config`, `chat`, `status`, `events` | Primary v0.1 path. `help --all` is maturity-labeled and tested. |
| Provider basics | Stable | Anthropic, OpenAI, Groq, Mistral, Ollama | Shared by onboarding, doctor, chat, wake, Telegram, and TUI flows. |
| Identity and ledger | Stable | `create`, `status`, `events`, `export`, `import` | Events are signed; key export/import supports encrypted material. |
| MCP registration | Stable | `mcp add/list/configure/test`, `chat --mcp` | Direct POST `/mcp/v1/call` is experimental and returns 501. |
| Sync bundles | Stable | `sync export/import/diff/status` | Relay is local/LAN-oriented and token protected. |
| Telegram | Stable extension | `tg status/doctor/set-token/start` | Token/profile setup and daemon handoff with provider and process readiness checks. |
| TUI | Stable extension | `tui --check`, `tui` | Terminal UI over the stable wake/chat path. |
| Startup identity | Stable | `identity show/continuity/verify`, `capability show` | Small-octopus Zaion identity, capability boundary manifest, and hash-chained continuity ledger. |
| Conversational config | Beta | `config suggest/apply-suggestion`, `preference show/set` | Optional settings stay out of onboard and require explicit apply. |
| Omni-session | Beta | `omni status/trace` | Canonical envelope and route trace for channel/session unification. |
| Infinite context packs | Beta | `context build/trace/verify/replay` | 4k-safe context manifests with chunk hashes and lineage. Wake turns bind packs into signed `answer.trace` span evidence. |
| Traceable memory atoms | Beta | `memory add-fact/trace/verify/invalidate/graph/recall-quality/recall-benchmark` | Facts require source events or explicit user-provided markers. Matched atoms are bound to answer spans through signed `evidence_hash` records; `memory recall-quality` persists provider/model/quality `embedding_trace` reports for expected recall assertions, and `memory recall-benchmark` aggregates declared cases into a persisted matrix report. |
| Activity continuity | Beta | `activity status/configure/sample/trace`, `thought list/show` | Off by default, opt-in with token/network warning, stochastic sampler, safety gates. |
| Reference comparison | Beta | `compare inventory/dossier/matrix` | Deterministic source inventory for Hermes and cc-haha plus dossier-backed evidence matrix. |
| Macro maturity gate | Beta | `macro status/verify/report` | Phase 8-C registry checks every macro module for source paths, surfaces, docs, tests, boundaries, promotion gates, and Phase 8-B evidence. For OPD/evolve, runtime promotion is separate and requires a verified promotion-chain `Promoted` record. |
| Other channels | Beta | dashboard, webhooks, channel profiles, WhatsApp and future platform bridges | Useful, but not first-day documentation path. |
| Rollup/ZK, OPD, Singularity, Enclave, Evolution | Experimental | CLI `EXPERIMENTAL` section | Not production security, not production ZK, not stable autonomy. |

## Promotion Rules

- Experimental to beta requires integration tests, doctor checks, and docs.
- Beta to stable requires user-path tests, recovery behavior, CI coverage, and
  documented security boundaries.
- Security and ZK placeholders must stay marked or hidden until real
  implementations land.
- Terminal/CLI is the Phase 7 baseline. Keep first-path help small, ASCII-only,
  and free of beta or experimental recommendations.
- Phase 8 macro promotion requires a doctor row, docs, tests, safety boundary,
  source-backed comparison evidence, and a passing `zaion macro verify`.
- High-risk modules such as Rollup/ZK, Enclave, OPD, and self-evolution remain
  experimental until their proof systems are real.
- `plans/macro-maturity/phase8c-macro-maturity.md` is the current macro-module
  proof report; "ready" means maturity-gated, not automatically stable.
- For OPD/evolve, "ready" is static registry readiness only. "Promoted" is a
  runtime adoption state shown by `zaion macro status` and `zaion doctor` only
  after `evolve/promotion_chain.jsonl` passes `PromotionChain::verify_all()` and
  contains a signed `Promoted` record; a missing chain stays `not-promoted`, and
  an invalid chain is a blocking doctor issue.
