//! Contextual first-touch onboarding hints.
//!
//! 翻译自 Hermes `agent/onboarding.py`，并按 Zaion 特性二次优化。
//!
//! 设计理念：不用首次运行问卷阻塞用户，而是在用户**第一次**撞上某个行为分叉点
//! （运行中发消息、首个长耗时工具、检测到遗留目录等）时，显示一次性提示。每条
//! 提示每个安装只显示一次（状态记录在 `config.toml` 的 `[onboarding].seen.<flag>`），
//! 之后永不再现。
//!
//! 与 Hermes 的差异（Zaion 二次优化）：
//!   - **主动性钩子**：新增 `curiosity_first_idle` 与 `evolve_first_suggestion`
//!     两个 Zaion 独有的提示分叉点，对应 System V（好奇心/空闲触发）与 evolve
//!     自我进化模块——这是 Zaion 区别于 Hermes 的核心 agentic 特性。
//!   - **多渠道感知**：`busy_input_hint_*` 区分 CLI / gateway（多渠道）两套文案。
//!   - **状态时间戳**：`OnboardingState` 额外记录每个提示的首触时间，供主动性
//!     系统评估用户熟练度（见 `config.rs`）。
//!
//! 本模块保持轻量、零重依赖，CLI 与未来的 gateway 都能直接引用。
//!
//! 接入状态（2026-06-09）：
//!   - `OPENCLAW_RESIDUE_FLAG` 已接入默认启动路径（`launcher::cmd_default_launch`）。
//!   - 其余分叉点（busy-input / tool-progress / curiosity / evolve）是**前瞻性
//!     公共 API**：对应的 `/busy`、`/verbose` 队列机制与 curiosity/evolve 主动触发
//!     钩子尚未在 Zaion 落地，待这些功能接入时再挂载。故标注 `#![allow(dead_code)]`
//!     —— 它们已有完整测试覆盖，是稳定契约而非废弃代码。
#![allow(dead_code)]

use crate::config::ZaionConfig;

// ── Flag 名称（稳定 — 用作 config.toml 中 [onboarding].seen 的键）────────────────

/// 用户运行中发消息时第一次触发。
pub const BUSY_INPUT_FLAG: &str = "busy_input_prompt";
/// 首个长耗时工具流式输出时第一次触发。
pub const TOOL_PROGRESS_FLAG: &str = "tool_progress_prompt";
/// 检测到遗留 OpenClaw 工作区时第一次触发。
pub const OPENCLAW_RESIDUE_FLAG: &str = "openclaw_residue_cleanup";

// ── Zaion 独有分叉点（二次优化新增）─────────────────────────────────────────────

/// System V（curiosity）首次空闲主动发起对话时触发。
pub const CURIOSITY_FIRST_IDLE_FLAG: &str = "curiosity_first_idle";
/// evolve 模块首次给出自我进化建议时触发。
pub const EVOLVE_FIRST_SUGGESTION_FLAG: &str = "evolve_first_suggestion";

// ── busy-input 文案 ─────────────────────────────────────────────────────────────

/// gateway（多渠道）版：用户在 agent 忙碌时发消息后第一次显示。
///
/// `mode` 是刚刚生效的 busy_input_mode，让文案与实际行为一致
/// （"我刚打断了…" vs "我刚排队了…"）。
pub fn busy_input_hint_gateway(mode: &str) -> String {
    match mode {
        "queue" => "💡 首次提示 — 我把你的消息排到了队列，而不是打断当前任务。\
             发送 `/busy interrupt` 让新消息立即停止当前任务，或 `/busy status` 查看状态。\
             此提示只显示一次。"
            .to_string(),
        "steer" => "💡 首次提示 — 我把你的消息注入了当前运行，它会在下一次工具调用后送达，\
             而不会打断当前任务。发送 `/busy interrupt` 或 `/busy queue` 修改此行为，\
             或 `/busy status` 查看状态。此提示只显示一次。"
            .to_string(),
        _ => "💡 首次提示 — 我刚打断了当前任务来回答你。\
             发送 `/busy queue` 让后续消息排队到当前任务之后，`/busy steer` 在不打断的\
             情况下中途注入，或 `/busy status` 查看状态。此提示只显示一次。"
            .to_string(),
    }
}

