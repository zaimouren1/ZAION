# 入口链完整闭环设计（2026-08-14）

> 目标: cancel 从命令面到工具执行的完整链路（避免孤儿 token）

## 组件现状

| 组件 | 状态 |
|---|---|
| CancelToken（pid 树-kill） | ✅ runtime |
| SessionActor cancel 传播 | ✅ S1-S3 |
| CodeExecutor with_cancel | ✅ step2-1 |
| daemon 注册表 | ✅ step1（token 已注册） |
| 产品工具循环 cancel 检查 | 📋 实施点确认（execute_native_tool_calls） |
| **cancel 命令面** | ❌ 缺失 |
| **进程间通道**（daemon→wake） | ❌ 缺失 |

## 闭环设计

1. **命令面**：`zaion turn cancel <turn-id>`（CLI）+ POST /api/v1/turns/cancel（gateway，Bearer 认证）
2. **进程间通道**：wake 执行时注册 pid + turn_id 到 daemon（现有 IPC/文件信号），命令面触发 daemon → 查注册表 → 向 wake 进程发信号（或写 cancel 标记文件）
3. **wake 内**：token 创建（execute_wake 开头）→ 注册到 daemon → 工具循环检查 is_cancelled → 中断 + 标记 cancelled
4. **持久化**：turn 状态 → Aborted(cancelled)（turn store transition）

## 实施顺序（工作线）

1. execute_native_tool_calls 加 cancel 参数 + 工具循环检查（wake 内）——独立可测
2. turn cancel 命令面（CLI/gateway）——触发 daemon 注册表
3. 进程间通道（pid 注册 + 信号/IPC）——daemon→wake
4. 集成测试：in-flight turn → cancel → 工具中断 + 子进程击杀 + 状态 aborted

## 决策

完整闭环是独立工作线（3-4 步，跨 daemon/wake/gateway）。单独实施 step2-2（wake 内 token 无命令面）会产生孤儿 token——建议工作线内连贯实施。