# ADR-0004: 证据门同步维护流程（M2 迁移的前置）

- 状态: Accepted
- 日期: 2026-08-14
- 背景: 代码库有繁重 source-gate 测试（cli_stable_surface 139 测试/68 锁定文件、phase8b 95 个 proof 路径、system.rs 45+ 字符串断言）。本会话 3 次被证据门拦截（session.rs proof 路径、webhook needle、cmd_onboard 边界）。
- 决策: 任何涉及锁定文件的重构必须"先更新证据断言，再改生产代码，最后跑全量测试"。证据门清单见 plans/evidence-gate-inventory.md。
- 后果: M2 迁移依赖图已建立；建议未来将证据门从字符串断言升级为契约文件（单一 source of truth）。
