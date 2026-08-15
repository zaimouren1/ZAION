# turn_contract_v2 全量迁移评估（2026-08-14）

> 目标: M2 turn 契约（持久化 begin/outbox/approval）覆盖所有生产入口

## 当前状态（勘察）

| 入口 | turn_contract_v2 | 位置 |
|---|---|---|
| cmd_wake（CLI 主入口） | ❌ 默认 false | wake.rs L169 |
| TUI（app.rs） | ❌ 默认 false | tui/app.rs L923 |
| /stop 等命令 | ✅ true | wake.rs L4648/4670 |
| 测试 | ✅ true | wake.rs 测试 |
| env 覆盖 | ZAION_TURN_CONTRACT_V2 | wake_contract_v2.rs L20 |

## 迁移步骤

1. cmd_wake（L169）：`WakeRequest::new(...)` 后默认 `.with_turn_contract_v2(true)`（或 env 默认 true）
2. TUI（app.rs L923）：同样默认开启
3. 全量验证：runtime 472 + cli 504 + 证据门 139+16 + daemon 19
4. 观察：持久化 turn 在主入口生效（begin → outbox → 审批/取消可用）

## 风险与缓释

| 风险 | 缓释 |
|---|---|
| 行为变化（持久化 turn） | 幂等 begin（Created/Existing）+ 证据门全绿 |
| 性能（SQLite 每次 wake） | turn store WAL 模式 + 已有基准 |
| 回滚 | env 可关闭（feature flag 保持） |

## 结论

迁移是低风险 flag 翻转（基础设施已就绪 + 测试覆盖）——建议逐步：cmd_wake → TUI → 观察 → 全量。

---

## 修正（第 131 轮）——迁移真实工作 = 渠道适配

**实测发现**：默认开启 v2 后 cli_stable_surface 4 个测试失败——**v2 仅支持 local CLI ingress**（source 白名单：cli/internal-queue/internal-background；wake_contract_v2.rs L176）。mcp-http/telegram 渠道的 wake 走 v2 会报错。

**修正迁移步骤**（真实工作）：
1. **扩展 source 白名单**：mcp-http / telegram / webhook 等渠道接入 v2（渠道 ingress 身份适配 + local_message_identity 兼容）
2. 渠道适配后：默认开启（cmd_wake/TUI）
3. 全量验证（cli_stable_surface 139 含渠道模拟）

**教训**：迁移评估低估了渠道依赖——flag 翻转前需渠道支持矩阵确认。

---

## 迁移完成（第 138 轮）——v2 默认开启全验证

**验证矩阵**（turn_contract_v2 默认开启）：
- cli 单元 **506/506**（全量回归）
- cli_stable_surface **139/139** + wake 52/52 + telegram 39/39 + wake_contract_v2 9/9
- clippy 0 · WS_ALL 通过

**成果**：M2 turn 契约（持久化 begin/outbox/approval）覆盖所有主入口（cli/telegram/http/mcp-http/acp-stdio/api/federation/slack/tui）——"accepted turn 零丢失"在产品主路径落实。

**P0 迁移项 ✅ 完成**。