/// CLI 版 busy-input 提示（纯文本，无 markdown）。
pub fn busy_input_hint_cli(mode: &str) -> String {
    match mode {
        "queue" => "(提示) 你的消息已排到下一轮。使用 /busy interrupt 让回车立即停止当前运行，\
             或 /busy steer 中途注入。此提示只显示一次。"
            .to_string(),
        "steer" => "(提示) 你的消息已注入当前运行，将在下一次工具调用后送达。\
             使用 /busy interrupt 或 /busy queue 修改此行为。此提示只显示一次。"
            .to_string(),
        _ => "(提示) 你的消息打断了当前运行。使用 /busy queue 让消息排队到下一轮，\
             或 /busy steer 中途注入。此提示只显示一次。"
            .to_string(),
    }
}

// ── tool-progress 文案 ──────────────────────────────────────────────────────────

/// gateway（多渠道）版：首个长耗时工具流式输出后第一次显示。
pub fn tool_progress_hint_gateway() -> String {
    "💡 首次提示 — 那个工具运行了一会儿，我在流式输出每一步。\
     如果进度消息太吵，发送 `/verbose` 循环切换模式（all → new → off）。此提示只显示一次。"
        .to_string()
}

/// CLI 版 tool-progress 提示。
pub fn tool_progress_hint_cli() -> String {
    "(提示) 那个工具运行了较长时间。使用 /verbose 循环切换工具进度显示模式\
     （all -> new -> off -> verbose）。此提示只显示一次。"
        .to_string()
}

// ── OpenClaw 遗留目录横幅 ───────────────────────────────────────────────────────

/// Zaion 首次启动并发现 `~/.openclaw/` 时显示的横幅。
///
/// 优先引导用户运行 `zaion claw migrate`（非破坏性地迁移配置、记忆与技能）。
/// `zaion claw cleanup` 作为已迁移用户归档旧目录的后续步骤被提及——并警告归档
/// 会让 OpenClaw 停止工作。
pub fn openclaw_residue_hint_cli() -> String {
    "在 ~/.openclaw/ 检测到遗留的 OpenClaw 目录。\n\
     要将你的配置、记忆与技能迁移到 Zaion，请运行 `zaion claw migrate`。\n\
     如果你已经迁移并想归档旧目录，运行 `zaion claw cleanup`\
     （会重命名为 ~/.openclaw.pre-migration — 此后 OpenClaw 将停止工作）。\n\
     此提示只显示一次。"
        .to_string()
}

// ── Zaion 独有提示文案（二次优化）──────────────────────────────────────────────

/// System V 首次空闲主动发起对话时显示。
///
/// Zaion 区别于被动型 agent 的核心：好奇心系统会在用户空闲时主动开口。首次发生
/// 时让用户知道这是预期行为，并给出关闭/调节入口。
pub fn curiosity_first_idle_hint_cli() -> String {
    "💡 首次提示 — 刚才是我（Zaion）在你空闲时主动发起的对话，这来自好奇心系统\
     （System V）。如果你不希望我主动开口，运行 `zaion curiosity off`；\
     用 `zaion curiosity status` 查看或调节主动频率。此提示只显示一次。"
        .to_string()
}

/// evolve 模块首次给出自我进化建议时显示。
pub fn evolve_first_suggestion_hint_cli() -> String {
    "💡 首次提示 — 我刚给出了一条自我进化建议，这来自 evolve 模块（持续自我改进）。\
     用 `zaion evolve review` 查看待审建议，`zaion evolve apply` 应用，或 `zaion evolve off` 关闭。\
     此提示只显示一次。"
        .to_string()
}

// ── 遗留目录检测 ────────────────────────────────────────────────────────────────

/// 若 `$HOME` 下存在 OpenClaw 工作区目录则返回 `true`。
///
/// 纯文件系统检查，无副作用。`home` 覆盖参数用于测试。一个名为 `.openclaw` 的
/// 普通文件**不算**工作区（与 Hermes 行为一致）。
pub fn detect_openclaw_residue(home: Option<&std::path::Path>) -> bool {
    let base = match home {
        Some(p) => p.to_path_buf(),
        None => match std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
            Ok(h) => std::path::PathBuf::from(h),
            Err(_) => return false,
        },
    };
    base.join(".openclaw").is_dir()
}

