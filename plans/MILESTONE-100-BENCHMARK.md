# 100 轮里程碑：基准可执行套件（2026-08-14）

> 目标状态: active (roundsStarted: 100/256)

## 可执行套件（44/300 任务，14.7%）

- **44 个任务全部真实完成**（正确行为的 sample executor 解题）
- 覆盖 25+ 分类：hero×8 / env×4 / memory×2 / session×2 / skills×2 / context×2 / security×3 / gateway×2 / mcp×2 / batch×3 / reliability×2 / acp×2 / onboarding / release / channels / tools / tui×2 / evidence / approval / idempotency
- 多维计分生效：recovery 维度（REC-001/REL-002/ENV-003/HERO-010/BE-003）+ cost_latency 维度（CTX-001）
- **executor 复用链**：hero（HERO-001/003 共享）、sre（HERO-007/008）、rollback（HERO-004/010）

## 增长曲线

| 轮次 | 任务 | 完成 | 均值 |
|---|---|---|---|
| 1 | 5 | 2 | 2.3 |
| 83 | 6 | 3 | 2.83 |
| 84 | 7 | 3 | 3.21 |
| 85 | 9 | 5 | 3.72 |
| 86 | 11 | 7 | 4.05 |
| 87 | 13 | 9 | 4.27 |
| 88 | 15 | 11 | 4.43 |
| 89 | 17 | 13 | 4.56 |
| 90 | 19 | 15 | 4.66 |
| 91 | 21 | 17 | 4.70 |
| 92 | 21 | 19 | 5.19 |
| 93 | 21 | 21 | 5.57 |
| 94-99 | 24-40 | 全部 | ~5.6 |
| **100** | **44** | **44/44** | **~5.6** |

## 意义

1. 评测管线（setup→executor→verifier→score→report）对 44 个任务端到端验证
2. 44 个 sample executor = "正确行为"基线——真实 agent（API 接入后）逐任务对比
3. 验证器-执行器契约（result JSON 五维短名）跨 25+ 分类一致
4. executor 复用证明任务-环境映射的诚实性