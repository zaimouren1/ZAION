# Zaion 300 任务基准分类设计提案

> 日期: 2026-08-14 | 状态: 提案（待确认后写入 manifest）| 对应 10/10 跃迁计划 M0"建立300任务基准"

## 1. 现状

`eval/benchmarks/zaion_300_v1.json` 已有 15 个分类、恰好 300 槽、0 个具体任务：

| 分类 | 槽 | 权重 | 分类 | 槽 | 权重 |
|---|---|---|---|---|---|
| onboarding | 21 | 7 | mcp | 18 | 6 |
| tui | 24 | 8 | acp | 15 | 5 |
| session | 24 | 8 | environments | 18 | 6 |
| tools | 30 | 10 | batch_eval | 15 | 5 |
| skills | 15 | 5 | release | 15 | 5 |
| memory | 24 | 8 | community | 9 | 3 |
| context | 24 | 8 | | | |
| gateway | 24 | 8 | | | |
| channels | 24 | 8 | **合计** | **300** | |

评分策略（已就绪）：风险调整 = 任务成功 40% + 无需重做 20% + 恢复能力 15% + 可信证明 15% + 成本延迟 10%。

## 2. 结构性缺口（本提案要补的）

现有 15 个分类都是**功能表面**（feature-surface）。但计划的核心承诺是：

1. **英雄任务工作流**（dev/SRE 纵向闭环：告警→调查→修改→审批→执行→验证→回滚→签名证据）——没有一个分类直接度量这个闭环
2. **必测场景**（重复请求、event commit 点崩溃、乱序、断网重连、provider 超时/429/畸形、审批超时/拒绝、进程树取消、磁盘满、签名篡改、跨租户 IDOR、sandbox 逃逸、升级中断/回滚）——没有恢复/安全维度

## 3. 提案：任务类型维度（cross-cutting）

给每个槽增加 `task_type`，在**每个**表面分类内分布：

| task_type | 占比 | 说明 | 对应评分项 |
|---|---|---|---|
| happy_path | 40% | 正常完成 | task_success |
| approval | 15% | 需审批/拒绝/豁免，含审批超时 | no_rework + trust |
| recovery | 15% | 中断/取消/崩溃/断网后恢复 | recovery |
| idempotency | 10% | 重复请求/重试/幂等 | no_rework |
| security | 10% | 注入/越权/敏感处理/sandbox | trust |
| evidence | 10% | 签名证据/证明验证/审计 | trust |

## 4. 提案：新增 2 个分类（重新分配 60 槽）

| 新分类 | id | 槽 | 权重 | 内容 |
|---|---|---|---|---|
| 英雄任务 | hero_mission | 30 | 10 | dev/SRE 端到端闭环任务（见 §5 任务原型） |
| 可靠性与安全 | reliability_security | 30 | 10 | 必测场景专项（chaos/故障注入/安全） |

**重新分配方案**（从表面分类按权重比例削减 60 槽，保持 300 总槽）：

| 分类 | 原槽 | 新槽 | 分类 | 原槽 | 新槽 |
|---|---|---|---|---|---|
| onboarding | 21 | 15 | mcp | 18 | 15 |
| tui | 24 | 18 | acp | 15 | 15 |
| session | 24 | 18 | environments | 18 | 15 |
| tools | 30 | 24 | batch_eval | 15 | 12 |
| skills | 15 | 12 | release | 15 | 15 |
| memory | 24 | 18 | community | 9 | 9 |
| context | 24 | 18 | **hero_mission** | — | **30** |
| gateway | 24 | 18 | **reliability_security** | — | **30** |
| channels | 24 | 18 | | | |

（17 分类合计 = 15+18+18+24+12+18+18+18+18+15+15+15+12+15+9+30+30 = 300 ✓）

## 5. 英雄任务分类的任务原型（30 槽）

| 原型 | 槽 | 任务示例 |
|---|---|---|
| 告警→根因 | 5 | 给定告警/日志，定位根因并给出修复方案 |
| 修复→验证 | 6 | 改代码/配置，跑测试，验证通过 |
| 审批流 | 5 | 高风险操作需审批：批准→执行；拒绝→中止 |
| 执行→证据 | 5 | 执行真实动作（部署/脚本），产出签名证据包 |
| 回滚 | 5 | 变更后故障→回滚到已知良好状态 |
| 跨会话延续 | 4 | 中断后恢复/交接，保留上下文与证据 |

## 6. 任务模板（每槽一个）

```json
{
  "id": "ZAION-300-HERO-001",
  "category": "hero_mission",
  "task_type": "approval",
  "title": "生产配置变更需审批后执行",
  "description": "...",
  "environment": "sandbox_repo_v1",
  "setup": "...",
  "expected_action": "...",
  "verification": "...",
  "risk_profile": {"filesystem": "write", "network": "denied", "execution": "approval_required"},
  "parity_baseline": "hermes"
}
```

## 7. 实施顺序

1. 本提案确认 → 更新 manifest（categories 重分配 + task_type 字段 + 新增 hero_mission/reliability_security 分类定义）
2. 建立任务环境（sandbox 仓库 + 故障注入工具：进程树取消/磁盘满/乱序事件模拟）
3. 按槽填充任务（每分类先 happy_path 种子，再 recovery/approval/security）
4. 接入评测 harness（同模型、同预算、同环境）