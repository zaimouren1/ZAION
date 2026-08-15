# Zaion 全量优化执行方案

> 状态: 执行中(P0) | 日期: 2026-08 | 适用工作区: D:/zaion-rust
> 执行日志:
> - R1(已完成): P0.1 删 usearch/textwrap ✅(cargo check 全绿); P0.2 cfg(test) 门控 mock_vllm_server/webhook_e2e_test ✅(opd 146+runtime 481 测试过); P0.3 清理 codex/lsp.rs 7 处 dead_code ✅(clippy 绿)
> - R2(已完成): 删 import_openclaw_cmd.rs(158L)✅; zk_compression 去假旋钮✅(clippy 绿,135+9 测试过); codex/ast.rs LineMap 重构✅(clippy 绿); security.rs cmd_skill 重复(108L)✅; app.rs render 死簇(123L)✅; hub.rs codex_db_path 重复/daemon.rs interval 死字段/tool_parsers.rs tc 助手/executor.rs 过期 allow/onboard.rs cfg(test)✅
> - R2 后状态: allow(dead_code) 29→19 文件(41→21 处); 净删 ~590 行; 全部 cargo check/clippy -D warnings 绿
> - R3(已完成): 同上全部✅; 验证: cli clippy -D warnings 绿; cli 测试 503+15+139+2 过(契约测试修复后 139 全过); cargo check --workspace WS_CHECK=0; singularity/tui/memory/runtime 各 crate clippy+测试绿
> - R4(已完成): 同上全部✅; 验证: cli clippy -D warnings 绿; cli 全量测试 678 个全过(503+15+139+2+5+11+3,含 R3 慢的 gateway 套件 5s 过); cargo check --workspace WS=0
> - R4 后状态: allow(dead_code) 仅剩 2 文件 3 处,均为文档化刻意保留; **P0.3 完成**
> - R5(已完成): P0.4 全绿(fmt 0 漂移 + workspace clippy 0 告警); P0.5 部分完成 —— rand 升级 audit 5→4; 4 传递依赖豁免待迁移; zaion-ai 404 确认不能加 remote; 编码损坏量化(MASTER_PLAN 18×U+FFFD+447×?? / gap_report 96+871)待专门恢复 pass; settings.local.json 待用户决策
> - R5 后状态: **P0 全部完成**(P0.1 死依赖/P0.2 测试归位/P0.3 死代码/P0.4 门禁/P0.5 治理); 验证: WS=0, WS_CLIPPY=0, FMT_CHECK=0, 全 crate 测试绿
> - R6(进行中): **P1.1 前提修正** —— 深度对比 cli/network/telegram.rs(13,295L) 与 adapters/telegram_adapter.rs(4,731L): 仅 10 个同名函数(多为通用小助手),135 个 adapter 独有; cli 已通过 TelegramLiveSender trait 委托 adapter(L160 构造, 0 处直连 api.telegram.org); 86 个测试在 cli 文件内。结论: 分层架构正常, 非双实现, **-8~10K 行合并目标撤销**; 真实重复仅 ~40 行 multipart/mime 助手(跨 crate 共享不划算, 记录不动)
> - P1 重排: 真实合并目标 = P1.2 webhook 三套(~9K) > P1.4 agent loop 群(5 套) > P1.3 TUI > P1.5 session_store > P1.6 reality_sync
> - R6(已完成): P1.6 分析 —— memory vs watchdog reality_sync: 设计统一专项,非复制删除
> - R7(已完成): **P1 系统性前提修正** —— webhook 三套验证为分层(adapters=服务器+DeliveryReceipt被3渠道用 / runtime=agent触发桥被 loop 用 / cli=命令+薄编排, mount_* 调用 adapters 的 mount_*_route); agent loop 验证为管道(fsm→loop→integrated→unified 各司其职); session_store 验证为适配层(adapter 包 ledger SessionStore); identity/did 验证为不同层。**P1 合并目标大部分撤销**; 真实孤儿: memory_agent_loop(218L,被 integrated_agent_loop 取代,零引用)已删除 ✅(CHECK/CLIPPY/TEST 绿)
> - P1 修订结论: 本代码库为深度分层架构,合并空间远小于 R1 估计; 真实可做 = 孤儿模块清扫(如 memory_agent_loop) + P2 巨型文件拆分 + reality_sync 设计统一(专项)
> - R8(已完成): runtime 孤儿扫描 —— skill_catalog 删除(-483L)
> - R9(已完成): 删除 tui×4+provider_chain(-1,502L); ⚠️ agui 误删恢复(通配符再导出漏检)
> - R10(已完成): 删除 4 个孤儿(-781L); 误删恢复 2 个(applier/codegen)
> - R11(已完成): 23 crate 全扫, 删 core/ipc(-156L); 孤儿清扫收官 13 模块 -3,140L
> - R12(已完成): 删 core/ipc(-156L) + cli syntect 声明清理
> - R13(已完成): 删 modern_tui(489L); P4 前提修正; flaky 网关测试诊断
> - R14(已完成): 编码恢复评估 + flaky 网关测试确诊
> - R15(已完成): flaky 网关修复 + R10 session.rs 证据疏漏恢复 + cli 678 全绿
> - R16(已完成): reality_sync 统一设计成文 + 全仓测试 WS_TEST=0
> - R17(已完成): 最终验证闭环 + 执行报告成文
> - R18(已完成): P1.6 reality_sync 统一执行完成
> - R19(已完成): 统一后全仓测试复跑 WS_TEST=0(89 套件全绿); **目标状态评估: 用户决策阻塞已持续 5 轮(R15-R19), 5 项决策未回复(提交/settings.local.json/orphan 保留项/opd 去留/依赖告警), 决策无关可执行工作全部完成并验证**
> 依据: 全仓机器级扫描(1,418 个 .rs 符号/规模/标记盘点) + 项目文档(AGENTS/CONTRIBUTING/PROJECT_STATUS/PROJECT_MAP/ROADMAP/DECISION_MATRIX)
> 原则: 遵守 CONTRIBUTING —— 每阶段独立分支; bulk formatter 单独提交; 动 .claude/worktrees 前先 `git worktree list`; 每步跑项目验证链。

