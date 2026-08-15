# Phase 8-B Implementation Proof Ledger

Batch: `all`

Status: all modules passed 2/3 hermes behavior copy -> zaion improvement

| Module | Stage | Proof hash | Commands |
| --- | --- | --- | --- |
| Agent Runtime Loop | stage2-zaion-improvement-proved | ed7172bf5496ea35a470e4c7a87d3ca31b4c91b03182c0c1f385044cc27e344a | zaion chat "Hello"<br>zaion chat --query <message> -m <model> --provider <provider> --resume <session> --continue <name> --skills research --max-turns 5 --source telegram --quiet<br>zaion -c --check --worktree --skills research --yolo --pass-session-id<br>zaion turn latest<br>zaion answer trace <event-id><br>cargo test -p zaion-cli chat_parser_accepts_reference_query_model_and_session_flags -- --test-threads=1<br>cargo test -p zaion-cli --test beginner_golden_path -- --test-threads=1 |
| Identity And Continuity | stage2-zaion-improvement-proved | 68d8eb366d2c792942e2b4f552425a9141672ec1da8571c926bd59649870cb34 | zaion profile list<br>zaion --profile work config show<br>zaion --profile missing config show<br>zaion profile create chat<br>zaion profile use work && zaion config show && zaion profile use default<br>zaion profile create copy --clone --clone-from work --no-alias<br>zaion profile create copyall --clone-all --clone-from work --no-alias<br>zaion profile show <name><br>zaion profile rename <old> <new><br>zaion profile import <archive><br>zaion identity show<br>zaion identity continuity<br>zaion identity verify<br>cargo test -p zaion-cli --test phase8_surface -- --test-threads=1 |
| Channel Gateway And Bridge | stage2-zaion-improvement-proved | 79631cad9c1e94685c4337eeae78a577576a36e55d73b7f5feacf79fb06624be | zaion gateway status --deep --system<br>zaion gateway stop --all --system<br>zaion --profile edge gateway status --deep<br>zaion gateway setup<br>zaion webhook add research --prompt <template> --events paper.found --description <text> --skills papers,summary --deliver telegram --deliver-chat-id <chat><br>zaion pairing list && zaion pairing approve telegram <code> && zaion pairing revoke telegram <user-id> && zaion pairing clear-pending<br>zaion tg doctor<br>zaion tg doctor --json<br>zaion tg set-token <token> --allow 42,43 --home-channel 42 --reply-mode first<br>zaion whatsapp status<br>zaion omni trace --channel telegram --sender owner --thread t --message-id m<br>cargo test -p zaion-cli --test beginner_golden_path wake_channel_envelope_records_telegram_thread_in_turn_proof -- --test-threads=1<br>cargo test -p zaion-cli --test cli_stable_surface telegram_command_copies_reference_allowlist_home_channel_setup -- --test-threads=1 |
| Memory And Session Memory | stage2-zaion-improvement-proved | f66a05f60388b56d16ab83d65b67723a9eca2f4ac29e53cc28727f6cf5cd14de | zaion memory setup --provider <provider> --model <embedding-model><br>zaion memory status<br>zaion memory off<br>zaion memory add-fact <pid> <fact> --user-provided<br>zaion memory trace <memory-id><br>zaion memory trace <memory-id> --json<br>zaion memory verify <memory-id><br>zaion memory invalidate <memory-id><br>zaion memory graph <pid> --json<br>zaion sessions list --source telegram<br>zaion sessions browse --source telegram --limit 50<br>zaion sessions export <out.jsonl> --session-id <id> --source telegram<br>zaion sessions delete <id> --yes<br>zaion sessions prune --older-than 30 --source telegram --yes<br>zaion sessions stats<br>zaion sessions rename <id> <title><br>zaion insights --days 7 --source telegram<br>cargo test -p zaion-cli --test beginner_golden_path wake_memory_turn_proof_links_context_pack_and_memory_atoms -- --test-threads=1<br>cargo test -p zaion-cli --test cli_stable_surface sessions_command_copies_reference_filters_and_yes_flags -- --test-threads=1<br>cargo test -p zaion-cli --test phase8_surface phase8b_config_auth_sessions_and_tools_copy_reference_cli_behaviors -- --test-threads=1 |
| Context Compression And Infinite Context | stage2-zaion-improvement-proved | 7683657d39b4ccfe97d50cfb5887510dfe7c206010d9df70ff457981e064dd86 | zaion context build <pid> --budget 4000 --verify<br>zaion context trace <context-pack-id><br>zaion context verify <context-pack-id> --json<br>zaion context replay <context-pack-id><br>cargo test -p zaion-cli --test phase8_surface phase8b_context_pack_large_history_under_4k_has_event_lineage -- --test-threads=1 |
| Tools, Permissions, And Safety | stage2-zaion-improvement-proved | 3cd936349f9531abedfcb76b3d0a78488300632983824c2f1363c09f39887357 | zaion capability show<br>zaion tools --summary<br>zaion mcp add node-server --transport stdio --command npx --args @modelcontextprotocol/server-filesystem . --auth oauth<br>zaion mcp configure node-server --args server --auth header<br>zaion mcp serve --verbose<br>zaion tool receipts <pid><br>zaion tool verify <pid><br>cargo test -p zaion-cli --test cli_stable_surface mcp_aliases_and_positional_add_match_reference_behavior -- --test-threads=1<br>cargo test -p zaion-cli --test beginner_golden_path wake_parser_tool_call_records_permission_receipt -- --test-threads=1 |
| Skills And Plugins | stage2-zaion-improvement-proved | d2d482b261882f3a9402a52a23002bfc73181f6081d7ad2367487d903e04eb0d | zaion skills learn <rule><br>zaion skills search <query><br>zaion skills browse --page 2 --size 5 --source github<br>zaion skills install openai/skills/skill-creator --category planning --force --yes<br>zaion skills list --source github<br>zaion skills check skill-creator && zaion skills update skill-creator && zaion skills audit skill-creator<br>zaion skills snapshot export - && zaion skills tap add owner/repo && zaion skills tap remove owner-repo<br>zaion skill promote <skill_dir> --capability <scope><br>zaion skill forget <skill-id><br>zaion plugins install <owner/repo> --force && zaion plugins uninstall <name><br>zaion plugins install owner/repo --name example --force && zaion example --help && zaion example run arg<br>zaion plugins inspect <name><br>cargo test -p zaion-cli --test cli_stable_surface skills_and_tools_accept_reference_style_global_forms -- --test-threads=1<br>cargo test -p zaion-cli --test cli_stable_surface plugins_install_copies_reference_git_manifest_and_safety_behavior -- --test-threads=1<br>cargo test -p zaion-cli --test phase8_surface phase8_identity_config_activity_context_memory_and_compare_are_wired -- --test-threads=1 |
| Activity Continuity, Cron, Proactive, Dreaming | stage2-zaion-improvement-proved | eb0db0dbea5a6ea21cca4d6607a120c9c966af2d04dc3b2e9d59c2c9ebdb2559 | zaion activity status<br>zaion activity status --json<br>zaion activity configure --enable --ack-cost<br>zaion activity sample --seed 42 --dry-run<br>zaion activity sample --seed 42<br>zaion thought show <thought-id><br>zaion cron create 30m <prompt> --name research --deliver local<br>zaion cron edit <job-id> --deliver telegram:42 --repeat 3 --skill research --add-skill summarize --remove-skill old<br>zaion cron status<br>zaion cron run <job-id><br>cargo test -p zaion-cli --test cli_stable_surface cron_command_accepts_reference_create_without_explicit_pid -- --test-threads=1<br>cargo test -p zaion-cli --test phase8_surface phase8_identity_config_activity_context_memory_and_compare_are_wired -- --test-threads=1 |
| Multi-Agent, Delegation, Teams | stage2-zaion-improvement-proved | 80b1668e9f477fc35be342761babf6cf978888bcd05011305aa092d52bdf2029 | zaion acp --help<br>zaion acp --check<br>zaion agent proof <pid> <delegate_principal> <task> --scope <scope><br>zaion agent receipts <pid><br>cargo test -p zaion-cli --test phase8_surface phase8_identity_config_activity_context_memory_and_compare_are_wired -- --test-threads=1 |
| Provider, Credential, Cost, Budget | stage2-zaion-improvement-proved | 7b26a9ccbb0fe8062f7d1ab01575bb14142fea0120dfaca258d88e415305b717 | zaion model --check<br>zaion model --provider openai --base-url <url> --api-key <key> --model <model-id><br>zaion model --model openrouter:anthropic/claude-sonnet-4.5 --api-key <key><br>zaion model --provider google-ai-studio --api-key <key> --model gemini-3.1-pro-preview<br>zaion model --provider vercel-ai-gateway --api-key <key> --model claude-sonnet-4.6<br>zaion model --provider moonshot --api-key sk-kimi-... --model kimi-k2.5<br>zaion model --inference-url <url> --client-id <id> --scope <scope> --no-browser --timeout 15 --ca-bundle <pem> --insecure<br>zaion auth add <provider> --api-key <key> --client-id <id> --scope <scope> --no-browser --timeout 15 --ca-bundle <pem> --insecure<br>zaion login --provider openai-codex --portal-url <url> --inference-url <url> --client-id <id> --scope <scope> --no-browser --timeout 15 --ca-bundle <pem> --insecure<br>zaion logout --provider openai-codex<br>zaion provider status<br>zaion provider models ollama --base-url http://localhost:11434/v1<br>zaion provider models google --base-url http://127.0.0.1:9<br>zaion provider cost --model llama3.2 --input 1000 --output 500<br>cargo test -p zaion-cli --test cli_stable_surface auth_command_copies_reference_oauth_flags -- --test-threads=1<br>cargo test -p zaion-cli --test cli_stable_surface model_command_copies_reference_gateway_aliases_and_model_normalization -- --test-threads=1<br>cargo test -p zaion-cli --test cli_stable_surface provider_models_falls_back_to_reference_curated_catalog -- --test-threads=1<br>cargo test -p zaion-cli --test beginner_golden_path onboard_fetches_model_list_and_saves_selected_model -- --test-threads=1 |
| Execution Environments, Computer Use, Sandbox | stage2-zaion-improvement-proved | 78777540e64b1b5753e594294c167f8654b9b477e5fe827efd0064e035be0030 | zaion checkpoint guard <dir> <label> --scope <scope> --syntax-file <file><br>zaion checkpoint restore <dir> <checkpoint-id><br>cargo test -p zaion-cli --test phase8_surface phase8_identity_config_activity_context_memory_and_compare_are_wired -- --test-threads=1 |
| OPD, Trajectory, Learning Loop | stage2-zaion-improvement-proved | 172889817ce1c7291828bc9b4a45b1722ae83fed81b16798640727f70219a63a | zaion opd export <pid> --out <trajectory.json><br>zaion opd verify <trajectory.json><br>cargo test -p zaion-cli --test phase8_surface phase8_identity_config_activity_context_memory_and_compare_are_wired -- --test-threads=1 |
| Frontend, TUI, Desktop, Control Plane | stage2-zaion-improvement-proved | b074555ae07c942636eb2b6a47003f24d4771eb9973863b11cfc6a9bd15e107a | zaion logs list<br>zaion logs agent -n 5 --level WARNING --session <id> --since 30m<br>zaion doctor --fix<br>zaion completion bash<br>zaion completion fish<br>zaion completion zsh<br>zaion dashboard status <pid><br>zaion dashboard trace <pid><br>zaion dashboard open<br>cargo test -p zaion-cli --test phase8_surface phase8_identity_config_activity_context_memory_and_compare_are_wired -- --test-threads=1 |
| Release, Tests, Public Proof | stage2-zaion-improvement-proved | 45d074b98a1ebda6c13e7a3a931172a76c6230fa1131ba5f355ea5ecdeaeae05 | zaion version<br>zaion -V<br>zaion update --check --gateway<br>zaion uninstall --full<br>zaion uninstall --keep-data --dry-run<br>zaion claw migrate --dry-run --source <missing-openclaw><br>zaion claw migrate --source <openclaw> --workspace-target <workspace> --preset user-data --yes<br>zaion phase8b source-map --verify<br>zaion phase8b crosswalk --verify<br>zaion phase8b proof --batch foundation --verify<br>cargo test -p zaion-cli --test phase8_surface -- --test-threads=1 |

