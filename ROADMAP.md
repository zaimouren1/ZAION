# Zaion Active Roadmap

Status date: 2026-07-22  
Overall project organization: `PARTIAL`  
Latest-Hermes comparison: `PARTIAL`

This is the only active execution roadmap. Current measured facts live in
`docs/PROJECT_STATUS.md`; stable ownership and navigation live in
`docs/PROJECT_MAP.md`. The large historical ledgers remain evidence archives
and are read only when their subject is relevant.

## Completed organization decisions

- The standalone `zaion-website/` project is retired. The supported browser
  surface is the embedded Rust gateway `/ui`.
- Repository-local `.claude/hooks/` are retired. Static permission policy stays
  in `.claude/settings.json`.
- Docker, systemd, and Homebrew services use foreground `zaion _daemon_run`.
- Cargo CI uses locked resolution, slash-branch coverage, and one test thread
  for stateful workspace tests.
- Canonical project map, status, documentation index, plan index, license,
  contribution guide, and read-only audit exist.
- Root/docs AGENTS now use the concise status, roadmap, and project map as the
  default loop context; the three large ledgers are scoped legacy evidence.
- Workspace rustfmt, locked all-target check, strict Clippy, serial tests,
  trybuild cases, doctests, and release-asset validation pass locally on Rust
  1.93 / Windows MSVC.
- `zaion doctor` now checks installed runtime state only. Source, architecture
  contract, and historical evidence gates run explicitly through
  `zaion architecture-audit --root <workspace>` and CI invokes that command.
- Current architecture audit and Phase 8 proofs follow the split MCP modules
  and embedded Rust browser control plane rather than removed source paths.
- The repository pins Rust `1.93.0`; all 36 workspace crates inherit
  `rust-version = "1.93"`, and CI labels that toolchain as the declared MSRV.
- `cmd_tui` is now the single terminal gate and ready interactive terminals
  enter the chat-first `run_tui_app` ratatui application. The old blocking
  inline runner and its unused legacy full-screen render branch are removed.
- Gateway G0 centralizes loopback-by-default bind resolution and strong
  `zaion.gateway.health.v1` identity checks across dashboard, daemon, and
  standalone gateway lifecycle paths.
- Successful wake completion now closes `answer.trace`, `turn.proof`, and the
  tool-receipt/proof join before emitting turn-completed and legacy stream
  completion events.
- `WakeRequest`, `StreamEvent`, `StreamCallback`, `ToolCallEvent`, and
  `WakeOperationRecorder` now belong to `zaion-runtime`; CLI surfaces re-export
  the compatibility types and the old CLI-local `wake_stream.rs` is removed.
- `WakeFeaturePolicy` now resolves memory, MCP, compression, webhooks, cache,
  and smart-route once before execution. Negative flags win, default/unified
  paths consume the same effective values, and stable proofs record them.
  Automatic compression inherits the configured threshold; explicit compression
  uses a real forced runtime path and records an honest no-op when no safe middle
  exists.
- `TurnExecution`, one canonical `ProofClosure`, and deterministic
  `EvidenceSubgraph` contracts now belong to `zaion-runtime`. Successful default
  and unified wakes return proof-bound typed completion only after signed trace,
  proof-hash, evidence-graph, receipt-join, and ledger-chain verification.
- M0 now has machine-readable governance: a product scorecard, threat model,
  300-slot benchmark manifest/schema, and a strict product-gate validator. CI
  runs that validator once on Windows. The benchmark remains honestly
  `0/300` verified and cannot support a 10/10 or competitor-surpassed claim.
- Both current gateway loops now enforce bounded requests/connections,
  same-origin CORS, bearer authentication for configured access, and a strong
  token requirement on non-loopback binds. Health and preflight remain public;
  the two handwritten server owners and browser token bootstrap remain open.
- Docker and installer contracts now use a non-root runtime user, explicit
  private state directories, a strong health check, archive-bound checksums,
  and a release gate that distinguishes integrity from signing. Current
  templates remain deliberately `NOT PUBLISHABLE` and artifacts `UNSIGNED`.
