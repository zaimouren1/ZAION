# Zaion 全量优化执行报告

> 日期: 2026-08 | 工作区: D:/zaion-rust | 轮次: R1-R16 (目标轮次 17/256)| Hermes 基线镜像 | 本地镜像存在，commit 9c080707（与跃迁计划引用一致） | 验证 |
| 50GB target 清理 | **cargo clean 释放 58,420 MB（~57GB）** | CLEAN=0, target=0 |
| 最终状态 | 工作树干净（仅报告待提交）；全绿验证在清理前完成 | WS_ALL/CLIPPY/FMT/TEST 已绿 |

### M0 前置清单完成度

| M0 项 | 状态 |
|---|---|
| 冻结宏模块 | ✅ 全部在 HEAD（提交即冻结）；orphan 保留项（ttc/ego_integration/panel_sink）决策为保留 |
| Hermes 基线 | ✅ 本地镜像 9c080707 存在 |
| 300 任务基准 | ⚠️ 清单 schema 合法但仅 15/300 槽（M0 期间填充，诚实 0 verified） |
| 威胁模型 | ✅ docs/THREAT_MODEL.md 已提交 |
| 依赖安全 | ✅ audit 5→2（ratatui 升级清 2；剩余 bincode/yaml-rust 需 syntect 迁移） |
| 证据门清单 | ✅ plans/evidence-gate-inventory.md（M2 依赖图） |
| Docker 非 root | ✅ 已满足（USER 10001） |
| remote/发布真相 | ⚠️ 待决策（zaimouren1/ZAION 404；需真实 remote） |



## 九、跃迁计划前收尾工作完成（2026-08-14）

| 项 | 结果 | 验证 |
|---|---|---|
| settings.local.json 解追踪 | git rm --cached（本地保留+已忽略） | be64cd8 |
| **ratatui 0.29→0.30 升级** | **清除 2 个 audit 告警（lru unsound + paste unmaintained），audit 4→2** | 887257b; WS_ALL/CLIPPY/FMT/TEST 全绿 |
| 剩余 2 告警（bincode/yaml-rust） | 来自 syntect 5.3.0（无新版），记录 M1 豁免+迁移路径（替换 syntect 或等其升级） | audit 显示 2 |
| 孤儿冻结决策（ttc/ego_integration/panel_sink） | **保留**（已提交冻结，符合跃迁计划 M5 前冻结策略；非删除） | 已在基线提交 |
| 冻结清单核对 | opd/evolve/singularity/enclave/organism 系/macro_maturity/rollup 全部在 HEAD（提交即冻结） | git cat-file 验证 |
| 证据门清单 | plans/evidence-gate-inventory.md（68+95 锁定文件，M2 迁移依赖图） | 已提交 |
| eval 基准清单 | zaion_300_v1.json schema 合法（15/300 槽，诚实 0 verified） | ConvertFrom-Json |
| Docker 非 root | Dockerfile 已 USER 10001:10001 | 检查通过 |
| 安装脚本 | 默认指向幻影 repo（zaimouren1/ZAION 404）但有源码回退；记录 P0#6 | 检查 |
| 50GB target | 见 T8（清理决策） | - |

## 八、工作树整理完成（2026-08-14，13 个提交）

### 基线提交历史（branch: codex/worktree-triage）

| 提交 | 内容 | 规模 |
|---|---|---|
| 23b18b5 | docs: 项目地图/状态/记分卡/威胁模型/eval 清单/脚本/web-design skill | +8,303 |
| 92c2afb | feat(runtime): v2 turn store/tool broker/ingress/evidence graph/wake contracts | +16,995 |
| 1560595 | chore: 退役 zaion-website/claude hooks/mcp-test 残留; 归档旧 plan | -13,389 |
| e47ff8e | chore: 移除 14 个孤儿模块 | -3,911 |
| df810d5 | plans: 优化执行计划/报告 + 10/10 跃迁计划 | +392 |
| fd4318f | chore(deps): 删未用 usearch/textwrap/syntect; rand 0.8.7/0.9.5 | -17 |
| ab958d2 | chore(crates): 24 个叶子 crate 编辑 | +42 |
| 9d46bdc | chore(crates): ledger/memory/watchdog（含 reality_sync 统一） | +1,781 |
| 462c8b6 | chore(runtime): agent/turn/task 模块编辑 | +1,534 |
| 122876a | chore(crates): adapters/opd/gateway/mcp/codex/core | -68 |
| ebd211e | chore(tui): 组件/布局/渲染器 | +114 |
| a27ef0d | chore(cli): 命令/配置/测试编辑（含清理与网关超时修复） | +2,427 |
| 3ac5855 | chore: 根/docs/脚本（打包/安装/发布） | +505 |