## Three-Layer Proof

### agent-runtime-loop - Agent Runtime Loop

Copied behavior:
- native default launcher opens the interactive path after model setup
- runtime slash registry covers help, retry, undo, queue, background, model, provider, config, usage, and quit
- chat, wake, and TUI share the same lower-level process turn path
- chat accepts reference-style query, model, provider, session, worktree, checkpoint, max-turn, source, skills, and quiet/verbose flags without rejecting migrated workflows
- top-level resume, continue, worktree, skills, yolo, and pass-session-id flags launch the Zaion-native interactive path instead of being rejected

Zaion improvement:
- turn execution emits identity, capability, context pack, answer span, and channel lineage evidence
- bare zaion remains Zaion-native and does not expose reference-project commands

Paradigm breakthrough:
- none

Source paths:
- crates/zaion-cli/src/commands/launcher.rs
- crates/zaion-cli/src/commands/mod.rs
- crates/zaion-cli/src/commands/process/chat.rs
- crates/zaion-cli/src/commands/process/wake.rs
- crates/zaion-cli/src/commands/process/wake_shared.rs
- crates/zaion-cli/src/commands/turn.rs
- crates/zaion-cli/src/commands/answer.rs
- crates/zaion-runtime/src/turn_proof.rs
- crates/zaion-runtime/src/slash_commands.rs