- `AuthenticatedIngress`, `VersionedTurnState`, and `ToolBroker` now reach one
  real strangler path behind `ZAION_TURN_CONTRACT_V2=1`: local CLI wake. That
  path binds the local keypair to tenant/subject/workspace/profile/session,
  persists actor-scoped CAS state and a state-event outbox in the principal
  `ledger.db`, executes broker-allowed native tools once in the tested process,
  and emits blocked receipts with zero execution for denied tools. Serial and
  concurrent retries reconcile on stable message identity while preserving the
  first request/authority snapshot; row materialization verifies hashes,
  authority columns, deterministic IDs, terminal outcomes, and outbox bindings
  before use. Runtime and Ledger now also expose an explicit, typed path that
  revalidates a leased tenant head, appends its exact signed event, and completes
  it only with a sealed `VerifiedEventCommit`. A bounded runtime dispatcher now
  drives that path, and the foreground daemon starts one dispatcher per durable
  principal ledger discovered in `ProcessStore`. This is not a claim of
  HTTP/channel/MCP authorization coverage, global chronology, or exactly-once
  external side effects.
- `EventLedger` append is now serialized across independent SQLite connections:
  WAL, `synchronous=FULL`, a five-second busy timeout, `BEGIN IMMEDIATE`, and the
  unique `ux_events_principal_seq` index keep tail read, zero-based sequence
  allocation, previous-hash derivation, and insert in one transaction.
  Every open validates chain metadata, while index inspection requires the exact
  non-partial `(principal_id BINARY ASC, seq_num BINARY ASC)` shape.
  `verify_chain` rejects gaps and non-zero genesis positions. This closes the
  ledger sink race and supports verified keyed state-event append; the daemon
  dispatcher now consumes this sink with bounded workers, retries, quarantine,
  health checks, and graceful shutdown.
- The root workspace now selects only `zaion-cli` as its Cargo default member.
  Explicit `--workspace` CI/check/test coverage remains unchanged; this reduces
  accidental local build roots but does not shrink the CLI dependency graph or
  its binary.

## 2026-07-22 execution record

- Changed files: Runtime outbox dispatcher, persistence/quarantine support,
  TurnStore integration and exports; daemon dispatcher composition, shutdown and
  PID-ownership state machine; active roadmap and project status.
- `OutboxDispatcher` now owns a bounded worker pool with polling, wakeup, fenced
  lease renewal, deterministic retry backoff, immutable quarantine evidence,
  health snapshots, sticky worker-failure reporting, and bounded shutdown. The
  signer is resolved before a renewed lease, full TurnStore revalidation and a
  final deterministic-event check; signing and append occur only after an
  explicit append-admission fence.
- Shutdown first prevents new append admission, then lets already admitted
  append/completion finish. Each caller supplies one absolute deadline; timed-out
  handles remain joinable and `Drop` hands them to a reaper. Worker panic and
  incomplete post-append completion fail closed instead of reporting success.
- Foreground daemon startup discovers persistent process identities and starts
  one single-worker dispatcher per principal `ledger.db`, capped at 32. It loads
  private signing material only from persistent `ProcessStore`, retains
  public-key-only recovery for already signed events, periodically refreshes
  identities, fails closed on unhealthy dispatchers, and broadcasts shutdown
  before joining all dispatchers against one aggregate absolute deadline.
- `zaion stop` now reserves 20 seconds for cooperative drain, then bounded
  two-second TERM and force-kill windows. PID zero/corrupt files are rejected;
  PID-file ownership changes while the old process remains alive are distinct
  from exit; ownership and liveness are rechecked before and after signals.
- Verification passed: dispatcher `17/17`; TurnStore `51/51`; Runtime `481/481`
  plus one doctest; daemon `19/19`; workspace strict Clippy and all-target check;
  format, architecture audit, project audit, and diff check. Independent
  read-only reviews found no remaining P0/P1 in the dispatcher or latest daemon
  shutdown state machine.
