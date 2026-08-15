# Phase 8-B Behavior Contract

Batch: `all`

Active stage: `paradigm`

Status: all modules have executable behavior contracts for 3/3 hermes behavior copy -> zaion improvement -> paradigm breakthrough

Hermes zip: `hermes-agent-2026.4.8.zip`

Strict order:
- 1. copy every Hermes module behavior into Zaion module-by-module
- 2. improve the copied Zaion module behavior
- 3. prove the module-level paradigm breakthrough
- 4. only then continue Zaion native 12 modules

| Module | Copied behaviors | Proof hash |
| --- | ---: | --- |
| Agent Runtime Loop | 7 | e9cd221afd40212289c42923f42ca06e4c5c6f3f9cefcdf87cd3847114fdaa99 |
| Identity And Continuity | 9 | d41d9b8cb58f4dce53d172080434a35c12efbe156f4daa2632180e53ef0a5268 |
| Channel Gateway And Bridge | 8 | f0697495d27057ccdf03095c3bc19589831f1378fa2b23f8f7067876f8859e32 |
| Memory And Session Memory | 8 | 81fed4c554a9f55fbc076a178582e44cdd57dbf63567d01dcd9add875332febf |
| Context Compression And Infinite Context | 4 | 82f9961f805a92bb83f55c7a7cd84f41ad2c7a4330e0b9c97e0ea39bd90c8495 |
| Tools, Permissions, And Safety | 8 | 0d727809629f11c7f0805222ef4efac454f23a9d863d927ccdc90717e46fcf56 |
| Skills And Plugins | 8 | e6ab53c5ca1f315200893150808b13e22739cc2e137e24fe1a5aadfa1dc1723d |
| Activity Continuity, Cron, Proactive, Dreaming | 4 | bb9e333e1e46f62ecdf52a80de10328ced66322bcaacb0157c1085d08e52645c |
| Multi-Agent, Delegation, Teams | 5 | 83959ac64b7e5ce0e42dd46a251549f810e15570bfa31ff7592b886037380503 |
| Provider, Credential, Cost, Budget | 11 | 45ed888556a53ca90795b8afd83cfcd84418da0ec1695fd364d28e7b392bd707 |
| Execution Environments, Computer Use, Sandbox | 5 | 4d12f09a67cc80e71fc02747bbb1954b8d9fa75a2ba1a79159bfdf6d0dfbbcc3 |
| OPD, Trajectory, Learning Loop | 4 | cd6038e67de84a6e590fd800030d35abb597a3c329062008b04e91b41f134532 |
| Frontend, TUI, Desktop, Control Plane | 6 | f21e3b1b0b3c80a793cbb7cf90691bd13f77bb7ab8cb314fa6459d0499895710 |
| Release, Tests, Public Proof | 6 | 71fc1731bd639a9e07b988ab322a0df9f254f99a894bb1cbc064850fc2faf43f |

## Module Obligations

### agent-runtime-loop - Agent Runtime Loop

Hermes source evidence:
- agent/context_compressor.py
- agent/prompt_builder.py
- gateway/run.py
- hermes_cli/commands.py
- hermes_cli/main.py
- model_tools.py
- run_agent.py
- tools/registry.py

Copied behavior obligations:
- `cli-chat-entry`: CLI chat/default entry dispatches user prompts into the agent runtime loop -> `zaion chat` (verify: `zaion chat "Hello"`)
- `cli-chat-reference-flags`: CLI chat accepts query, model, provider, session, worktree, checkpoint, max-turn, source, skills, and quiet/verbose flags without rejecting migrated workflows -> `zaion chat` (verify: `zaion chat --query <message> -m <model> --provider <provider> --resume <session> --continue <name> --skills research --max-turns 5 --source telegram --quiet`)
- `top-level-session-flags`: top-level resume, continue, worktree, skills, yolo, and pass-session-id flags launch the interactive path without being rejected -> `zaion -c` (verify: `zaion -c --check --worktree --skills research --yolo --pass-session-id`)
- `slash-command-registry`: central slash command registry covers new, retry, undo, background, queue, model, provider, usage, and quit -> `zaion tui` (verify: `zaion tui --check`)
- `prompt-assembly`: prompt construction combines system prompt, context, history, tools, and compression boundaries -> `zaion context build` (verify: `zaion context build <pid> --budget 4000 --verify`)
- `tool-dispatch-loop`: model tool calls are parsed and dispatched through a tool registry inside the turn loop -> `zaion tool receipts` (verify: `zaion tool receipts <pid>`)
- `turn-history-retry`: retry, undo, resume, branch, compress, and rollback operate on the active session turn history -> `zaion turn latest` (verify: `zaion turn latest`)

