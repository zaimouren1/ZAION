# CLI Stability Baseline

Phase 7 makes terminal and CLI behavior the product baseline. The
stable CLI path stays small and testable so later channels and macro modules do
not blur the first-day experience.

## Stable First Path

These commands are stable first-day surfaces:

```bash
zaion --help
zaion help --all
zaion onboard
zaion doctor
zaion identity show
zaion identity continuity
zaion identity verify
zaion capability show
zaion config show
zaion config set provider ollama
zaion create
zaion chat "Hello"
zaion wake <pid> "Hello"
zaion status
zaion list
zaion events
zaion logs
zaion export <pid>
zaion import <keypair_path>
```

Rules:

- `zaion` opens the chat-first ratatui application when identity, provider,
  stdin TTY, and stdout TTY preconditions are ready. Otherwise it prints a
  non-mutating neural status snapshot that guides users to `zaion onboard`.
- `zaion --help` and `zaion help` print state-aware setup guidance and do not
  mutate state.
- `zaion help --all` must group commands by maturity:
  `STABLE FIRST PATH`, `STABLE EXTENSIONS`, `BETA / ADVANCED`,
  `EXPERIMENTAL`, and `ENVIRONMENT`.
- Quick help must explain the product entry map: `zaion` and explicit
  `zaion tui` use the authoritative terminal TUI/snapshot gate, `zaion dashboard` is
  the browser WebUI, `zaion start` is the full runtime/channels launcher, and
  `zaion gateway start` is the advanced HTTP gateway service.
- Quick help must not recommend self-evolution, Rollup/ZK, Singularity, OPD,
  Enclave, or other macro modules.
- Help and onboarding output must remain ASCII-only for predictable terminal
  rendering and snapshot tests on Windows, macOS, and Linux.
- Identity and capability commands are stable startup-contract surfaces. They
  must not claim tools, permissions, or memory evidence that the local state
  cannot show.

## Stable Extensions

These commands are stable extensions once the first path is healthy:

```bash
zaion mcp add|remove|list|configure|test|serve
zaion chat "use tools" --mcp
zaion sync export|import|diff|status
zaion sync relay
zaion tg status|doctor|set-token|start
zaion dashboard
zaion tui --check
zaion tui
```

The sync relay is token-protected and local/LAN-oriented. It is documented as a
stable extension, not as a public hosted service.

Telegram is a stable extension for token/profile setup and daemon handoff. Its
doctor path checks token source, provider readiness, default process readiness,
and daemon state. `zaion dashboard` opens the browser WebUI control plane and
starts the HTTP gateway if needed. The TUI is a stable extension over the same
wake/chat path; `zaion tui --check` validates provider and process readiness.
`cmd_tui` is the single entry gate: ready interactive terminals call
`run_tui_app`, while non-ready or non-interactive invocations remain
mutation-free snapshots. Parser, theme, feature flags, and structured gateway
stdio configuration are consumed by that production path.

Stable wake-dispatched turn entries share one proof topology in the signed
ledger: `channel.received -> omni.route -> channel.sent -> answer.trace ->
turn.proof`. Successful legacy stream completion is emitted only after the
answer trace, turn proof, and any tool-receipt/proof join have closed. The
current proof-matrix coverage includes `wake`, `chat`,
Telegram simulation/loop dispatch, API `POST /v1/runs`, webhook agent dispatch,
and TUI turns. MCP HTTP direct calls are stable-extension tool receipt paths,
not turn-proof runtime entries unless the request explicitly opts into
`runtime_route = "wake"`; ACP stdio is canonical ingress evidence unless the run
explicitly requests the wake runtime route.

Webhook agent dispatch through `zaion webhook serve` is part of that
wake-dispatched matrix when a subscription has a `principal_id`. The HTTP
response exposes the signed receipt schema version and the wake proof ids under
`agent_trigger`: `runtime_scope = "turn_runtime"`, `runtime_route = "wake"`,
`proof_chain`, `ingress_event_id`, `output_event_id`,
`answer_trace_event_id`, and `turn_proof_event_id`. If the ledger chain cannot
be verified, webhook dispatch must return a failed agent trigger instead of
claiming a completed turn.

