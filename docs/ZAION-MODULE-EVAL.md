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

## 3. 36 crate 的 Module Eval Contract（完整六字段）

每个 crate 一段：Capability → Test Scenario → Pass Condition → Metric → Failure Signal → Evidence。

### 3.1 核心运行时

#### zaion-runtime (RT-001)
- Capability: Long-Horizon Execution Correctness
- Test Scenario: 连续 24 轮工具链执行；工具失败后恢复；取消中断；并发工具批处理
- Pass Condition: 50 动作后终态与预期一致；取消后子进程终止且无孤儿；压缩后继续执行不丢上下文
- Metric: Turn Success Rate / Tool Loop Completion / Cancel Correctness / Context Preservation
- Failure Signal: 终态不一致 / 取消后进程残留 / 上下文丢失
- Evidence: cargo test -p zaion-runtime（473 测试）

#### zaion-core (CORE-001)
- Capability: Process Lifecycle Integrity
- Test Scenario: spawn/stop/restart 循环；daemon 崩溃；子进程崩溃；重复启动
- Pass Condition: 每次生命周期转换后无孤儿进程；崩溃后 IPC 可恢复
- Metric: Spawn/Stop/Restart Success / Orphan Rate / IPC Recovery Rate
- Failure Signal: daemon 崩溃后状态残留 / 孤儿进程
- Evidence: cargo test -p zaion-core（25 测试）

#### zaion-types (TYPES-001)
- Capability: Type Contract Stability
- Test Scenario: 新旧版本 Event/MemoryEntry/SessionKey 序列化往返；非法状态注入
- Pass Condition: 往返序列化无损；Schema 兼容；非法状态被拒绝
- Metric: Serialization Roundtrip / Schema Compatibility / Invalid State Rejection
- Failure Signal: 新旧版本互操作失败 / 非法状态被接受
- Evidence: cargo test -p zaion-types（28 测试）

#### zaion-paths (PATHS-001)
- Capability: Path Isolation & Migration
- Test Scenario: XDG/自定义 root；多个 profile；空白 env；派生路径遍历
- Pass Condition: 路径确定；空白 env 回退默认；所有派生路径在 home 下
- Metric: Path Determinism / Collision Rate / Migration Success
- Failure Signal: 多 profile 串数据 / 空白 ZAION_HOME 产生空路径
- Evidence: cargo test -p zaion-paths（5 测试，含隔离/回退对抗）

### 3.2 身份 / 加密 / 安全

#### zaion-crypto (CRY-001)
- Capability: Crypto Correctness & Identity Non-forgery
- Test Scenario: 篡改 payload；换 key 验签；重放旧签名；损坏 key
- Pass Condition: 签名/验签准确；篡改被检测；重放被拒绝
- Metric: Sign/Verify Accuracy / Tamper Detection / Key Isolation
- Failure Signal: 重放旧签名被接受 / 换 key 后验签通过
- Evidence: cargo test -p zaion-crypto（14 测试）

#### zaion-secrets (SEC-001)
- Capability: Confidentiality & Lifecycle Safety
- Test Scenario: 错 key 解密；损坏密文；rotation；中途崩溃；日志泄漏
- Pass Condition: 解密仅成功于正确 key；轮换正确；审计完整；日志无密钥
- Metric: Secret Leakage Rate / Decrypt Success / Rotation Correctness / Audit Completeness
- Failure Signal: 日志泄露密钥 / 错 key 解密成功
- Evidence: cargo test -p zaion-secrets（11 测试）

#### zaion-enclave (ENC-001)
- Capability: Isolation & Seal Integrity
- Test Scenario: seal→restart→unseal；封存文件篡改；identity mismatch
- Pass Condition: unseal 完整；篡改被检测；身份不匹配被拒绝
- Metric: Seal/Unseal Integrity / Attestation Consistency / Tamper Detection
- Failure Signal: 封存文件被改后 unseal 成功 / 身份不匹配仍解封
- Evidence: cargo test -p zaion-enclave（9 测试）

#### zaion-safety (SAF-001)
- Capability: Risk Interception Effectiveness
- Test Scenario: prompt injection 多类；日志含 API key/token；恶意工具描述
- Pass Condition: 注入被检测；密钥被脱敏；正常字符串误报率低
- Metric: Injection Detection Recall / False Positive Rate / Redaction Leakage Rate
- Failure Signal: API key 出现在日志 / 注入未被检测
- Evidence: cargo test -p zaion-safety（31 测试，含 6 类注入 + 脱敏对抗）