Zaion improvement gate:
- turn execution emits identity, capability, context pack, answer span, and channel lineage evidence; bare zaion remains Zaion-native and does not expose reference-project commands

Paradigm breakthrough gate:
- a turn is no longer an opaque model response; it is a replayable proof object with parent lineage; terminal and channel turns can be verified through one TurnProof chain

### identity-continuity - Identity And Continuity

Hermes source evidence:
- acp_adapter/server.py
- acp_adapter/session.py
- docker/SOUL.md
- gateway/run.py
- gateway/session.py
- hermes_cli/config.py
- hermes_cli/default_soul.py
- hermes_cli/main.py
- hermes_cli/profiles.py

Copied behavior obligations:
- `profile-home-config`: profiles and home directories persist runtime identity and config across restarts -> `zaion profile` (verify: `zaion profile list`)
- `global-profile-selection`: top-level --profile/-p selects an isolated profile home before command dispatch -> `zaion --profile <name>` (verify: `zaion --profile work config show`)
- `profile-sticky-default`: profile use writes a sticky active profile, and later commands use that profile until default is selected again -> `zaion profile use` (verify: `zaion profile use work && zaion config show && zaion profile use default`)
- `profile-strict-resolution`: named profiles must already exist, and reserved command names cannot become profile aliases -> `zaion --profile <name>` (verify: `zaion --profile missing config show && zaion profile create chat`)
- `profile-management`: profiles can be listed with gateway/skill status, created, config-cloned, full-cloned with runtime strip, shown, renamed, aliased, exported with credential/runtime exclusions, imported with archive-name inference, selected, and deleted -> `zaion profile` (verify: `zaion profile show <name> && zaion profile rename <old> <new>`)
- `config-cli`: config supports show, edit, set, optional set key/value query forms, path, env-path, check, and migrate command flows -> `zaion config` (verify: `zaion config set && zaion config set provider && zaion config path && zaion config check`)
- `gateway-session-identity`: gateway sessions bind user/channel identity to persistent conversation state -> `zaion identity continuity` (verify: `zaion identity continuity`)
- `acp-session-principal`: ACP sessions preserve agent/client identity through remote tool and event exchange -> `zaion identity verify` (verify: `zaion identity verify`)
- `default-soul-bootstrap`: first startup includes a default soul/personality seed before normal conversation -> `zaion identity show` (verify: `zaion identity show`)

Zaion improvement gate:
- startup identity contract names the small-octopus role, environment, tools, and forbidden claims; rename and verify operations preserve continuity instead of replacing the identity

Paradigm breakthrough gate:
- model personality is subordinated to a signed identity contract and continuity ledger; provider, channel, and import/export changes are treated as continuity checks

### channel-gateway-bridge - Channel Gateway And Bridge

Hermes source evidence:
- gateway/channel_directory.py
- gateway/delivery.py
- gateway/pairing.py
- gateway/platforms/base.py
- gateway/platforms/telegram.py
- gateway/platforms/webhook.py
- gateway/run.py
- hermes_cli/commands.py
- hermes_cli/gateway.py
- hermes_cli/main.py
- hermes_cli/profiles.py
- scripts/whatsapp-bridge/bridge.js