Tests:
- crates/zaion-cli/tests/beginner_golden_path.rs
- crates/zaion-runtime/src/slash_commands.rs

### identity-continuity - Identity And Continuity

Copied behavior:
- persistent state and session identity survive normal CLI restarts
- top-level --profile/-p switches profile home before config and command dispatch
- profile use writes a sticky active profile that later commands honor until profile use default
- missing named profiles are rejected instead of being silently created
- reserved command names cannot be used as profile names or aliases
- profiles can be listed with gateway/skill status, used, created, config-cloned, full-cloned with runtime strip, shown, renamed, aliased, exported with credential/runtime exclusions, imported with archive-name inference, and deleted
- identity and status commands expose the active process and continuity state

Zaion improvement:
- startup identity contract names the small-octopus role, environment, tools, and forbidden claims
- rename and verify operations preserve continuity instead of replacing the identity

Paradigm breakthrough:
- none

Source paths:
- crates/zaion-cli/src/commands/mod.rs
- crates/zaion-cli/src/commands/profile.rs
- crates/zaion-cli/src/commands/identity.rs
- crates/zaion-ego/src/lib.rs
- crates/zaion-crypto/src/did.rs
- crates/zaion-sync/src/export.rs
- crates/zaion-sync/src/import.rs

Tests:
- crates/zaion-cli/tests/phase8_surface.rs
- crates/zaion-cli/tests/cli_stable_surface.rs