### 3.3 记忆 / 账本 / 同步

#### zaion-memory (MEM-001)
- Capability: Memory Lifecycle Correctness
- Test Scenario: 新事实/修改/冲突/过期/删除；噪声干扰
- Pass Condition: Recall/Precision 达标；更新准确；过期事实不被返回；遗忘成功
- Metric: Recall / Precision / Write Accuracy / Update Accuracy / Stale Rate / Forget Success
- Failure Signal: 过期事实被返回 / 冲突事实错误覆盖
- Evidence: cargo test -p zaion-memory（56 测试）

#### zaion-ledger (LED-001)
- Capability: Event Non-repudiation
- Test Scenario: 断电；WAL recovery；事件篡改；重复提交
- Pass Condition: 追加持久；签名有效；篡改被检测；重放被拒绝
- Metric: Append Durability / Signature Validity / Tamper Detection / Replay Rejection
- Failure Signal: 断电后事件丢失 / 重复提交被接受
- Evidence: cargo test -p zaion-ledger（54 测试）

#### zaion-gitledger (GIT-001)
- Capability: Spatiotemporal Rebuild
- Test Scenario: 任意时间点恢复；branch merge；rollback 后再次提交
- Pass Condition: 回放确定性；回滚保真；分支一致
- Metric: Replay Determinism / Rollback Fidelity / Branch Consistency
- Failure Signal: 任意时间点恢复失败 / rollback 后状态不一致
- Evidence: cargo test -p zaion-gitledger（16 测试）

#### zaion-federation (FED-001)
- Capability: Distributed Observation Consistency
- Test Scenario: Peer 延迟；断线；乱序；重复事件；部分 peer 在线
- Pass Condition: 观察新鲜度达标；最终收敛；重复被去重
- Metric: Observation Freshness / Eventual Convergence / Duplicate Rate
- Failure Signal: 乱序事件导致分歧 / 重复事件未去重
- Evidence: cargo test -p zaion-federation（13 测试）

#### zaion-sync (SYNC-001)
- Capability: Cross-device Convergence
- Test Scenario: A/B 同步；离线修改；冲突；重复 import/export
- Pass Condition: 收敛；冲突解决正确；无数据丢失
- Metric: Convergence Rate / Conflict Resolution Accuracy / Loss Rate
- Failure Signal: A/B 离线冲突丢数据 / 重复 import 产生重复
- Evidence: cargo test -p zaion-sync（24 测试）

#### zaion-checkpoint (CKPT-001)
- Capability: Disaster Recovery Integrity
- Test Scenario: 写一半崩溃；checkpoint 损坏；restore；连续 checkpoint
- Pass Condition: 恢复成功；数据丢失率 0；回滚正确
- Metric: Recovery Success / Data Loss Rate / Rollback Correctness
- Failure Signal: 写一半崩溃恢复失败 / restore 后状态损坏
- Evidence: cargo test -p zaion-checkpoint（12 测试）

### 3.4 通信 / 工具 / 协议

#### zaion-adapters (ADP-001)
- Capability: Provider Behavior Consistency
- Test Scenario: OpenAI/Anthropic 格式差异；timeout；rate limit；stream 中断；thinking 签名回传
- Pass Condition: 各 provider 契约通过；重试正确；流完整；回退成功
- Metric: Contract Pass Rate / Retry Correctness / Streaming Integrity / Fallback Success
- Failure Signal: 格式差异导致错解析 / stream 中断后状态损坏 / 多轮工具 thinking 签名丢失
- Evidence: cargo test -p zaion-adapters（251 测试）

#### zaion-mcp (MCP-001)
- Capability: Tool Safety & Protocol Correctness
- Test Scenario: 恶意 tool schema；参数类型错；超时；工具并发；权限拒绝
- Pass Condition: 工具成功；Schema 合规；allowlist 不逃逸；畸形输入可恢复
- Metric: Tool Success / Schema Compliance / Allowlist Escape Rate / Malformed Recovery
- Failure Signal: 恶意 tool schema 逃逸 allowlist / 未授权工具被调用
- Evidence: cargo test -p zaion-mcp（102 测试）

#### zaion-a2a (A2A-001)
- Capability: Agent Interop
- Test Scenario: 不同 agent card；协议版本差异；断线重连；未知能力
- Pass Condition: 握手成功；消息兼容；路由准确
- Metric: Handshake Success / Message Compatibility / Routing Accuracy
- Failure Signal: 协议版本差异握手失败 / 路由到错误 agent
- Evidence: cargo test -p zaion-a2a（25 测试）

