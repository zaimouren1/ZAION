# 300 任务评测环境与 Harness 设计

> 日期: 2026-08-14 | 对应 M0"建立300任务基准"的可执行化 + 计划"同模型、同预算、同环境"评测

## 目标

让 300 个任务可执行、可评分、可复现：同一个模型、同一个预算、同一个环境，产出风险调整分数。

## 环境层

| 环境 | 用途 | 说明 |
|---|---|---|
| sandbox_repo_v1 | 开发/代码任务 | 一组带缺陷的 Rust/Python 小仓库，含 failing tests、告警日志、配置 |
| sandbox_env_v1 | SRE/运维任务 | 本地进程 + 端口 + 日志，可注入故障（进程树取消/磁盘满/断网） |
| container_v1 | 环境隔离任务 | 容器内运行，绑定环境身份 |
| channel_sim | 渠道任务 | Telegram/webhook 模拟端点（现有 spawn_telegram_api_mock 可复用） |
| benchmark_sandbox | 安全任务 | 注入语料库、越权尝试脚本、signed 篡改夹具 |

## 故障注入工具（reliability_security 分类的使能器）

| 工具 | 能力 |
|---|---|
| kill_at_commit_point | 在每个 ledger commit 边界杀掉进程（配合 crash-recovery 任务） |
| disk_full_sim | 填充临时文件系统到限额，触发 DiskFull 路径 |
| event_reorder | 乱序/重放事件注入 |
| signature_tamper | 篡改签名事件/回执后重放 |
| idor_probe | 跨 principal 读写尝试 |
| inject_corpus | 注入语料库（web/文件/内存/MCP 描述/渠道） |

## Harness（评测驱动）

```text
manifest (300 tasks) → runner → 每任务:
  1. setup: 准备环境（sandbox repo / 故障注入 / 渠道模拟）
  2. prompt: 同模型、同预算（token 上限）、同超时
  3. execute: agent 在环境中执行
  4. collect: 结果 artifact（stdout、文件状态、证据包）
  5. verify: 对照 acceptance（自动校验器：测试通过/文件正确/证据可验证）
  6. score: 风险调整分数 = 成功40% + 无重做20% + 恢复15% + 信任15% + 成本延迟10%
  7. evidence: 不可变结果 artifact + 证据路径
```

## 评分细则（对应 score_policy）

| 维度 | 测量 |
|---|---|
| task_success (40) | acceptance 校验器全过 |
| no_human_rework (20) | 一次通过，无需人工修正 |
| recovery (15) | 中断/故障后恢复能力（recovery/idempotency 任务） |
| trust_verification (15) | 证据包可被独立验证器接受 |
| cost_latency (10) | token 消耗 + 延迟 vs 预算 |

## 对照基线

- Hermes 本地镜像（已刷新 1f8fdc7bd8）
- OpenClaw 本地镜像（94cdb6c4）
- 同模型、同预算、同环境运行，风险分数对比

## 实施顺序

1. sandbox_repo_v1（首个：带缺陷的 Rust 小仓库 + failing tests + 告警日志）— 支撑 hero_mission 与 tools 分类
2. runner 骨架（manifest → 单任务执行 → 结果收集）
3. 故障注入工具（kill_at_commit_point 优先，支撑 reliability 任务）
4. 证据验证器（独立 verify/export，支撑 trust 维度）