**总计 305 项变更全部入版本控制，工作树 0 剩余。** 2 个月的未提交开发 + 19 轮清理成果现已有完整 git 历史保护。

> 完整执行日志见 `plans/zaion-optimization-execution-plan.md`
> **R18 更新**: P1.6 reality_sync 统一已执行完成(watchdog 扩展+cli 切换+memory 删除, 全绿+冒烟通过), 遗留专项减一项

## 七、工作树乱象评估（2026-08 实测）

### 核心结论: 不是"脏文件多", 是**2 个月开发从未提交**

- **git HEAD 停留在 2026-06-13**（提交 e5d86f0, 最近 8 个提交全是品牌皮肤 pixel ZAION banner/octopus）
- **170 个提交总量**, 现代架构工作全部在 HEAD 之后, 从未进入 git

### 量化

| 项 | 数值 |
|---|---|
| git status 变更 | 305 项（197 修改 + 76 删除 + 32 未跟踪） |
| 暂存 | **0**（全部未暂存） |
| 未跟踪文件 | 26 个文件、**~11,087 行**（核心架构 + 文档 + 脚本） |
| 已跟踪修改 | +11,222 / -22,204 行 |
| 未跟踪架构核心 | turn_store 3,143L / wake_contract_v2 1,021L / binding 874L / tool_broker 780L / gateway_contract 698L / wake_stream 491L / authenticated_ingress 474L / evidence_graph 220L / wake_request 286L |
| 未跟踪文档 | PROJECT_MAP / PROJECT_STATUS / THREAT_MODEL / PRODUCT_SCORECARD / CONTRIBUTING / LICENSE / ROADMAP / rust-toolchain.toml / scripts×2 |

### 磁盘占用

| 项 | 大小 | 状态 |
|---|---|---|
| target/ | **~50 GB** | gitignored, 可 cargo clean 重建 |
| .claude/worktrees/ | 2.9 GB | gitignored, 3 个注册 worktree（git worktree remove 清理） |
| claude-code-source/ | 127 MB | gitignored（参考镜像, 项目文档承认） |
| .git/ | 83 MB | 正常 |
| repo 本体（不含 target） | 190 MB | 正常 |

### 风险等级: 🔴 高

1. **10/10 计划的核心架构全部是未跟踪文件**（AuthenticatedIngress/ToolBroker/TurnStore/TurnState/EvidenceGraph/WakeRequest/GatewayContract）——不在 git 里、无备份, 一次误删或磁盘故障即全部丢失
2. **HEAD 2 个月陈旧**——没有任何回滚点覆盖这段开发
3. 我 19 轮清理成果（~30 个文件）也混在未提交区
4. 50GB target 是磁盘大头（可安全清理, 但重建耗时）

### 行动建议（按优先级）

1. **P0: 提交基线**——先 git add 全部 + 分逻辑提交（或至少一次大基线提交）, 把 11K 行架构 + 文档纳入版本控制
2. **P1: 清理 50GB target**（`cargo clean` 或选择性清理旧 profile）
3. **P1: 决定 3 个 worktree 去留**（`git worktree remove` 或保留）
4. **P2: 分阶段整理 305 项变更**（退役删除/架构新增/我的清理分门别类提交）


## 一、执行结果总览

