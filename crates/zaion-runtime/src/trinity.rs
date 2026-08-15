//! Trinity — 三位一体并行推演系统 (Sprint 3, Genesis v4.0)
//!
//! 架构：
//!   TrinityRole       — Architect / Developer / Tester 三个角色
//!   TrinityPlan       — 单个角色生成的执行计划（含 ACI 动作序列）
//!   TrinityVerdict    — Tester 对所有计划的仲裁结果
//!   TrinityEngine     — 并行启动三角色，收集计划，Tester 投票选最优
//!
//! 工作流：
//!   1. 接收高难度任务（ComplexityScore > Deep 阈值）
//!   2. 并行启动 Architect / Developer 各生成 N 个候选计划
//!   3. Tester 对每个计划打分（语法检查 + 逻辑一致性 + 风险评估）
//!   4. 选出最高分计划 → 返回给 AciDispatcher 执行
//!   5. 每个角色的思考过程签名写入 Event Ledger
use crate::RuntimeError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::task::JoinSet;

// ── TrinityRole ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TrinityRole {
    /// 架构师：关注全局设计、模块边界、扩展性
    Architect,
    /// 开发者：关注具体实现、代码细节、性能
    Developer,
    /// 测试员：关注正确性、边界条件、回归风险
    Tester,
}

impl std::fmt::Display for TrinityRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TrinityRole::Architect => write!(f, "Architect"),
            TrinityRole::Developer => write!(f, "Developer"),
            TrinityRole::Tester => write!(f, "Tester"),
        }
    }
}

impl TrinityRole {
    /// 返回该角色的 LLM system prompt
    pub fn system_prompt(&self) -> &'static str {
        match self {
            TrinityRole::Architect => {
                "You are the Architect in a Trinity parallel reasoning system. \
                 Focus on system design, module boundaries, and long-term extensibility. \
                 Propose a high-level plan with clear separation of concerns. \
                 Format your response as JSON: {\"approach\": \"...\", \"steps\": [...], \"risks\": [...]}"
            }
            TrinityRole::Developer => {
                "You are the Developer in a Trinity parallel reasoning system. \
                 Focus on concrete implementation details, code structure, and performance. \
                 Propose a step-by-step implementation plan with specific file changes. \
                 Format your response as JSON: {\"approach\": \"...\", \"steps\": [...], \"file_changes\": [...]}"
            }
            TrinityRole::Tester => {
                "You are the Tester in a Trinity parallel reasoning system. \
                 You will evaluate candidate plans from Architect and Developer. \
                 Score each plan 0-100 on: correctness, completeness, risk, testability. \
                 Format your response as JSON: {\"scores\": [{\"plan_id\": \"...\", \"score\": 0-100, \"reason\": \"...\"}], \"winner\": \"plan_id\"}"
            }
        }
    }
}

// ── TrinityPlan ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrinityPlan {
    pub plan_id: String,
    pub role: TrinityRole,
    /// 方案核心描述
    pub approach: String,
    /// 执行步骤列表
    pub steps: Vec<String>,
    /// 识别到的风险
    pub risks: Vec<String>,
    /// LLM 原始响应
    pub raw_response: String,
    /// 生成耗时（毫秒）
    pub elapsed_ms: u64,
}

impl TrinityPlan {
    pub fn parse(plan_id: &str, role: TrinityRole, raw: String, elapsed_ms: u64) -> Self {
        // 尝试解析 JSON
        let json_str = extract_json(&raw);
        let v: serde_json::Value = serde_json::from_str(json_str).unwrap_or_default();

        let approach = v["approach"]
            .as_str()
            .unwrap_or("No approach specified")
            .to_string();

        let steps = v["steps"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|s| s.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let risks = v["risks"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|s| s.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        TrinityPlan {
            plan_id: plan_id.to_string(),
            role,
            approach,
            steps,
            risks,
            raw_response: raw,
            elapsed_ms,
        }
    }
}

// ── TrinityVerdict ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrinityVerdict {
    /// 最优计划 ID
    pub winner_plan_id: String,
    /// 所有计划的评分
    pub scores: HashMap<String, u32>,
    /// Tester 的推理说明
    pub reasoning: String,
    /// 最优计划的完整内容
    pub winning_plan: Option<TrinityPlan>,
}

// ── TrinityConfig ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct TrinityConfig {
    /// Architect 生成的候选计划数
    pub architect_candidates: usize,
    /// Developer 生成的候选计划数
    pub developer_candidates: usize,
    /// 单个角色的超时（毫秒）
    pub role_timeout_ms: u64,
    /// LLM endpoint
    pub llm_endpoint: String,
    pub llm_api_key: String,
    pub llm_model: String,
}

