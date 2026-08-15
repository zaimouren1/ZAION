# Zaion Project Status

Audit date: 2026-07-22  
Branch: `codex/worktree-triage`  
HEAD: `e5d86f006ea27af6a49c1cab34e40cb8b22198b7`

This is a dated repository-health snapshot, not a product marketing claim. Run
`scripts/project-audit.ps1` to refresh the mechanical evidence.

## Snapshot

| Measure | Observed |
| --- | ---: |
| Cargo workspace crates | 36 |
| Declared Rust version | 1.93 |
| Rust source lines under crate `src/` directories | 218,204 |
| Rust lines in crate `tests/` directories | 21,593 |
| Rust files at or above 1,000 lines | 46 |
| `zaion-cli` source lines | 90,623 |
| `zaion-runtime` source lines | 40,584 |
| `zaion-adapters` source lines | 21,397 |
| Current Git dirty entries | 275 |

The line counts exclude tests embedded inside `src/*.rs`, so they are a lower
bound on total test code. The worktree is intentionally dirty and contains
pre-existing user work; counts describe the current filesystem, not a clean
commit.

## P0 truth and repository-integrity gaps

1. **Gateway ownership remains split.** Daemon and standalone CLI servers now
   share loopback defaults, strong identity, bearer/CORS/request-boundary
   enforcement, and bounded connection handling, but they are still separate
   handwritten loops. `zaion-gateway` remains a leaf crate, standalone gateway
   lacks daemon WebSocket parity, and `/ui` has no secure bearer/session
   bootstrap for authenticated deployments.
2. **End-to-end tool authorization is still partial.** The feature-flagged local
   CLI wake path now constructs `AuthenticatedIngress`, advances
   durable `VersionedTurnState` through actor-scoped CAS, and invokes
   `ToolBroker` before native dispatch. Default wake behavior, HTTP/channel
   ingress, external MCP calls, and `zaion mcp serve` are not yet under that
   authority. Ledger append is serialized and uniqueness-guarded across
   independent SQLite connections. Runtime now owns a bounded state-event outbox
   dispatcher with lease renewal, signing revalidation, exact keyed append,
   sealed completion, retry/quarantine, health and shutdown contracts. The
   foreground daemon starts it for persistent principal ledgers, but outbox
   production and tool authorization remain limited to the feature-flagged local
   CLI path. Real write/execute/network approval grants remain open; global
   chronology and external side-effect exactly-once are not claimed.
3. **Historical ledger text contains encoding damage.** At least
   `MASTER_PLAN.md` and `plans/openclaw_latest_gap_report.md` contain Unicode
   replacement characters, and all three large ledgers contain suspicious
   legacy `??` sequences. Recover historical text only from intact Git history
   or original evidence.
4. **The dependency security baseline is not clean.** Local
   `cargo audit --no-fetch` reports five allowed warnings: unmaintained
   `bincode 1.3.3`, `paste 1.0.15`, and `yaml-rust 0.4.5`, plus unsoundness
   advisories for `lru 0.12.5` and transitive `rand 0.9.2`. The local advisory
   database was last refreshed before this stage, so this is not a fresh
   network-backed audit.
5. **Machine-local Claude configuration is still tracked.**
   `.claude/settings.local.json` is ignored for future files, but removing the
   existing tracked copy from the index still requires an explicit user
   decision so the local file is not deleted accidentally.
6. **The public release origin is unverified.** This checkout has no Git
   remote, while README/install/release metadata still advertise
   `zaimouren1/ZAION`.

## P1 structural debt

- `zaion-cli` directly depends on 30 workspace crates and remains the dominant
  composition root.
- The largest production files are `network/telegram.rs` (13,294 lines),
  `commands/system.rs` (7,996), `process/tui/app.rs` (6,351),
  `process/wake.rs` (6,684), and `telegram_adapter.rs` (4,730).