### channel-gateway-bridge - Channel Gateway And Bridge

Copied behavior:
- terminal, Telegram, gateway, webhook, and TUI surfaces are represented as channels
- gateway lifecycle exposes run, start, stop, stop --all, restart, status, install, uninstall, and setup with reference flags
- gateway service names and generated service definitions are scoped by active profile
- webhook add/list/remove/test support prompt, events, description, skills, delivery, chat target, and secret options
- Telegram setup has status, doctor, token save, token clear, and start guidance
- Telegram setup preserves allowed users, home channel, and reply mode, and runtime polling denies senders outside the allowlist unless open access is explicit
- WhatsApp setup supports bridge mode, enablement, allowlist, and session pairing guidance
- gateway pairing approve only succeeds for an existing pending code and moves the user into approved access

Zaion improvement:
- there is one official Telegram entry point: zaion tg
- channel input is normalized into a canonical envelope before runtime proof creation
- Telegram access policy is stored beside the channel profile and denied runtime messages produce signed telegram.denied evidence
- Telegram doctor can emit a machine-readable JSON readiness and access-policy report for gateways and dashboards

Paradigm breakthrough:
- none

Source paths:
- crates/zaion-cli/src/commands/network/telegram.rs
- crates/zaion-cli/src/commands/network/gateway.rs
- crates/zaion-cli/src/commands/network/pair.rs
- crates/zaion-cli/src/commands/system.rs
- crates/zaion-cli/src/commands/omni.rs
- crates/zaion-runtime/src/omni_session.rs
- crates/zaion-adapters/src/telegram_adapter.rs
- crates/zaion-cli/src/commands/webhook/mod.rs