#### zaion-gateway (GW-001)
- Capability: Boundary Security & Request Integrity
- Test Scenario: 未授权请求；越权；SSRF；TLS 握手；断连重连；恶意 header
- Pass Condition: Auth Bypass=0；SSRF Escape=0；RBAC 准确；WS 稳定
- Metric: Auth Bypass Rate / SSRF Escape Rate / RBAC Accuracy / WS Stability
- Failure Signal: 未授权请求通过 / SSRF 逃逸 / 越权访问
- Evidence: cargo test -p zaion-gateway（83 测试）

### 3.5 智能体 / 界面 / 代码 / 进化

#### zaion-cli (CLI-001)
- Capability: Control-plane Operability
- Test Scenario: 同命令重复执行；非法参数；部分失败；旧 config
- Pass Condition: 命令成功；退出码正确；重复执行幂等
- Metric: Command Success / Exit Code Correctness / Idempotency
- Failure Signal: 重复执行产生副作用 / 非法参数崩溃
- Evidence: cargo test -p zaion-cli（500+ 测试 + 真实 LLM chat/hero）

#### zaion-tui (TUI-001)
- Capability: Interactive State Consistency
- Test Scenario: 快速输入；窗口 resize；长输出；错误状态
- Pass Condition: 状态/UI 一致；输入恢复；渲染稳定
- Metric: State/UI Consistency / Input Recovery / Render Stability
- Failure Signal: resize 后状态错乱 / 长输出渲染崩溃
- Evidence: cargo test -p zaion-tui（68 测试）

#### zaion-codex (CDX-001)
- Capability: Code Semantic Locate
- Test Scenario: 同名变量；跨文件引用；trait/impl；宏；复杂 repo
- Pass Condition: 符号检索召回达标；引用精度；语义搜索 NDCG
- Metric: Symbol Retrieval Recall / Reference Precision / Semantic Search NDCG
- Failure Signal: 同名变量错位 / 跨文件引用未找到
- Evidence: cargo test -p zaion-codex（35 测试）

#### zaion-aci (ACI-001)
- Capability: Code Change Safety
- Test Scenario: 错误 patch；跨文件修改；语法破坏；测试失败后恢复
- Pass Condition: AST 有效；patch 精度；不变量保持；回滚成功
- Metric: AST Validity / Patch Precision / Invariant Preservation / Rollback Success
- Failure Signal: 语法破坏 patch 被应用 / 回滚失败
- Evidence: cargo test -p zaion-aci（52 测试）

#### zaion-evolve (EVO-001)
- Capability: Net Evolution Gain
- Test Scenario: 扫描→提案→审核→修改→测试→回滚 全流程
- Pass Condition: patch 成功且 Net Evolution Gain >= 0（越改越好，回归率可控）
- Metric: Proposal Acceptance / Patch Success / Regression Rate / Rollback Rate / Net Evolution Gain
- Failure Signal: Net Evolution Gain < 0（越改越差）/ 回归引入
- Evidence: cargo test -p zaion-evolve（62 测试）

### 3.6 自治系统（Autonomy Eval）

#### zaion-autonomic (AUT-001)
- Capability: Reflex Response Correctness
- Test Scenario: 不同刺激强度；连续刺激；噪声刺激
- Pass Condition: 刺激→响应准确；反应延迟达标；不过度反应
- Metric: Stimulus-Response Accuracy / Reaction Latency / Overreaction Rate
- Failure Signal: 噪声刺激触发反射 / 过度反应
- Evidence: cargo test -p zaion-autonomic（34 测试）

#### zaion-proprioception (PRP-001)
- Capability: Self-state Awareness
- Test Scenario: 资源下降；依赖失效；环境变化；异常注入
- Pass Condition: 状态估计准确；休克检测召回；误报低
- Metric: State Estimation Accuracy / Shock Detection Recall / False Alarm
- Failure Signal: 资源下降未感知 / 误报触发锁定
- Evidence: cargo test -p zaion-proprioception（42 测试）

#### zaion-metabolic (MET-001)
- Capability: Resource-aware Decision
- Test Scenario: token 超预算；工具昂贵；连续任务；资源枯竭
- Pass Condition: 预算准确；超支被阻断；效用-成本权衡合理
- Metric: Budget Accuracy / Overspend Rate / Task Utility-Cost
- Failure Signal: token 超预算仍继续 / 预算耗尽未停止
- Evidence: cargo test -p zaion-metabolic（62 测试，含预算超支对抗）

