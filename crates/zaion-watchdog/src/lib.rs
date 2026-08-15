//! zaion-watchdog — Ouroboros 衔尾蛇自愈协议
//!
//! 架构：
//!   WatchdogConfig  — 配置（主进程路径、LLM endpoint、Ledger 路径）
//!   ProcessMonitor  — 心跳轮询主进程存活
//!   CrashDetector   — 崩溃堆栈捕获 + 损坏文件识别
//!   CrashHealer     — 调云端 LLM 获取修复方案
//!   Resurrector     — 覆写坏文件 + 签名写入 Ledger + 重启主进程
//!
//! 完整 Ouroboros 闭环：
//!   Monitor → detect crash → capture stack
//!   → Healer.heal(stack, file) → LLM returns fix
//!   → Resurrector.apply_fix() → overwrite + sign ledger + restart
//!   → terminal: "We are back online."
pub mod config;
pub mod crash;
pub mod error;
pub mod healer;
pub mod history;
pub mod ledger_writer;
pub mod monitor;
pub mod reality_sync;
pub mod resurrect;
pub mod toxic;

pub use config::WatchdogConfig;
pub use crash::{CrashDetector, CrashReport};
pub use error::WatchdogError;
pub use healer::{CrashHealer, HealFixType, HealPlan};
pub use history::{RepairEntry, RepairHistory, RepairResult};
pub use monitor::ProcessMonitor;
pub use resurrect::Resurrector;