Copied behavior obligations:
- `gateway-lifecycle`: gateway has run, start, stop, stop --all, restart, status, install, uninstall, and setup lifecycle commands -> `zaion gateway` (verify: `zaion gateway stop --all && zaion gateway status --deep`)
- `gateway-profile-scoped-service`: gateway service names and generated service definitions are scoped by active profile to avoid cross-profile collisions -> `zaion --profile <name> gateway status` (verify: `zaion --profile edge gateway status --deep`)
- `telegram-platform`: Telegram adapter receives messages, normalizes platform metadata, enforces allowed-user/home-channel setup, and sends replies -> `zaion tg` (verify: `zaion tg set-token <token> --allow <ids> --home-channel <id> && zaion tg status`)
- `whatsapp-setup`: WhatsApp setup chooses bot/self-chat mode, enables the bridge, records allowed users, and prepares session pairing -> `zaion whatsapp` (verify: `zaion whatsapp setup --mode self-chat --allow <phone>`)
- `platform-delivery`: gateway delivery abstracts outbound messages across platform adapters and webhooks -> `zaion webhook` (verify: `zaion webhook list`)
- `webhook-dynamic-subscriptions`: webhooks can be added, listed, removed, tested, and bound to dynamic prompt subscriptions -> `zaion webhook` (verify: `zaion webhook add research --prompt <template> --events paper.found`)
- `gateway-slash-commands`: messaging channels expose slash commands like help, model, status, approve, deny, and background -> `zaion omni trace` (verify: `zaion omni trace --channel telegram --sender owner --thread t --message-id m`)
- `pairing-and-home-channel`: gateway supports pending pairing codes, approval, revocation, clearing pending requests, approved users, and selecting a home channel -> `zaion pairing` (verify: `zaion pairing list && zaion pairing approve telegram <code> && zaion pairing revoke telegram <user-id>`)

Zaion improvement gate:
- there is one official Telegram entry point: zaion tg; channel input is normalized into a canonical envelope before runtime proof creation; Telegram access policy is stored beside the channel profile and denied runtime messages produce signed telegram.denied evidence; Telegram doctor can emit a machine-readable JSON readiness and access-policy report for gateways and dashboards

Paradigm breakthrough gate:
- channels are views over one identity/session/event graph instead of separate bot contexts; Telegram thread and message IDs are visible inside the same TurnProof lineage as terminal turns

### memory-session-memory - Memory And Session Memory

Hermes source evidence:
- agent/builtin_memory_provider.py
- agent/memory_manager.py
- agent/memory_provider.py
- hermes_cli/main.py
- hermes_cli/memory_setup.py
- hermes_state.py
- tests/hermes_cli/test_session_browse.py
- tests/hermes_cli/test_sessions_delete.py
- tests/tools/test_memory_tool.py
- tools/memory_tool.py
- website/docs/user-guide/sessions.md

Copied behavior obligations:
- `memory-manager`: memory manager stores and retrieves session memory for later prompt construction -> `zaion memory status` (verify: `zaion memory status`)
- `builtin-memory-provider`: built-in and external memory providers share a common provider interface -> `zaion memory setup` (verify: `zaion memory setup --provider <provider> --model <embedding-model>`)
- `memory-tool`: memory is also reachable as a tool path in the runtime -> `zaion memory add-fact` (verify: `zaion memory add-fact <pid> <fact> --user-provided`)
- `memory-setup-cli`: CLI exposes memory setup, status, and disable flows -> `zaion memory off` (verify: `zaion memory off && zaion memory status`)
- `session-control-cli`: CLI exposes sessions list, browse, export, delete, prune, stats, and rename -> `zaion sessions` (verify: `zaion sessions list --source telegram`)
- `session-source-filtering`: session listing and browsing hide tool sessions by default and honor explicit source filters -> `zaion sessions list` (verify: `zaion sessions list --source tool`)
- `session-delete-confirmation`: session deletion resolves id/key targets and refuses destructive deletion until --yes is supplied in non-interactive flows -> `zaion sessions delete` (verify: `zaion sessions delete <session-id> --yes`)
- `session-prune-scope`: session pruning uses confirmation by default and accepts --yes bypass with older-than days and optional source scope -> `zaion sessions prune` (verify: `zaion sessions prune --older-than 30 --source telegram --yes`)

Zaion improvement gate:
- memory facts carry source evidence, explicit user-provided markers, verification, and invalidation; sync export/import preserves proof artifacts for later trace commands; memory atoms carry proof hashes and trace/graph commands can emit JSON evidence for control planes