// ── 状态读写（基于 ZaionConfig）─────────────────────────────────────────────────

/// 用户是否已看过某条首触提示。
///
/// 接受任意 `&ZaionConfig`，便于在已加载配置的场景复用，避免重复读盘。
pub fn is_seen(config: &ZaionConfig, flag: &str) -> bool {
    config.onboarding.is_seen(flag)
}

/// 将 `onboarding.seen.<flag> = true` 持久化到磁盘。
///
/// 尽力而为（best-effort）：任何错误都返回 `false`，绝不让提示逻辑破坏主流程。
/// 幂等：已标记的 flag 重复调用不会覆盖其首触时间戳，且仍返回 `true`。
pub fn mark_seen(flag: &str) -> bool {
    let mut cfg = ZaionConfig::load();
    if cfg.onboarding.is_seen(flag) {
        return true; // 已标记 — 无需写盘
    }
    cfg.onboarding.mark_seen(flag);
    cfg.save().is_ok()
}

/// 组合工具：若提示未读则返回其文案并立即落盘标记，否则返回 `None`。
///
/// 这是调用点最常用的入口——一行完成「检查 + 取文案 + 落盘」三步，避免每个
/// 调用点重复样板代码。
pub fn take_hint_once<F>(config: &ZaionConfig, flag: &str, render: F) -> Option<String>
where
    F: FnOnce() -> String,
{
    if is_seen(config, flag) {
        return None;
    }
    let msg = render();
    mark_seen(flag);
    Some(msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::OnboardingState;
    use std::collections::BTreeMap;

    fn cfg_with_seen(pairs: &[(&str, bool)]) -> ZaionConfig {
        let mut seen = BTreeMap::new();
        for (k, v) in pairs {
            seen.insert((*k).to_string(), *v);
        }
        ZaionConfig {
            onboarding: OnboardingState {
                seen,
                first_seen_at: BTreeMap::new(),
            },
            ..Default::default()
        }
    }

    // ── is_seen ────────────────────────────────────────────────────────────
    #[test]
    fn empty_config_unseen() {
        assert!(!is_seen(&ZaionConfig::default(), BUSY_INPUT_FLAG));
    }

    #[test]
    fn seen_missing_flag_unseen() {
        let cfg = cfg_with_seen(&[(TOOL_PROGRESS_FLAG, true)]);
        assert!(!is_seen(&cfg, BUSY_INPUT_FLAG));
    }

    #[test]
    fn seen_flag_true() {
        let cfg = cfg_with_seen(&[(BUSY_INPUT_FLAG, true)]);
        assert!(is_seen(&cfg, BUSY_INPUT_FLAG));
    }

    #[test]
    fn seen_flag_falsy() {
        let cfg = cfg_with_seen(&[(BUSY_INPUT_FLAG, false)]);
        assert!(!is_seen(&cfg, BUSY_INPUT_FLAG));
    }

    #[test]
    fn other_flags_isolated() {
        let cfg = cfg_with_seen(&[(BUSY_INPUT_FLAG, true)]);
        assert!(is_seen(&cfg, BUSY_INPUT_FLAG));
        assert!(!is_seen(&cfg, TOOL_PROGRESS_FLAG));
    }

    // ── OnboardingState::mark_seen 幂等性 ───────────────────────────────────
    #[test]
    fn mark_seen_transitions_then_idempotent() {
        let mut st = OnboardingState::default();
        assert!(st.mark_seen(BUSY_INPUT_FLAG)); // unseen -> seen
        assert!(st.is_seen(BUSY_INPUT_FLAG));
        let stamp = st.first_seen_at.get(BUSY_INPUT_FLAG).cloned();
        assert!(stamp.is_some());
        // 第二次为 no-op，不改时间戳
        assert!(!st.mark_seen(BUSY_INPUT_FLAG));
        assert_eq!(st.first_seen_at.get(BUSY_INPUT_FLAG).cloned(), stamp);
    }

    #[test]
    fn mark_seen_independent_flags() {
        let mut st = OnboardingState::default();
        st.mark_seen(BUSY_INPUT_FLAG);
        st.mark_seen(TOOL_PROGRESS_FLAG);
        assert!(st.is_seen(BUSY_INPUT_FLAG));
        assert!(st.is_seen(TOOL_PROGRESS_FLAG));
        assert!(!st.is_seen(OPENCLAW_RESIDUE_FLAG));
    }

    // ── 文案 ─────────────────────────────────────────────────────────────────
    #[test]
    fn busy_input_gateway_variants_mention_alternatives() {
        let interrupt = busy_input_hint_gateway("interrupt");
        assert!(interrupt.contains("/busy queue"));
        let queue = busy_input_hint_gateway("queue");
        assert!(queue.contains("/busy interrupt"));
        let steer = busy_input_hint_gateway("steer");
        assert!(steer.contains("/busy interrupt"));
        assert!(steer.contains("/busy queue"));
    }

    #[test]
    fn busy_input_cli_variants_mention_alternatives() {
        assert!(busy_input_hint_cli("interrupt").contains("/busy queue"));
        assert!(busy_input_hint_cli("queue").contains("/busy interrupt"));
        let steer = busy_input_hint_cli("steer");
        assert!(steer.contains("/busy interrupt"));
        assert!(steer.contains("/busy queue"));
    }

    #[test]
    fn tool_progress_hints_mention_verbose() {
        assert!(tool_progress_hint_gateway().contains("/verbose"));
        assert!(tool_progress_hint_cli().contains("/verbose"));
    }

    #[test]
    fn all_hints_non_empty() {
        for hint in [
            busy_input_hint_gateway("queue"),
            busy_input_hint_gateway("interrupt"),
            busy_input_hint_gateway("steer"),
            busy_input_hint_cli("queue"),
            busy_input_hint_cli("interrupt"),
            busy_input_hint_cli("steer"),
            tool_progress_hint_gateway(),
            tool_progress_hint_cli(),
            openclaw_residue_hint_cli(),
            curiosity_first_idle_hint_cli(),
            evolve_first_suggestion_hint_cli(),
        ] {
            assert!(!hint.trim().is_empty());
        }
    }

    // ── OpenClaw 横幅文案 ────────────────────────────────────────────────────
    #[test]
    fn openclaw_hint_mentions_migrate_and_path() {
        let msg = openclaw_residue_hint_cli();
        assert!(msg.contains("zaion claw migrate"));
        assert!(msg.contains("~/.openclaw"));
    }

    #[test]
    fn openclaw_hint_mentions_cleanup_and_warns() {
        let msg = openclaw_residue_hint_cli();
        assert!(msg.contains("zaion claw cleanup"));
        assert!(msg.contains("停止工作"));
    }

    // ── Zaion 独有提示 ───────────────────────────────────────────────────────
    #[test]
    fn curiosity_hint_mentions_controls() {
        let msg = curiosity_first_idle_hint_cli();
        assert!(msg.contains("zaion curiosity off"));
        assert!(msg.contains("System V") || msg.contains("好奇心"));
    }

    #[test]
    fn evolve_hint_mentions_controls() {
        let msg = evolve_first_suggestion_hint_cli();
        assert!(msg.contains("zaion evolve review"));
        assert!(msg.contains("zaion evolve off"));
    }

    // ── 遗留目录检测 ─────────────────────────────────────────────────────────
    #[test]
    fn detect_residue_true_when_dir_present() {
        let tmp = std::env::temp_dir().join("zaion_onboard_test_dir");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join(".openclaw")).unwrap();
        assert!(detect_openclaw_residue(Some(&tmp)));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn detect_residue_false_when_absent() {
        let tmp = std::env::temp_dir().join("zaion_onboard_test_absent");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        assert!(!detect_openclaw_residue(Some(&tmp)));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn detect_residue_false_when_path_is_file() {
        let tmp = std::env::temp_dir().join("zaion_onboard_test_file");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join(".openclaw"), "oops").unwrap();
        assert!(!detect_openclaw_residue(Some(&tmp)));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn detect_residue_default_home_does_not_panic() {
        // smoke：真实 $HOME 查找不论状态都不能 panic
        let _ = detect_openclaw_residue(None);
    }

    // ── take_hint_once 组合行为 ──────────────────────────────────────────────
    #[test]
    fn take_hint_once_returns_none_when_seen() {
        let cfg = cfg_with_seen(&[(TOOL_PROGRESS_FLAG, true)]);
        let out = take_hint_once(&cfg, TOOL_PROGRESS_FLAG, || "should not render".into());
        assert!(out.is_none());
    }
}
