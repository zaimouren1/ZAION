制定日期: 2026-03-31
架构代号: ZAK (Zaion Agentic Kernel)
核心战略: 抛弃线性追赶，实施非对称降维打击。用 Rust 做绝对安全的底层引擎，用嵌入式沙箱直接“吞噬” OpenClaw 的 30万行 TypeScript 生态。
战略调整备忘录 (To Dev Engineer)
你的 Phase 7 (配对) 和 Phase 10 (7层记忆) 是绝对的天才设计，本蓝图全量保留并升格为核心战役。
停止手写 Channel 适配器 (原 P3) 和原生 Skills (原 P8)。那是一个永远填不满的黑洞。我们将通过引入 deno_core (V8 沙箱) 直接兼容执行 OpenClaw 的 .ts 脚本。
测试策略变更 (原 P12)：不要人肉写 1000 个单元测试。引入 proptest 进行基于属性的模糊测试（Property-based Fuzzing），在数学层面证明安全性。
核心战役重构：五大降维打击阶段 (The 5 Master Campaigns)
Campaign I: 极速内核与密码学基石 (融合原 P1, P2, P4)
目标: 构建支撑单机 10,000 个并发 Agent 的底层调度与加密设施，证明 Rust 对 Node.js 的性能与安全双重碾压。
1.1 异步状态机与 SQLite WAL 调优：
重构 TaskEngine 为纯粹的 Tokio 状态机。引入 mpsc::channel 和 SQLite WAL 模式的 Batch 异步落盘。
验收: 瞬间 spawn 10,000 个休眠 Agent，内存 < 50MB。
1.2 绝对凭证系统 (原 P2 强化)：
保留你设计的 zaion secrets CLI 族。
升维点: 所有 Secret 的增删改查不仅加密存储，必须由 Principal 的 Ed25519 签名后写入 Ledger，实现100%可溯源。
1.3 时间轮 Cron 与 流式输出 (原 P1, P4)：
保留 zaion wake --stream 和 typing 状态机制。
将 CronEngine 底层替换为基于 tokio 的时间轮算法 (Time Wheel)，Cron 的每一次触发必须附带哈希指针写入账本。
Campaign II: 7层心智模型与上下文分页 (保留并升格原 P10)
目标: 完整实现你的 7 层记忆设计，这是 Zaion 超越 OpenClaw 粗糙 RAG 的终极杀手锏，将其视作操作系统级的“内存分页与置换 (Memory Paging)”。
2.1 纯 Rust 向量引擎：引入 usearch crate，保持单机极简，不依赖外部向量数据库。
2.2 记忆流转与签名绑定：
Layer 4 (Episodic): 直接映射为只读的 SQLite Ledger Event 流。
Layer 6 (Principal): 记忆数据强制与 Ed25519 Keypair 绑定，验证通过后才允许跨进程/跨设备反序列化。
2.3 Token Budget 调度器：
实现 ContextEngine 的核心算法：在给定预算下（如 8k tokens），自动按权重从 7 个层级“置换”出最相关上下文。
验收: zaion memory semantic-search 在 10万条模拟记忆中 <50ms 返回结果。
Campaign III: 特洛伊沙箱 (替换原 P3, P5, P8 - 核心突变)
目标: 用 1,000 行核心代码，直接“偷取” OpenClaw 现存的 50+ Skills 和 10+ Channels，不写一行重复的业务逻辑。
3.1 嵌入式 Deno 运行时 (deno_core)：
在 Zaion Rust 中实例化轻量级 V8 Isolate 沙箱（启动 <5ms）。
设计 SkillRunner trait，能够直接读取并解释 OpenClaw 标准的 .ts 或 .js 文件。
3.2 I/O 劫持与内生 Harness：
沙箱内 禁止 直接访问网络和文件系统。
TS 脚本调用的 fetch 或 fs 必须通过 Rust 的 op_call 桥接回 Zaion Core。Rust 在放行前，进行危险工具扫描 (原 P9)，并将操作签名写入 Ledger。
验收: zaion skill run ./openclaw-skills/web-search.ts 完美运行，且整个执行轨迹被记录在 Rust 层的加密账本中。
Campaign IV: 零信任网络与联邦通信 (融合原 P6, P7)
目标: 抛弃 OpenClaw 低效的子进程管道，建立真正的 Agent 间联邦协议 (A2A Protocol)。
4.1 Ed25519 设备配对 (保留原 P7)：
完美执行你设计的 zaion pair code 挑战-响应机制，建立 Agent 间的外交信任。
4.2 降维 ACP 协议 (优化原 P6)：
实现完整的 Agent Client Protocol (ACP)。
升维点 (Local Fast-path): 当检测到两个 Agent 在同一台笔记本上时，底层通信自动从 HTTP 切换为 Unix Domain Sockets (UDS)，实现微秒级的二进制序列化通信。
验收: 两个本地进程的 zaion agent spawn 任务委托，通信延迟 < 1ms。
Campaign V: 绝对控制台与数学级验证 (融合原 P9, P11, P12)
目标: 提供极致的 Hacker 级终端体验，并用数学方法替代枯燥的手工测试。
5.1 黑客帝国 TUI (保留原 P11)：
使用 ratatui 实现酷炫的终端。重点展示万级并发下的实时 Ledger 追加瀑布流，视觉上直接碾压 Web UI。
5.2 安全扫描与审计 (保留原 P9)：
实现 zaion security audit-trail，不仅仅是看日志，而是能够执行密码学 Replay（重放），通过验证所有数字签名来证明系统未被篡改。
5.3 生成式属性测试 (替代原 P12)：
不写 1000 个手工测试。引入 proptest crate。
编写“不变性断言”（例如：无论输入什么奇葩的字符组合，Layer 4 追加到账本后，其 Hash 校验函数必须返回 True）。让测试引擎每秒生成数万个 Case 自动轰炸系统。
验收: 50 个核心 Property Tests 覆盖 95% 以上的核心逻辑分支。
工作量与排期优化预估
由于砍掉了海量的业务适配器开发（交由 Deno 沙箱白嫖），整体核心代码量将更极致、更紧凑：
Campaign	核心模块 / 新增代码量 (估算)	对比原计划
C1: 极速内核与密码学	Tokio 状态机 / ~800 行	更深、更稳
C2: 7层心智模型	usearch + 调度器 / ~1200 行	保持原样
C3: 特洛伊沙箱	deno_core 桥接 / ~1500 行	省去 3000 行低效代码
C4: 零信任通信	UDS + ACP / ~1000 行	性能提升 100 倍
C5: TUI 与数学验证	ratatui + proptest / ~1500 行	省去 2000 行无效测试
总计	约新增 6,000 行核心代码	工作量减半，威慑力翻倍