Paradigm breakthrough gate:
- memory is an atom graph with validity and evidence rather than a pile of summarized text; old answers can be rechecked against active or invalidated memory atoms

### context-infinite-context - Context Compression And Infinite Context

Hermes source evidence:
- agent/context_compressor.py
- agent/context_references.py
- agent/memory_manager.py
- agent/prompt_builder.py
- agent/trajectory.py
- trajectory_compressor.py

Copied behavior obligations:
- `context-compressor`: conversation history is compressed before prompt construction when budgets require it -> `zaion context build` (verify: `zaion context build <pid> --budget 4000 --verify`)
- `context-references`: context references preserve links back to source files and prior turns -> `zaion context trace` (verify: `zaion context trace <context-pack-id>`)
- `prompt-builder-context`: prompt builder integrates compressed history, memory, and tool descriptions -> `zaion answer trace` (verify: `zaion answer trace <event-id>`)
- `trajectory-compression`: trajectory compression turns long interaction history into compact training/runtime artifacts -> `zaion context replay` (verify: `zaion context replay <context-pack-id>`)

Zaion improvement gate:
- ContextPack records budget, source events, memory atoms, projection refs, and replay hash; 4k budgets are verified without losing source traceability; context verification can emit JSON proof for dashboards, gates, and small-window runtime controllers

Paradigm breakthrough gate:
- small-window models receive a bounded execution cache while full memory remains outside the prompt; context replay detects missing source events and stale projections

### tools-permissions-safety - Tools, Permissions, And Safety

Hermes source evidence:
- environments/tool_call_parsers/hermes_parser.py
- environments/tool_context.py
- hermes_cli/main.py
- hermes_cli/mcp_config.py
- hermes_cli/tools_config.py
- mcp_serve.py
- model_tools.py
- tests/tools/test_approval.py
- tools/approval.py
- tools/registry.py
- tools/tirith_security.py
- tools/url_safety.py

Copied behavior obligations:
- `tool-registry`: tool definitions are registered centrally and exposed to models through controlled schemas -> `zaion capability show` (verify: `zaion capability show`)
- `approval-gate`: dangerous actions pass through explicit approval gates -> `zaion tool verify` (verify: `zaion tool verify <pid>`)
- `security-policy`: security policy blocks sensitive, local, and exfiltration-prone operations -> `zaion security` (verify: `zaion security status`)
- `mcp-control-plane`: MCP servers can be added, removed, listed, tested, configured, and served with stdio inference from --command, stdio args, auth mode, overwrite confirmation bypass, and verbose serve startup -> `zaion mcp` (verify: `zaion mcp add node-server --command npx --args @modelcontextprotocol/server-filesystem . --auth oauth --force`)
- `mcp-stdio-inference`: MCP add infers stdio transport from --command without requiring a separate --transport stdio flag -> `zaion mcp add` (verify: `zaion mcp add node-server --command npx --args @modelcontextprotocol/server-filesystem .`)
- `tools-cli`: tools can be summarized, listed, enabled, disabled, default reference-off for moa/homeassistant/rl, and scoped by platform or MCP server tool target -> `zaion tools` (verify: `zaion tools --summary && zaion tools disable web --platform telegram`)
- `tools-reference-defaults`: reference toolset keys include image_gen, moa, skills, todo, session_search, clarify, delegation, cronjob, and rl, with moa/homeassistant/rl off by default -> `zaion tools list` (verify: `zaion tools list --platform cli`)
- `tool-call-parsers`: multiple model tool-call formats are parsed into a normalized tool context -> `crate:zaion-adapters tool parsers` (verify: `cargo test -p zaion-adapters`)

Zaion improvement gate:
- parser-visible tool calls are recorded as receipts when explicit dispatch is not granted; capability manifest and tool verification fail closed instead of silently executing

Paradigm breakthrough gate:
- tool use becomes an auditable capability receipt, not raw function dispatch; unsafe autonomy can be proven blocked by receipt state and capability scope

### skills-plugins - Skills And Plugins

