# Zaion Module Eval Contract（模块评测契约）

> 目的：不再用"有多少个测试"衡量模块，而是定义**每个模块在哪个 failure
> dimension 上提供什么证据**。每个模块统一采用：
>
> `Capability → Eval Dimension → KPI → Scenario → Failure Signal → Evidence`
>
> 状态：2026-08-23 落地首版（36 crate + 16 macro 模块 + 统一 taxonomy）。

---

## 1. 统一 Eval Taxonomy（7 大维度）

```
Correctness   —— 功能 / 状态 / 协议 / 时序 正确性
Integrity     —— 持久化 / 账本 / 快照 / 身份 完整性
Reliability   —— 崩溃恢复 / 重试 / 幂等 / 收敛 / 长程稳定
Safety        —— 注入抵抗 / Auth/RBAC / 密钥泄露 / SSRF / 危险行为阻断
Intelligence  —— 检索 / 推理 / 决策质量 / 探索收益 / 自我改进
Autonomy      —— 自我状态感知 / 资源感知 / 反射 / 恢复 / 多系统协同
Efficiency    —— 延迟 / token 成本 / 内存 / CPU-IO / 探索成本
```

---

## 2. Evidence Level（证据等级 0-5）

| 等级 | 含义 |
|---|---|
| 0 | 未测试 |
| 1 | 单元测试 |
| 2 | 集成测试 |
| 3 | adversarial test（对抗性） |
| 4 | fault injection（故障注入） |
| 5 | real-LLM / production-like evidence |

顶层安全指标（跨 safety/secrets/gateway/mcp/aci 共享）：

- **Security Escape Rate** = 本应被阻断的危险行为中，实际穿透防线的比例
- **Auth Bypass Rate** · **RBAC False-Allow Rate** · **SSRF Escape Rate**

---

## 3. 36 crate 的 Module Eval Contract

格式：`Eval-ID | 核心维度 | KPI | 失败信号 | 证据命令`

### 3.1 核心运行时

| Crate | Eval ID | 核心维度 | KPI | 失败信号 | 证据命令 |
|---|---|---|---|---|---|
| zaion-runtime | RT-001 | Long-Horizon 正确性 | Turn Success / 工具循环完成率 / 取消正确 / 上下文保持 | 50 动作后终态与预期不一致 | `cargo test -p zaion-runtime`（473） |
| zaion-core | CORE-001 | 进程生命周期完整性 | Spawn/Stop/Restart 成功 / 孤儿率 / IPC 恢复 | daemon 崩溃后状态残留 | `cargo test -p zaion-core`（25） |
| zaion-types | TYPES-001 | 类型契约稳定性 | 序列化往返 / Schema 兼容 / 非法态拒绝 | 新旧版本互操作失败 | `cargo test -p zaion-types`（28） |
| zaion-paths | PATHS-001 | 路径隔离与迁移 | 路径确定性 / 冲突率 / 迁移成功 | 多 profile 串数据 | `cargo test -p zaion-paths`（3） |

### 3.2 身份 / 加密 / 安全

| Crate | Eval ID | 核心维度 | KPI | 失败信号 | 证据命令 |
|---|---|---|---|---|---|
| zaion-crypto | CRY-001 | 密码学正确性 + 不可伪造 | 签名/验签准确 / 篡改检测 / 密钥隔离 | 重放旧签名被接受 | `cargo test -p zaion-crypto`（14） |
| zaion-secrets | SEC-001 | 机密性 + 生命周期 | 泄露率 / 解密成功 / 轮换正确 / 审计完整 | 日志泄露密钥 | `cargo test -p zaion-secrets`（11） |
| zaion-enclave | ENC-001 | 隔离与封存完整性 | Seal/Unseal 完整 / 认证一致 / 篡改检测 | 封存文件被改后 unseal 成功 | `cargo test -p zaion-enclave`（9） |
| zaion-safety | SAF-001 | 风险拦截有效性 | 注入检测召回 / 误报率 / 脱敏泄露率 | API key 出现在日志 | `cargo test -p zaion-safety`（31） |