## 0. 分支与验证链（所有里程碑通用）

- 分支: 每里程碑一个: opt/p0-hygiene, opt/p1-consolidation, opt/p2-giant-files, opt/p3-dead-code, opt/p4-robustness, opt/p5-governance
- 每步验证(按需取用, 提交前全跑):
  ```powershell
  cargo check --workspace --all-targets --locked
  cargo test --workspace --locked -j1 -- --test-threads=1
  cargo clippy --workspace --all-targets --locked -- -D warnings
  cargo fmt --all -- --check
  bash scripts/check-release-assets.sh
  powershell -NoProfile -ExecutionPolicy Bypass -File scripts/project-audit.ps1
  ```
- 回滚: 单步 `git revert`; 大项先在 worktree 里做, 验证后合回。
- 前置: 当前 275 个 dirty 条目 —— 每个里程碑开工前先 `git status --short` 确认只动自己文件; 不清理他人未提交工作。

---

## P0 基线卫生（1-2 天, 低风险, 纯机械）

### P0.1 删除死依赖（2 个）
> 已核实: usearch 全仓 0 使用(workspace 级声明, memory 实际用 instant-distance); textwrap 在 cli 0 使用。
> 注意: secrecy **不删** —— honcho.rs 6 处 SecretString 是刻意安全实践。

1. 编辑 `Cargo.toml`(根): 删除 [workspace.dependencies] 中 `usearch = "2"` 行
2. 编辑 `crates/zaion-cli/Cargo.toml`: 删除 textwrap 行
3. 验证: `cargo check --workspace --locked`(自动更新 Cargo.lock) + `cargo tree -i usearch` 应报 not found
4. 提交信息: `chore(deps): remove unused usearch and textwrap`

