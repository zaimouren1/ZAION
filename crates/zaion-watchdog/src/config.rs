/// WatchdogConfig — Ouroboros 守护者配置
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchdogConfig {
    /// 主进程可执行路径
    pub main_binary: PathBuf,
    /// 主进程启动参数
    pub main_args: Vec<String>,
    /// PID 文件路径（读取主进程 PID）
    pub pid_file: PathBuf,
    /// 心跳检测间隔（毫秒）
    pub heartbeat_interval_ms: u64,
    /// 崩溃日志目录（捕获 stderr）
    pub crash_log_dir: PathBuf,
    /// Zaion 配置文件路径（自愈目标）
    pub config_file: PathBuf,
    /// Ledger 数据库路径（写入 System_Resurrection 事件）
    pub ledger_db_path: PathBuf,
    /// LLM API endpoint（OpenAI 兼容）
    pub llm_endpoint: String,
    /// LLM API Key
    pub llm_api_key: String,
    /// LLM 模型 ID
    pub llm_model: String,
    /// Watchdog 自身的 principal_id（用于签名 Ledger）
    pub principal_id: String,
    /// 最大自愈尝试次数（防止死循环）
    pub max_heal_attempts: u32,
}

impl WatchdogConfig {
    pub fn default_local() -> Self {
        let paths = zaion_paths::paths();
        let principal_id = std::env::var("ZAION_WATCHDOG_PRINCIPAL_ID")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_default();
        WatchdogConfig {
            main_binary: PathBuf::from("zaion"),
            main_args: vec!["_daemon_run".into()],
            pid_file: paths.data_dir.path.join("daemon.pid"),
            heartbeat_interval_ms: 1_000,
            crash_log_dir: paths.data_dir.path.join("crash_logs"),
            config_file: paths.config_path(),
            ledger_db_path: paths.data_dir.path.join("ledger.db"),
            llm_endpoint: "https://api.openai.com/v1".into(),
            llm_api_key: std::env::var("ZAION_LLM_KEY")
                .or_else(|_| std::env::var("OPENAI_API_KEY"))
                .unwrap_or_default(),
            llm_model: "gpt-4o-mini".into(),
            principal_id,
            max_heal_attempts: 3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_pid_file() {
        let cfg = WatchdogConfig::default_local();
        assert!(cfg.pid_file.to_str().unwrap().contains("daemon.pid"));
    }

    #[test]
    fn default_interval_is_one_second() {
        let cfg = WatchdogConfig::default_local();
        assert_eq!(cfg.heartbeat_interval_ms, 1_000);
    }

    #[test]
    fn max_heal_attempts_is_bounded() {
        let cfg = WatchdogConfig::default_local();
        assert!(cfg.max_heal_attempts > 0 && cfg.max_heal_attempts <= 10);
    }

    #[test]
    fn default_restart_uses_current_foreground_runtime_entry() {
        let cfg = WatchdogConfig::default_local();
        assert_eq!(cfg.main_args, ["_daemon_run"]);
    }
}
