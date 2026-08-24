# ZAION EVAL（顶层三层评测 + Evidence Score）

把评测体系整合为三层，统一到一个 Evidence Score。

```
                 ZAION EVAL
                     |
        ┌────────────┼────────────┐
        │            │            │
     Module       Cross-System   Agent
     Eval            Eval        Eval
        │            │            │
      36 crate      语义链 5层    300 tasks
      contracts     故障定位      Real LLM
        │            │            │
        └────────────┼────────────┘
                     |
              Evidence Score
```

## 三层

| 层 | 脚本 | 证据 | 当前状态 |
|---|---|---|---|
| Module Eval | module_eval_runner.py | 36 crate 的 cargo test + evidence_level | 36/36，level 1 清零 |
| Cross-System Eval | cross_system_eval.py | 语义链 5 层（fact/principal/skill/ledger/sync）分层诊断 | 5/5 |
| Agent Eval | runner.py + hero_eval.py | 300 任务 + 真实 LLM 4 场景 | 3/4 稳定 |

## Evidence Score

Evidence Score 综合三层证据深度（0-100）：

```
Module Score      = Σ(36 crate evidence_level) / (36 × 5) × 100
Cross-System Score = (pass 层数 / 5) × 100
Agent Score        = (pass 场景数 / 4) × 100（真实 LLM 层）

Evidence Score     = 0.5 × Module + 0.3 × Cross-System + 0.2 × Agent
```

由 eval/harness/evidence_score.py 自动计算并生成报告。

## 顶层安全指标（附加）

Security Escape Rate（security_metrics.py）：4 维度 60 对抗测试，0% 穿透。