### P0.2 src 内嵌测试归位（2 个文件）
1. `crates/zaion-runtime/src/lib.rs` 第 62 行:
   `pub mod webhook_e2e_test;` → `#[cfg(test)] pub mod webhook_e2e_test;`
   (已核实该模块仅 lib.rs 引用, 内部是 #[cfg(test)] mod tests)
2. `crates/zaion-opd/src/lib.rs` 第 43 行:
   `pub mod mock_vllm_server;` → `#[cfg(test)] pub mod mock_vllm_server;`
   (batch_runner.rs:652 的调用在 #[cfg(test)] 内, 不受影响)
3. `crates/zaion-opd/tests/integration_tests.rs` 顶部加:
   ```rust
   #[path = "../src/mock_vllm_server.rs"]
   mod mock_vllm_server;
   ```
   并把 `use zaion_opd::mock_vllm_server::...` 改为本地 `use mock_vllm_server::...`
4. 验证: `cargo test -p zaion-runtime -p zaion-opd --locked`

### P0.3 allow(dead_code) 清扫（30 文件, 按文件逐个确认）
- 做法: 对每个文件 `cargo clippy -p <crate> --all-targets -- -D warnings`, 按报错删死代码后去掉 allow
- 优先序(标记最多): zaion-codex/src/lsp.rs(7处) → cli/commands/process/tui/app.rs(3) → cli/commands/{onboarding,network/telegram_commands}(各2) → 其余 1 处
- 大额目标: cli/commands/import_openclaw.rs(731L) + import_openclaw_cmd.rs(158L) 双文件死代码; memory/src/runtime_integration.rs(1037L)
- 验收: clippy 全仓 -D warnings 通过; 每文件单独 commit

### P0.4 格式与 Lint 门禁补齐
1. 独立提交: `cargo fmt --all`(一次性解决 73 个文件漂移, 不与任何功能改动混提)
2. `cargo clippy --workspace --all-targets --locked -- -D warnings` 修复到全绿
3. CI 确认这些 gate 已在跑(ROADMAP 声称已配, 需实测)

### P0.5 治理 P0(项目自列, 顺手做)
- `git remote add origin`(已执行: 新库 github.com/zaimouren1/ZAION, 旧地址 zaion-ai/zaion-rust 已确认 404 并修正)
- `git rm --cached .claude/settings.local.json`(解追踪但保留本地文件, P0#5)
- `cargo audit` 处理 5 个告警(bincode/paste/yaml-rust 未维护; lru/rand unsoundness) —— 安全项, 升级或记录豁免
- MASTER_PLAN.md 编码损坏: 从 git 历史恢复原文(P0#3, 文档级, 单独 commit)

---

## P1 单一来源合并（1-2 周, 高收益, 中风险）

> 对应 ROADMAP 开放项 "Select one authoritative TUI, one turn kernel, one gateway/WebUI server"。
> 每个合并前: `git log --oneline -- <file>` + 调用点 grep 确认活跃路径; 合并后跑该 crate 全部测试。

### P1.1 Telegram 双实现合一（收益最大, -8,000~10,000 行）
- 现状: cli/commands/network/telegram.rs(13,295L,298 fn) 与 adapters/telegram_adapter.rs(4,731L,146 fn) 功能重叠(describe_sticker/vision/document preview/multipart 双份)
- 步骤:
  1. 提取共享原语(multipart 构造, openai 兼容 URL, vision/sticker/doc 预览) 收进 adapters/telegram_adapter.rs
  2. cli/network/telegram.rs 改为调用 adapter + 保留 zaion tg CLI 面(轮询循环, 命令参数)
  3. 删除重复函数体; `cargo test -p zaion-adapters -p zaion-cli --locked`
  4. 手工冒烟: `zaion tg status` / `zaion tg doctor`
- 验收: 两个文件合计 < 6,000 行; tg 功能测试全绿

### P1.2 webhook 三套合一（-6,000~7,000 行）
- 现状: adapters/webhook_runtime.rs(3,845L 手写服务器) + cli/commands/webhook/(mod 3,167L + webhook_serve 1,621L) + runtime/webhook_runtime.rs(370L)
- 决策: 保留 runtime 薄整合层为唯一入口; adapters 版按"axum 原生能力(限流/幂等/签名)重写为小型工具函数"吸收进 runtime; cli 版降为薄命令
- 步骤: 1) runtime 版扩展为完整能力 2) cli webhook 命令改调 runtime 3) 删 adapters/webhook_runtime.rs 4) 全量 webhook 测试
- 验收: 三处合计 < 2,500 行; webhook 端到端测试(含 webhook_e2e_test)全绿

