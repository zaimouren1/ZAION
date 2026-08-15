# Zaion 功能决策清单

**日期:** 2026-06-03  
**目的:** 帮助你决定哪些功能保留、优先开发、或延后/删除

---

## 📊 决策矩阵总览

| 类别 | 模块数 | 状态 | 与"主动性"相关度 | 建议 |
|------|--------|------|------------------|------|
| 🤖 自主性系统 | 6 | Experimental | ⭐⭐⭐ 最高 | **立即优先开发** |
| 🔧 自我进化 | 2 | Experimental | ⭐⭐⭐ 最高 | 保留并完善 |
| 🎯 核心运行时 | 5 | Stable | ⭐⭐ 高 | 维护现状 |
| 🔌 多渠道通信 | 3 | Stable/Beta | ⭐⭐ 高 | 维护现状 |
| 🔒 安全与密码学 | 3 | Stable | ⭐⭐ 高 | 维护现状 |
| 🧠 AI/LLM 系统 | 5 | Stable/Beta | ⭐⭐ 高 | 维护现状 |
| 🛠️ 基础设施 | 7 | Beta | ⭐ 中 | 维护现状 |
| 🔬 实验性功能 | 3 | Experimental | ⭐ 低 | 评估后决定 |

---

## 🎯 核心决策：你需要决定的 3 件事

### 决策 1：自主性系统（Systems I-V）的优先级

**这 6 个模块是 Zaion 的核心差异化特性**，但目前都是 Experimental 状态。

| 系统 | 功能 | 当前状态 | 你的选择 |
|------|------|----------|----------|
| **System I: Ego** | 人格清单，定义 agent 的"性格" | 实现但缺少测试 | □ 立即完善<br>□ 延后<br>□ 删除 |
| **System II: Autonomic** | 反射式响应，零 token 快速反应 | 实现但缺少测试 | □ 立即完善<br>□ 延后<br>□ 删除 |
| **System III: Proprioception** | 硬件指纹，检测"移植冲击" | 实现但缺少测试 | □ 立即完善<br>□ 延后<br>□ 删除 |
| **System IV: Metabolic** | Token 预算，"饥饿感"驱动 | 实现但缺少测试 | □ 立即完善<br>□ 延后<br>□ 删除 |
| **System V: Curiosity** | **空闲触发器，主动发起对话** | **这是核心功能！** | □ 立即完善<br>□ 延后<br>□ 删除 |
| **Singularity** | 统一编排以上 5 个系统 | 实现但缺少测试 | □ 立即完善<br>□ 延后<br>□ 删除 |

**我的建议:**
- **必须保留:** System V (Curiosity) + Singularity — 这是"主动性"的核心
- **建议保留:** System I (Ego) — 定义 agent 个性
- **可选:** System II, III, IV — 有趣但不是核心功能
- **立即行动:** 为 Curiosity 和 Singularity 添加测试和文档

---

### 决策 2：自我进化系统的范围

Zaion 有 2 个自我进化模块，但都很庞大。

| 模块 | 功能 | 代码量 | 你的选择 |
|------|------|--------|----------|
| **zaion-evolve** | 扫描代码 → LLM 提议改进 → Trinity 审查 | 4,395 LOC | □ 全功能保留<br>□ 仅保留扫描器<br>□ 延后开发<br>□ 删除 |
| **zaion-opd** | On-Policy Distillation：从轨迹中学习 | 8,314 LOC | □ 全功能保留<br>□ 简化实现<br>□ 延后开发<br>□ 删除 |

**我的建议:**
- **zaion-evolve:** 保留，这是"自我改进"的可见证据
- **zaion-opd:** 可以延后（复杂度高，短期价值不明显）

---

### 决策 3：多渠道支持的范围

Zaion 支持 9+ 个通信渠道，但只有 Telegram 是生产级。

