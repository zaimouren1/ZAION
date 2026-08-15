# ADR-0006: 发布仓库真相——zaimouren1/ZAION

- 状态: Accepted
- 日期: 2026-08-14
- 背景: README/安装脚本宣传 zaion-ai/zaion-rust（404 不存在）。实测真实仓库为 zaimouren1/ZAION（旧 Python 版，已删除）。
- 决策: 建立新私有仓库 github.com/zaimouren1/ZAION，推送完整基线（main @ 8d4d2a5）。修正 11 处幻影地址。凭证（gh keyring + Windows Credential Manager，user zaimouren1）已验证可用。
- 后果: P0#6（无 remote/发布来源未验证）解决。M1 发布真相有真实基础。
