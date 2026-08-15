use crate::WatchdogError;
use serde::{Deserialize, Serialize};
/// CrashDetector — 崩溃堆栈捕获与损坏文件识别
///
/// 从崩溃日志目录读取最新的 stderr 转储，提取：
///   - panic! 堆栈信息
///   - 受影响的文件路径（解析错误消息中的文件名）
///   - 崩溃时间戳
use std::path::{Path, PathBuf};

// ── CrashReport ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrashReport {
    /// 原始堆栈文本
    pub stack_trace: String,
    /// 解析出的受损文件路径（可能为空）
    pub damaged_files: Vec<PathBuf>,
    /// 崩溃时间戳 (ISO 8601)
    pub crashed_at: String,
    /// 崩溃的退出码（如已知）
    pub exit_code: Option<i32>,
    /// 一句话错误摘要（首行 panic 信息）
    pub summary: String,
}

impl CrashReport {
    /// 生成发往 LLM 的简明问题描述
    pub fn to_heal_prompt(&self) -> String {
        let files = if self.damaged_files.is_empty() {
            "unknown".to_string()
        } else {
            self.damaged_files
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        };
        format!(
            "Zaion process crashed with the following error:\n\n\
             Summary: {summary}\n\n\
             Affected files: {files}\n\n\
             Stack trace (truncated):\n{stack}\n\n\
             Please provide the corrected file content for the TOML/JSON config file \
             if this is a parse error, or a brief fix description otherwise. \
             Respond with JSON: {{\"fix_type\": \"file_content\" | \"description\", \
             \"file_path\": \"<path>\", \"content\": \"<corrected content or fix steps>\"}}",
            summary = self.summary,
            files = files,
            stack = &self.stack_trace[..self.stack_trace.len().min(2000)],
        )
    }
}

// ── CrashDetector ─────────────────────────────────────────────────────────────

pub struct CrashDetector {
    crash_log_dir: PathBuf,
    config_file: PathBuf,
}

impl CrashDetector {
    pub fn new(crash_log_dir: PathBuf, config_file: PathBuf) -> Self {
        CrashDetector {
            crash_log_dir,
            config_file,
        }
    }

    /// 从崩溃日志目录读取最新崩溃报告。
    /// 如果日志目录为空，从 stderr 捕获（生产环境）或生成诊断报告。
    pub fn detect(&self) -> Result<CrashReport, WatchdogError> {
        // 尝试读取最新崩溃日志文件
        if let Some(log) = self.find_latest_crash_log()? {
            return self.parse_crash_log(&log);
        }

        // fallback: 生成配置文件诊断报告
        self.diagnose_config_file()
    }