- Remaining boundaries: lease revalidation and Ledger append are separate
  SQLite transactions, bounded by a 12-second minimum commit window and
  deterministic idempotent recovery rather than a strict global fence. A
  synchronous external KMS/HSM resolver has no cancellation protocol; timed-out
  workers are reaped. Quarantine has no append-only resolution/reauthorization
  protocol. Tenant-prefix validation can become `O(N^2)`, and global chronology,
  external-side-effect exactly-once, gateway connection ownership, Hermes
  conformance, the 300-task benchmark and enterprise proof remain open.

## 2026-07-19 execution record

- Changed files: Ledger verified-event binding, database-instance identity and
  exports; Runtime durable turn store, dispatch module and focused tests; active
  roadmap and project status.
- Durable outbox order is now a database-wide, append-only commit ordinal with
  deterministic legacy backfill evidence, exact schema/index/trigger checks,
  tenant-head fencing, and delivered-prefix validation. A signing lease is
  accepted only after the complete `0..=revision` history, current durable turn,
  actor/authority bindings, transition legality, event payload and tenant prefix
  have been revalidated.
- `EventLedger` now seals canonical path and a persisted logical database UUID
  into `VerifiedEventCommit`. Completion opens a second `BEGIN IMMEDIATE`,
  re-verifies the prefix and exact event through that transaction, stores the
  verified event ID/public key/database UUID in an append-only evidence table,
  then atomically marks the outbox delivered. Crash-after-evidence failpoints
  roll back both writes; retries return the same delivery or an explicit
  `AlreadyDelivered` result.
- `PrincipalId` remains its compatible SHA-256/Base58 public-key digest and is
  not treated as a reversible DID. Each delivered row retains its own verified
  signer public key, so a tenant prefix can contain multiple principals or key
  rotations without borrowing the current turn's key. Missing legacy evidence
  fails closed and can be repaired only by re-verifying the original sealed
  commit in commit order.
- Verification passed: 34 focused TurnStore tests; all 464 Runtime tests plus
  one doctest; all 54 Ledger tests plus doctests; strict Runtime/Ledger and full
  workspace Clippy; workspace all-target check; format, architecture audit,
  project audit, and diff check.
- At this slice the next gap was the background dispatch lifecycle; the
  2026-07-22 record closes that bounded worker/retry/quarantine/health/shutdown
  step. Before scale,
  replace cumulative tenant-prefix revalidation, which can become O(N²), with
  authenticated checkpoints or batched verification. Admission/outbox hashes
  still lack a signed or MACed admission root, signature v2 still does not bind
  event identity/time/sequence/previous hash, and arbitrary coherent SQLite DDL
  rewrites remain outside the guarantee. No global chronology or exactly-once
  external side-effect claim is made.

## 2026-07-15 execution record

- Changed files: runtime evidence graph, turn kernel/outcome/proof, wake stream,
  feature policy, authenticated ingress, turn state, durable turn store/outbox,
  tool broker, compressor, compression split, and unified runtime; CLI
  default/unified wake construction, local V2 contract bridge, wake/chat/TUI
  help and flags, gateway security, MCP/HTTP/Telegram/ACP ingress builders,
  architecture gates and tests; ledger append/migration/tests and sync import;
  root Cargo default membership; M0
  scorecard/threat/benchmark assets, CI product gate, Docker/install/release
  hardening, and active project documents.
- `RuntimeOutput` now contains only real provider/context/memory/tool artifacts.
  `TurnExecution` distinguishes finished, locally handled, and scheduled work;
  cancellation returns typed `Aborted` instead of an empty success object.
- One runtime-owned verifier constructs `ProofClosure` only from matching signed
  events, verified turn/evidence hashes, required evidence nodes, receipt joins,
  and an intact principal ledger chain. Compression parent-to-child namespace
  changes additionally require a signed, proof-bound transition event.
- Feature resolution is now contradiction-free at the runtime boundary.
  Unified cache and smart-route are no longer silently ignored; chat/TUI expose
  supported negative flags; internal service ingress inherits config-driven
  automatic compression without claiming an explicit compression request.