Tests:
- crates/zaion-cli/tests/beginner_golden_path.rs
- crates/zaion-cli/tests/cli_stable_surface.rs

### memory-session-memory - Memory And Session Memory

Copied behavior:
- session memory and explicit fact storage are available from the CLI
- memory setup, status, and off manage an external provider while built-in memory remains active
- memory retrieval can participate in normal wake/chat paths
- sessions list, browse, export, delete, prune, stats, and rename mirror the reference session control surface
- sessions list/browse hide tool-source sessions by default while explicit --source filters reveal the requested source
- sessions prune accepts --older-than, --source, and --yes for scoped cleanup
- insights analytics accept reference-style --days and --source without requiring an explicit process id

Zaion improvement:
- memory facts carry source evidence, explicit user-provided markers, verification, and invalidation
- sync export/import preserves proof artifacts for later trace commands
- memory atoms carry proof hashes and trace/graph commands can emit JSON evidence for control planes

Paradigm breakthrough:
- none

Source paths:
- crates/zaion-cli/src/commands/memory.rs
- crates/zaion-cli/src/commands/memory_atoms.rs
- crates/zaion-cli/src/commands/sessions_extended.rs
- crates/zaion-memory/src/lib.rs
- crates/zaion-memory/src/projection.rs
- crates/zaion-ledger/src/session_store.rs

Tests:
- crates/zaion-cli/tests/beginner_golden_path.rs
- crates/zaion-cli/tests/cli_stable_surface.rs
- crates/zaion-cli/tests/phase8_surface.rs

### context-infinite-context - Context Compression And Infinite Context

Copied behavior:
- conversation history can be compressed before model calls
- context construction has a budgeted CLI surface

Zaion improvement:
- ContextPack records budget, source events, memory atoms, projection refs, and replay hash
- 4k budgets are verified without losing source traceability
- context verification can emit JSON proof for dashboards, gates, and small-window runtime controllers

Paradigm breakthrough:
- none

Source paths:
- crates/zaion-cli/src/commands/context_packs.rs
- crates/zaion-runtime/src/context.rs
- crates/zaion-runtime/src/compressor.rs
- crates/zaion-runtime/src/compression_split.rs

Tests:
- crates/zaion-cli/tests/phase8_surface.rs
- crates/zaion-cli/tests/cli_stable_surface.rs

### tools-permissions-safety - Tools, Permissions, And Safety