Hermes source evidence:
- agent/skill_commands.py
- agent/skill_utils.py
- hermes_cli/commands.py
- hermes_cli/main.py
- hermes_cli/plugins_cmd.py
- hermes_cli/tools_config.py
- optional-skills/DESCRIPTION.md
- skills/software-development/plan/SKILL.md
- tools/skill_manager_tool.py
- tools/skills_hub.py
- tools/skills_sync.py
- tools/skills_tool.py

Copied behavior obligations:
- `skill-loading`: skills are discovered from installed skill directories and injected into runtime prompts -> `zaion skill list` (verify: `zaion skill list`)
- `skill-manager-tool`: skills can be searched, inspected, installed, updated, and managed through tool surfaces -> `zaion skill promote` (verify: `zaion skill promote <skill_dir> --capability <scope>`)
- `skill-hub`: skill registries and sync flows support browse, check, update, audit, and snapshot operations -> `zaion skill search` (verify: `zaion skill search capability_scope=<scope>`)
- `skills-and-plugins-cli`: skills expose publish, snapshot export/import, tap, and plugin install/update/remove/list/enable/disable command surfaces with restored snapshot state -> `zaion skills and plugins` (verify: `zaion skills snapshot export - && zaion plugins list`)
- `skills-snapshot-import-state`: skills snapshot import restores configured taps, hub-installed skills, and plugin registry state rather than only parsing the file -> `zaion skills snapshot import` (verify: `zaion skills snapshot import <snapshot.json> --force`)
- `plugin-git-installer`: plugins install from Git URLs or owner/repo shorthand, validate plugin names against path traversal, read plugin.yaml, copy .example files, show after-install.md, update git plugins, and remove plugin directories -> `zaion plugins install` (verify: `zaion plugins install owner/repo --dry-run && zaion plugins install <local-plugin> --force`)
- `builtin-skill-pack`: built-in and optional skill packs provide reusable domain behavior -> `zaion skill promote` (verify: `zaion skill promote <skill_dir> --capability <scope>`)
- `plugin-command-registry`: plugins can register top-level CLI command surfaces without hardcoded product command entries -> `zaion <plugin-name>` (verify: `zaion plugins install owner/repo --name example --force && zaion example --help`)

Zaion improvement gate:
- promotion refuses packages without docs, test proof, explicit capability scope, or safety scan pass; promotion prints the rollback command before writing the skill entry; plugin install records capability scope, permissions, required environment variables, source digest, safety digest, install path, and rollback command; plugin inspect exposes that metadata so installed top-level commands remain accountable capabilities rather than anonymous command aliases

Paradigm breakthrough gate:
- skills become accountable capability modules instead of prompt snippets; promotion is gated by source trace, tests, capability boundary, safety scan, and rollback path

### activity-continuity - Activity Continuity, Cron, Proactive, Dreaming

Hermes source evidence:
- cron/jobs.py
- cron/scheduler.py
- gateway/delivery.py
- gateway/run.py
- hermes_cli/cron.py
- hermes_cli/main.py
- tests/tools/test_cronjob_tools.py
- tools/cronjob_tools.py

Copied behavior obligations:
- `cron-scheduler`: scheduler ticks due jobs and stores scheduled prompt/job metadata -> `zaion activity status` (verify: `zaion activity status`)
- `cron-cli`: CLI exposes cron list, create, edit, pause, resume, run, remove, status, and tick with reference-style optional principal resolution -> `zaion cron` (verify: `zaion cron create 30m <prompt> --name research --deliver local --repeat 2 --skill papers`)
- `cron-tools`: scheduled jobs can be managed as tools under runtime policy -> `zaion thought list` (verify: `zaion thought list`)
- `gateway-cron-ticker`: gateway starts a ticker so scheduled activity can deliver through messaging channels -> `zaion activity sample` (verify: `zaion activity sample --seed 42`)

Zaion improvement gate:
- activity is disabled by default and enabling requires an explicit token/network cost acknowledgement; thought birth uses a bounded stochastic sampler over traceable user preferences; activity status emits JSON for control planes and thought seeds carry replayable proof hashes; activity sample supports dry-run preview so random thought birth can be audited before it is persisted

