# 证据门清单（M2 迁移依赖图）

> 日期: 2026-08-14 | 用途: 10/10 跃迁计划 M2"单一运行内核"迁移时的契约影响图
> 数据来源: 本会话实测（cli_stable_surface.rs 68 文件 + phase8b.rs 95 文件）

## 什么是证据门

Zaion 代码库有**繁重的 source-gate 测试文化**：测试直接读取源码文件，断言其中包含特定的架构证据字符串（或 proof 路径存在）。任何重构如果删改了这些字符串/文件，对应测试立即失败（本会话已 3 次踩中：federation/session.rs 的 proof 路径、webhook needle、cmd_onboard 边界标记）。

## 两道主门

| 门 | 位置 | 规模 | 断言方式 |
|---|---|---|---|
| architecture_audit_source_gate | crates/zaion-cli/tests/cli_stable_surface.rs | 40+ 个测试函数，139 测试总数 | 读取 68 个文件，断言含特定字符串 |
| phase8b ModuleProofSpec | crates/zaion-cli/src/commands/phase8b.rs | 95 个 source_paths | 断言所列文件存在（proof verify） |
| system.rs 架构审计 | crates/zaion-cli/src/commands/system.rs | 45+ 字符串断言 | 读取 system.rs 等，断言字符串 |

## 锁定文件清单（合并去重）

### 关键架构文件（M2 迁移重灾区）
```
crates/zaion-runtime/src/{lib, turn_kernel, turn_outcome, turn_proof, turn_store,
  integrated_agent_loop, unified_agent_runtime, agent_loop, wake_request, wake_stream,
  evidence_graph, architecture_graph, execute_code, execute_code_js, execute_code_uds,
  batch_runner, compression_split, compressor, context, cron, genesis/skill_forge, moa,
  omni_session, policy, sandbox, sandbox_tools, shadow_agent, slash_commands}
crates/zaion-cli/src/commands/{system, wake, process_unified, network/routes,
  network/console, network/telegram, webhook/mod, webhook/webhook_serve, mcp, provider,
  capability, opd, phase8b, receipt_join, slash_integration, security, tool,
  macro_maturity, process/mod}
crates/zaion-adapters/src/{telegram_adapter, webhook_runtime, email, sms}
crates/zaion-ledger/src/session_store, crates/zaion-memory/src/{lib, skill, projection},
crates/zaion-evolve/src/{promotion, record}, crates/zaion-opd/src/*,
crates/zaion-federation/src/session, crates/zaion-tui/src/tui_app,
crates/zaion-core/src/{controller, process}, crates/zaion-safety/src/redact,
crates/zaion-sync/src/{export, import}, crates/zaion-a2a/src/{federation, protocol, stdio_service},
crates/zaion-aci/src/{dispatcher, syntax_gate}, crates/zaion-mcp/src/builtin_tools/mod
```

### 文档/夹具（只读，迁移影响小）
```
MASTER_PLAN.md, plans/{ZAION_ARCHITECTURE_SOURCE_AUDIT, hermes_surpass_master_plan,
  openclaw_latest_gap_report, phase8-b/*}, docs/superpowers/plans/2026-05-05-*, 各类测试夹具
```

## M2 迁移影响规则（经验教训）

1. **删除文件**：必须先查 phase8b source_paths 和 cli_stable_surface 的 root.join 列表——本会话 R10 删除 federation/session.rs 即被 phase8b proof verify 拦截
2. **重命名/移动**：等价于删除+新增，同样触发上述门
3. **删除字符串**：任何 needle 字符串被删/改，对应 architecture_audit 测试失败（本会话 R3 修 webhook needle、R4 修 cmd_onboard 边界标记）
4. **合并函数**：若函数名出现在 needle 中（如 operation_websocket_upgrade_response），需同步更新门测试
5. **缓解策略**：M2 迁移应建立"门测试先行修改"流程——先更新 evidence 断言，再改生产代码，最后跑全量测试

## 建议

- 将本清单纳入 10/10 计划 M0 交付物，作为 M2 的迁移依赖图
- 可考虑将证据门从"字符串断言"升级为"契约文件"（单一 source of truth），减少重构摩擦——这是 M5"公开版本化事件与证明规范"的前置