- Wake request, stream, callback/cancellation, tool-call, and operation-recorder
  types plus typed execution, deterministic evidence graphs, and proof closure
  are runtime-owned. Provider/tool/ledger choreography still runs in CLI
  `process/wake.rs`; compatibility surfaces still discard or flatten typed
  terminal results. Live cancellation does not yet interrupt an active provider
  stream or tool loop. Wake feature defaults and request overrides now resolve
  once into a runtime-owned policy consumed by default and unified execution;
  typed result propagation and real turn-kernel ownership remain open.
- `ZAION_TURN_CONTRACT_V2=1` is a deliberately narrow local CLI strangler. Its
  actor/CAS state and pending outbox are durable in the principal `ledger.db`.
  Only native tools with recognized legacy capability metadata receive a
  transitional manifest, sensitive effects have no grants, actor leases still
  use the ingress deadline, and non-CLI ingress fails closed. Turn-store order
  and verified-commit migrations now have exact fixtures and tamper tests. The
  foreground daemon automatically drains eligible durable outboxes for up to 32
  persistent principals with one worker each. This is evidence of one real
  producer and one production lifecycle, not completion of M2 or a claim that
  all tool surfaces are protected.
- The CLI production TUI is now selected, but reusable component ownership is
  still split between `zaion-cli/src/commands/process/tui/` and `zaion-tui`.
  Additional inline/modern/v2 paths in `zaion-tui` still need a keep/archive/
  remove decision and a real PTY launch/restore smoke test.
- `zaion-gateway`, `zaion-opd`, and `zaion-telemetry` still have no workspace
  consumers. They may be legitimate leaf products, but launch and integration
  contracts must be explicit.
- All crate manifests now inherit the workspace Rust version. Package
  descriptions, repository metadata, publishability, and dependency-style
  normalization remain incomplete.
- `Cargo.lock` resolves 593 packages. Docker now builds with Rust 1.93 and
  locked resolution, but an image build/runtime smoke test has not run because
  no Docker daemon is available in this environment.
- Three reverse-chronological ledgers total roughly 690 KiB and duplicate many
  stage entries. They remain evidence archives rather than navigation sources.
- `plans/ZAION_ARCHITECTURE_SOURCE_AUDIT.md` is not valid UTF-8.

## Resolved in this organization program

- Added canonical project/status/documentation/plan indexes, a read-only audit
  script, Apache-2.0 license, and contribution guide.
- Retired the standalone `zaion-website/` tree and repository-local
  `.claude/hooks/`; the embedded Rust `/ui` surface and static Claude permission
  policy remain.
- Removed six tracked, timestamp/PID-specific MCP test outputs from a nested
  `target/` directory; tests regenerate these paths.
- CI uses locked resolution, slash-containing branch coverage, serialized
  stateful tests, Rust `1.93.0`, an explicit architecture-audit step, and one
  Windows product-gate validation step.
- M0 governance assets now include a strict product scorecard, threat model,
  benchmark manifest/schema with 300 planned slots, and a validator that locks
  category/mission weights and evidence rules. Verified benchmark work remains
  `0/300`; static validation is not a product score.
- Docker, systemd, and Homebrew use foreground `zaion _daemon_run`; Docker opts
  into external gateway binding with `ZAION_GATEWAY_BIND=0.0.0.0`. The final
  container user is `10001:10001`, state paths are explicit/private, and
  installers reject placeholder or archive-mismatched checksums.
- `zaion doctor` now checks installed runtime state only. Development and CI
  source contracts run through `zaion architecture-audit --root <workspace>`.
- The repository pins Rust `1.93.0`; every workspace crate inherits
  `rust-version = "1.93"`; direct workspace `rand` is `0.8.6`.
- `cmd_tui` is the sole terminal gate. Ready identity/provider plus stdin and
  stdout TTY enters `run_tui_app`; non-ready/non-interactive invocations remain
  non-mutating snapshots. Parser, theme, feature flags, preference learning,
  terminal restoration, and structured gateway stdio configuration reach the
  production path.
