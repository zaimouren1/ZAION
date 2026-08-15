# Zaion 300-task eval harness (runner skeleton)

> 用途: 把基准 manifest 变成可执行评测管线（M0"建立300任务基准"的使能器）
> 自测: `powershell -File selftest.ps1`（list → run --dry-run → score → report）

## 管线

```text
manifest (zaion_300_v1.json) → setup (准备沙箱环境) → execute (可插拔 executor)
→ collect (result.json) → score (风险调整分) → report (聚合)
```

## 用法

```powershell
python runner.py --list                    # 列出全部任务
python runner.py --run ZAION-300-HERO-001 --dry-run
                                           # dry-run: 只准备环境 + 空结果
python runner.py --run TASK --executor CMD # 真实执行: CMD 接收 env 目录 + task.json，stdout 输出 result JSON
python runner.py --score <result.json>     # 风险调整分（权重 40/20/15/15/10）
python runner.py --report <result_dir>     # 递归聚合分数
```

## Executor 契约

Executor 命令接收两个参数: `<env_dir> <task_json_path>`，stdout 输出一行 result JSON:
```json
{"task_id": "...", "success": 0-10, "rework": 0-10, "recovery": 0-10,
 "trust": 0-10, "cost_latency": 0-10, "evidence_path": "...", "notes": "..."}
```
各维度 0-10（越高越好；cost_latency 10 = 便宜快）。风险调整分 = 加权平均（0-10）。

## 环境准备

- setup 自动从 `environments/sandbox_repo_v1` 复制环境，并**删除 TASKS.md**（防止 agent 看到答案）。
- 故障注入: 由 executor 在任务执行前调用 `environments/fault_inject/fault_inject.py`（kill-after/disk-full/reorder/repeat/tamper）。

## 下一步

- 真实 executor: 接入被测 agent（同模型/同预算/同超时），调用验证器对照 acceptance。
- 验证器: 每个任务的可执行校验脚本（cargo test 通过 / 文件正确 / 证据可验证）。