Paradigm breakthrough gate:
- activity continuity is not a fixed cron loop; it creates budgeted thought seeds from preference evidence; destructive, credential, purchase, and code-modifying autonomy is blocked at policy creation time

### multi-agent-delegation - Multi-Agent, Delegation, Teams

Hermes source evidence:
- acp_adapter/entry.py
- acp_adapter/events.py
- acp_adapter/permissions.py
- acp_adapter/server.py
- acp_adapter/session.py
- acp_adapter/tools.py
- agent/auxiliary_client.py
- agent/copilot_acp_client.py
- tests/tools/test_delegate.py
- tools/delegate_tool.py

Copied behavior obligations:
- `acp-server`: ACP server exposes help, readiness check, agent sessions, and event streams for external clients without accidentally blocking on help -> `zaion acp` (verify: `zaion acp --check`)
- `acp-session`: remote sessions preserve event history, tool calls, and session state -> `zaion agent proof` (verify: `zaion agent proof <pid> <delegate_principal> <task> --scope <scope>`)
- `acp-tools`: remote tools carry permission context and capability boundaries -> `zaion agent receipts` (verify: `zaion agent receipts <pid>`)
- `delegate-tool`: delegation exists as a tool-level operation with scoped toolsets -> `zaion honcho` (verify: `zaion honcho status`)
- `copilot-acp-client`: auxiliary/ACP clients allow work to be delegated outside the local process -> `zaion agent spawn` (verify: `zaion agent status <pid>`)

Zaion improvement gate:
- local delegation proof writes principal, delegate, scope, input hash, output hash, and merge receipt to the ledger; delegation receipts can be listed without contacting a remote worker

Paradigm breakthrough gate:
- subagents become accountable delegated principals with proof receipts instead of hidden workers; merge evidence is represented by a deterministic receipt hash tied to the delegated IO boundary

### providers-credentials-cost - Provider, Credential, Cost, Budget

Hermes source evidence:
- agent/credential_pool.py
- agent/insights.py
- agent/model_metadata.py
- agent/smart_model_routing.py
- agent/usage_pricing.py
- hermes_cli/auth.py
- hermes_cli/main.py
- hermes_cli/model_normalize.py
- hermes_cli/model_switch.py
- hermes_cli/models.py
- hermes_cli/providers.py

Copied behavior obligations:
- `model-switch`: model switching parses flags, validates models, resolves provider-specific IDs, and can save provider URL/key/model directly -> `zaion model` (verify: `zaion model --provider openai --base-url <url> --api-key <key> --model <model-id>`)
- `provider-model-catalog`: model catalogs can be fetched or curated per provider before selection -> `zaion provider models` (verify: `zaion provider models ollama --base-url http://localhost:11434/v1`)
- `provider-model-syntax`: provider aliases and provider:model syntax are normalized before model config is saved -> `zaion model --model <provider>:<model>` (verify: `zaion model --model openrouter:anthropic/claude-sonnet-4.5 --api-key <key>`)
- `provider-gateway-aliases`: provider aliases cover Google/Gemini, GLM/Z.AI, Moonshot/Kimi, MiniMax, Vercel AI Gateway, OpenCode, Kilo Code, DashScope, and Hugging Face -> `zaion model --provider <alias>` (verify: `zaion model --provider google-ai-studio --api-key <key> --model gemini-3.1-pro-preview`)
- `provider-gateway-default-urls`: gateway providers carry default inference base URLs and provider-specific base URL environment overrides -> `zaion provider list` (verify: `zaion provider list`)
- `provider-env-key-aliases`: provider key resolution checks provider-specific environment variables before declaring credentials missing -> `zaion provider doctor` (verify: `zaion provider doctor`)
- `kimi-code-endpoint`: Kimi keys prefixed sk-kimi- route to the Kimi coding endpoint when no explicit base URL overrides it -> `zaion model --provider moonshot` (verify: `zaion model --provider moonshot --api-key sk-kimi-... --model kimi-k2.5`)
- `provider-model-normalization`: model identifiers normalize per provider for aggregators, Anthropic/OpenCode hyphen rules, OpenCode Go bare names, and DeepSeek aliases -> `zaion model --provider <provider> --model <model>` (verify: `zaion model --provider vercel-ai-gateway --api-key <key> --model claude-sonnet-4.6`)
- `credential-pool`: credential pools support labels, provider keys, exhaustion reset, login/logout --provider, OAuth-shaped flags, and auth commands -> `zaion auth` (verify: `zaion login --provider openai-codex --client-id <id> --scope <scope> && zaion auth reset openai-codex && zaion logout --provider openai-codex`)
- `smart-routing`: routing chooses models using metadata, provider state, and task needs -> `zaion provider status` (verify: `zaion provider status`)
- `usage-pricing`: usage and pricing are summarized as cost analytics -> `zaion provider cost` (verify: `zaion provider cost --model llama3.2 --input 1000 --output 500`)