    fn find_latest_crash_log(&self) -> Result<Option<PathBuf>, WatchdogError> {
        if !self.crash_log_dir.exists() {
            return Ok(None);
        }
        let mut logs: Vec<(PathBuf, std::time::SystemTime)> =
            std::fs::read_dir(&self.crash_log_dir)?
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.path()
                        .extension()
                        .map(|x| x == "log" || x == "txt")
                        .unwrap_or(false)
                })
                .filter_map(|e| {
                    let meta = e.metadata().ok()?;
                    let mtime = meta.modified().ok()?;
                    Some((e.path(), mtime))
                })
                .collect();

        logs.sort_by_key(|(_, t)| *t);
        Ok(logs.last().map(|(p, _)| p.clone()))
    }

    fn parse_crash_log(&self, log_path: &Path) -> Result<CrashReport, WatchdogError> {
        let content = std::fs::read_to_string(log_path)?;
        self.parse_stack_text(content)
    }

    fn diagnose_config_file(&self) -> Result<CrashReport, WatchdogError> {
        let now = chrono::Utc::now().to_rfc3339();

        // 尝试读取配置文件以获取诊断信息
        let (stack_trace, damaged_files, summary) = match std::fs::read_to_string(&self.config_file)
        {
            Ok(content) => {
                // 尝试解析 TOML，获取具体错误
                let parse_err: Option<String> = match toml::from_str::<toml::Value>(&content) {
                    Ok(_) => None,
                    Err(e) => Some(e.to_string()),
                };
                match parse_err {
                    Some(err) => {
                        let summary_len = err.len().min(120);
                        (
                            format!("TOML parse error in config file:\n{err}"),
                            vec![self.config_file.clone()],
                            format!("TOML parse error: {}", &err[..summary_len]),
                        )
                    }
                    None => (
                        "Process crashed: config file OK, unknown cause".to_string(),
                        vec![],
                        "Unknown crash — config file valid".to_string(),
                    ),
                }
            }
            Err(e) => (
                format!("Config file unreadable: {e}"),
                vec![self.config_file.clone()],
                format!("Config file unreadable: {e}"),
            ),
        };

        Ok(CrashReport {
            stack_trace,
            damaged_files,
            crashed_at: now,
            exit_code: None,
            summary,
        })
    }

    fn parse_stack_text(&self, content: String) -> Result<CrashReport, WatchdogError> {
        let now = chrono::Utc::now().to_rfc3339();

        // 提取 summary（第一个 panic 行或第一行）
        let summary = content
            .lines()
            .find(|l| l.contains("panic") || l.contains("error") || l.contains("TOML"))
            .unwrap_or(content.lines().next().unwrap_or("Unknown crash"))
            .chars()
            .take(200)
            .collect::<String>();

        // 扫描已知配置/数据文件路径
        let damaged_files = self.extract_file_paths(&content);

        Ok(CrashReport {
            stack_trace: content,
            damaged_files,
            crashed_at: now,
            exit_code: None,
            summary,
        })
    }

    /// 从堆栈文本中提取可能损坏的文件路径
    fn extract_file_paths(&self, text: &str) -> Vec<PathBuf> {
        let mut paths = Vec::new();

        // 扫描 .toml / .json / .db 文件引用
        for line in text.lines() {
            for word in line.split_whitespace() {
                let w = word.trim_matches(|c: char| {
                    !c.is_alphanumeric()
                        && c != '/'
                        && c != '\\'
                        && c != '.'
                        && c != '_'
                        && c != '-'
                });
                if (w.ends_with(".toml") || w.ends_with(".json") || w.ends_with(".db"))
                    && (w.contains('/') || w.contains('\\'))
                {
                    paths.push(PathBuf::from(w));
                }
            }
        }

        // 始终包含主配置文件（最可能的损坏目标）
        if !paths.contains(&self.config_file) && self.config_file.exists() {
            // 仅在实际存在时包含
            paths.insert(0, self.config_file.clone());
        }

        paths.dedup();
        paths
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_detector() -> CrashDetector {
        CrashDetector::new(
            PathBuf::from("/nonexistent/crash_logs"),
            PathBuf::from("/nonexistent/config.toml"),
        )
    }

    #[test]
    fn crash_report_to_heal_prompt_contains_summary() {
        let report = CrashReport {
            stack_trace: "panic at 'TOML parse error at line 42'".to_string(),
            damaged_files: vec![PathBuf::from("/home/user/.zaion/config.toml")],
            crashed_at: "2026-04-03T00:00:00Z".to_string(),
            exit_code: Some(101),
            summary: "TOML parse error at line 42".to_string(),
        };
        let prompt = report.to_heal_prompt();
        assert!(prompt.contains("TOML parse error"));
        assert!(prompt.contains("config.toml"));
        assert!(prompt.contains("fix_type"));
    }

    #[test]
    fn parse_stack_text_extracts_toml_error() {
        let detector = make_detector();
        let stack = "thread 'main' panicked at 'called `Result::unwrap()` on an `Err` value: \
                     Error { inner: ErrorInner { kind: InvalidEscape, line: Some(42), \
                     col: 5, at: Some(1234), message: \"invalid escape\" } }'\n\
                     /home/user/.zaion/config.toml line 42"
            .to_string();
        let report = detector.parse_stack_text(stack).unwrap();
        assert!(!report.summary.is_empty());
        assert!(!report.crashed_at.is_empty());
    }

    #[test]
    fn extract_file_paths_finds_toml() {
        let detector = make_detector();
        let text = "Error loading /home/user/.zaion/config.toml at line 5";
        let paths = detector.extract_file_paths(text);
        assert!(paths
            .iter()
            .any(|p| p.to_str().unwrap().contains("config.toml")));
    }

    #[test]
    fn detect_falls_back_to_diagnose_when_no_logs() {
        let detector = make_detector();
        // No crash log dir, no config file → returns a report (not an error)
        let report = detector.detect();
        assert!(report.is_ok());
    }
}
