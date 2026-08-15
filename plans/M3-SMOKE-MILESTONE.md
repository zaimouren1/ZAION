# M3 冒烟里程碑：产品内 hero mission 成功（2026-08-14）

## 完整链路（已验证）

1. onboard（pipe wizard）→ principal + anthropic 配置
2. provider 配置（ANTHROPIC_API_KEY/BASE_URL——x-api-key 原生兼容）
3. 工具格式适配（自定义端点 openai-compat tools）
4. 工具循环（per-turn token + cancel marker 修复——create→cleanup）
5. **多轮工具消息**（openai 风格 assistant/tool + reasoning_content 回传）
6. **hero 修复任务**：真实 LLM 诊断 3 缺陷 → 工具修复 → cargo test 6/6 → **0.5min**

## M3 门禁对照

| 门禁 | 状态 |
|---|---|
| 英雄任务首任务 < 15 分钟 | ✅ 0.5min |
| 成功任务 100% 可验证 | ✅ cargo test 6/6 |
| 真实 PTY/仓库任务 E2E | ✅ 产品内真实执行 |
| 8-12 名设计伙伴 | ⏳ 外部（待招募） |

## M3 启动清单（剩余）

1. **设计伙伴招募**（8-12 名——外部）
2. **更多 hero 场景验证**（SRE/恢复等——可选）
3. **工具集策略**（hero 模式精简工具——已验证方向）
4. **产品化**（配置引导/TUI 呈现——可选）

## 结论

M3 的技术能力闭环（产品内真实 hero 执行）**已打通**——剩余为外部依赖（设计伙伴）与扩展验证。

## 扩展验证（第 162-163 轮）——多场景 hero 矩阵

| 场景 | 结果 | 耗时 |
|---|---|---|
| 代码修复（sandbox 3 缺陷） | ✅ cargo 6/6 | 0.5min |
| SRE 配置修复（端口/阈值） | ✅ service.py 遵守 config | ~1min |

## 第 4 场景（第 164 轮）——安全/验证 hero

| 验证 | 结果 |
|---|---|
| 收据验证（签名检测） | ✅ report：valid 1 + tampered 1（computed vs stored 正确） |
| python 执行 | ✅ shell allow-list 扩展（python/python3） |
| 耗时 | 0.6min |

**完整 hero 矩阵（4 场景）**：代码修复 / SRE 配置 / 崩溃恢复 / 安全验证——全部产品内成功。

## M3 技术侧完成声明（第 165 轮）

M3 的技术能力闭环全部验证：产品内真实 hero 执行（4 类任务）+ 工具链完整（fs/shell/git/cargo/python）+ 门禁达标（首任务 <15min + 100% 可验证）。**剩余为外部依赖（设计伙伴招募）与产品化扩展**。

| 崩溃恢复（journal 应用） | ✅ items 更新 + committed 标记 | 1.2min |

**工具循环上限**：MAX_NATIVE_TOOL_TURNS 8→24（复杂任务完成所需）。
