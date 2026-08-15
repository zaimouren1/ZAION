# M2b SessionActor 设计（基于现有 turn_store actor 概念）

> 日期: 2026-08-14 | 对应计划 M2: 统一 SessionActor、状态机、outbox、真实取消

## 1. 现有基础（勘察）

| 组件 | 角色 |
|---|---|
| DurableTurnStore + TurnActorIdentity | turn 持久化 + actor 键（for_ingress 构造） |
| DurableTurnAdmission / TurnOutboxRecord / TurnOutboxStatus | admission + outbox 记录 |
| turn_state.rs（8.4K） | turn 状态（terminal_state 等） |
| CancelToken（M2c 新） | 取消链原语（pid tree-kill） |
| UnifiedAgentRuntime | 当前主运行路径 |

## 2. SessionActor 定义

SessionActor = 一个 session 的状态转换所有者，满足：
- **单一所有权**：一个 session 同一时刻只有一个 actor 处理（TurnActorIdentity 天然支持）
- **状态机**：turn_state 的状态转换集中在此（accepted → running → terminal）
- **outbox 持久化**：accepted turn 先写 outbox 再执行（零丢失）；执行完标记 outbox 完成（零双终态）
- **真实取消**：持有 CancelToken，cancel 时击杀关联子进程（M2c 已就绪）

## 3. 状态转换

```
        ┌─ accept(admission) ─→ outbox.pending ─→ outbox.leased ─┐
entry ──┤                                                    ├─→ executing ─→ terminal
        └─ (reject/duplicate via idempotency key)              │
                                                               └─ cancel(kill) ─→ cancelled
```

关键不变量:
1. accepted turn 写入 outbox（pending）后才返回 ack——崩溃后可从 outbox 恢复（零丢失）
2. 执行完成才标记 outbox done——崩溃恢复时 pending/leased 的 turn 重放或标记失败（零双终态）
3. 每个 turn 有 idempotency key（skills 幂等模式已示范）
4. cancel 只作用于当前 executing 的 turn（不取消已 terminal 的）

## 4. 与 M2a/M2c 的关系

- M2a（turn contract）: SessionActor 是契约的执行主体（入口 → contract → SessionActor → outbox）
- M2c（取消）: SessionActor 持有 CancelToken，执行阶段传入工具链
- 复用: TurnActorIdentity（actor 键）、DurableTurnStore（outbox）、turn_state（状态）

## 5. 实施步骤

| 步骤 | 内容 | 验证 |
|---|---|---|
S1 | SessionActor 骨架：new(actor_identity, store, cancel) + begin_turn/complete_turn | 单测：admission→executing→terminal |
S2 | outbox 协议：pending→leased→done；崩溃恢复测试 | failpoint：在 outbox 各状态杀进程后恢复 |
S3 | cancel 集成：executing 时 cancel → kill + 标记 cancelled | 复用 cancel 延迟测试（p95 235ms） |
S4 | 接入 UnifiedAgentRuntime（替换其内部编排） | runtime 467 测试回归 |

## 6. 退出指标（对照 M2 门禁）

---

## S4 接入分析（2026-08-14）——接入点修正

**勘察发现**:
- UnifiedAgentRuntime **不使用** DurableTurnStore（主路径内部无持久化 turn 引用）
- turn store 实际在 **daemon 层**（daemon.rs L240/L1470+ 每 principal 打开）+ wake_contract_v2（契约路径）
- 架构 = daemon → turn_store(进程级) → UnifiedAgentRuntime(执行)

**S4 重新定位**:
- 接入点 = daemon/wake 层（那里已经打开 turn store）
- SessionActor 作为 store 的**统一包装**（幂等 begin_turn + outbox + cancel）替代散落的 open/begin 调用
- UnifiedAgentRuntime 内部不需要引用 store——保持其纯执行角色（职责分离）

**实施注意**:
- daemon.rs / wake.rs 是证据门锁定文件（cli_stable_surface 读取）——改动需同步证据断言
- 建议：S4 作为独立工作线，先改 daemon 层的 store 使用点（逐点换 SessionActor），每点全量测试验证


1. accepted turn 零丢失（outbox 崩溃恢复矩阵）
2. 零双终态（同 idempotency key 不重复执行）
3. cancel p95 < 250ms（已实测 235ms）
4. 单 session 单 actor（并发断言）