- Removed the old blocking inline TUI runner and an uncalled legacy full-screen
  observability render branch. The selected chat-first rail/overlay application
  remains covered by focused tests.
- Gateway G0 centralizes bind resolution and service identity: default
  `127.0.0.1:7821`, environment/CLI overrides, non-redirecting strong
  `zaion.gateway.health.v1` probes, verified-only port reuse, and explicit
  Docker external binding. Both current loops now enforce non-loopback token
  strength, same-origin/allowlisted CORS, constant-time bearer comparison,
  bounded headers/bodies/connections, read deadlines, and unambiguous body
  framing.
- Wake success completion now occurs after `answer.trace`, `turn.proof`, and
  tool-receipt/proof closure. Runtime event ordering and architecture source
  gates lock the sequence.
- `zaion-runtime` now owns `WakeRequest`, `StreamEvent`, `StreamCallback`,
  `ToolCallEvent`, and `WakeOperationRecorder`; CLI process surfaces only
  re-export them. Recursive source gates and a negative fixture reject local
  protocol redefinitions or restoration of the removed CLI `wake_stream.rs`.
  Runtime conversion of visible tool calls now applies panel redaction
  internally.
- `zaion-runtime` now also owns `TurnExecution`, the only `ProofClosure`,
  `ProofClosureVerifier`, and deterministic `EvidenceSubgraph` construction.
  Default and unified successful wakes return proof-bound typed completion;
  handled/scheduled/cancelled paths no longer forge provider output. The
  verifier closes signed trace/proof/receipt topology, hashes, evidence nodes,
  ledger chain, and signed compression namespace transitions.
- Runtime now also owns validated `AuthenticatedIngress`, the nine-state CAS
  model, and a default-deny `ToolBroker`. A separate CLI V2 bridge wires them
  through one real local `zaion wake` path under `ZAION_TURN_CONTRACT_V2=1`;
  operation evidence includes admitted state/revision, broker allows execute one
  time, and denials produce blocked receipts without invoking the handler.
- Runtime TurnStore now assigns an append-only database-wide outbox ordinal,
  validates complete per-turn history plus the tenant delivered prefix before
  signing, and accepts completion only from a sealed `VerifiedEventCommit` that
  is reverified inside a second `BEGIN IMMEDIATE`. Completion first stores the
  event ID, signer public key and Ledger instance UUID in an immutable evidence
  table, then marks delivered in the same transaction. Legacy missing evidence
  fails closed and requires sealed, in-order repair. The bounded runtime
  dispatcher invokes this path, and the foreground daemon owns dispatcher
  lifecycle for persistent principal ledgers.
- `EventLedger` now uses WAL, `synchronous=FULL`, a five-second busy timeout,
  transactional migration/append, and a unique `(principal_id, seq_num)` index.
  Every open validates the chain, and `pragma_index_xinfo` requires the exact
  non-partial BINARY/ASC index shape. Legacy chain metadata is deterministically
  rebuilt with an atomic digest/count record; a durable marker rebuilds missing
  or stale FTS content once; non-legacy corrupt or mixed chains fail closed.
  Explicit keyed append and preassigned-ID insertion distinguish identical
  retries from `EventIdConflict`, while sync imports predecode and atomically
  commit the whole bundle instead of using an existence-check race.
- Compression policy now distinguishes config-driven automatic attempts from
  explicit forced attempts. Forced compression can run below the configured
  threshold, uses the same middle selection for provider-summary preview and
  session splitting, and records an honest no-op when no safe middle exists.
- Ouroboros/Watchdog restart configuration now uses the current foreground
  runtime entry `zaion _daemon_run` instead of the removed
  `zaion daemon run-inline`. Restarted children detach stdio, and tests inject a
  harmless test executable rather than accidentally launching the real daemon.

## Local disk and worktree facts

