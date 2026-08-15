# Phase 8-B Behavior Contract

Batch: `all`

Active stage: `hermes-copy`

Status: all modules have executable behavior contracts for 1/3 hermes behavior copy only

Hermes zip: `hermes-agent-2026.4.8.zip`

Strict order:
- 1. copy every Hermes module behavior into Zaion module-by-module
- 2. improve the copied Zaion module behavior
- 3. prove the module-level paradigm breakthrough
- 4. only then continue Zaion native 12 modules

| Module | Copied behaviors | Proof hash |
| --- | ---: | --- |
| Agent Runtime Loop | 7 | 4ce387ba8633195cea002647eaa4428be162ac3bd54a84c212a84fe2fdb35de5 |
| Identity And Continuity | 9 | 724f071c9407f1bff3b47259d0c006e947aa50bc4e26077af0a885565f8a4d76 |
| Channel Gateway And Bridge | 8 | 9a4ec714d6ae9d18f0d1dd97f9796b736fd7858d906d58b01d5805674fb9132d |
| Memory And Session Memory | 8 | 82f560c0e822b2f2ca68163b57322ac3b4e277385605e05756b33096e19d49d8 |
| Context Compression And Infinite Context | 4 | 761cdde234da96525e369ac6c9638a249ca51fca53483ec2d1dda0448ad1a6db |
| Tools, Permissions, And Safety | 8 | c9d793b8d3a38a1a29c6c420cd438f2017ff245794c1d7fe087cea628f0d036d |
| Skills And Plugins | 8 | 009b000d93f03184c49d139451eca20114522d53e6a87281f515f39777b3c2fe |
| Activity Continuity, Cron, Proactive, Dreaming | 4 | 952dc5c35b5db1ba72cc3e62f063e3d651de21455389b9f996295a370c6b1537 |
| Multi-Agent, Delegation, Teams | 5 | 58bd8852fefee399839dda852030be59059f39c26a0a99a711bc64daa8c971de |
| Provider, Credential, Cost, Budget | 11 | a64aec0223796a2e2c7dd26a5521de21785ae66ba5d615e8f8925ac214a02ce6 |
| Execution Environments, Computer Use, Sandbox | 5 | b545f50ff32f0308a5802a281c11776dff5b5cfc44aa232df2e20ed3059ea0fb |
| OPD, Trajectory, Learning Loop | 4 | 40af52a5c4ab8a62f3c023c7b6354761c9088d6fd82bb2b5f83d29cc6479f10d |
| Frontend, TUI, Desktop, Control Plane | 6 | e1c021f08bf0644c0101e1e8f0eae50694f3ff3e7923362a8d2f43df9c2c13c9 |
| Release, Tests, Public Proof | 6 | 85000d6a9defbae5d8ce6d90a3d28a325e98449e7e33c978843aff183368df39 |

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
- stage2 locked: Zaion improvement must not be claimed before copy contract passes

Paradigm breakthrough gate:
- stage3 locked: paradigm breakthrough must wait until copy and improvement gates pass

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
- stage2 locked: Zaion improvement must not be claimed before copy contract passes

Paradigm breakthrough gate:
- stage3 locked: paradigm breakthrough must wait until copy and improvement gates pass

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
- stage2 locked: Zaion improvement must not be claimed before copy contract passes

Paradigm breakthrough gate:
- stage3 locked: paradigm breakthrough must wait until copy and improvement gates pass

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
- stage2 locked: Zaion improvement must not be claimed before copy contract passes

Paradigm breakthrough gate:
- stage3 locked: paradigm breakthrough must wait until copy and improvement gates pass

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
- stage2 locked: Zaion improvement must not be claimed before copy contract passes

Paradigm breakthrough gate:
- stage3 locked: paradigm breakthrough must wait until copy and improvement gates pass

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
- stage2 locked: Zaion improvement must not be claimed before copy contract passes

Paradigm breakthrough gate:
- stage3 locked: paradigm breakthrough must wait until copy and improvement gates pass

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
- stage2 locked: Zaion improvement must not be claimed before copy contract passes

Paradigm breakthrough gate:
- stage3 locked: paradigm breakthrough must wait until copy and improvement gates pass

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
- stage2 locked: Zaion improvement must not be claimed before copy contract passes

Paradigm breakthrough gate:
- stage3 locked: paradigm breakthrough must wait until copy and improvement gates pass

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
- stage2 locked: Zaion improvement must not be claimed before copy contract passes

Paradigm breakthrough gate:
- stage3 locked: paradigm breakthrough must wait until copy and improvement gates pass

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
- stage2 locked: Zaion improvement must not be claimed before copy contract passes

Paradigm breakthrough gate:
- stage3 locked: paradigm breakthrough must wait until copy and improvement gates pass

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
- stage2 locked: Zaion improvement must not be claimed before copy contract passes

Paradigm breakthrough gate:
- stage3 locked: paradigm breakthrough must wait until copy and improvement gates pass

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
- stage2 locked: Zaion improvement must not be claimed before copy contract passes

Paradigm breakthrough gate:
- stage3 locked: paradigm breakthrough must wait until copy and improvement gates pass

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
- stage2 locked: Zaion improvement must not be claimed before copy contract passes

Paradigm breakthrough gate:
- stage3 locked: paradigm breakthrough must wait until copy and improvement gates pass

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
- stage2 locked: Zaion improvement must not be claimed before copy contract passes

Paradigm breakthrough gate:
- stage3 locked: paradigm breakthrough must wait until copy and improvement gates pass