### P1.3 TUI 归属决策 + 合并（-4,000~6,000 行, 决策项）
- 现状: cli/process/tui/app.rs(6,354L 手写全部: 主题/动画/网关传输) 与 zaion-tui crate(9,063L) 各自为政; cli 实际只用 zaion-tui 的 brand+ThemeName
- 决策点(二选一, 建议 A):
  - A. 以 zaion-tui 为准: app.rs 的渲染/主题/传输层迁移进 zaion-tui, cli 保留交互壳
  - B. 以 cli app.rs 为准: 冻结 zaion-tui 未用组件(标记 deprecated), 避免双维护
- 步骤: 按决策执行后, 用 `cargo test -p zaion-tui -p zaion-cli` + 手工 TUI 冒烟
- 风险: 最高(UI 回归); 建议放在 P1 最后, 且拆成多个小 PR

### P1.4 agent loop 群合一（-3,000~5,000 行）
- 现状: agent_fsm(886) / agent_loop / integrated_agent_loop / memory_agent_loop / unified_agent_runtime(1,190) / omni_session(1,839)
- 步骤: 1) 用 `rg "mod |use .*::(run|execute|start)"` 盘点每套的真实调用方 2) 选定 canonical(建议 agent_loop 或 unified_agent_runtime) 3) 其余 feature-gate 后逐个删 4) `cargo test -p zaion-runtime --locked`
- 注意: turn_proof/evidence_graph/turn_outcome 是刻意架构(proof-bound), 不动

### P1.5 session_store 双份合一（-600~1,000 行）
- ledger/session_store.rs(721L) 为 owner; runtime/session_store_adapter.rs(475L) 改为纯委托薄层或删除

### P1.6 reality_sync 双份合一（-700 行）
- watchdog/reality_sync.rs(368L) 与 memory/reality_sync.rs(324L) 合并进 ledger(SHA-256 文件锚定); 保留统一 trait

---

## P2 巨型文件拆分（1-2 周, 可与 P1 并行, 只拆不改逻辑）

> 46 个 ≥1,000 行文件是项目已测量债务(MASTER_PLAN "giant-file splits")。拆分规则: 提取模块目录, 每拆一步 `cargo check -p <crate>` + 该模块测试。

