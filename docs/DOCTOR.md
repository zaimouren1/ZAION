# Doctor Troubleshooting

`zaion doctor` is the first command to run when setup or chat fails.

```bash
zaion doctor
```

`zaion doctor` only checks installed runtime state. Repository source,
architecture-contract, and historical evidence checks are intentionally
separate:

```bash
zaion architecture-audit --root /path/to/zaion-rust
```

The architecture audit is a development/CI command and requires a source
checkout. Installed users do not need the checkout or progress ledgers for
runtime diagnosis.

Read it top to bottom:

1. Confirm the config path is the `ZAION_HOME` you expect.
2. Confirm the data path is isolated for the run you are testing.
3. Check provider type, API key status, base URL, and model.
4. Check default principal/process status.
5. Check MCP only after basic chat works.
6. Check `[identity]` and `[capability]` to confirm Zaion's startup contract.
7. Check `[activity]` before enabling background thought birth.
8. Check the `[maturity]` table to confirm Phase 7/8 order and boundaries.

## Common Issues

### `provider not set`

```bash
zaion onboard
```

or:

```bash
zaion config set provider ollama
```

### `API key is MISSING`

Set the config key:

```bash
zaion config set openai_api_key <key>
```

or use the provider's environment variable, such as `OPENAI_API_KEY`,
`ANTHROPIC_API_KEY`, `GROQ_API_KEY`, or `MISTRAL_API_KEY`.

### Ollama cannot connect

```bash
ollama pull llama3.2
zaion config set provider ollama
zaion config set ollama_base_url http://localhost:11434/v1
zaion doctor
```

### Default process is missing

```bash
zaion create
zaion doctor
```

### MCP tools are missing

```bash
zaion mcp add --name local --url http://127.0.0.1:3001
zaion mcp test local
zaion chat "use tools" --mcp
```

The direct development endpoint `POST /mcp/v1/call` is body-aware now: it
requires a persisted default principal, appends signed `channel.received`
ingress, and records signed `tool.receipt` evidence. It is still a tool receipt
path, not a wake turn. Its response and signed receipt payload are labelled
`runtime_scope = "receipt_only"` with `proof_chain = null`; expect
`answer.trace` and `turn.proof` only when the request explicitly sets
`runtime_route = "wake"`.

### Turn proof chain is missing

Stable wake-dispatched runtime entries should write this signed ledger chain:

```text
channel.received -> omni.route -> channel.sent -> answer.trace -> turn.proof
```

This applies to `wake`, `chat`, Telegram simulation/loop turns, API
`POST /v1/runs`, webhook agent dispatch, TUI turns, explicit MCP HTTP wake
requests, and explicit ACP stdio wake requests. MCP HTTP direct calls remain
receipt-only unless the POST body sets `runtime_route = "wake"`; ACP stdio
remains ingress-only unless the JSON-RPC `runs/create` request sets
`runtime_route = "wake"`.

For non-turn protocol entries, `architecture-audit` source gates require
explicit scope labels. MCP direct calls must persist `receipt_only` in returned ingress and
receipt payloads. ACP stdio must persist `ingress_only` in returned and signed
ingress payloads. Both paths must expose `proof_chain = null` rather than
implying a wake turn proof exists.

For explicit ACP wake dispatch, `architecture-audit` also source-gates the host bridge:
`zaion acp` must inject a wake runtime dispatcher, dispatch with the validated
ACP `CanonicalEnvelope`, collect stream output, verify the ACP stdio ledger
chain, and return `turn_runtime` proof ids only after the signed
received-to-proof chain exists.

For explicit MCP HTTP wake dispatch, `architecture-audit` source-gates the same bridge
shape inside `zaion mcp serve`: the handler must preserve the POST body, build a
canonical `WakeRequest` envelope, collect runtime stream output, verify the MCP
HTTP `channel.received` to `turn.proof` ledger chain, and return
`turn_runtime` plus proof ids only after that signed chain exists.

For webhook agent dispatch, `architecture-audit` source-gates the runtime bridge inside
`zaion webhook serve`: the handler must ingest the canonical webhook envelope,
dispatch through `cmd_wake_with_request`, collect wake stream output, verify the
HTTP webhook `channel.received` to `turn.proof` chain for
`channel_id = "http-webhook"`, and return `turn_runtime`, `proof_chain`,
`ingress_event_id`, `output_event_id`, `answer_trace_event_id`, and
`turn_proof_event_id` only after that signed chain exists. The HTTP delivery
receipt also exposes `schema_version` so old placeholder receipts cannot be
mistaken for the current Ed25519 receipt contract.

For unified runtime diagnostics, `architecture-audit` source-gates that
`memory_context_size` comes from the integrated execution report and
`mcp_tools_loaded` comes from a loaded `McpToolRegistry`. A runtime with memory
providers but no MCP registry should report memory context bytes without
claiming MCP tools were loaded.

### execute_code is unavailable or experimental

Runtime `execute_code` and `batch_runner` APIs are hidden from the stable CLI
path. The top-level `CodeExecutor` now delegates to the existing Python/Node
local RPC bridge through an explicit dispatcher. Unix uses Unix domain sockets;
non-Unix uses an explicit `127.0.0.1` loopback JSONL RPC listener.
Generated Python/JavaScript tool stubs include a per-run `ZAION_RPC_TOKEN` with
each local RPC request, and the parent validates that token before tool
dispatch.

The runtime `BatchRunner` no longer creates placeholder assistant trajectories.
Callers must construct it with `BatchRunner::with_executor(...)`, which receives
`BatchExecutionRequest` and returns `BatchExecutionResult`; the default
constructor fails closed until that real executor is supplied.

`zaion architecture-audit` source-gates this boundary. It checks that help keeps
`execute_code` out of the stable path, the top-level facade still reaches
`UdsCodeExecutor`, Windows/non-Unix exposes explicit loopback transport, and the
Unix bridge still contains the process, IO, thread, timeout, token binding,
tool-dispatch, and parse-error context needed to compile when promoted or
tested on Unix. It also checks that the runtime batch runner keeps the explicit
executor API and does not reintroduce fabricated assistant responses.

### Telegram is not ready

```bash
zaion tg doctor
zaion tg set-token <token>
zaion create
zaion config set provider ollama
zaion tg doctor
```

`zaion tg doctor` checks token source, channel-store path, provider readiness,
default process readiness, and whether the daemon is currently running.

### TUI exits before opening

```bash
zaion tui --check
```

The check validates the default process and provider before the TUI enters the
alternate screen.

### Identity or capability looks wrong

```bash
zaion identity show
zaion identity verify
zaion capability show
```

The identity profile lives under `ZAION_HOME`. A rename changes the display
name only; it does not change the cryptographic principal or continuity chain.

### Phase 8 context or memory proof fails

```bash
zaion context build <pid> --budget 4000 --verify
zaion context trace <context-pack-id>
zaion memory trace <memory-id>
zaion memory verify <memory-id>
```

Facts added without a source event must be explicitly marked `--user-provided`.

### Activity continuity is unexpectedly off

```bash
zaion activity status
zaion activity configure --enable --ack-cost --mode suggest-only
```

Activity continuity is off by default. Enabling it requires acknowledging the
token/network cost warning. Autonomous activity cannot perform destructive
actions, access credentials, purchase anything, modify code, or externally
deliver drafts unless explicit policy allows it.