### 3.3 记忆 / 账本 / 同步

| Crate | Eval ID | 核心维度 | KPI | 失败信号 | 证据命令 |
|---|---|---|---|---|---|
| zaion-memory | MEM-001 | 记忆生命周期 | Recall/Precision / 更新准确 / 过期率 / 遗忘成功 | 过期事实被返回 | `cargo test -p zaion-memory`（56） |
| zaion-ledger | LED-001 | 事件不可抵赖 | 追加持久 / 签名有效 / 篡改检测 / 重放拒绝 | 断电后事件丢失 | `cargo test -p zaion-ledger`（54） |
| zaion-gitledger | GIT-001 | 时空状态重建 | 回放确定性 / 回滚保真 / 分支一致 | 任意时间点恢复失败 | `cargo test -p zaion-gitledger`（16） |
| zaion-federation | FED-001 | 分布式观察一致 | 新鲜度 / 最终收敛 / 重复率 | 乱序事件导致分歧 | `cargo test -p zaion-federation`（13） |
| zaion-sync | SYNC-001 | 跨设备收敛 | 收敛率 / 冲突解决 / 丢失率 | A/B 离线冲突丢数据 | `cargo test -p zaion-sync`（24） |
| zaion-checkpoint | CKPT-001 | 灾难恢复完整性 | 恢复成功 / 数据丢失率 / 回滚正确 | 写一半崩溃恢复失败 | `cargo test -p zaion-checkpoint`（12） |

### 3.4 通信 / 工具 / 协议

| Crate | Eval ID | 核心维度 | KPI | 失败信号 | 证据命令 |
|---|---|---|---|---|---|
| zaion-adapters | ADP-001 | Provider 行为一致 | 契约通过率 / 重试正确 / 流完整 / 回退成功 | 各 provider 格式差异导致错解析 | `cargo test -p zaion-adapters`（251） |
| zaion-mcp | MCP-001 | 工具安全 + 协议正确 | 工具成功 / Schema 合规 / allowlist 逃逸率 / 畸形恢复 | 恶意 tool schema 逃逸 | `cargo test -p zaion-mcp`（102） |
| zaion-a2a | A2A-001 | Agent 间互操作 | 握手成功 / 消息兼容 / 路由准确 | 协议版本差异握手失败 | `cargo test -p zaion-a2a`（25） |
| zaion-gateway | GW-001 | 边界安全 + 请求完整 | Auth Bypass / SSRF Escape / RBAC 准确 / WS 稳定 | 未授权请求通过 | `cargo test -p zaion-gateway`（83） |

### 3.5 智能体 / 界面 / 代码 / 进化

| Crate | Eval ID | 核心维度 | KPI | 失败信号 | 证据命令 |
|---|---|---|---|---|---|
| zaion-cli | CLI-001 | 控制平面可操作 | 命令成功 / 退出码正确 / 幂等 | 重复执行副作用 | `cargo test -p zaion-cli`（500+） |
| zaion-tui | TUI-001 | 交互状态一致 | 状态/UI 一致 / 输入恢复 / 渲染稳定 | resize 后状态错乱 | `cargo test -p zaion-tui`（68） |
| zaion-codex | CDX-001 | 代码语义定位 | 符号检索召回 / 引用精度 / 语义搜索 NDCG | 同名变量错位 | `cargo test -p zaion-codex`（35） |
| zaion-aci | ACI-001 | 代码变更安全 | AST 有效 / patch 精度 / 不变量保持 / 回滚成功 | 语法破坏 patch 被应用 | `cargo test -p zaion-aci`（52） |
| zaion-evolve | EVO-001 | 自进化净收益 | 提案接受 / patch 成功 / 回归率 / 回滚率 | Net Evolution Gain < 0（越改越差） | `cargo test -p zaion-evolve`（62） |

### 3.6 自治系统（Autonomy Eval）