Copied behavior:
- tool calls can be parsed from model output
- MCP and local tool surfaces are exposed through CLI and runtime modules
- tools summary can be requested with the reference-style --summary flag
- MCP add/configure preserve stdio --args and --auth oauth|header options
- MCP add infers stdio transport from --command and supports --force overwrite
- MCP serve accepts reference-style verbose startup flag
- tools list uses reference toolset keys and keeps moa/homeassistant/rl disabled by default

Zaion improvement:
- parser-visible tool calls are recorded as receipts when explicit dispatch is not granted
- capability manifest and tool verification fail closed instead of silently executing

Paradigm breakthrough:
- none

Source paths:
- crates/zaion-cli/src/commands/capability.rs
- crates/zaion-cli/src/commands/tool.rs
- crates/zaion-cli/src/commands/mcp.rs
- crates/zaion-mcp/src/builtin_tools.rs
- crates/zaion-runtime/src/policy.rs
- crates/zaion-runtime/src/sandbox_tools.rs
- crates/zaion-safety/src/redact.rs

Tests:
- crates/zaion-cli/tests/beginner_golden_path.rs
- crates/zaion-cli/tests/cli_stable_surface.rs

### skills-plugins - Skills And Plugins

Copied behavior:
- skills can be learned, listed, searched, forgotten, and run from the CLI
- skills list, learn, search, install, run, and uninstall accept omitted principal resolution
- skill packages can be promoted from filesystem source into the local skill store
- plugins install supports force reinstall and remove/rm/uninstall aliases
- plugins install resolves owner/repo shorthand to GitHub, installs plugin directories, reads plugin.yaml, rejects path traversal names, copies .example config files, shows after-install.md, and reports missing required environment variables
- plugins update pulls git-installed plugins when possible and reports non-git plugin state without mutating unrelated files
- skills registry browse/search/install/inspect/list/check/update/audit/uninstall preserve reference flags
- skills snapshot export/import and tap list/add/remove preserve reference management surfaces
- skills snapshot import restores taps, hub skills, and plugin registry state
- installed enabled plugins resolve as top-level Zaion commands, while disabled plugins are rejected

Zaion improvement:
- promotion refuses packages without docs, test proof, explicit capability scope, or safety scan pass
- promotion prints the rollback command before writing the skill entry
- plugin install records capability scope, permissions, required environment variables, source digest, safety digest, install path, and rollback command
- plugin inspect exposes that metadata so installed top-level commands remain accountable capabilities rather than anonymous command aliases

Paradigm breakthrough:
- none

Source paths:
- crates/zaion-cli/src/commands/skills.rs
- crates/zaion-memory/src/skill.rs
- crates/zaion-runtime/src/sandbox.rs
- crates/zaion-runtime/src/genesis/skill_forge.rs

Tests:
- crates/zaion-cli/tests/phase8_surface.rs
- crates/zaion-cli/tests/cli_stable_surface.rs

### activity-continuity - Activity Continuity, Cron, Proactive, Dreaming

Copied behavior:
- background scheduling and proactive activity have explicit CLI controls
- activity status, configure, pause, resume, sample, and trace commands are present
- cron list, create, edit, pause, resume, run, remove, status, and tick accept reference-style omitted principal resolution
- cron create and edit preserve deliver, repeat, skill, add-skill, remove-skill, clear-skills, and script options
- cron list and status report whether the gateway is running, show active job counts and next run, and warn that jobs will not fire automatically without the gateway
- cron run reports reference-style next scheduler tick execution semantics

Zaion improvement:
- activity is disabled by default and enabling requires an explicit token/network cost acknowledgement
- thought birth uses a bounded stochastic sampler over traceable user preferences
- activity status emits JSON for control planes and thought seeds carry replayable proof hashes
- activity sample supports dry-run preview so random thought birth can be audited before it is persisted

Paradigm breakthrough:
- none

