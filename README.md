# Zaion

Zaion is a local, auditable agent runtime. The stable v0.1 path is intentionally
small: configure one provider, run `doctor`, send the first chat, inspect status
and events, then add MCP or sync when the basics are healthy.

## Stable First Path

One-command release install:

```bash
curl -fsSL https://raw.githubusercontent.com/zaimouren1/ZAION/main/install.sh | sh
```

```powershell
irm https://raw.githubusercontent.com/zaimouren1/ZAION/main/install.ps1 | iex
```

This installs one global command: `zaion`. On a ready terminal where both stdin
and stdout are interactive, the default command opens the chat-first neural TUI.
Without a ready identity, provider, or terminal, it prints a non-mutating neural
status snapshot instead. `zaion tui` enters the same authoritative TUI gate
explicitly, `zaion dashboard`
opens the embedded browser WebUI control plane, `zaion start` launches the full
background runtime and channels, and `zaion gateway start` is the lower-level
HTTP gateway service for advanced use.
Before the first tagged release exists, the same installer falls back to
`cargo install --git ... --bin zaion --locked --force` when Rust and Git are
available.

```bash
zaion onboard
zaion doctor
zaion identity show
zaion capability show
zaion chat "Hello"
zaion status
zaion events
```

Use `ZAION_HOME` to isolate config, profiles, MCP, channels, and local data:

```bash
# PowerShell
$env:ZAION_HOME="$env:TEMP\zaion-demo"

# POSIX shell
export ZAION_HOME="$(mktemp -d)"
```

## What Is Stable

Stable first-day commands:

- `zaion --help`, `zaion help --all`
- `zaion onboard`
- `zaion doctor`
- `zaion identity show`, `zaion identity continuity`, `zaion identity verify`
- `zaion capability show`
- `zaion chat "Hello"`
- `zaion status`, `zaion events`

Stable extensions:

- `zaion mcp add|list|configure|test` and `zaion chat --mcp`
- `zaion sync export|import|diff|status|relay`
- `zaion tg status|doctor|set-token|start`
- `zaion dashboard`
- `zaion tui --check`, `zaion tui`

Product entry map:

- `zaion` opens the chat-first neural TUI when ready, otherwise a neural status
  snapshot with next steps.
- `zaion help` prints state-aware setup guidance and the product entry map.
- `zaion onboard` configures provider/model/channels and points to the product
  TUI, browser WebUI, full runtime, and HTTP gateway.
- `zaion dashboard` opens the browser WebUI; `status` and `trace` remain CLI
  compatibility views.

Experimental modules are marked in CLI help under `EXPERIMENTAL`. Do not treat
Rollup/ZK, OPD, Singularity, Enclave, direct MCP `/call`, or self-evolution as
production security or production ZK features yet.

Phase 8 adds the executable paradigm proof layer:

- `zaion compare inventory|dossier|matrix` for Hermes/cc-haha source
  inventories and source-backed breakthrough proof.
- `zaion macro status|verify|report` for the Phase 8-C macro-module maturity
  gate.
- `zaion config suggest|apply-suggestion` and `zaion preference show|set` for
  optional conversational configuration without lengthening onboard.
- `zaion omni status|trace` for canonical channel/session envelopes.
- `zaion context build|trace|verify|replay` for small-window context packs.
- `zaion memory add-fact|trace|verify|invalidate|graph` for traceable memory
  atoms.
- `zaion activity status|configure|sample|trace` and `zaion thought list|show`
  for opt-in activity continuity.

## Providers

Supported stable provider keys:

| Provider | Config key | Environment key | Default base URL |
| --- | --- | --- | --- |
| Anthropic | `anthropic_api_key` | `ANTHROPIC_API_KEY` | `https://api.anthropic.com` |
| OpenAI | `openai_api_key` | `OPENAI_API_KEY` | `https://api.openai.com/v1` |
| Groq | `groq_api_key` | `GROQ_API_KEY` | `https://api.groq.com/openai/v1` |
| Mistral | `mistral_api_key` | `MISTRAL_API_KEY` | `https://api.mistral.ai/v1` |
| Ollama | no key required | no key required | `http://localhost:11434/v1` |

Example:

```bash
zaion config set provider ollama
ollama pull llama3.2
zaion doctor
zaion chat "Hello"
```

## Documentation

- [Changelog](CHANGELOG.md)
- [Project map](docs/PROJECT_MAP.md)
- [Current project status](docs/PROJECT_STATUS.md)
- [Active roadmap](ROADMAP.md)
- [Documentation index](docs/README.md)
- [Contributing](CONTRIBUTING.md)
- [Quick start](docs/QUICK_START.md)
- [Capability status](docs/CAPABILITY_STATUS.md)
- [CLI stability baseline](docs/CLI_STABILITY.md)
- [Provider setup](docs/PROVIDERS.md)
- [Doctor troubleshooting](docs/DOCTOR.md)
- [Phase 8 runtime proof](docs/PHASE8.md)
- [Maturity roadmap](plans/ZAION_MATURITY_ROADMAP.md)

## Verification

The current full Rust verification path is:

```bash
cargo check --workspace --all-targets --locked
cargo test --workspace --locked -j1 -- --test-threads=1
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
bash scripts/check-release-assets.sh
```

The former standalone public website has been retired. The product dashboard
served at `/ui` is embedded in the Rust gateway.