impl Default for TrinityConfig {
    fn default() -> Self {
        TrinityConfig {
            architect_candidates: 2,
            developer_candidates: 3,
            role_timeout_ms: 30_000,
            llm_endpoint: std::env::var("OPENAI_BASE_URL")
                .unwrap_or_else(|_| "https://api.openai.com/v1".into()),
            llm_api_key: std::env::var("OPENAI_API_KEY")
                .or_else(|_| std::env::var("ZAION_LLM_KEY"))
                .unwrap_or_default(),
            llm_model: "gpt-4o-mini".into(),
        }
    }
}

// ── TrinityEngine ─────────────────────────────────────────────────────────────

pub struct TrinityEngine {
    config: TrinityConfig,
}

impl TrinityEngine {
    pub fn new(config: TrinityConfig) -> Self {
        TrinityEngine { config }
    }

    /// 执行完整的 Trinity 推演并返回最优计划。
    ///
    /// 步骤：
    ///   1. 并行启动 Architect + Developer 角色任务
    ///   2. 收集所有候选计划
    ///   3. Tester 评估并选出最优计划
    pub async fn deliberate(&self, task: &str) -> Result<TrinityVerdict, RuntimeError> {
        let mut all_plans: Vec<TrinityPlan> = Vec::new();

        // 并行启动 Architect 和 Developer
        let mut join_set: JoinSet<Vec<TrinityPlan>> = JoinSet::new();

        // Architect 候选
        for i in 0..self.config.architect_candidates {
            let cfg = self.config.clone();
            let task_str = task.to_string();
            let plan_id = format!("arch-{i}");
            join_set.spawn(async move {
                match Self::call_role(&cfg, TrinityRole::Architect, &task_str, &plan_id).await {
                    Ok(plan) => vec![plan],
                    Err(_) => vec![],
                }
            });
        }

        // Developer 候选
        for i in 0..self.config.developer_candidates {
            let cfg = self.config.clone();
            let task_str = task.to_string();
            let plan_id = format!("dev-{i}");
            join_set.spawn(async move {
                match Self::call_role(&cfg, TrinityRole::Developer, &task_str, &plan_id).await {
                    Ok(plan) => vec![plan],
                    Err(_) => vec![],
                }
            });
        }

        // 收集所有结果
        while let Some(result) = join_set.join_next().await {
            if let Ok(plans) = result {
                all_plans.extend(plans);
            }
        }

        if all_plans.is_empty() {
            return Err(RuntimeError::Task(
                "Trinity: all role invocations failed".into(),
            ));
        }

        // Tester 仲裁
        let verdict = self.tester_arbitrate(task, &all_plans).await?;
        Ok(verdict)
    }

    /// 调用单个角色生成一个计划
    async fn call_role(
        cfg: &TrinityConfig,
        role: TrinityRole,
        task: &str,
        plan_id: &str,
    ) -> Result<TrinityPlan, RuntimeError> {
        let start = std::time::Instant::now();

        let client = reqwest::Client::new();
        let url = format!(
            "{}/chat/completions",
            cfg.llm_endpoint.trim_end_matches('/')
        );

        let body = serde_json::json!({
            "model": cfg.llm_model,
            "messages": [
                { "role": "system", "content": role.system_prompt() },
                { "role": "user",   "content": format!("Task: {task}") }
            ],
            "temperature": match role {
                TrinityRole::Architect => 0.7,  // 偏创意
                TrinityRole::Developer => 0.4,  // 偏精确
                TrinityRole::Tester    => 0.1,  // 偏严格
            },
            "max_tokens": 1500
        });

        let resp = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", cfg.llm_api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .timeout(std::time::Duration::from_millis(cfg.role_timeout_ms))
            .send()
            .await
            .map_err(|e| RuntimeError::Task(format!("Trinity HTTP: {e}")))?;

        if !resp.status().is_success() {
            return Err(RuntimeError::Task(format!(
                "Trinity LLM returned {}",
                resp.status()
            )));
        }

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| RuntimeError::Task(format!("Trinity parse: {e}")))?;

