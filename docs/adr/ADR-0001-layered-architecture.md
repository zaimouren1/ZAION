# ADR-0001: 代码库为深度分层架构，合并目标需验证优先

- 状态: Accepted
- 日期: 2026-08-14
- 背景: 早期审计基于文件大小/名称相似，将 Telegram(13K 行)、webhook(9K 行)、agent loop(5 套)、session_store 判为"重复实现"，计划合并 -20K 行。
- 决策: 逐对深度验证后确认这些是**分层架构**（adapter/bridge/facade 模式），非重复。合并目标撤销。仅真正孤儿（memory_agent_loop/skill_catalog 等 14 个模块，-3,629 行）被删除。
- 后果: 未来的"单一所有权"迁移（跃迁计划 M2）必须**先验证再动**，禁止基于表面相似性的合并。