#### zaion-curiosity (CUR-001)
- Capability: Exploration ROI
- Test Scenario: 无任务 idle；自主探索；重复探索抑制
- Pass Condition: 新颖产出；有用发现率达标；探索成本可控
- Metric: Novelty Yield / Useful Discovery Rate / Exploration Cost
- Failure Signal: 重复探索无收益 / 探索成本失控
- Evidence: cargo test -p zaion-curiosity（42 测试）

#### zaion-ego (EGO-001)
- Capability: Identity Continuity
- Test Scenario: restart；memory reload；跨 session
- Pass Condition: 身份一致；SoulHash 稳定；人格不漂移
- Metric: Identity Consistency / SoulHash Stability / Persona Drift
- Failure Signal: 重启后人格漂移 / SoulHash 变化
- Evidence: cargo test -p zaion-ego（21 测试）

#### zaion-singularity (SNG-001)
- Capability: Autonomy Coordination
- Test Scenario: metabolic 限制 curiosity；proprioception 触发 watchdog；多系统并发
- Pass Condition: 跨系统稳定；无死锁；无振荡
- Metric: Cross-System Stability / Deadlock Rate / Oscillation Rate
- Failure Signal: 多系统互相打架 / 死锁 / 振荡
- Evidence: cargo test -p zaion-singularity（30 测试）

#### zaion-shadow (SHD-001)
- Capability: Parallel Strategy Value
- Test Scenario: 多策略并发；shadow 与主进程结论不同
- Pass Condition: shadow 效用；预测一致；资源开销可控
- Metric: Shadow Utility / Prediction Agreement / Resource Overhead
- Failure Signal: shadow 与主进程结论冲突无意义 / 资源开销过大
- Evidence: cargo test -p zaion-shadow（42 测试）

#### zaion-watchdog (WDG-001)
- Capability: Fault Detect & Self-heal
- Test Scenario: crash；deadlock；坏配置；错误修复 proposal
- Pass Condition: 检测召回；修复成功；不安全修复率 0；恢复时间达标
- Metric: Detection Recall / Repair Success / Unsafe Repair Rate / Recovery Time
- Failure Signal: 错误修复引入新问题 / 检测漏报
- Evidence: cargo test -p zaion-watchdog（53 测试）

#### zaion-opd (OPD-001)
- Capability: Distillation Fidelity
- Test Scenario: teacher/student 对比；corner case；OOD trajectory
- Pass Condition: 轨迹保真；能力保持；策略不漂移
- Metric: Trajectory Fidelity / Capability Retention / Policy Drift
- Failure Signal: 蒸馏后能力退化 / 策略漂移
- Evidence: cargo test -p zaion-opd（144 测试）

### 3.7 辅助模块

#### zaion-pricing (PRC-001)
- Capability: Cost Estimation Accuracy
- Test Scenario: 多模型；输入输出 token；价格版本切换
- Pass Condition: 价格准确；快照有效；币种正确
- Metric: Price Accuracy / Snapshot Validity / Unit Correctness
- Failure Signal: 跨模型价格算错 / 版本切换后快照失效
- Evidence: cargo test -p zaion-pricing（22 测试）

#### zaion-telemetry (TEL-001)
- Capability: Observability Completeness
- Test Scenario: 高并发；崩溃；flush 前退出
- Pass Condition: 事件丢失率 0；时间戳准确；trace 完整
- Metric: Event Loss Rate / Timestamp Accuracy / Trace Completeness
- Failure Signal: flush 前退出丢事件 / trace 不完整
- Evidence: cargo test -p zaion-telemetry（9 测试）

#### zaion-contract-macros (CM-001)
- Capability: Contract Enforcement
- Test Scenario: 故意违反契约；边界类型；宏嵌套
- Pass Condition: 编译期违规检测；漏报率 0
- Metric: Compile-time Violation Detection / False Negative Rate
- Failure Signal: 故意违规通过编译
- Evidence: cargo test -p zaion-contract-macros（6 测试，含 trybuild compile-fail）

#### zaion-proptest (PRP-001)
- Capability: Property Discovery
- Test Scenario: 随机状态；序列事件；并发状态机
- Pass Condition: bug 发现率；收缩质量；可复现
- Metric: Bug Discovery Rate / Shrinking Quality / Reproducibility
- Failure Signal: 随机状态漏 bug / 收缩结果不可复现
- Evidence: cargo test -p zaion-proptest（25 测试）

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