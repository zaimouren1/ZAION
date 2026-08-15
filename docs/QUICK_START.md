# Zaion Quick Start

This is the stable v0.1 path. It should work for a new user before they need to
understand the larger Zaion architecture.

## 1. Install

Release install:

```bash
curl -fsSL https://raw.githubusercontent.com/zaimouren1/ZAION/main/install.sh | sh
```

```powershell
irm https://raw.githubusercontent.com/zaimouren1/ZAION/main/install.ps1 | iex
```

Both installers install one global command: `zaion`. On a ready terminal where
stdin and stdout are interactive, `zaion` opens the chat-first neural TUI;
`zaion tui` is the explicit form of the same authoritative path. Otherwise it
prints a non-mutating readiness snapshot. `zaion dashboard open` starts/checks
the local gateway and opens the separate browser WebUI.
Before the first release asset exists, the same one-command installer falls
back to source installation with `cargo install --git` when Rust and Git are
available.

Source install for contributors:

```bash
cargo install --path crates/zaion-cli --locked
```

## 2. Choose One State Home

```powershell
$env:ZAION_HOME="$env:TEMP\zaion-demo"
```

```bash
export ZAION_HOME="$(mktemp -d)"
```

`ZAION_HOME` contains config, profiles, MCP config, channels, and data. Use it
for demos and tests so state does not leak between runs.

## 3. Configure

```bash
zaion onboard
```

Choose one provider. For local-only use, choose Ollama and make sure the model
exists:

```bash
ollama pull llama3.2
```

## 4. Run Doctor

```bash
zaion doctor
```

Do not skip this step. Fix provider, key, base URL, model, or state issues
before treating the first chat as healthy.

## 5. First Chat

```bash
zaion chat "Hello"
```

If it fails, run `zaion doctor` again and follow `docs/DOCTOR.md`.
