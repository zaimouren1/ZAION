# 入口链闭环第 2-3 步设计（2026-08-14）

> 前置: 第 1 步完成（wake 内 per-turn token + 工具循环取消检查）

## 架构现实

- wake 执行 = 独立进程（zaion wake 命令），token 在 wake 进程内（第 1 步）
- daemon 注册表（step1）token = daemon 的 SessionActor token——与 wake 的 token 不同进程
- 命令面（CLI/gateway）在第三进程——需统一触发路径

## 统一方案：cancel 文件标记（进程内轮询，零 IPC 依赖）

1. **wake 执行开始**：写 cancel 标记文件 `<data>/turns/<turn_id>.cancel`（内容空），并创建 token
2. **工具循环检查**（第 1 步已做）：is_cancelled() **或标记文件存在** → 中断
3. **命令面**：`zaion turn cancel <turn-id>` / POST /api/v1/turns/cancel → 写该标记文件
4. **wake 清理**：执行结束删除标记文件

## 优点

- 零 IPC/信号依赖（文件即通道）——跨进程简单可靠
- 与 turn_id 绑定（每 turn 独立文件）——并发安全
- 命令面无状态（写文件即取消）

## 实施顺序

1. wake 写/检查/清理标记文件（与 token 检查并行）
2. CLI turn cancel 命令（写标记文件）
3. gateway cancel 路由（Bearer 认证，写标记文件）
4. 集成测试（in-flight wake + cancel 文件 → 工具循环中断）

## 风险

| 风险 | 缓释 |
|---|---|
| 文件轮询延迟 | 工具循环每轮检查（毫秒级） |
| 残留文件 | wake 结束删除 + 启动时清理 stale |
| 安全 | 标记文件在 data 目录（本地）· gateway 路由认证 |