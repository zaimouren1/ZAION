# TUI 审批提示接入分析（2026-08-14）

> 决策点 2：M3 审批 UI 面

## 现状

| 系统 | 状态 |
|---|---|
| Gateway 审批（app.rs） | ✅ 已有（GatewayApproval + /approve 命令 + respond_gateway_approval） |
| Turn 审批（M2b） | ✅ 运行时 + CLI + Gateway 路由（124-125 轮） |
| Turn 审批 TUI 提示 | ❌ 待接入（本分析） |

## 接入设计

1. TUI 显示 waiting_approval turn：从 turn store 读 WaitingApproval 状态（undelivered/incomplete_turns 或新查询）→ 状态栏/列表显示
2. /approve-turn 命令：TUI 命令 → 调用 approve（复用 turn approve 逻辑）
3. 事件流：turn 进入 WaitingApproval → 操作流事件（operation_stream）→ TUI 渲染审批提示

## 优先级判断

- M3 审批流核心（运行时/CLI/路由）✅ 已就绪——用户可通过 CLI/Web 审批
- TUI 审批提示是 UX 增强（低优先级）——CLI/Web 已覆盖功能
- 建议：TUI 提示接入排在 M3 启动后（设计伙伴反馈驱动）

---

## 实施评估更新（第 181 轮）——TUI turn 审批呈现空缺

**现状**：TUI 已集成 gateway 审批（/approve + pending_gateway_approval）——但 turn 审批（WaitingApproval——SessionActor 流）未呈现（TUI 无 TurnStore 访问，0 引用）。

**轻量方案**（TUI 状态栏提示）：
1. TUI 启动/定时读取 TurnStore（process_dir 的 turns.db——list WaitingApproval）
2. 状态栏显示："N turns awaiting approval——run: zaion turn approve <id>"
3. 不内嵌审批 UI（CLI/Web 已覆盖——TUI 仅提示）

**成本**：TurnStore 读取集成（TUI 无 store 访问）+ 状态栏渲染。低优先级（CLI/Web 覆盖）——留待用户要求时实施。