| Path | Safety note |
| --- | --- |
| `target/` | Reproducible build/test output; large but safe only for scoped cleanup |
| `.claude/worktrees/` | Contains three registered secondary Git worktrees; inspect each before cleanup |
| `claude-code-source/` and zip | Ignored reference mirror/archive |
| `zaion-data/` | Local identity/ledger/runtime data; never bulk-delete |

Do not run broad `git clean -fdX`. Registered worktrees are independent dirty
worktrees, not cache folders.

## Verification observed through the 2026-07-22 slice

| Check | Result |
| --- | --- |
| `scripts/project-audit.ps1` | PASS with five documented governance warnings |
| `cargo +stable check -p zaion-cli --all-targets --locked` | PASS |
| `cargo +stable clippy -p zaion-cli --all-targets --locked -- -D warnings` | PASS |
| `cargo +stable check/clippy -p zaion-runtime --all-targets --locked` | PASS; strict Clippy warnings denied |
| `cargo +stable test -p zaion-runtime --locked -- --test-threads=1` | PASS: 481 tests plus one doctest |
| `cargo test -p zaion-cli --locked -j1 -- --test-threads=1` | PASS: 501 unit tests plus integration suites of 15, 139, 2, 5, 11, and 3 tests |
| `cargo +stable test -p zaion-runtime turn_store --locked -- --test-threads=1` | PASS: 51 durable admission/CAS/recovery/order/signing/completion/dispatcher/integrity tests |
| `cargo +stable test -p zaion-runtime turn_store::dispatcher::tests --locked -- --test-threads=1` | PASS: 17 dispatcher lifecycle/fencing/retry/quarantine/reaper tests |
| `cargo +stable test -p zaion-cli commands::network::daemon::tests --locked -j1 -- --test-threads=1` | PASS: 19 daemon gateway/outbox/shutdown tests |
| `cargo test -p zaion-cli commands::process::wake::tests --locked -- --test-threads=1` | PASS: 43 focused wake tests, including duplicate replay and explicit quarantine |
| `cargo test -p zaion-cli commands::process::wake_contract_v2::tests --locked -- --test-threads=1` | PASS: 8 V2 ingress/state/tool-broker/retry tests |
| `cargo +stable test -p zaion-cli wake_protocol --locked -- --test-threads=1` | PASS: 2 positive/negative ownership-gate tests |
| `cargo +stable test -p zaion-cli process::tui --locked -- --test-threads=1` | PASS: 82 focused tests |
| `cargo +stable test -p zaion-cli --test gateway_characterization --locked -- --test-threads=1` | PASS: 5 process-level tests |
| Gateway contract, CORS, and concurrent health tests | PASS: 12 contract tests plus 2 focused response/concurrency tests |
| Wake completion order and proof-closure source-gate tests | PASS: 2 focused tests |
| `cargo +stable run -p zaion-cli --locked -- architecture-audit --root .` | PASS: all source gates |
| `cargo +stable test -p zaion-cli --test cli_stable_surface --locked -j1 -- --test-threads=1` | PASS: 139 tests, including stable ingress proof and compression namespace regressions |
| `cargo +stable check --workspace --all-targets --locked` | PASS |
| `cargo +stable clippy --workspace --all-targets --locked -- -D warnings` | PASS |
| `cargo test -p zaion-ledger -p zaion-sync --locked -j1 -- --test-threads=1` | PASS: 41 ledger tests and 24 sync tests; crate doctests also pass |
| `cargo +stable test -p zaion-ledger --locked -- --test-threads=1` | PASS: 54 Ledger tests plus doctests |
| root `cargo metadata --locked --no-deps` / `cargo check --locked` | PASS: 36 workspace members, one default member (`zaion-cli`); explicit workspace gates remain unchanged |
| `cargo test --workspace --exclude zaion-cli --locked -j1 -- --test-threads=1` | PASS: all 35 non-CLI crates, including unit/integration/property/trybuild/doctest targets |
| `cargo +stable test --workspace --doc --locked -j1` | PASS: all workspace doctests, four passed and one ignored |
| `cargo +stable test -p zaion-watchdog --locked -- --test-threads=1` | PASS after restart-entry cleanup: 53 tests plus doctests |
| `cargo +stable fmt --all -- --check` | PASS |
| `git diff --check` | PASS; only existing line-ending notices |
| `scripts/check-release-assets.sh` via Git Bash | PASS; templates remain `NOT PUBLISHABLE`, artifacts remain `UNSIGNED` |
| `scripts/validate-product-gates.ps1` | PASS: static contract 300/300 slots; verified evidence remains 0/300 |
| `cargo audit --no-fetch` | WARN: five allowed warnings; advisory DB not freshly fetched |
| Docker image build/runtime smoke | Not run: Docker daemon unavailable |
| Homebrew formula Ruby syntax check | Not run: Ruby unavailable |