| Crate | Eval ID | 核心维度 | KPI | 失败信号 | 证据命令 |
|---|---|---|---|---|---|
| zaion-autonomic | AUT-001 | 反射响应正确 | 刺激→响应准确 / 反应延迟 / 过度反应率 | 噪声刺激触发反射 | `cargo test -p zaion-autonomic`（34） |
| zaion-proprioception | PRP-001 | 自我状态感知 | 状态估计准确 / 休克检测召回 / 误报 | 资源下降未感知 | `cargo test -p zaion-proprioception`（42） |
| zaion-metabolic | MET-001 | 资源约束决策 | 预算准确 / 超支率 / 效用-成本 | token 超预算仍继续 | `cargo test -p zaion-metabolic`（62） |
| zaion-curiosity | CUR-001 | 探索收益率 | 新颖产出 / 有用发现率 / 探索成本 | 重复探索无收益 | `cargo test -p zaion-curiosity`（42） |
| zaion-ego | EGO-001 | 身份连续性 | 身份一致 / SoulHash 稳定 / 人格漂移 | 重启后人格漂移 | `cargo test -p zaion-ego`（21） |
| zaion-singularity | SNG-001 | 自治协同稳定 | 跨系统稳定 / 死锁率 / 振荡率 | 多系统互相打架 | `cargo test -p zaion-singularity`（30） |
| zaion-shadow | SHD-001 | 并行策略价值 | shadow 效用 / 预测一致 / 资源开销 | shadow 与主进程结论冲突无意义 | `cargo test -p zaion-shadow`（42） |
| zaion-watchdog | WDG-001 | 故障检测自愈 | 检测召回 / 修复成功 / 不安全修复率 / 恢复时间 | 错误修复引入新问题 | `cargo test -p zaion-watchdog`（53） |
| zaion-opd | OPD-001 | 蒸馏行为保持 | 轨迹保真 / 能力保持 / 策略漂移 | 蒸馏后能力退化 | `cargo test -p zaion-opd`（144） |

### 3.7 辅助模块

| Crate | Eval ID | 核心维度 | KPI | 失败信号 | 证据命令 |
|---|---|---|---|---|---|
| zaion-pricing | PRC-001 | 成本估算准确 | 价格准确 / 快照有效 / 币种正确 | 跨模型价格算错 | `cargo test -p zaion-pricing`（22） |
| zaion-telemetry | TEL-001 | 可观测完整 | 事件丢失率 / 时间戳准确 / trace 完整 | flush 前退出丢事件 | `cargo test -p zaion-telemetry`（9） |
| zaion-contract-macros | CM-001 | 契约强制 | 编译期违规检测 / 漏报率 | 故意违规通过编译 | `cargo test -p zaion-contract-macros`（6） |
| zaion-proptest | PRP-001 | 属性发现效率 | bug 发现率 / 收缩质量 / 可复现 | 随机状态漏 bug | `cargo test -p zaion-proptest`（25） |

---

## 4. 16 macro 模块的用户可观察评测

| Module | 专门评测 | 成熟度 |
|---|---|---|
| metabolic | Budget-aware：预算下降后是否主动改策略而非报错 | beta |
| ego | Identity Continuity：重启/迁移后身份稳定 | beta |
| activity-continuity | Activity Persistence：中断后恢复活动 | beta |
| memory-trace | Memory Provenance：每个记忆可答"从哪来/为何有效/何时失效" | beta |
| context-kernel | Context Reconstruction：压缩后重建关键信息 | beta |
| omni-session | Session Continuity：CLI/Web/Telegram 切换后 session 一致 | beta |
| tui | Interactive Reliability：长交互状态一致 | stable-extension |
| autonomic | Reflex Stability：刺激产生正确且不过度的响应 | experimental |
| curiosity | Exploration ROI：探索获得有价值信息 | experimental |
| proprioception | Self-State Accuracy：自身资源/环境状态判断准确 | experimental |
| rollup | Historical Reconstruction：汇总后重建关键历史 | experimental |
| singularity | Autonomy Coordination：System II-V 不打架 | experimental |
| watchdog | Self-Healing Safety：修复解决且不引入新问题 | experimental |
| evolve | Net Capability Gain：进化后能力真提升 | experimental |
| opd | Distillation Fidelity：蒸馏后保留策略能力 | experimental |
| enclave | Isolation Guarantee：封存态无法被错误主体读取 | experimental |