Source paths:
- crates/zaion-cli/src/commands/activity.rs
- crates/zaion-cli/src/commands/preference.rs
- crates/zaion-autonomic/src/runtime.rs
- crates/zaion-curiosity/src/ideation.rs
- crates/zaion-runtime/src/cron.rs

Tests:
- crates/zaion-cli/tests/phase8_surface.rs

### multi-agent-delegation - Multi-Agent, Delegation, Teams

Copied behavior:
- remote agents can be listed, bound, removed, spawned, and queried through ACP-style URLs
- ACP stdio mode can print help, launch, or self-check a JSON-RPC server for editor integration
- delegation is represented as a signed A2A message payload

Zaion improvement:
- local delegation proof writes principal, delegate, scope, input hash, output hash, and merge receipt to the ledger
- delegation receipts can be listed without contacting a remote worker

Paradigm breakthrough:
- none

Source paths:
- crates/zaion-cli/src/commands/system.rs
- crates/zaion-a2a/src/stdio_service.rs
- crates/zaion-cli/src/commands/network/agent.rs
- crates/zaion-a2a/src/protocol.rs
- crates/zaion-a2a/src/federation.rs
- crates/zaion-runtime/src/shadow_agent.rs
- crates/zaion-federation/src/session.rs

Tests:
- crates/zaion-cli/tests/phase8_surface.rs

### providers-credentials-cost - Provider, Credential, Cost, Budget

Copied behavior:
- setup and model flows collect provider, key, URL, and explicit model ID
- model command accepts direct provider URL, API key, model ID, and model-list discovery flags
- model command accepts portal-url, inference-url, client-id, scope, no-browser, timeout, ca-bundle, and insecure auth flags
- model command normalizes provider aliases and provider:model syntax before saving config
- provider aliases cover Gemini, Z.AI, Kimi, MiniMax, AI Gateway, OpenCode, Kilo Code, Alibaba, and Hugging Face gateway names
- provider defaults cover Hermes gateway base URLs for Google AI Studio, Moonshot, MiniMax, DashScope, Vercel AI Gateway, OpenCode, Kilo Code, and Hugging Face
- provider model listing falls back to Hermes-style curated provider catalogs when live discovery fails
- provider key resolution honors Hermes-style provider-specific environment variables and provider-scoped saved credentials
- Kimi sk-kimi-* keys route to the Kimi coding endpoint when no explicit base URL overrides it
- model names normalize per provider for aggregators, Anthropic/OpenCode hyphen rules, OpenCode Go bare names, and DeepSeek reasoner aliases
- auth add/list/remove/reset supports provider-scoped pooled credentials and OAuth-shaped flags
- login/logout support reference-style --provider and OAuth-shaped login flags
- provider health can be checked before runtime dispatch

Zaion improvement:
- model discovery fetches provider model IDs when an endpoint supports it
- provider status ties configured model, key state, pricing snapshot, and route decision together

Paradigm breakthrough:
- none

Source paths:
- crates/zaion-cli/src/commands/onboard.rs
- crates/zaion-cli/src/commands/security.rs
- crates/zaion-cli/src/commands/provider.rs
- crates/zaion-cli/src/config.rs
- crates/zaion-cli/src/commands/budget.rs
- crates/zaion-cli/src/commands/route.rs
- crates/zaion-pricing/src/cost.rs
- crates/zaion-pricing/src/pricing.rs

Tests:
- crates/zaion-cli/tests/beginner_golden_path.rs
- crates/zaion-cli/tests/cli_stable_surface.rs

### execution-sandbox-computer-use - Execution Environments, Computer Use, Sandbox

Copied behavior:
- local filesystem actions have checkpoint and restore commands
- sandbox, ACI, shadow execution, and syntax-gate modules are available in the runtime

Zaion improvement:
- checkpoint guard snapshots a directory before a labeled action and emits a receipt
- optional syntax-file gate refuses invalid code before a guarded write proceeds