## 2026-07-22 stage record

- Changed files: `crates/zaion-runtime/src/{turn_store.rs,lib.rs}`;
  `crates/zaion-runtime/src/turn_store/dispatcher.rs` and
  `turn_store/dispatcher/{persistence.rs,tests.rs}`;
  `crates/zaion-cli/src/commands/network/daemon.rs`; active `ROADMAP.md` and
  `docs/PROJECT_STATUS.md`.
- `OutboxDispatcher` now owns bounded worker startup, polling/wakeup, fenced
  leasing and renewal, deterministic bounded retry, immutable terminal
  quarantine, health snapshots and graceful shutdown. It resolves signing
  material, renews and fully revalidates the claim, rechecks the deterministic
  event and crosses an explicit append-admission fence before signing.
- Shutdown and append admission share one ordering lock. Work before admission
  is cancellable and releases its lease; admitted append plus sealed completion
  is allowed to finish. Absolute caller deadlines bound concurrent shutdowns,
  timeout handles stay joinable, `Drop` transfers them to a reaper, worker panic
  is health-visible, and incomplete post-append completion fails closed.
- Foreground daemon composition starts one dispatcher for each durable
  `ProcessStore` principal ledger, capped at 32 and one worker per ledger. It
  uses persistent private keys for new signatures and persistent public keys to
  recover already signed events, refreshes identities, fails closed on unhealthy
  workers, broadcasts shutdown first and joins all dispatchers in parallel under
  one aggregate deadline.
- Daemon stop now uses a 20-second cooperative window plus bounded two-second
  TERM and force-kill windows. PID zero and malformed files cannot enter process
  APIs; ownership changes while the original PID is still alive are not reported
  as exit, and ownership/liveness are revalidated around every signal.
- Verification: dispatcher `17/17`; TurnStore `51/51`; Runtime `481/481` plus
  one doctest; daemon `19/19`; workspace strict Clippy and all-target check;
  format, architecture audit, project audit and diff check. Independent
  dispatcher and daemon review found no remaining P0/P1 in this stage. The
  project audit still reports the five documented repository warnings.
- Remaining boundaries: outbox lease revalidation and Ledger append are not one
  SQLite transaction; the implementation uses a 12-second minimum commit window
  and deterministic idempotent recovery, not a strict global fence. Synchronous
  external KMS/HSM resolution has no cancellation protocol, so the reaper owns
  timed-out workers. Quarantine has no append-only resolution/reauthorization
  protocol. Tenant-prefix verification can become `O(N^2)`. Telegram and gateway
  connection threads still lack complete cooperative join/drain ownership; the
  HTTP server owner remains split. No global chronology, external exactly-once,
  Hermes-surpassed, enterprise-ready or 10/10 claim is made; benchmark evidence
  remains `0/300`.

## 2026-07-19 stage record

- Changed files: `crates/zaion-ledger/src/{binding.rs,ledger.rs,lib.rs}`;
  `crates/zaion-runtime/src/{turn_store.rs,turn_store/dispatch.rs,turn_store/tests.rs,lib.rs}`;
  active `ROADMAP.md` and `docs/PROJECT_STATUS.md`.