| 渠道 | 代码量 | 状态 | 你的选择 |
|------|--------|------|----------|
| **Telegram** | 13,000 LOC | Stable，功能完整 | □ 继续维护<br>□ 冻结功能 |
| **Discord** | ~1,500 LOC | Beta | □ 继续开发<br>□ 冻结<br>□ 删除 |
| **Slack** | ~1,200 LOC | Beta | □ 继续开发<br>□ 冻结<br>□ 删除 |
| **Email** | ~1,200 LOC | Beta | □ 继续开发<br>□ 冻结<br>□ 删除 |
| **SMS** | ~800 LOC | Beta | □ 继续开发<br>□ 冻结<br>□ 删除 |
| **Signal/Matrix/WhatsApp** | 各 ~500 LOC | Beta | □ 继续开发<br>□ 冻结<br>□ 删除 |

**我的建议:**
- **保留:** Telegram（已成熟）
- **冻结但不删除:** Discord, Slack, Email（已有代码，不占维护成本）
- **延后:** SMS, Signal, Matrix, WhatsApp（价值不高）

---

## 📋 完整功能清单（按优先级）

### 🔴 Priority 1: 必须立即开发（核心差异化）

| 模块 | 功能 | 当前问题 | 需要做什么 |
|------|------|----------|-----------|
| **zaion-curiosity** | 主动对话触发器 | 无测试，无文档 | 添加测试 + 写文档 `docs/PROACTIVE_BEHAVIOR.md` |
| **zaion-singularity** | 编排 Systems I-V | 无测试 | 添加集成测试 |
| **zaion-ego** | Agent 人格定义 | 无测试 | 添加测试 + 示例 ego.toml |
| **README.md** | 项目首页 | 未体现"主动性"特色 | 重写，加入演示 GIF |
| **CLI onboarding** | 新用户入门 | 流程复杂 | 实现 `zaion quickstart` 命令 |

**预计时间:** 2-4 周  
**成功标准:** 用户可以在 5 分钟内体验主动对话

---

### 🟠 Priority 2: 应该开发（完善核心）

| 模块 | 功能 | 当前问题 | 需要做什么 |
|------|------|----------|-----------|
| **zaion-evolve** | 自我进化引擎 | 无 doctor 检查 | 添加测试 + doctor 检查 |
| **zaion-autonomic** | 反射式响应 | 无测试 | 添加测试 |
| **zaion-proprioception** | 硬件指纹 | 无测试 | 添加测试 |
| **zaion-metabolic** | Token 预算 | 无测试 | 添加测试 |
| **zaion-a2a + federation** | Agent 间通信 | 未集成 | 集成测试 |

**预计时间:** 1-2 个月  
**成功标准:** Systems I-V 全部升级到 Beta

---

### 🟡 Priority 3: 可以延后（改进体验）

| 模块 | 功能 | 当前状态 | 建议 |
|------|------|----------|------|
| **zaion-opd** | On-Policy Distillation | 实现但很复杂 | 拆分为子模块，或延后开发 |
| **zaion-cli 重构** | CLI 命令模块化 | 97.5K LOC 单体 | 拆分为子 crate（非紧急） |
| **zaion-federation** | 联邦学习 | 未测试 | 等 a2a 成熟后再集成 |
| **Discord/Slack/Email** | 额外渠道 | Beta 状态 | 保持现状，不投入资源 |

**预计时间:** 3-6 个月  
**优先级:** 低于 Priority 1 和 2

---

### 🟢 Priority 4: 可选/评估（价值不明确）

| 模块 | 功能 | 当前状态 | 建议 |
|------|------|----------|------|
| **zaion-enclave** | 软件 TEE | 占位符 | 除非有真实硬件 TEE，否则删除 |
| **zaion-proptest** | 属性测试 | 实验性 | 低优先级，可延后或删除 |
| **SMS/Signal/Matrix/WhatsApp** | 小众渠道 | Beta 状态 | 冻结，不投入资源 |

**建议:** 评估后决定是否删除