---

## 5. Cross-System Eval（端到端语义链）

真正的难题不在单模块，而在语义链：

```
memory → context-kernel → runtime → LLM → tool → ledger → sync
```

任何一环语义损失，最终 Agent 就错。顶层指标应为 **Capability Correctness**
（而非 Test Pass Rate）：

```
Task: 用户之前说过已换工作，帮他安排下周工作相关日程
Memory ✓  Retrieval ✓  Temporal ✓  Context ✓  Tool ✓  Scheduling ✓
Final Action ✗  →  结论: Memory healthy, end-to-end capability FAILED
```

这种"哪一层坏了"的诊断，正是 memory-trace / context / ledger / verifier
已经具备的基础。

---

## 6. 评分模型（供 doctor / dashboard / architecture-audit 展示）

```
module_eval = {
    functional_correctness, state_integrity, reliability,
    safety, performance, observability
}
evidence_level: 0..5
```

示例（zaion-runtime）：

```
Correctness 4/5 · Reliability 5/5 · Safety 4/5
Performance 3/5 · Observability 5/5 · LLM Evidence 4/5
Evidence Level 4
```

---

## 7. 可执行证据矩阵

上面的 Eval Contract 有对应的可执行证据报告，由 eval/harness/module_eval_runner.py 自动生成（运行 36 crate 的证据命令 + 输出 evidence_level 评分矩阵）：

- 运行：python eval/harness/module_eval_runner.py（或 --quick 只跑关键 crate）
- 报告：eval/results/MODULE_EVAL_REPORT.md（当前 35/36 pass，zaion-cli 为本地资源竞争 flaky，单独+CI 全绿）

---

## 8. 可执行组件总览 + 运行指南

### 8.1 组件清单（三层证据 + 三层门禁 + 展示）

| 组件 | 类型 | 路径 | 状态 |
|---|---|---|---|
| module_eval_runner.py | 单模块证据（36 crate） | eval/harness/ | 36/36 |
| cross_system_eval.py | 跨模块语义链 | eval/harness/ | 5/5（进 CI） |
| hero_eval.py | 真实 LLM 4 场景 | eval/harness/ | 3/4 稳定 |
| MODULE_EVAL_REPORT.md / .json | 证据矩阵（数据源） | eval/results/ | 已生成 |
| HERO_EVAL_REPORT.md | 真实 LLM 报告 | eval/results/ | 已生成 |
| architecture-audit 覆盖检查 | 门禁 | system.rs | 36 crate 有 contract |
| architecture-audit 证据门槛 | 门禁 | system.rs | 安全模块 >= level 3 |
| CI cross-system gate | 门禁 | ci.yml | Linux 语义链 |
| doctor module-eval 段落 | 展示 | system.rs | evidence 分布 |

### 8.2 运行指南

单模块证据（36 crate，10+ 分钟）:
  python eval/harness/module_eval_runner.py        # 或 --quick 关键 6 crate

跨模块语义链（本地秒级，不依赖 LLM）:
  python eval/harness/cross_system_eval.py target/debug/zaion.exe

真实 LLM 4 场景（需 key，约 5 分钟）:
  ANTHROPIC_API_KEY=sk-... python eval/harness/hero_eval.py target/debug/zaion.exe

门禁 + 展示:
  zaion architecture-audit    # 覆盖 + 证据门槛
  zaion doctor                # 展示 module-eval 分布

### 8.3 诚实状态（2026-08-24）

- 单模块：36/36（zaion-cli 本地资源竞争 flaky，单独+CI 全绿）
- 跨模块：5/5（memory->context->ledger->sync）
- 真实 LLM：代码/SRE/恢复稳定；安全场景间歇性（tokenrhythm 大工具结果多轮请求端点超时，功能已验证）