Non-turn protocol entries must label their runtime scope in machine-readable
output and ledger payloads. MCP HTTP direct calls use
`runtime_scope = "receipt_only"` and `proof_chain = null`; their signed
`channel.received`, `mcp.tool_called`, and `tool.receipt` events prove tool
execution, not assistant answer lineage. ACP stdio `runs/create` uses
`runtime_scope = "ingress_only"` and `proof_chain = null`; its signed
`channel.received` event proves queued ingress only. If an MCP HTTP body or ACP
JSON-RPC request sets `runtime_route = "wake"`, the host builds a canonical wake
request from the validated envelope, dispatches through the wake runtime,
fail-closes unless the signed proof chain exists, and returns
`runtime_scope = "turn_runtime"`, `runtime_route = "wake"`, `proof_chain`, and
the `ingress_event_id`, `output_event_id`, `answer_trace_event_id`, and
`turn_proof_event_id` fields from the same
`channel.received -> omni.route -> channel.sent -> answer.trace -> turn.proof`
matrix.

## Beta / Advanced

Beta and advanced commands are useful but not part of the first-day promise:

- `channels`, `webhook`, WhatsApp, and future platform bridges outside the
  Telegram stable-extension path
- `start`, `stop`, `daemon`, `gateway`
- `agent`, `pair`, `profile`, `honcho`
- memory, context, embedding, sessions, insights
- traceable memory atoms and context pack proofs
- omni-session envelope diagnostics
- conversational config suggestions and preferences
- reference inventory/matrix
- opt-in activity continuity and thought seed inspection
- secrets, auth, audit, security
- codex, hub, models
- git, undo, checkpoint, route
- budget, reality, DID, proprioception diagnostics, benchmark

Promotion from beta to stable requires user-path tests, recovery behavior, CI
coverage, and documented security boundaries.

The current HTTP gateway loops share a G0 bind and health-identity contract:
the default is `127.0.0.1:7821`, `ZAION_GATEWAY_BIND` and explicit
`--host`/`--port` override it, and `/health` must identify
`zaion.gateway.health.v1`. This does not yet make the gateway stable: the two
server loops, `zaion-gateway` library adoption, auth, CORS, and write-surface
hardening remain beta/advanced work.

## Experimental

Experimental commands remain clearly labeled in `zaion help --all` and should
print an `EXPERIMENTAL:` warning when executing non-help operations:

- Rollup/ZK memory folding
- Proprioception `unlock`
- Singularity orchestration
- Watchdog/Ouroboros self-healing
- Shadow process executor
- Ego prompt configuration
- Autonomic runtime
- Curiosity loop
- Self-evolution
- Enclave simulation
- Runtime `execute_code` and `batch_runner` APIs
- Implicit MCP `POST /mcp/v1/call` as a turn runtime. The default endpoint is a
  signed ingress plus `tool.receipt` path labelled `receipt_only`; only the
  explicit `runtime_route = "wake"` body joins the turn proof runtime.

`execute_code` is deliberately not a stable CLI command. The stable contract is
the boundary: the top-level `CodeExecutor` delegates to the experimental
Python/Node UDS bridge only as a runtime library API, Windows must report the
Unix-only UDS bridge as unavailable, and `zaion architecture-audit` source gates must keep the
facade, Unix bridge, and parse-error path compilable and error-transparent
while it remains hidden from the stable CLI.

`batch_runner` is also a runtime library API, not a stable CLI command. The
runtime `BatchRunner` now requires `BatchRunner::with_executor(...)` for real
LLM/tool execution and fails closed from the default constructor instead of
emitting fabricated assistant trajectories. `zaion architecture-audit` source gates keep the
executor injection API, request/result types, and hidden CLI boundary in place.

Promotion from experimental to beta requires integration tests, runtime doctor
checks, source architecture audit, and documentation.