        let raw = json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let elapsed_ms = start.elapsed().as_millis() as u64;
        Ok(TrinityPlan::parse(plan_id, role, raw, elapsed_ms))
    }

    /// Tester 角色对所有候选计划打分，返回最优计划
    async fn tester_arbitrate(
        &self,
        task: &str,
        plans: &[TrinityPlan],
    ) -> Result<TrinityVerdict, RuntimeError> {
        // 构建评估 prompt
        let plans_summary: Vec<serde_json::Value> = plans
            .iter()
            .map(|p| {
                serde_json::json!({
                    "plan_id": p.plan_id,
                    "role": p.role.to_string(),
                    "approach": p.approach,
                    "steps": p.steps,
                    "risks": p.risks,
                })
            })
            .collect();

        let eval_prompt = format!(
            "Task: {task}\n\nCandidate plans:\n{}\n\nEvaluate each plan and choose the best one.",
            serde_json::to_string_pretty(&plans_summary).unwrap_or_default()
        );

        let tester_raw = Self::call_role(
            &self.config,
            TrinityRole::Tester,
            &eval_prompt,
            "tester-verdict",
        )
        .await?;

        // 解析 Tester 的评分结果
        let verdict = self.parse_verdict(tester_raw.raw_response, plans);
        Ok(verdict)
    }

    fn parse_verdict(&self, raw: String, plans: &[TrinityPlan]) -> TrinityVerdict {
        let json_str = extract_json(&raw);
        let v: serde_json::Value = serde_json::from_str(json_str).unwrap_or_default();

        let mut scores: HashMap<String, u32> = HashMap::new();

        if let Some(score_arr) = v["scores"].as_array() {
            for item in score_arr {
                if let (Some(pid), Some(score)) = (item["plan_id"].as_str(), item["score"].as_u64())
                {
                    scores.insert(pid.to_string(), score as u32);
                }
            }
        }

        // 如果没有有效评分，fallback：给所有计划平均分
        if scores.is_empty() {
            for plan in plans {
                scores.insert(plan.plan_id.clone(), 50);
            }
        }

        // 找出最高分计划
        let winner_plan_id = scores
            .iter()
            .max_by_key(|(_, &score)| score)
            .map(|(id, _)| id.clone())
            .unwrap_or_else(|| plans.first().map(|p| p.plan_id.clone()).unwrap_or_default());

        let winning_plan = plans.iter().find(|p| p.plan_id == winner_plan_id).cloned();

        let reasoning = v["winner"]
            .as_str()
            .map(|_| format!("Tester selected plan '{winner_plan_id}' as optimal"))
            .unwrap_or_else(|| format!("Fallback: highest score plan '{winner_plan_id}'"));

        TrinityVerdict {
            winner_plan_id,
            scores,
            reasoning,
            winning_plan,
        }
    }
}

// ── Dry-run / Mock Engine（用于测试，不调真实 LLM）────────────────────────────

/// 测试用的模拟 Trinity — 返回预设计划，不发 HTTP 请求
pub struct MockTrinityEngine {
    pub plans: Vec<TrinityPlan>,
}

impl MockTrinityEngine {
    pub fn deliberate_sync(&self, _task: &str) -> TrinityVerdict {
        let mut scores: HashMap<String, u32> = HashMap::new();
        for (i, plan) in self.plans.iter().enumerate() {
            scores.insert(plan.plan_id.clone(), 60 + (i as u32 * 10));
        }
        let winner_plan_id = scores
            .iter()
            .max_by_key(|(_, &s)| s)
            .map(|(id, _)| id.clone())
            .unwrap_or_default();
        let winning_plan = self
            .plans
            .iter()
            .find(|p| p.plan_id == winner_plan_id)
            .cloned();
        TrinityVerdict {
            winner_plan_id,
            scores,
            reasoning: "Mock: last plan wins".into(),
            winning_plan,
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn extract_json(text: &str) -> &str {
    let trimmed = text.trim();
    if let Some(start) = trimmed.find("```json") {
        let after = &trimmed[start + 7..];
        if let Some(end) = after.find("```") {
            return &after[..end];
        }
    }
    if trimmed.starts_with('{') {
        return trimmed;
    }
    if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            return &trimmed[start..=end];
        }
    }
    trimmed
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_plan(id: &str, role: TrinityRole, approach: &str) -> TrinityPlan {
        TrinityPlan {
            plan_id: id.to_string(),
            role,
            approach: approach.to_string(),
            steps: vec!["step 1".into(), "step 2".into()],
            risks: vec!["minimal risk".into()],
            raw_response: format!(
                r#"{{"approach": "{approach}", "steps": ["step 1"], "risks": []}}"#
            ),
            elapsed_ms: 100,
        }
    }

    #[test]
    fn trinity_role_display() {
        assert_eq!(TrinityRole::Architect.to_string(), "Architect");
        assert_eq!(TrinityRole::Developer.to_string(), "Developer");
        assert_eq!(TrinityRole::Tester.to_string(), "Tester");
    }

    #[test]
    fn trinity_role_has_system_prompt() {
        for role in [
            TrinityRole::Architect,
            TrinityRole::Developer,
            TrinityRole::Tester,
        ] {
            let prompt = role.system_prompt();
            assert!(!prompt.is_empty());
            assert!(prompt.len() > 50);
        }
    }

    #[test]
    fn trinity_plan_parse_valid_json() {
        let raw = r#"{"approach": "use dependency injection", "steps": ["refactor", "test"], "risks": ["breaking change"]}"#;
        let plan = TrinityPlan::parse("arch-0", TrinityRole::Architect, raw.to_string(), 150);
        assert_eq!(plan.approach, "use dependency injection");
        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.risks.len(), 1);
        assert_eq!(plan.elapsed_ms, 150);
    }

