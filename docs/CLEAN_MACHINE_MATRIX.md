# 干净机器安装/回滚矩阵（M1 门禁）

> 对应计划 M1 硬门禁: "干净机器安装/回滚全过"

## 场景（每平台 × 每安装方式）

| 场景 | 步骤 | 退出指标 |
|---|---|---|
| 干净安装 | 全新 HOME → install → zaion --version + 首次运行 | 无残留；版本正确；home 创建 |
| 升级 | v1 → 重新 install（v2）→ 验证状态保留 | 用户状态不丢；新版本生效 |
| 卸载 | 移除 binary + 配置 | 无任何 zaion 残留（binary/config/data） |
| 回滚 | 升级后回滚到 v1 | 已知良好版本恢复；状态完整 |

## 平台矩阵

| 平台 | 安装路径 | 状态 |
|---|---|---|
| Linux x86_64 | install.sh + source fallback | 🟡 脚本就绪（容器 CI 执行） |
| macOS Intel/Apple Silicon | install.sh + homebrew | 🟡 待 CI |
| Windows x86_64 | install.ps1 + winget | 🟡 待 CI |

## 执行

```sh
# 源码安装（需要可 fetch 的仓库）
sh scripts/clean-machine-matrix.sh --from-source

# 预构建二进制
sh scripts/clean-machine-matrix.sh --binary=/path/to/zaion
```

## 状态

矩阵脚本已就绪（fresh-home 检查 → 安装 → 冒烟 → 升级模拟 → 卸载 → 回滚模拟）。
真实执行需干净容器/VM（CI 环境）。当前 release 无预构建二进制（仓库刚建立），源码路径需完整构建。
