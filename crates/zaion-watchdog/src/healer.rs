use crate::{CrashReport, WatchdogConfig, WatchdogError};
/// CrashHealer — 云端 LLM 修复引擎
///
/// 将崩溃报告发往 OpenAI 兼容 LLM，解析修复方案，返回 HealPlan。
/// HealPlan 包含：修复类型（文件内容替换 / 操作描述）+ 具体内容
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ── HealPlan ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealPlan {
    pub fix_type: HealFixType,
    /// 要写入的文件路径（fix_type == FileContent 时有效）
    pub file_path: Option<PathBuf>,
    /// 修复内容（文件全文 或 操作描述）
    pub content: String,
    /// LLM 返回的完整原始响应（保留用于 Ledger 审计）
    pub raw_llm_response: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum HealFixType {
    /// LLM 返回了修正后的文件内容，直接覆写
    FileContent,
    /// LLM 返回了操作步骤描述（无法自动应用，记录到 Ledger）
    Description,
    /// LLM 无法给出修复方案
    Unknown,
}

impl HealFixType {
    pub fn as_str(&self) -> &'static str {
        match self {
            HealFixType::FileContent => "file_content",
            HealFixType::Description => "description",
            HealFixType::Unknown => "unknown",
        }
    }
}

// ── CrashHealer ───────────────────────────────────────────────────────────────

pub struct CrashHealer {
    config: WatchdogConfig,
}

impl CrashHealer {
    pub fn new(config: WatchdogConfig) -> Self {
        CrashHealer { config }
    }

    /// 调云端 LLM 获取修复方案。
    /// 失败时返回 WatchdogError::HealFailed。
    pub async fn heal(&self, report: &CrashReport) -> Result<HealPlan, WatchdogError> {
        let prompt = report.to_heal_prompt();
        let raw = self.call_llm(&prompt).await?;
        Ok(self.parse_llm_response(raw, report))
    }

    async fn call_llm(&self, prompt: &str) -> Result<String, WatchdogError> {
        let client = reqwest::Client::new();
        let url = format!(
            "{}/chat/completions",
            self.config.llm_endpoint.trim_end_matches('/')
        );

        let body = serde_json::json!({
            "model": self.config.llm_model,
            "messages": [
                {
                    "role": "system",
                    "content": "You are a Zaion system healer. When given a crash report, \
                                analyze the error and return a JSON repair plan. \
                                Always respond with valid JSON only."
                },
                {
                    "role": "user",
                    "content": prompt
                }
            ],
            "temperature": 0.1,
            "max_tokens": 2000
        });

        let resp = client
            .post(&url)
            .header(
                "Authorization",
                format!("Bearer {}", self.config.llm_api_key),
            )
            .header("Content-Type", "application/json")
            .json(&body)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| WatchdogError::Http(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(WatchdogError::HealFailed(format!(
                "LLM API returned {status}: {body}"
            )));
        }

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| WatchdogError::HealFailed(e.to_string()))?;

        // 提取 choices[0].message.content
        json["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| WatchdogError::HealFailed("LLM response missing content".into()))
    }

    fn parse_llm_response(&self, raw: String, report: &CrashReport) -> HealPlan {
        // 尝试解析 LLM 返回的 JSON
        let trimmed = raw.trim();

        // 提取 ```json ... ``` 代码块（LLM 有时会包裹在 markdown 中）
        let json_str = if let Some(start) = trimmed.find("```json") {
            let after = &trimmed[start + 7..];
            after.find("```").map(|end| &after[..end]).unwrap_or(after)
        } else if trimmed.starts_with('{') {
            trimmed
        } else {
            // 在文本中寻找第一个 JSON 对象
            if let Some(start) = trimmed.find('{') {
                if let Some(end) = trimmed.rfind('}') {
                    &trimmed[start..=end]
                } else {
                    trimmed
                }
            } else {
                trimmed
            }
        };

        match serde_json::from_str::<serde_json::Value>(json_str) {
            Ok(v) => {
                let fix_type = match v["fix_type"].as_str() {
                    Some("file_content") => HealFixType::FileContent,
                    Some("description") => HealFixType::Description,
                    _ => HealFixType::Unknown,
                };
                let file_path = v["file_path"]
                    .as_str()
                    .map(PathBuf::from)
                    .or_else(|| report.damaged_files.first().cloned());
                let content = v["content"]
                    .as_str()
                    .unwrap_or("No content provided by LLM")
                    .to_string();

                HealPlan {
                    fix_type,
                    file_path,
                    content,
                    raw_llm_response: raw,
                }
            }
            Err(_) => {
                // LLM 没有返回 JSON — 记录为描述类修复
                HealPlan {
                    fix_type: HealFixType::Description,
                    file_path: report.damaged_files.first().cloned(),
                    content: raw.clone(),
                    raw_llm_response: raw,
                }
            }
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WatchdogConfig;
    use std::path::PathBuf;

    fn make_healer() -> CrashHealer {
        CrashHealer::new(WatchdogConfig::default_local())
    }

    fn make_report() -> CrashReport {
        CrashReport {
            stack_trace: "TOML parse error at line 42: invalid escape".to_string(),
            damaged_files: vec![PathBuf::from("/tmp/config.toml")],
            crashed_at: "2026-04-03T00:00:00Z".to_string(),
            exit_code: Some(101),
            summary: "TOML parse error at line 42".to_string(),
        }
    }

    #[test]
    fn parse_json_response_file_content() {
        let healer = make_healer();
        let raw = r#"{"fix_type":"file_content","file_path":"/tmp/config.toml","content":"[core]\nkey = \"value\""}"#.to_string();
        let plan = healer.parse_llm_response(raw, &make_report());
        assert_eq!(plan.fix_type, HealFixType::FileContent);
        assert!(plan.content.contains("core"));
    }

    #[test]
    fn parse_markdown_wrapped_json() {
        let healer = make_healer();
        let raw = "Here is the fix:\n```json\n{\"fix_type\":\"description\",\"file_path\":\"/tmp/config.toml\",\"content\":\"Remove line 42\"}\n```".to_string();
        let plan = healer.parse_llm_response(raw, &make_report());
        assert_eq!(plan.fix_type, HealFixType::Description);
    }

    #[test]
    fn parse_plain_text_falls_back_to_description() {
        let healer = make_healer();
        let raw = "The config file has a typo at line 42. Please fix the quotes.".to_string();
        let plan = healer.parse_llm_response(raw, &make_report());
        assert_eq!(plan.fix_type, HealFixType::Description);
    }

    #[test]
    fn heal_plan_has_file_path_from_report_when_llm_omits_it() {
        let healer = make_healer();
        let raw = r#"{"fix_type":"file_content","content":"[core]\nok = true"}"#.to_string();
        let plan = healer.parse_llm_response(raw, &make_report());
        // Should fall back to damaged_files[0]
        assert!(plan.file_path.is_some());
    }
}
