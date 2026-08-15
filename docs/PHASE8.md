# Phase 8 Runtime Proof

Phase 8 makes Zaion's paradigm executable instead of only conceptual:

```text
one continuity identity
+ unified channel/session envelope
+ small-window context packs over traceable memory
+ optional activity continuity
+ source-verified reference comparison
```

## 8.0 Truth And Source Freeze

Reference inventory commands:

```bash
zaion compare inventory hermes --zip D:\zaion-rust\hermes-agent-2026.4.8.zip
zaion compare inventory cchaha --zip D:\zaion-rust\cc-haha-main.zip
zaion compare dossier --verify
zaion compare matrix --verify
```

Outputs:

- `plans/reference-inventory/hermes.json`
- `plans/reference-inventory/cchaha.json`
- `plans/reference-inventory/breakthrough-dossier.json`
- `plans/reference-inventory/breakthrough-dossier.md`
- `plans/reference-inventory/paradigm-matrix.md`

The inventory harness hashes source files and classifies capabilities without
modifying the reference archives.

## 8.1 Identity And Capability Contract

Zaion starts with a small-octopus identity contract before claiming capability:

```bash
zaion identity show
zaion identity rename <name>
zaion identity continuity
zaion identity verify
zaion capability show
```

Identity continuity is stored in `identity.toml` and
`identity-continuity.toml`. The display name can change, but the continuity
layer is independent of provider, model, channel, workspace, import, export,
and sync state.

## 8.1b Conversational Configuration

`onboard` stays short. Optional settings move to reviewable suggestions:

```bash
zaion config suggest
zaion config apply-suggestion identity.rename --value <name>
zaion config apply-suggestion preference.learning
zaion config apply-suggestion activity.suggest_only --ack-cost
zaion preference show
zaion preference set <key> <value>
```

Costly or background behavior requires explicit warning acknowledgement.

## 8.2 Omni-Session Foundation

The canonical envelope diagnostics are:

```bash
zaion omni status
zaion omni trace --channel telegram --sender owner --thread default --message-id m1
```

Terminal, TUI, Telegram, HTTP, MCP, and future channels attach to the same
identity/session model instead of creating separate agent personalities.

## 8.3 Infinite Context Kernel

Zaion treats the model window as a small execution cache:

```bash
zaion context build <pid> --budget 4000 --verify --query traceability
zaion context trace <context-pack-id>
zaion context verify <context-pack-id>
zaion context replay <event-id>
```

Context packs are persisted under each process in `context-packs/` and include
chunk hashes, token estimates, and lineage.

## 8.4 Traceable Memory Upgrade

Traceable memory atoms require source evidence or an explicit user-provided
marker:

```bash
zaion memory add-fact <pid> "fact text" --source-event <event-id>
zaion memory add-fact <pid> "fact text" --user-provided
zaion memory trace <memory-id>
zaion memory verify <memory-id>
zaion memory invalidate <memory-id>
zaion memory graph <pid>
```

Invalidation preserves history instead of overwriting the atom.

## 8.5 Macro Module Promotion Factory

Macro maturity is now a verified registry instead of a static note:

```bash
zaion macro status
zaion macro status opd
zaion macro verify
zaion macro report --verify
```

The gate checks crate/source paths, status surfaces, docs, tests, safety
boundaries, promotion gates, and Phase 8-B dossier evidence. The generated
reports are:

- `plans/macro-maturity/phase8c-macro-maturity.json`
- `plans/macro-maturity/phase8c-macro-maturity.md`

Current Phase 8-C rows cover metabolic, ego, autonomic, activity-continuity,
curiosity, proprioception, memory-trace, context-kernel, omni-session, rollup,
singularity, watchdog, evolve, OPD, enclave, and TUI.

Ready means the module has honest status, proof surfaces, docs/tests, a safety
boundary, and a promotion gate. It does not mean high-risk modules are stable:
rollup/ZK, low-level autonomic, curiosity runtime, proprioception unlock,
singularity, watchdog recovery, self-evolution, OPD, and enclave remain
experimental unless their promotion gates are completed.

For OPD/evolve, this distinction is enforced at runtime. The registry can be
`ready` while the module remains `not-promoted`; `promoted` appears only when
`ZAION_DATA_DIR/evolve/promotion_chain.jsonl` verifies through
`PromotionChain::verify_all()` and contains a signed append-only `Promoted`
record. Missing promotion-chain evidence is an honest `not-promoted` state;
tampered or invalid chain evidence is a blocking doctor issue.

## 8.5b Answer Trace Span Evidence

Stable wake turns now bind memory/context evidence at answer-span granularity.
The signed `answer.trace` event includes `answer_trace_spans`; each span records
`zaion.answer_trace_span.v1`, `span_hash`, `response_hash`, `context_pack_id`,
matched context layers, matched memory atom ids, `evidence_kind`, and
`evidence_hash`.

`zaion answer trace <proof-event-id> --pid <pid>` exposes these ledger-backed
evidence hashes. This closes the proof gap between a context pack existing and
a specific answer span showing which memory/context evidence it used.

## 8.5c Recall Quality Proof

Recall quality is now a persisted proof surface instead of a transient search
printout:

```bash
zaion memory setup --provider openai --model text-embedding-3-small
zaion memory recall-quality <pid> "traceable context proof" --expect "compression proof preference" --json
```

The report schema is `zaion.memory_recall_quality.v1`. It records the query,
expected recall assertions, matched memory atom ids, atom proof hashes, source
hashes, `embedding_trace` provider/model/quality, `quality_gate_passed`,
`evidence_hash`, and `report_path`.

This proves the recall-quality assertion for the current memory atom store and
configured embedding surface. Broader live-provider benchmark matrices remain
hardening work.

## 8.5d Recall Benchmark Matrix

Declared recall cases can now be aggregated into a persisted benchmark report:

```bash
zaion memory recall-benchmark <pid> --cases recall-cases.json --json
```

The cases file is a JSON array. Each case includes a `query` and an `expect`
array, with optional `id`. The benchmark report schema is
`zaion.memory_recall_benchmark.v1`; it embeds the per-case
`zaion.memory_recall_quality.v1` reports, pass/fail counts, provider/model/
quality `embedding_trace`, aggregate `evidence_hash`, and `report_path`.

This is a reproducible local recall matrix over declared memory expectations,
not a claim that all live provider retrieval surfaces have been exhaustively
benchmarked.

## 8.6 Reference Breakthrough Matrix

The matrix allows only classified rows. A paradigm-breaking verdict must point
to runnable Zaion commands or tests.

```bash
zaion compare dossier --verify
zaion compare matrix --verify
```

## 8.7 Activity Continuity Engine

Activity continuity is off by default:

```bash
zaion activity status
zaion activity configure --enable --ack-cost --mode suggest-only
zaion activity configure --enable --ack-cost --mode autonomous-research --network-domain arxiv.org
zaion activity sample --seed 42
zaion activity trace <thought-id>
zaion thought list
zaion thought show <thought-id>
```

The sampler is stochastic and bounded, not a fixed cron. It derives topics from
traceable preferences instead of hardcoded research subjects.

Autonomous boundaries:

- no destructive actions;
- no credential access;
- no purchases;
- no code modification;
- no external auto-delivery unless explicitly configured.

## 8.8 Regression Gate

Target verification:

```bash
cargo fmt --all -- --check
cargo test -p zaion-cli --test phase8_surface -j1
cargo test -p zaion-cli --test cli_stable_surface -j1
cargo test -p zaion-cli -j1
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
zaion compare matrix --verify
zaion macro verify
zaion macro report --verify
```