- `VerifiedEventCommit` now seals the exact event ID, signer public key,
  canonical live SQLite path, persisted logical database UUID and binding
  digest. Same-path replacement with a different UUID, identity-row drift,
  wrong path/principal/binding, malformed signature and missing event all fail
  closed; v1 proof verification remains additive and unchanged.
- TurnStore assigns durable commit ordinals, preserves exact legacy event types,
  reconstructs and verifies every revision, enforces the tenant head and fencing
  lease, then returns a private `SigningValidatedOutbox`. Completion reloads the
  Store under `BEGIN IMMEDIATE`, re-verifies all delivered predecessors and the
  target event through the same connection, persists immutable per-event signer
  evidence, and only then changes status to delivered. Failpoints prove that an
  evidence insert without the status update rolls back atomically.
- `PrincipalId` is intentionally not changed: it is a non-reversible
  SHA-256/Base58 public-key digest, not `did:key`. Tenant-prefix verification now
  uses each delivered event's persisted verified public key, including prefixes
  containing multiple principals, and derives the expected PrincipalId before
  rechecking the Ledger signature. Missing legacy evidence blocks followers and
  can be repaired only by the original sealed commit in ordinal order.
- Verification: TurnStore `34/34`; Runtime `464/464` plus one doctest; Ledger
  `54/54` plus doctests; strict Runtime/Ledger and workspace Clippy; workspace
  all-target check; format, architecture audit, project audit and diff check all
  pass. The project audit still reports the five documented repository warnings.
- At this slice the background dispatch lifecycle was still the next gap; the
  2026-07-22 stage closes the bounded worker/retry/quarantine/health/shutdown
  portion. Cumulative tenant-prefix verification can become `O(N^2)` and needs authenticated
  checkpoints/batching before scale. Pre-signing turn/outbox hashes do not yet
  have a signed or MACed admission root. Coherent arbitrary SQLite DDL rewrites
  remain outside the guarantee; signature v2 still does not bind event
  ID/time/sequence/previous hash. No global chronology, external exactly-once,
  Hermes-surpassed, enterprise-ready, or 10/10 claim is made.

## 2026-07-15 stage record

- M0/M1/M2 continuation changed files: product scorecard, threat model,
  benchmark schema/manifest/validator and CI; gateway contract, daemon,
  standalone server and characterization tests; Docker/install/release assets;
  runtime authenticated ingress, turn state, tool broker and wake request; CLI
  local V2 bridge/wake/profile wiring and focused tests; ledger
  append/migration/tests and sync import; active roadmap/status.
- The product gate passes only as a static governance contract: weights are
  locked, 300 slots exist, and all remain planned with `0/300` verified. Release
  validation likewise reports current templates `NOT PUBLISHABLE` and artifacts
  `UNSIGNED`; neither result is a market-readiness claim.
- Gateway requests now have real authentication/CORS/size/deadline/connection
  boundaries in both current loops. Non-loopback startup fails closed without a
  token of at least 32 bytes. Server ownership, browser credential bootstrap,
  CSRF/session design, unified rate limiting, and production OIDC/mTLS remain
  open.
- `ZAION_TURN_CONTRACT_V2=1` now activates one local CLI production path. It
  binds the loaded Ed25519 principal and process workspace to validated ingress,
  persists canonical request/authority snapshots and hashes, serializes one
  active turn per stable actor key, advances CAS state with a hash-bound outbox
  in the same SQLite transaction, and gates native tools through `ToolBroker`.
  Duplicate terminal ingress returns the stored result before a second
  `channel.received`. New-process retries may have a different `received_at`;
  strict comparison ignores only that field, reuses the first authority and
  deadline, and reconciles concurrent admission after the atomic insert.
  Durable materialization returns typed integrity failures and verifies JSON
  hashes, denormalized authority columns, deterministic IDs, terminal outcomes,
  and outbox bindings. Expired pre-effect states abort and uncertain
  running/tool states quarantine. The pending outbox is not yet
  signed/dispatched, and sensitive approvals, MCP serve, HTTP/channel identity,
  and other surfaces remain outside the V2 authority.