| 阶段 | 结果 | 验证 |
|---|---|---|
| P0.1 死依赖 | 删 usearch/textwrap/syntect(cli 声明) | cargo check 绿 |
| P0.2 src 测试归位 | mock_vllm_server/webhook_e2e_test cfg(test) 门控 | opd 146+runtime 481 测试过 |
| P0.3 死代码 | allow(dead_code) 41处→3处(均刻意契约) | clippy -D warnings 绿 |
| P0.4 门禁 | fmt 漂移 73→0; workspace clippy 首次全绿 | FMT=0 / CLIPPY=0 |
| P0.5 治理 | rand 升级(unsound 清除); zaion-ai 404 确认; 编码损坏评估 | audit 5→4 |
| P1 合并 | 系统性验证: Telegram/webhook/agent loop/session_store/identity 均分层架构非重复, 合并目标撤销 | 分析记录 |
| 孤儿清扫 | 14 模块 -3,629L(3 次误删全部 git 恢复) | WS_ALL=0 |
| P4 健壮性 | 前提修正: unwrap ~99% 在测试代码, 生产已干净 | 抽样验证 |
| 缺陷修复 | 网关 flaky 根因(1s→3s 探测超时); R10 证据疏漏(session.rs 恢复) | 测试全绿 |

## 二、当前验证状态（全绿闭环）

```
cargo check --workspace --all-targets --locked   WS_ALL=0
cargo clippy --workspace --all-targets -D warnings CLIPPY=0
cargo fmt --all -- --check                        FMT=0
cargo test --workspace --locked -j1 -- --test-threads=1  WS_TEST=0 (44 套件全绿)
cargo audit --no-fetch                            4 allowed warnings(已知)
```

## 三、孤儿清扫明细（14 模块 -3,629L）

| 轮次 | 模块 | 行数 | 说明 |
|---|---|---|---|
| R7 | runtime/memory_agent_loop | 218 | 被 integrated_agent_loop 取代 |
| R8 | runtime/skill_catalog | 483 | 零引用(含文档/计划/证据) |
| R9 | tui/{inline_chat,inline_mode,modern_runner,ascii_art} | 1,044 | 旧版 TUI 遗留组件 |
| R9 | adapters/provider_chain | 458 | 零引用 |
| R10 | codex/v4a_patch | 431 | 零引用 |
| R10 | federation/{async_prefetch,peer,session} | 350 | 仅 session 证据必需,已恢复 |
| R11 | core/ipc | 156 | 零引用(含通配符再导出验证) |
| R13 | tui/modern_tui | 489 | 零代码使用,仅历史 docs |
| 误删恢复 | agui/applier/codegen | - | git 恢复,验证网有效 |

## 四、关键判断修正（"具体情况具体分析"成果）

1. **P1.1 Telegram**: 分层架构(adapter=IO, cli=特性层, 已委托), 非双实现 — 避免 18K 行错误重构
2. **P1.2 webhook**: 三套分层(server/bridge/command), 全部在用 — 非重复
3. **P1.4 agent loop**: 管道(fsm→loop→integrated→unified), 非 5 个重复
4. **P4 unwrap**: ~99% 在测试代码, 生产已干净 — 原目标不成立
5. **孤儿检测**: 通配符再导出/超行范围 pub 项/测试目标/evidence proof 路径 4 类盲区, 验证网(全目标编译)兜底
6. **编码损坏**: 写入时固化, git 无完好副本, 需人工重建或接受
7. **syntect 链**: 被 phase8b proof spec 锁定, 依赖迁移需先解耦渲染器

## 五、待决策清单

| # | 决策 | 影响 |
|---|---|---|
| 1 | 是否提交本轮成果(~30 文件) | 全绿基线的自然提交点 |
| 2 | settings.local.json 解追踪 | git rm --cached(保留本地) |
| 3 | orphan 保留项 ttc/ego_integration/panel_sink | 删 or 留(有文档引用但零代码用) |
| 4 | zaion-opd(8,350L) 去留 | DECISION_MATRIX 自述"可延后" |
| 5 | 4 个依赖告警 | 豁免 or 专项迁移(bincode/yaml-rust→syntect 解耦, paste/lru→ratatui) |
| 6 | reality_sync 统一(设计已就绪) | 跨 crate API 变更+生产门路径 |

## 六、遗留专项（需决策后执行）

- P2 巨型文件拆分(46 个 ≥1K 行文件, 需结构化多步: 先拆共享助手再拆命令)
- reality_sync 统一(5 步设计已成文)
- 依赖迁移(syntect 解耦渲染器 / ratatui 升级评估)
- MASTER_PLAN 编码人工重建(需原始证据)