- Explicit compression requests, including `--compress`, now bypass the
  automatic threshold through matching forced summary-preview and split APIs.
  This keeps provider-backed summaries, child-session lineage, and persisted
  iterative summaries aligned at the configured 200K default budget.
- Local V2 durability now binds a stable actor key across tenant, principal,
  workspace, profile, channel, and thread; persists canonical request/authority
  snapshots and hashes; enforces one active actor turn; and atomically commits
  state CAS with a hash-bound, lease-claimable state-event outbox. Expired
  `Accepted/Routed/WaitingApproval` turns abort, while expired
  `Running/ToolRunning` turns quarantine rather than retry uncertain effects.
  Duplicate terminal ingress returns the stored typed result before a second
  `channel.received` write. A second CLI process may construct a new envelope
  with a different `received_at`; strict retry comparison ignores only that
  field, preserves the original deadline/authority, and reconciles a concurrent
  admission race after the atomic insert. Ordinary wake errors explicitly
  persist a terminal state; crash recovery does not depend on `Drop`.
- Durable reads now fail closed with typed integrity errors. Materialization
  recomputes request/authority/terminal/outbox hashes, binds denormalized
  authority columns and deadlines, verifies deterministic turn/outbox IDs, and
  rejects state/outcome mismatches before records reach retry or dispatch code.
- Legacy ledgers missing one or both chain columns, or containing only the old
  default chain metadata, are deterministically re-chained in
  `(principal_id, seq_num, rowid)` order. The migration transaction records
  before/after SHA-256 digests and event count, creates the unique sequence
  index, and uses a durable schema-migration marker to rebuild missing or stale
  legacy FTS content once. Non-legacy corrupt chains, including indexed chains,
  and mixed default/non-default prefixes fail closed and require explicit
  repair.
- Explicit keyed signed append reuses one deterministic `evt-idem-*` event ID
  only when immutable event content matches; key reuse with different content
  returns `EventIdConflict`. Existing append APIs retain UUID/new-fact
  semantics. Preassigned-ID insertion returns `Inserted` or `Existing`; sync
  predecodes the full bundle and inserts it in one transaction, so malformed or
  conflicting later events roll back the valid prefix rather than partially
  importing it.
- Verification passed: format; workspace all-target check and strict Clippy;
  all 443 runtime tests plus one doctest; all 501 CLI unit tests and the
  15/139/2/5/11/3 integration suites; 13 focused turn-store tests, 43 wake
  tests, and 8 V2 bridge tests; all 41 ledger and 24 sync tests; architecture
  audit, project audit, product/release gates, workspace doctests, and diff
  check. The full CLI suite and all 35 non-CLI workspace crates, including their
  unit/integration/property/trybuild/doctest targets, completed successfully as
  separate commands. The earlier combined timeout remains uncounted. Docker
  runtime smoke remains blocked because the local daemon is unavailable.
- Next gap: build the state-event outbox dispatcher only after adding a
  persistent cross-turn commit ordinal, revalidating each leased payload against
  current turn state before signing, and making `complete_outbox` verify the
  referenced ledger event and immutable content. Production dispatch also needs
  bounded retry, dead-letter, health, and shutdown contracts plus a
  backup-and-explicit-repair workflow for mixed legacy chains. Signature v2
  remains permanently verifiable but does not bind `event_id`, `created_at`,
  `seq_num`, or `prev_hash`; an additive v3 must bind those fields. The current
  deterministic `evt-idem-*` value is not a cryptographically signed identity.
  Then bind approval grants to real write/execute/network invocations
  and migrate credential-derived HTTP/channel/MCP ingress onto this durable
  authority. Live provider/tool cancellation, typed surface outcomes, one
  gateway owner, browser auth bootstrap, signed release artifacts, and 300
  benchmark executions remain open.

## 2026-07-14 execution record