Zaion improvement gate:
- model discovery fetches provider model IDs when an endpoint supports it; provider status ties configured model, key state, pricing snapshot, and route decision together

Paradigm breakthrough gate:
- provider choice is an auditable route decision under pricing and budget evidence; model switching preserves identity because provider config is below the continuity contract

### execution-sandbox-computer-use - Execution Environments, Computer Use, Sandbox

Hermes source evidence:
- environments/agent_loop.py
- environments/hermes_base_env.py
- tests/tools/test_checkpoint_manager.py
- tests/tools/test_terminal_tool_requirements.py
- tools/browser_camofox.py
- tools/browser_tool.py
- tools/checkpoint_manager.py
- tools/file_operations.py
- tools/file_tools.py
- tools/terminal_tool.py

Copied behavior obligations:
- `base-environment`: evaluation/runtime environments define reset, step, tool execution, and observation contracts -> `zaion checkpoint guard` (verify: `zaion checkpoint guard <dir> <label> --scope <scope>`)
- `terminal-tool`: terminal execution is wrapped with timeouts, output handling, and safety checks -> `zaion shadow spawn` (verify: `zaion shadow list`)
- `file-tools`: file reads and writes are handled through guarded file tools -> `zaion checkpoint snap` (verify: `zaion checkpoint list <dir>`)
- `checkpoint-manager`: filesystem checkpoints can be created, listed, and restored around risky edits -> `zaion checkpoint restore` (verify: `zaion checkpoint restore <dir> <checkpoint-id>`)
- `browser-computer-use`: browser/computer-use tools expose stateful web interaction behind policy gates -> `zaion capability show` (verify: `zaion capability show`)

Zaion improvement gate:
- checkpoint guard snapshots a directory before a labeled action and emits a receipt; optional syntax-file gate refuses invalid code before a guarded write proceeds

Paradigm breakthrough gate:
- local action safety is a receipt-bearing envelope of checkpoint, syntax gate, scope, and rollback command; write-before recovery becomes a verifiable action boundary rather than an informal operator habit

### opd-trajectory-learning - OPD, Trajectory, Learning Loop

Hermes source evidence:
- batch_runner.py
- environments/agentic_opd_env.py
- rl_cli.py
- tests/test_trajectory_compressor.py
- tools/rl_training_tool.py
- trajectory_compressor.py

Copied behavior obligations:
- `agentic-opd-env`: agentic OPD environment converts agent steps into trainable observation/action/reward traces -> `zaion opd export` (verify: `zaion opd export <pid> --out <trajectory.json>`)
- `batch-runner`: batch runner executes configured tasks and records trajectory outcomes -> `zaion opd verify` (verify: `zaion opd verify <trajectory.json>`)
- `rl-cli`: RL training command surfaces connect trajectories to training/evaluation workflows -> `zaion evolve` (verify: `zaion evolve status`)
- `trajectory-compressor-tests`: trajectory compression is regression-tested for long traces -> `zaion opd verify` (verify: `zaion opd verify <trajectory.json>`)

Zaion improvement gate:
- OPD export reads the signed ledger and records source event hashes, turn proofs, tool receipts, delegation receipts, and evolution counts; trajectory verify recomputes the proof hash before accepting an export

Paradigm breakthrough gate:
- learning data is no longer detached logs; it is a replayable proof over source runtime events; distillation candidates inherit identity and receipt provenance before training use

