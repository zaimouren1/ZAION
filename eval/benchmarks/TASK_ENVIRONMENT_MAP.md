# Zaion 300 任务 → 环境映射清单

> 日期: 2026-08-14 | 用途: 评测可复现性（每个任务的可执行环境、验证器、运行方式）
> 状态: **300/300 任务已填充**（manifest active）；可执行 = 环境+验证器已 E2E 验证；待接线 = 有任务但验证器未专门化

## 可执行环境矩阵（E2E 已验证）

| 环境 | 路径 | 验证器 | 适用任务 |
|---|---|---|---|
| sandbox_repo_v1 | eval/environments/sandbox_repo_v1 | cargo test 全过 | hero_mission（代码修复）、tools |
| sre_env_v1 | eval/environments/sre_env_v1 | 配置端口+阈值行为 | HERO-007/008、ENV-003 |
| channel_sim | eval/environments/channel_sim | sim_state 有回复 | CH-001+（channels 分类） |
| crash_recovery_env_v1 | eval/environments/crash_recovery_env_v1 | journal 应用+提交 | REC-001（reliability recovery） |
| security_env_v1 | eval/environments/security_env_v1 | 报告正确标记 tampered | SEC-006 |

## 运行方式

```powershell
# 单任务完整回路（示例 executor）
python eval/harness/runner.py --run ZAION-300-HERO-001 --executor "python eval/harness/sample_executor.py" --env $env:TEMP\e1
python eval/harness/runner.py --score $env:TEMP\e1\result.json

# 渠道任务
python eval/harness/runner.py --run ZAION-300-CH-001 --executor "python eval/harness/sample_channel_executor.py" --env $env:TEMP\e2

# 直接验证（不经过 executor）
python eval/harness/verifier.py --check TASK_ID --env ENV_DIR

# 全部自测
powershell -File eval/harness/test_verifier.py        # dev 修复回路
powershell -File eval/harness/test_sre_verifier.py    # SRE 回路
powershell -File eval/harness/test_channel_e2e.ps1    # 渠道回路
powershell -File eval/harness/test_recovery_verifier.py # 恢复回路
powershell -File eval/harness/test_security_verifier.py # 安全回路
```

## 按分类的可执行性

| 分类 | 槽 | 任务数 | 可执行性 |
|---|---|---|---|
| hero_mission | 30 | 15 | ✅ 代码类（sandbox）+ SRE 类（sre_env）；任务级验证器 |
| reliability_security | 30 | 11 | ✅ REC（恢复）+ SEC-006（安全）；其余待接线 |
| tools | 24 | 13 | ✅ sandbox 通用校验 |
| channels | 24 | 8 | ✅ channel_sim |
| gateway | 24 | 13 | ⚪ 待专用验证器 |
| session | 24 | 12 | ⚪ 待专用验证器 |
| memory | 24 | 10 | ⚪ 待专用验证器 |
| mcp | 18 | 8 | ⚪ 待专用验证器 |
| skills | 15 | 7 | ⚪ 待专用验证器 |
| tui | 18 | 6 | ⚪ 待专用验证器 |
| acp | 15 | 5 | ⚪ 待专用验证器 |
| context | 18 | 5 | ⚪ 待专用验证器 |
| onboarding/batch_eval/release/community/environments | 各种 | 各 4-5 | ⚪ 需集成/安装测试环境 |

## 待办（可复现性缺口）

1. 验证器按 category 泛化（session/memory/gateway 等需专用校验逻辑）
2. 产品运行时接线（真实 executor 调用本地 zaion 二进制——需 API/CLI 契约）
3. 安装/升级/回滚类任务的环境（需干净机器或容器）