    #[test]
    fn trinity_plan_parse_fallback_on_bad_json() {
        let raw = "I think you should refactor the auth module.";
        let plan = TrinityPlan::parse("dev-0", TrinityRole::Developer, raw.to_string(), 200);
        assert_eq!(plan.approach, "No approach specified");
        assert!(plan.steps.is_empty());
    }

    #[test]
    fn mock_engine_deliberate_returns_verdict() {
        let plans = vec![
            make_plan("arch-0", TrinityRole::Architect, "clean architecture"),
            make_plan("dev-0", TrinityRole::Developer, "fast implementation"),
            make_plan("dev-1", TrinityRole::Developer, "optimized implementation"),
        ];
        let engine = MockTrinityEngine { plans };
        let verdict = engine.deliberate_sync("Refactor auth module");
        assert!(!verdict.winner_plan_id.is_empty());
        assert_eq!(verdict.scores.len(), 3);
        assert!(verdict.winning_plan.is_some());
    }

    #[test]
    fn mock_engine_last_plan_wins() {
        // Scores are 60, 70, 80 — dev-1 should win
        let plans = vec![
            make_plan("arch-0", TrinityRole::Architect, "a"),
            make_plan("dev-0", TrinityRole::Developer, "b"),
            make_plan("dev-1", TrinityRole::Developer, "c"),
        ];
        let engine = MockTrinityEngine { plans };
        let verdict = engine.deliberate_sync("task");
        assert_eq!(verdict.winner_plan_id, "dev-1");
    }

    #[test]
    fn parse_verdict_picks_highest_score() {
        let engine = TrinityEngine::new(TrinityConfig::default());
        let plans = vec![
            make_plan("p1", TrinityRole::Architect, "a"),
            make_plan("p2", TrinityRole::Developer, "b"),
        ];
        let raw = r#"{"scores": [{"plan_id": "p1", "score": 40}, {"plan_id": "p2", "score": 85}], "winner": "p2"}"#;
        let verdict = engine.parse_verdict(raw.to_string(), &plans);
        assert_eq!(verdict.winner_plan_id, "p2");
        assert_eq!(verdict.scores["p2"], 85);
    }

    #[test]
    fn parse_verdict_fallback_when_no_scores() {
        let engine = TrinityEngine::new(TrinityConfig::default());
        let plans = vec![make_plan("only-plan", TrinityRole::Developer, "x")];
        let raw = "I cannot evaluate these plans.";
        let verdict = engine.parse_verdict(raw.to_string(), &plans);
        assert_eq!(verdict.winner_plan_id, "only-plan");
    }

    #[test]
    fn extract_json_from_markdown() {
        let text = "Here is my plan:\n```json\n{\"approach\": \"test\"}\n```\nEnd.";
        let json = extract_json(text);
        assert!(json.contains("\"approach\""));
    }

    #[test]
    fn extract_json_bare_object() {
        let text = "{\"key\": \"value\"}";
        assert_eq!(extract_json(text), text);
    }

    #[test]
    fn trinity_config_default_has_sensible_values() {
        let cfg = TrinityConfig::default();
        assert!(cfg.architect_candidates > 0);
        assert!(cfg.developer_candidates > 0);
        assert!(cfg.role_timeout_ms >= 5_000);
    }
}

// Trinity module — uses reqwest for LLM calls