- `EventLedger` append now takes one `BEGIN IMMEDIATE` transaction from tail read
  through sequence/previous-hash allocation and insert, with WAL,
  `synchronous=FULL`, a five-second busy timeout, and a unique principal sequence
  index. Every open validates the chain; the index must be non-partial with exact
  BINARY/ASC principal and sequence keys. Chain verification rejects gaps and
  non-zero genesis positions. Compatible legacy/default metadata is
  deterministically re-chained with before/after digest and count evidence, and
  a durable marker rebuilds an existing stale FTS index once; corrupt indexed,
  mixed, or non-default chains fail closed pending explicit backup and repair.
- Ordinary append retains new-fact UUID semantics. New keyed signed append uses
  deterministic `evt-idem-*` IDs and accepts only identical retries;
  preassigned-ID insert reports `Inserted`/`Existing`, and sync now fails closed
  on conflicting immutable content without a racy pre-check. Sync predecodes the
  entire bundle and commits it through one atomic batch API; malformed or
  conflicting later entries leave no newly imported prefix. Signature v2 does
  not bind the event ID, event time, sequence, or previous hash, so these IDs
  are deterministic local identities rather than cryptographically signed IDs.
- Continuation verification: workspace all-target check and strict Clippy;
  runtime 443 tests plus one doctest; full CLI 501 unit tests plus all six
  integration binaries; 13 turn-store tests; 43 wake tests; 8 bridge tests; 41
  ledger tests; 24 sync tests; 5 gateway characterization, 12 gateway contract,
  and 2 focused CORS/concurrent health tests; format, architecture/project audit,
  product/release validators, workspace doctests, and diff check pass. Docker
  client exists but the daemon is unavailable, so image build/runtime smoke was
  not run. All 35 non-CLI workspace crates and the complete CLI suite passed as
  separate commands; the earlier combined timeout remains uncounted.
- Changed files: runtime `evidence_graph`, `turn_kernel`, `turn_outcome`,
  `turn_proof`, wake request/stream exports, feature policy, compressor,
  compression split, unified runtime, and architecture labels; CLI
  default/unified wake construction, wake/chat/TUI help and flags,
  MCP/HTTP/Telegram/ACP ingress builders, architecture gates, and stable-surface
  tests; active roadmap, status, and project map.
- Verification: runtime/CLI all-target checks and strict Clippy, format, 408
  runtime tests plus one doctest, 139 CLI stable-surface tests, stable runtime
  ingress proof matrix, architecture audit, diff check, and project audit pass.
- Full CLI regression exposed a too-strict proof-closure namespace check on the
  established compression parent-to-child session transition. The fix binds the
  existing signed child continuation event into the answer evidence graph and
  `TurnProof`; runtime positive/negative tests reject mismatched transition
  metadata while all three compression continuation tests pass.
- One runtime-owned `WakeFeaturePolicy` now resolves memory, MCP, compression,
  webhooks, cache, and smart-route before dispatch. Disable flags win, chat/TUI
  surfaces forward them, unified cache/smart-route are live, and internal
  MCP/HTTP/Telegram/ACP requests inherit automatic compression without claiming
  an explicit request. Signed proofs record the effective cache/smart-route and
  memory/MCP values.
- Correcting the default wake budget from a hardcoded 8K value to the configured
  budget exposed that explicit compression requests, including `--compress`,
  still stopped at the automatic threshold. Runtime now has a real forced
  compressor/split path and a matching forced provider-summary preview. The
  compression proof, active-child resolver, and iterative-summary regressions
  all pass with the configured 200K default.