- Changed files: the CLI TUI gate/application/launcher/help/tests; gateway
  contract, daemon, dashboard, routes, Docker/release checks and gateway tests;
  wake completion ordering and architecture audit; toolchain/manifests/lockfile;
  active project and user documentation.
- Verification: CLI stable-surface, focused TUI/Gateway/proof tests, full
  workspace format/check/strict Clippy/serial tests, trybuild, doctests,
  architecture audit, diff check, and release-asset validation all pass.
- Full testing exposed and this stage fixed one stale Ouroboros restart path:
  Watchdog now launches `zaion _daemon_run`, detaches child stdio, and uses a
  harmless injected executable in restart tests. All 53 Watchdog tests and
  doctests pass after the change.
- The runtime-protocol continuation passed runtime and CLI all-target checks,
  strict Clippy, 6 runtime wake tests, 2 positive/negative ownership-gate tests,
  all 137 CLI stable-surface tests, and the architecture audit.
- Next gap: connect `RuntimeOutput` to one runtime-owned typed outcome/proof
  closure, propagate cancellation through provider/tool execution, normalize
  contradictory wake feature flags, then continue gateway/TUI convergence and
  dependency/file-size work.

## P0: Make product entry architecture true

### 1. Choose one authoritative TUI

State: `PARTIAL` - production entry selected; remaining TUI generations and PTY
coverage still require consolidation.

Current evidence:

- `crates/zaion-cli/src/commands/process/tui/mod.rs::cmd_tui` is the single gate.
- Ready identity/provider plus stdin/stdout TTY enters
  `process/tui/app.rs::run_tui_app`; all other invocations print the
  non-mutating snapshot.
- Parser, theme, memory/MCP/cache/smart-route flags, preference learning, and
  structured gateway stdio program/arguments reach the selected application.
- Terminal setup/restoration and owned gateway-child shutdown use RAII and
  explicit cleanup; 82 focused TUI tests pass.
- `zaion-tui` still retains additional inline, modern, v2, and component paths,
  and a real PTY launch/restore smoke test is still missing.

Acceptance:

- One documented interactive entry path.
- Entry behavior tests cover ready TTY, fresh/non-interactive snapshot, and
  explicit `zaion tui`.
- Unselected TUI generations are removed or archived.
- Queue, steer, interrupt, rail, and gateway controls are tested through the
  selected production entry.

### 2. Put the real turn kernel in `zaion-runtime`

State: `PARTIAL` - request/stream and proof-bound result contracts are
runtime-owned; execution choreography remains CLI-owned.

Current evidence:

- Active execution remains in the large CLI `process/wake.rs` path.
- `zaion-runtime` now owns `WakeRequest`, `StreamEvent`, `StreamCallback` and its
  cancellation state, `ToolCallEvent`, and `WakeOperationRecorder`. CLI process
  surfaces only re-export those types, and ownership gates reject local
  redefinitions or restoration of the removed CLI `wake_stream.rs`.
- Tool-call stream conversion now performs panel redaction inside the runtime
  API. Runtime tests cover canonical-envelope binding, operation ordering,
  cancellation state/typed events, and raw secret redaction.
- `TurnExecution` represents finished, handled, and scheduled control paths;
  `RuntimeOutput` is reserved for actual provider/context/memory/tool artifacts.
- Runtime owns one `ProofClosure` and verifier plus deterministic answer-local
  evidence graphs. The verifier checks event signatures, principal and namespace
  scope, parent topology, proof/evidence hashes, required nodes, tool receipts,
  receipt joins, and the principal ledger chain. Signed compression namespace
  transitions are explicit proof/evidence nodes rather than a scope exception.
- `zaion-runtime::turn_kernel` is primarily a contract, while CLI implements
  the effective kernel entry.
- `ZAION_TURN_CONTRACT_V2=1` enables the first local CLI strangler. It constructs
  runtime-owned `AuthenticatedIngress`, advances the nine-state CAS contract,
  and invokes `ToolBroker` before native dispatch. Focused tests prove one
  allowed handler invocation and zero denied-handler invocations.