### frontends-control-plane - Frontend, TUI, Desktop, Control Plane

Hermes source evidence:
- gateway/status.py
- hermes_cli/banner.py
- hermes_cli/callbacks.py
- hermes_cli/curses_ui.py
- hermes_cli/doctor.py
- hermes_cli/logs.py
- hermes_cli/main.py
- website/docs/user-guide/skills/godmode.md
- website/src/pages/skills/index.tsx

Copied behavior obligations:
- `cli-main-and-banner`: CLI provides a branded interactive entry, banner, command parser, and help surface -> `zaion help --all` (verify: `zaion help --all`)
- `terminal-curses-ui`: terminal UI/callbacks provide an interactive control surface beyond raw log output -> `zaion tui` (verify: `zaion tui --check`)
- `doctor-control-plane`: doctor/status commands summarize config, provider, gateway, and local runtime health, with a safe fix flag for missing local state -> `zaion doctor` (verify: `zaion doctor --fix`)
- `logs-viewer`: logs command lists and filters agent, error, and gateway log files by line count, severity level, session, and relative time window -> `zaion logs` (verify: `zaion logs agent -n 50 --level WARNING --session <id> --since 30m`)
- `completion-script`: completion command prints shell completion scripts and completes profile names after -p/--profile -> `zaion completion` (verify: `zaion completion bash`)
- `skills-website`: web/docs surfaces expose skills and operational concepts to users -> `zaion dashboard status` (verify: `zaion dashboard status <pid>`)

Zaion improvement gate:
- dashboard status shows identity continuity, provider route evidence, channels, activity, process, ledger, memory, context, tools, delegation, OPD, and checkpoint guards in one plane; dashboard trace maps every control-plane panel back to the exact Zaion proof command that verifies it; the control plane stays Zaion-native and exposes no reference-project user-facing commands

Paradigm breakthrough gate:
- the interface is a proof-aware control plane over identity, context, memory, permission, activity, delegation, OPD, and checkpoint evidence; users can audit the agent's state graph from the UI surface instead of trusting a chat transcript or scrolling logs

### release-tests-public-proof - Release, Tests, Public Proof

Hermes source evidence:
- .github/workflows/supply-chain-audit.yml
- .github/workflows/tests.yml
- README.md
- RELEASE_v0.8.0.md
- docker/entrypoint.sh
- hermes_cli/claw.py
- hermes_cli/main.py
- hermes_cli/setup.py
- hermes_cli/uninstall.py
- pyproject.toml
- scripts/install.sh
- tests/test_mcp_serve.py

Copied behavior obligations:
- `ci-tests`: test workflows and regression tests are tracked as release gates -> `zaion phase8b proof` (verify: `cargo test -p zaion-cli --test phase8_surface -- --test-threads=1`)
- `supply-chain-audit`: supply chain checks and package metadata are explicit release artifacts -> `zaion doctor` (verify: `zaion doctor`)
- `documentation-release-notes`: README and release notes document behavior, install, and operational surfaces -> `zaion phase8b status` (verify: `zaion phase8b status`)
- `version-update-uninstall`: version, update, uninstall, and completion-style release commands are exposed as explicit lifecycle surfaces with gateway/check/dry-run safety flags -> `zaion version` (verify: `zaion -V && zaion update --check --gateway && zaion uninstall --keep-data --dry-run`)
- `claw-workspace-migration`: OpenClaw migration accepts --workspace-target and --yes, copies workspace instruction files, and treats full preset secrets as enabled by default -> `zaion claw migrate` (verify: `zaion claw migrate --source <openclaw> --workspace-target <workspace> --yes`)
- `install-packaging`: packaging and installers make the runtime reproducible outside a dev checkout -> `zaion compare inventory` (verify: `zaion compare inventory <reference> --zip <path>`)

Zaion improvement gate:
- source map, crosswalk, dossier, matrix, and implementation proof are separate verifiable gates; full completion verification is stricter than foundation-batch verification

Paradigm breakthrough gate:
- Zaion refuses full Phase 8-B completion claims unless every module has source evidence and implemented proof; proof commands are checked for reference-project command name leakage