| 文件 | 行数 | 拆分方案 |
|---|---|---|
| cli/commands/network/telegram.rs | 13,295 | tg/ 目录: polling.rs, vision.rs, document.rs, multipart.rs, cli.rs(配合 P1.1) |
| cli/tests/cli_stable_surface.rs | 11,263 | 按命令拆 tests/stable/*.rs |
| cli/commands/system.rs | 7,997 | config.rs, version.rs, acp.rs, whatsapp.rs, claw.rs, uninstall.rs |
| cli/commands/process/wake.rs | 6,685 | wake/ 目录: request.rs, execute.rs, compression.rs, schedule.rs, tools.rs |
| cli/commands/process/tui/app.rs | 6,354 | app/ 目录: theme.rs, render.rs, transport.rs, state.rs(配合 P1.3) |
| cli/commands/phase8b.rs | 3,575 | 保持单一(活跃 proof), 只拆内部 helper 到 phase8b/ |
| cli/commands/network/routes.rs | 3,682 | routes/: sse.rs, websocket.rs, events.rs, api.rs |
| runtime/turn_store.rs | 3,291 | 保持分层(真实复杂度), 但 turn_store/tests.rs(3,065L) 按层拆测试 |
| cli/commands/webhook/mod.rs | 3,167 | 配合 P1.2 合并, 不单独拆 |
| cli/commands/memory.rs | 2,860 | 按子命令拆 |
| cli/commands/skills.rs | 2,362 | 按子命令拆 |
| cli/commands/mcp.rs | 2,284 | 按子命令拆 |
| cli/commands/network/daemon.rs | 1,974 | daemon/: server.rs, lifecycle.rs, auth.rs |
| cli/commands/network/console.rs | 1,568 | 配合 P1.3 |
| runtime/omni_session.rs | 1,839 | 按会话阶段拆 |
| runtime/turn_store/dispatcher.rs | 1,835 | dispatcher/: lease.rs, outbox.rs, verify.rs |
| adapters/telegram_adapter.rs | 4,731 | 配合 P1.1 |
| adapters/webhook_runtime.rs | 3,845 | 配合 P1.2(预计删除) |
| adapters/signal.rs | 1,380 | 拆 client/handler |
| tui/streaming_renderer.rs | 1,276 | 拆 frame/buffer/animation |
| 其余 ~25 个 1,000-2,000 行文件 | — | 逐文件按模块边界拆, 见附件清单 |

## P3 死代码与孤儿清理（~1 周, 文档已背书）

1. **zaion-opd**(8,350L): DECISION_MATRIX 裁定"可以延后"→ 从 `default-members` 移除(仅 cli 不默认依赖它; 保留 workspace 成员), 观察 1 个迭代周期后决定拆分或删除
2. **孤儿 crate 决策表**(工作区无依赖方):
   - zaion-telemetry(591L): 文档标 Keep/High —— 若 1 个月内无接入, 从 workspace 移除
   - zaion-gateway(1,392L): PROJECT_STATUS P0#1 已知 —— 与 P1.2 的 webhook/网关合并联动
   - zaion-proptest(543L): 测试支撑, 保留但明确归属(移入 dev-dependencies 用法文档)
3. **execute_code 三后端合一**: 保留 execute_code.rs(344L) 为主, 删 execute_code_js.rs(876L)/execute_code_uds.rs(1,023L) 的死代码后评估合并(-1,900L)
4. **P0.3 遗留争议项**: 有调用方但功能重叠的(如 tool_parsers 690L vs builtin_tools)单独评审

## P4 健壮性卫生（持续, 非 ponytail 范围但同批做）

- unwrap 治理(全仓 ~4,200 处), 按密度优先:
  1. cli/network/telegram.rs(500) → 2. turn_store/tests.rs(352) → 3. cli_stable_surface.rs(239) → 4. telegram_adapter.rs(194) → 5. ledger/tests.rs(152) → 6. shadow/tests.rs(108)
- 每处改 `?/` + thiserror/anyhow 上下文; 测试代码可批量 `let _ = ` 或 expect 带信息
- 目标: src 下 unwrap 密度 < 1/100 行

## P5 治理收口（持续）

- 每里程碑合并后更新 `docs/PROJECT_STATUS.md`(登记完成项/剩余项)
- 把本方案登记为 `plans/README.md` 的活动计划
- CI: 把 P0.4 的 fmt/clippy/audit gate 设为必过(现在只是声称配置)

---

## 执行顺序与总收益

```
P0(1-2天) → P1(1-2周) → P2(并行1-2周) → P3(1周) → P4/P5(持续)
net: -35,000 ~ -55,000 行(15-23%), 2 个死依赖, 46 个巨型文件归位
```

## 风险登记

| 风险 | 级别 | 缓解 |
|---|---|---|
| P1.3 TUI 合并 UI 回归 | 高 | 放最后, 拆小 PR, 手工冒烟 |
| P1.4 loop 合并破坏 turn 流程 | 中 | 先 feature-gate, 跑全量 runtime 测试 |
| P1.2 webhook 合并丢签名校验 | 中 | 保留测试夹具(webhook_e2e_test) |
| P2 拆分引入行为漂移 | 中 | 纯移动不改逻辑, 每步 cargo check |
| 275 个 dirty 条目被误清 | 高 | 严格遵守"只动自己的文件", 开工前 git status |