- Next gap: do not connect the state-event outbox dispatcher until it has a
  persistent cross-turn commit ordinal, leased-payload revalidation against
  current turn state before signing, and `complete_outbox` verification of the
  referenced ledger event/content. Production dispatch also needs bounded
  retry, dead-letter, health, shutdown, and mixed-chain backup/repair contracts;
  additive signature v3 must bind event identity/time/sequence/previous hash
  while v2 remains verifiable. Then add real turn-store schema migrations plus
  short actor-lease heartbeat/fencing, add
  real approval/clarification/resume grants for sensitive effects, and migrate
  credential-derived HTTP/channel/MCP ingress without fallback execution. Also
  propagate cancellation through live provider/tool execution, deliver typed
  terminal outcomes to every surface, unify gateway ownership and browser auth,
  and complete signed release/Docker/benchmark evidence.

## 2026-07-14 stage record

- Changed files: TUI gate/application/launcher/help/tests; gateway bind, daemon,
  dashboard, routes, Docker/release checks and gateway characterization tests;
  wake completion ordering and architecture audit; workspace toolchain/manifests
  and lockfile; runtime wake request/stream modules, CLI compatibility re-export
  and ownership gates; active project and user documentation.
- Verification: focused CLI/TUI/Gateway/proof tests, full workspace
  format/check/Clippy/tests, architecture audit, diff check, and release-asset
  validation all pass. The post-full-test Watchdog cleanup was rechecked with
  all Watchdog tests plus final workspace check and strict Clippy. The runtime
  protocol continuation additionally passed 6 runtime wake tests, 2 ownership
  gate tests, all 137 CLI stable-surface tests, strict runtime/CLI Clippy, and
  the architecture audit.
- Next gap: unify `RuntimeOutput`, typed turn outcome, and proof closure inside
  `zaion-runtime`; propagate cancellation through provider/tool execution;
  normalize wake feature flags; then continue with one production gateway
  server, remaining TUI generations, dependency warnings, and giant-file
  decomposition.

## Decision gates

1. Should the two registered `.claude/worktrees/` remain active? Their build
   outputs can be cleaned only after confirming no active task depends on them.
2. Should `.claude/settings.local.json` be removed from Git tracking while
   remaining on disk as an ignored machine-local file?

## Ordered cleanup program

| Stage | Scope | State |
| --- | --- | --- |
| 0 | Canonical project map, docs/plan indexes, read-only audit | Implemented |
| 1 | Choose and test one authoritative interactive TUI path | Partial: production path selected; PTY and remaining generations open |
| 2 | Retire standalone website and repository-local Claude hooks | Implemented |
| 3 | Make CI and release checks match the chosen repository shape | Implemented |
| 4 | Isolate a workspace-wide rustfmt-only change | Implemented |
| 5 | Split Telegram, wake, system, and TUI monoliths behind stable APIs | Partial: runtime wake protocol and proof-bound typed result contracts extracted; one dead TUI render generation removed; execution choreography and major splits remain |
| 6 | Normalize crate metadata and workspace dependency declarations | Partial: Rust version normalized |
| 7 | Archive/reconstruct corrupted historical ledger sections | Open |
| 8 | Establish a repeatable full check/test/clippy baseline | Implemented and rerun for the current slice |
| 9 | Separate user runtime doctor from source/ledger architecture audits | Implemented |

Overall project organization remains `PARTIAL`. The entry truth, toolchain,
doctor split, hardened Gateway G0, runtime protocol/proof-result ownership, one
feature-flagged authenticated local CLI/tool-broker path, verified state-outbox
signing/completion, and daemon-owned bounded dispatch lifecycle are stronger.
All-surface authorization, typed terminal-result propagation, execution/gateway
ownership, dispatcher coverage outside the foreground daemon, quarantine
resolution, deep file splits, historical recovery, dependency warnings,
benchmark evidence, and release identity remain open.
Latest-Hermes comparison also remains `PARTIAL`.
