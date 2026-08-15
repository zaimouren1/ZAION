# ADR-0005: 依赖政策——ratatui 0.30 与 audit 基线

- 状态: Accepted
- 日期: 2026-08-14
- 背景: cargo audit 4 个告警。ratatui 0.30 弃用 lru/paste，升级后 audit 4→2（清除 lru unsound + paste unmaintained）。剩余 bincode/yaml-rust 来自 syntect 5.3.0（无新版）。
- 决策: 依赖升级优先于豁免；接受"未维护"类告警需记录理由与迁移路径。syntect 迁移（替换或等升级）列为 M1 项。
- 后果: audit 基线 2 个已知豁免（bincode/yaml-rust，均 syntect 传递依赖）。