---

## 🎯 推荐的 3 个月路线图

### 月 1: 自主性核心（Curiosity + Singularity）

**目标:** 让用户能开启"主动模式"并体验到 agent 主动发起对话

- Week 1-2: 
  - 为 zaion-curiosity 添加集成测试
  - 实现 `zaion curiosity enable/disable/status` 命令
  - 编写 `docs/PROACTIVE_BEHAVIOR.md` 文档
  
- Week 3-4:
  - 为 zaion-singularity 添加集成测试
  - 实现 `zaion singularity status` 命令
  - 更新 README，加入主动对话演示

**里程碑:** 用户可以运行 `zaion curiosity enable`，然后 agent 会在空闲时主动发消息

---

### 月 2: 完善自主性系统（Ego + Autonomic + Metabolic + Proprioception）

**目标:** 让所有 Systems I-V 都有测试和文档

- Week 5-6:
  - 为 zaion-ego 添加测试
  - 提供示例 ego.toml 配置文件
  - 实现 `zaion ego show/edit` 命令

- Week 7-8:
  - 为 zaion-autonomic, zaion-metabolic, zaion-proprioception 添加测试
  - 实现 doctor 检查
  - 将所有 6 个模块升级到 Beta

**里程碑:** Systems I-V 全部 Beta，doctor 检查通过

---

### 月 3: 入门体验 + 自我进化

**目标:** 简化入门流程，展示自我进化能力

- Week 9-10:
  - 实现 `zaion quickstart` 命令（5 分钟体验主动对话）
  - 录制演示 GIF
  - 重写 README

- Week 11-12:
  - 为 zaion-evolve 添加测试和 doctor 检查
  - 实现 `zaion evolve scan/propose/apply` 命令
  - 展示"agent 发现自己的代码问题并自动修复"

**里程碑:** 
- 新用户 5 分钟内体验主动对话
- 展示自我进化能力

---

## ✅ 立即行动项（本周）

请在这个清单上打勾，告诉我你的决定：

### 自主性系统（Systems I-V）
- [ ] **System V (Curiosity)** — 主动对话核心 → **必须保留**
- [ ] **Singularity** — 编排器 → **必须保留**
- [ ] **System I (Ego)** — 人格定义 → □ 保留 □ 延后 □ 删除
- [ ] **System II (Autonomic)** — 反射响应 → □ 保留 □ 延后 □ 删除
- [ ] **System III (Proprioception)** — 硬件指纹 → □ 保留 □ 延后 □ 删除
- [ ] **System IV (Metabolic)** — Token 预算 → □ 保留 □ 延后 □ 删除

### 自我进化系统
- [ ] **zaion-evolve** (4.4K LOC) → □ 全功能保留 □ 简化 □ 延后
- [ ] **zaion-opd** (8.3K LOC) → □ 全功能保留 □ 简化 □ 延后

### 多渠道支持
- [ ] **Telegram** → **必须保留**（已生产级）
- [ ] **Discord/Slack/Email** → □ 继续开发 □ 冻结 □ 删除
- [ ] **SMS/Signal/Matrix/WhatsApp** → □ 继续开发 □ 冻结 □ 删除

### 实验性功能
- [ ] **zaion-enclave** (软件 TEE) → □ 保留 □ 删除
- [ ] **zaion-proptest** (属性测试) → □ 保留 □ 删除
- [ ] **zaion-federation** (联邦学习) → □ 立即开发 □ 延后

### 是否接受 3 个月路线图
- [ ] **接受** — 月1: Curiosity, 月2: Systems I-V, 月3: 入门+进化
- [ ] **修改** — 我想调整优先级：_______

---

## 📝 下一步

等你填完这个清单，我会：
1. 删除你标记为"删除"的模块
2. 制定详细的开发计划
3. 开始实现 Priority 1 项目
4. 每周报告进度

**你准备好了吗？告诉我你的选择！**