- This V2 state is durable but local-CLI-only. Actor/CAS rows and a state-event
  outbox live in each principal's `ledger.db`; recovery is
  fail-closed for uncertain effects, and duplicate terminal ingress replays the
  typed result without repeating ingress ledger writes. Concurrent retries are
  reconciled after atomic admission, and durable reads verify hash, authority,
  deterministic-ID, terminal-result, and outbox bindings before use. An explicit
  signing/append/sealed-completion primitive is verified and the foreground
  daemon now dispatches durable principal ledgers; provider/tool exactly-once
  is not claimed, and
  approval/resume, external MCP manifests, and credential-derived HTTP/channel
  subjects remain open. The flag intentionally rejects non-CLI ingress.
- The ledger sink now serializes append across independent connections and
  supports conflict-checked keyed retries. The bounded dispatcher preserves
  persistent ordering, repeats full state/history revalidation, records
  per-event signer evidence, retries transient failures, quarantines terminal
  failures and exposes health/graceful lifecycle. The daemon owns this runtime
  for up to 32 persistent principals; other process surfaces do not yet own an
  equivalent dispatcher lifecycle.
- Default wake and `UnifiedAgentRuntime` remain parallel inner engines, but both
  return `TurnExecution` through the outer `WakeTurnKernelEntry` and close a
  verified proof before typed completion.
- `cmd_wake_with_request` still discards `TurnExecution` for compatibility, and
  TUI/channel/gateway consumers still rely on legacy token-only completion.
- Live cancellation is still observed too shallowly to interrupt an active
  provider stream or tool loop. Public and programmatic wake feature overrides
  now resolve through one runtime-owned policy, with source gates preventing
  execution paths from returning to raw request-flag reads.
- Config-driven compression remains threshold-based, while explicit compression
  uses runtime-owned forced compressor and split APIs. Provider-summary preview
  selects the same forced middle, and signed evidence distinguishes requested,
  applied, and safe no-op outcomes.

Acceptance:

- Runtime owns `WakeRequest`, stream events, typed outcomes, proof closure, and
  cancellation.
- CLI, TUI, Telegram, HTTP, webhook, MCP, and ACP construct canonical ingress
  and call the same runtime API.
- One turn-proof topology is verified across every stable ingress.

### 3. Unify gateway and browser control plane

Current evidence:

- Active HTTP/WebSocket logic is implemented in CLI modules.
- `zaion-gateway` is a separate Axum crate with no workspace consumer.
- Daemon and standalone loops share `network/gateway_contract.rs`: default
  `127.0.0.1:7821`, `ZAION_GATEWAY_BIND`, explicit `--host`/`--port`, and a
  non-redirecting strong `/health` identity probe.
- Dashboard and daemon refuse to reuse generic HTTP 200 responses; Docker opts
  into `0.0.0.0` explicitly.
- Non-loopback binds require a bearer token of at least 32 bytes. Both loops
  enforce same-origin/allowlisted CORS, constant-time bearer comparison, 16 KiB
  headers, 1 MiB bodies, read deadlines, bounded connections, and rejection of
  ambiguous/chunked request bodies.
- Multiple handwritten gateway loops remain, and standalone gateway behavior
  still lacks daemon WebSocket parity. The embedded browser UI also has no
  secure token/session bootstrap when bearer auth is enabled.

Acceptance:

- `zaion-gateway` is the single server library.
- CLI owns lifecycle commands only.
- `/ui` has one asset source and loopback/auth/write-audit contracts.
- `zaion start`, `zaion gateway start`, and packaged service behavior have
  integration tests.

## P1: Restore reliable engineering gates

### 4. Establish a green Rust baseline

Completed locally on 2026-07-13 and extended on 2026-07-14:

- Workspace rustfmt pass and strict format check.
- Locked all-target workspace check.
- Strict workspace Clippy with warnings denied.
- Full workspace tests with stateful tests serialized, including trybuild and
  doctests.
