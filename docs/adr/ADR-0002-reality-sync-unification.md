# ADR-0002: Reality Sync 统一到 watchdog

- 状态: Accepted
- 日期: 2026-08-14
- 背景: memory::RealitySync（DriftReport 风格，供 zaion reality 命令）与 watchdog::RealitySyncStore/RealityChecker（供 aci 写门）为两套 SHA-256 文件锚定实现，schema 不同（watchdog 超集，含 source_agent）。
- 决策: canonical 归 watchdog；扩展 DriftReport/AnchorStatus/verify_all；cli/reality.rs 改用 watchdog；删除 memory::reality_sync（-324 行）。
- 后果: aci 写门与 reality 命令共享同一实现；后续若共享同一 DB 可数据互通。