Paradigm breakthrough:
- none

Source paths:
- crates/zaion-cli/src/commands/checkpoint.rs
- crates/zaion-checkpoint/src/lib.rs
- crates/zaion-aci/src/syntax_gate.rs
- crates/zaion-aci/src/dispatcher.rs
- crates/zaion-shadow/src/lib.rs

Tests:
- crates/zaion-cli/tests/phase8_surface.rs
- crates/zaion-checkpoint/tests/restore.rs

### opd-trajectory-learning - OPD, Trajectory, Learning Loop

Copied behavior:
- runtime trajectories can be exported as training-oriented artifacts
- trajectory proof is connected to batch, distillation, and evolution source modules

Zaion improvement:
- OPD export reads the signed ledger and records source event hashes, turn proofs, tool receipts, delegation receipts, and evolution counts
- trajectory verify recomputes the proof hash before accepting an export

Paradigm breakthrough:
- none

Source paths:
- crates/zaion-cli/src/commands/opd.rs
- crates/zaion-opd/src/trajectory.rs
- crates/zaion-opd/src/signed_trajectory.rs
- crates/zaion-opd/src/opd_pipeline.rs
- crates/zaion-evolve/src/record.rs

Tests:
- crates/zaion-cli/tests/phase8_surface.rs
- crates/zaion-opd/tests/integration_tests.rs

### frontends-control-plane - Frontend, TUI, Desktop, Control Plane

Copied behavior:
- CLI and dashboard entry points expose runtime status instead of requiring users to inspect raw logs
- doctor and status summarize runtime readiness and doctor accepts the reference-style safe fix flag
- logs can be listed and filtered by log type, line count, severity level, session, and relative time window
- shell completion scripts are available from the main command surface and complete profile names for -p/--profile
- TUI launch remains a first-class dashboard path from the main command surface
- frontend surfaces cover gateway, channels, model/provider status, and session/process state

Zaion improvement:
- dashboard status shows identity continuity, provider route evidence, channels, activity, process, ledger, memory, context, tools, delegation, OPD, and checkpoint guards in one plane
- dashboard trace maps every control-plane panel back to the exact Zaion proof command that verifies it
- the control plane stays Zaion-native and exposes no reference-project user-facing commands

Paradigm breakthrough:
- none

Source paths:
- crates/zaion-cli/src/commands/system.rs
- crates/zaion-cli/src/commands/hub.rs
- crates/zaion-cli/src/commands/process/tui/
- crates/zaion-tui/src/app.rs
- zaion-website/app/
- zaion-website/components/

Tests:
- crates/zaion-cli/tests/phase8_surface.rs
- crates/zaion-cli/tests/cli_stable_surface.rs

### release-tests-public-proof - Release, Tests, Public Proof

Copied behavior:
- source inventories, tests, docs, and release checks are first-class artifacts
- reference archives can be inventoried without unpacking into product source
- version, update, uninstall, and migration lifecycle commands are explicit release surfaces
- update accepts reference-style gateway/check-only flow and uninstall accepts full, keep-data, dry-run, and yes safety flags
- OpenClaw migration accepts --workspace-target and --yes and can copy workspace instructions into an explicit workspace

Zaion improvement:
- source map, crosswalk, dossier, matrix, and implementation proof are separate verifiable gates
- full completion verification is stricter than foundation-batch verification

Paradigm breakthrough:
- none

Source paths:
- crates/zaion-cli/src/commands/system.rs
- crates/zaion-cli/src/commands/import_openclaw.rs
- crates/zaion-cli/src/commands/phase8b.rs
- crates/zaion-cli/src/commands/compare.rs
- plans/phase8-b/full-module-crosswalk.md
- docs/PHASE8.md

Tests:
- crates/zaion-cli/tests/phase8_surface.rs
- crates/zaion-cli/tests/cli_stable_surface.rs