- Release/install-chain validation after website and hook retirement.
- Pinned Rust `1.93.0`, workspace `rust-version = "1.93"`, inheritance in all
  crate manifests, and a CI job explicitly labelled as the declared MSRV.
- Workspace `rand` advanced from `0.8.5` to `0.8.6` without changing the
  separately resolved `rand 0.9.2` advisory path through `proptest`.
- Root Cargo default membership is `zaion-cli`; every complete engineering gate
  continues to use explicit `--workspace`.

Remaining:

- Build and smoke-test the Docker image and packaged service entry.
- Refresh the advisory database and resolve the five current warnings:
  `bincode 1.3.3`, `paste 1.0.15`, `yaml-rust 0.4.5`, `lru 0.12.5`, and
  `rand 0.9.2`.

Acceptance commands:

```powershell
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked -j1 -- --test-threads=1
cargo audit --deny warnings
bash scripts/check-release-assets.sh
```

### 5. Reduce composition-root and giant-file coupling

Order of work:

1. Move embedded test modules out of the largest production files without
   changing behavior.
2. Split Telegram into transport, media, document extraction, access policy,
   task runner, proof, command, and tests.
3. **Completed 2026-07-13:** separate user runtime doctor checks from
   source/ledger architecture audits and give CI an explicit source-audit
   command.
4. Split system/doctor, wake, webhook, and routes by responsibility.
5. Extract shared configuration/state from the binary crate.
6. Rename conflicting MCP registries and OPD concepts.

Current progress: wake request/stream, typed execution, evidence graph, and
proof-closure contracts have moved behind the runtime boundary. Durable turn
storage is runtime-owned and its dispatch/tests are split into submodules, but
the production `turn_store.rs` is currently 3,272 lines and exceeds the stated
source-size threshold. Provider/tool choreography, background dispatch
lifecycle, and most giant-file splits remain open.

Acceptance:

- No production source file above 3,000 lines without a written exception.
- `zaion-cli` no longer directly owns reusable runtime, channel, or protocol
  state.
- Installed `zaion doctor` does not require the source checkout or progress
  ledgers; source architecture gates run only in development/CI.
- Orphan/dead modules are removed with compiler-verified dependency cleanup.

### 6. Normalize workspace metadata

- Completed: declare Rust `1.93` at workspace level and inherit it in all 36
  crate manifests.
- Decide which crates are publishable; set `publish = false` for internal-only
  crates.
- Add package descriptions and repository metadata once the canonical remote is
  known.
- Consolidate workspace dependency declarations and duplicate versions.
- Keep generated test/runtime files outside Git.

## P2: Repair documentation and evidence

### 7. Freeze and recover historical ledgers

- Do not append routine work to `MASTER_PLAN.md`,
  `plans/openclaw_latest_gap_report.md`, and
  `plans/hermes_surpass_master_plan.md`.
- Recover invalid UTF-8 and replacement-character sections only from intact Git
  history or original evidence.
- Move dated completion reports and superseded blueprints into archive
  directories without rewriting their historical claims.
- Move generated comparison inventories into an explicit evidence/artifact
  namespace with regeneration instructions.

### 8. Verify release identity

- Configure or document the canonical Git remote.
- Verify the public install URL before advertising it as a working release
  path.
- Replace Homebrew/Winget checksum placeholders during the first real release.
- Add changelog and security-reporting policy when the release owner/contact is
  known.

## Product invariants

Every refactor must preserve:

- Ed25519 principal identity and continuity.
- Signed append-only ledger and turn proofs.
- Provenance-aware traces and honest observed/estimated/unavailable labels.
- Ouroboros recovery evidence.
- ACI/AST-aware gated code actions.
- Chain-gated self-evolution and promotion evidence.

Hermes remains a reference for polish, breadth, and first-run quality. Zaion is
not a Hermes clone.

## Update policy

- Update this file when priorities or acceptance criteria change.
- Update `docs/PROJECT_STATUS.md` after measured state or verification changes.
- Update a legacy comparison ledger only when performing work in that exact
  comparison scope.
- Never use a plan entry as proof that implementation is complete.
