# ADR-0003: 孤儿模块删除策略（验证优先 + 全目标编译兜底）

- 状态: Accepted
- 日期: 2026-08-14
- 背景: pub 项提取式孤儿检测有 4 类盲区（通配符再导出、超行范围 pub 项、测试目标使用、phase8b proof 路径）。3 次误删（agui/applier/codegen）均被验证网拦截并 git 恢复。
- 决策: 删除孤儿必须 (1) 检查模块名 + 再导出名 + 全仓含测试引用 (2) 检查 phase8b proof 路径与证据门字符串 (3) 删除后跑 cargo check --workspace --all-targets 兜底。
- 后果: 删除 14 个模块 -3,629 行全部验证绿；误删恢复机制（git checkout）验证有效。
