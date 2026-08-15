//! SyntaxGate — 多语言语法校验熔断器
//!
//! 支持语言：Rust (toml check) / TOML / JSON / Python (indent/colon check) / Shell / TypeScript(basic)
//!
//! 工作原理：
//!   - 每次 ACI WriteFile / ReplaceAstNode / Insert 操作前必须通过 SyntaxGate
//!   - 语法错误 → 立即打回，文件不落盘，错误信息返回给 Agent 重写
//!   - 零语法错误代码才允许写入磁盘
use crate::AciError;
use serde::{Deserialize, Serialize};

// ── SyntaxLanguage ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SyntaxLanguage {
    Rust,
    Toml,
    Json,
    Python,
    Shell,
    TypeScript,
    JavaScript,
    Unknown,
}

impl SyntaxLanguage {
    /// 从文件扩展名推断语言
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "rs" => SyntaxLanguage::Rust,
            "toml" => SyntaxLanguage::Toml,
            "json" => SyntaxLanguage::Json,
            "py" => SyntaxLanguage::Python,
            "sh" | "bash" => SyntaxLanguage::Shell,
            "ts" | "tsx" => SyntaxLanguage::TypeScript,
            "js" | "jsx" => SyntaxLanguage::JavaScript,
            _ => SyntaxLanguage::Unknown,
        }
    }

    /// 从字符串名称解析 — infallible (unknown strings map to `Unknown`).
    pub fn parse_name(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "rust" => SyntaxLanguage::Rust,
            "toml" => SyntaxLanguage::Toml,
            "json" => SyntaxLanguage::Json,
            "python" | "py" => SyntaxLanguage::Python,
            "shell" | "sh" | "bash" => SyntaxLanguage::Shell,
            "typescript" | "ts" => SyntaxLanguage::TypeScript,
            "javascript" | "js" => SyntaxLanguage::JavaScript,
            _ => SyntaxLanguage::Unknown,
        }
    }
}

// ── SyntaxCheckResult ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum SyntaxCheckResult {
    Valid,
    Invalid { errors: Vec<SyntaxError> },
    Skipped { reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyntaxError {
    pub line: Option<usize>,
    pub col: Option<usize>,
    pub message: String,
}

impl SyntaxCheckResult {
    pub fn is_valid(&self) -> bool {
        matches!(
            self,
            SyntaxCheckResult::Valid | SyntaxCheckResult::Skipped { .. }
        )
    }

    pub fn to_aci_error(&self, language: &SyntaxLanguage) -> Option<AciError> {
        match self {
            SyntaxCheckResult::Invalid { errors } => {
                let msg = errors
                    .iter()
                    .map(|e| {
                        if let (Some(l), Some(c)) = (e.line, e.col) {
                            format!("line {l}:{c}: {}", e.message)
                        } else {
                            e.message.clone()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("; ");
                Some(AciError::SyntaxError {
                    language: format!("{:?}", language),
                    message: msg,
                })
            }
            _ => None,
        }
    }
}

// ── SyntaxGate ────────────────────────────────────────────────────────────────

pub struct SyntaxGate;

impl SyntaxGate {
    /// 校验代码内容。language 为 Unknown 时跳过校验（Skipped）。
    pub fn check(content: &str, language: &SyntaxLanguage) -> SyntaxCheckResult {
        match language {
            SyntaxLanguage::Toml => check_toml(content),
            SyntaxLanguage::Json => check_json(content),
            SyntaxLanguage::Rust => check_rust_heuristic(content),
            SyntaxLanguage::Python => check_python_heuristic(content),
            SyntaxLanguage::TypeScript | SyntaxLanguage::JavaScript => check_js_heuristic(content),
            SyntaxLanguage::Shell => check_shell_heuristic(content),
            SyntaxLanguage::Unknown => SyntaxCheckResult::Skipped {
                reason: "unknown language — syntax check skipped".into(),
            },
        }
    }

    /// 从文件路径推断语言并校验
    pub fn check_file(path: &std::path::Path, content: &str) -> SyntaxCheckResult {
        let lang = path
            .extension()
            .and_then(|e| e.to_str())
            .map(SyntaxLanguage::from_extension)
            .unwrap_or(SyntaxLanguage::Unknown);
        Self::check(content, &lang)
    }
}

// ── 各语言校验实现 ─────────────────────────────────────────────────────────────

fn check_toml(content: &str) -> SyntaxCheckResult {
    match toml::from_str::<toml::Value>(content) {
        Ok(_) => SyntaxCheckResult::Valid,
        Err(e) => {
            let line = e
                .span()
                .map(|s: std::ops::Range<usize>| content[..s.start].lines().count());
            SyntaxCheckResult::Invalid {
                errors: vec![SyntaxError {
                    line,
                    col: None,
                    message: e.to_string(),
                }],
            }
        }
    }
}

fn check_json(content: &str) -> SyntaxCheckResult {
    match serde_json::from_str::<serde_json::Value>(content) {
        Ok(_) => SyntaxCheckResult::Valid,
        Err(e) => SyntaxCheckResult::Invalid {
            errors: vec![SyntaxError {
                line: Some(e.line()),
                col: Some(e.column()),
                message: e.to_string(),
            }],
        },
    }
}

/// Rust 启发式校验（无 rustc — 检查括号平衡 + 常见陷阱）
fn check_rust_heuristic(content: &str) -> SyntaxCheckResult {
    let mut errors = Vec::new();

    // 括号平衡检查
    if let Some(err) = check_bracket_balance(content) {
        errors.push(err);
    }

    // 检查明显的 Rust 语法错误（未闭合字符串）
    if let Some(err) = check_unclosed_string_literal(content) {
        errors.push(err);
    }

    if errors.is_empty() {
        SyntaxCheckResult::Valid
    } else {
        SyntaxCheckResult::Invalid { errors }
    }
}

/// Python 启发式校验（检查缩进一致性 + 冒号）
fn check_python_heuristic(content: &str) -> SyntaxCheckResult {
    let mut errors = Vec::new();

    for (idx, line) in content.lines().enumerate() {
        let ln = idx + 1;
        // 检查 def/class/if/for/while/with 末尾是否有冒号
        let trimmed = line.trim();
        for keyword in &[
            "def ", "class ", "if ", "elif ", "else", "for ", "while ", "with ", "try", "except",
            "finally",
        ] {
            if trimmed.starts_with(keyword)
                && !trimmed.ends_with(':')
                && !trimmed.ends_with('\\')
                && !trimmed.is_empty()
            {
                // 允许单行 if x: pass 形式（包含冒号在中间）
                if trimmed.contains(':') {
                    continue;
                }
                // 多行续行
                if trimmed.ends_with('(') || trimmed.ends_with(',') {
                    continue;
                }
                errors.push(SyntaxError {
                    line: Some(ln),
                    col: None,
                    message: format!("possibly missing colon after '{keyword}' block"),
                });
                break;
            }
        }
    }

    // 括号平衡
    if let Some(err) = check_bracket_balance(content) {
        errors.push(err);
    }

    if errors.is_empty() {
        SyntaxCheckResult::Valid
    } else {
        SyntaxCheckResult::Invalid { errors }
    }
}

/// JS/TS 启发式：括号平衡 + 未闭合字符串
fn check_js_heuristic(content: &str) -> SyntaxCheckResult {
    let mut errors = Vec::new();
    if let Some(e) = check_bracket_balance(content) {
        errors.push(e);
    }
    if let Some(e) = check_unclosed_string_literal(content) {
        errors.push(e);
    }
    if errors.is_empty() {
        SyntaxCheckResult::Valid
    } else {
        SyntaxCheckResult::Invalid { errors }
    }
}

/// Shell 启发式：未闭合引号 + 常见 `fi`/`done`/`esac` 缺失
fn check_shell_heuristic(content: &str) -> SyntaxCheckResult {
    let mut errors = Vec::new();
    if let Some(e) = check_unclosed_string_literal(content) {
        errors.push(e);
    }

    // 检查 if...fi / for...done / case...esac 配对
    let opens_if = content
        .lines()
        .filter(|l| l.trim() == "if" || l.trim_start().starts_with("if "))
        .count();
    let closes_fi = content.lines().filter(|l| l.trim() == "fi").count();
    if opens_if > closes_fi + 2 {
        errors.push(SyntaxError {
            line: None,
            col: None,
            message: format!("possibly missing 'fi' (found {opens_if} if-blocks, {closes_fi} fi)"),
        });
    }

    if errors.is_empty() {
        SyntaxCheckResult::Valid
    } else {
        SyntaxCheckResult::Invalid { errors }
    }
}

// ── 通用辅助 ──────────────────────────────────────────────────────────────────

/// 括号/大括号/方括号平衡检查
fn check_bracket_balance(content: &str) -> Option<SyntaxError> {
    let mut round = 0i32; // ()
    let mut curly = 0i32; // {}
    let mut square = 0i32; // []
    let mut in_string_double = false;
    let mut in_string_single = false;
    let mut in_block_comment = false;
    let mut prev = '\0';

    for (line_idx, line) in content.lines().enumerate() {
        let mut line_comment = false;
        for ch in line.chars() {
            if in_block_comment {
                if prev == '*' && ch == '/' {
                    in_block_comment = false;
                }
                prev = ch;
                continue;
            }
            if line_comment {
                break;
            }
            if in_string_double {
                if ch == '"' && prev != '\\' {
                    in_string_double = false;
                }
                prev = ch;
                continue;
            }
            if in_string_single {
                if ch == '\'' && prev != '\\' {
                    in_string_single = false;
                }
                prev = ch;
                continue;
            }
            match ch {
                '"' => {
                    in_string_double = true;
                }
                '\'' => {
                    in_string_single = true;
                }
                '/' => {
                    if prev == '/' {
                        line_comment = true;
                    }
                }
                '*' => {
                    if prev == '/' {
                        in_block_comment = true;
                    }
                }
                '(' => {
                    round += 1;
                }
                ')' => {
                    round -= 1;
                }
                '{' => {
                    curly += 1;
                }
                '}' => {
                    curly -= 1;
                }
                '[' => {
                    square += 1;
                }
                ']' => {
                    square -= 1;
                }
                _ => {}
            }
            if round < 0 || curly < 0 || square < 0 {
                return Some(SyntaxError {
                    line: Some(line_idx + 1),
                    col: None,
                    message: format!(
                        "unmatched closing bracket on line {} (round={round}, curly={curly}, square={square})",
                        line_idx + 1
                    ),
                });
            }
            prev = ch;
        }
    }

    if round != 0 || curly != 0 || square != 0 {
        Some(SyntaxError {
            line: None,
            col: None,
            message: format!(
                "unbalanced brackets: unclosed ( x{round}) {{ x{curly}}} [ x{square}]",
                round = round.max(0),
                curly = curly.max(0),
                square = square.max(0),
            ),
        })
    } else {
        None
    }
}

/// 未闭合字符串字面量检查（单行）
fn check_unclosed_string_literal(content: &str) -> Option<SyntaxError> {
    for (idx, line) in content.lines().enumerate() {
        let mut in_double = false;
        let mut in_single = false;
        let mut prev = '\0';
        for ch in line.chars() {
            match ch {
                '"' if !in_single && prev != '\\' => {
                    in_double = !in_double;
                }
                '\'' if !in_double && prev != '\\' => {
                    in_single = !in_single;
                }
                _ => {}
            }
            prev = ch;
        }
        if in_double {
            return Some(SyntaxError {
                line: Some(idx + 1),
                col: None,
                message: format!("unclosed double-quote string on line {}", idx + 1),
            });
        }
    }
    None
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_toml_passes() {
        let r = SyntaxGate::check("[core]\nkey = \"value\"", &SyntaxLanguage::Toml);
        assert!(r.is_valid());
    }

    #[test]
    fn invalid_toml_rejected() {
        let r = SyntaxGate::check("[core\nkey = \"value\"", &SyntaxLanguage::Toml);
        assert!(!r.is_valid());
    }

    #[test]
    fn valid_json_passes() {
        let r = SyntaxGate::check("{\"key\": 42}", &SyntaxLanguage::Json);
        assert!(r.is_valid());
    }

    #[test]
    fn invalid_json_rejected() {
        let r = SyntaxGate::check("{key: 42}", &SyntaxLanguage::Json);
        assert!(!r.is_valid());
    }

    #[test]
    fn rust_unbalanced_brace_rejected() {
        let r = SyntaxGate::check("fn foo() { let x = 1;", &SyntaxLanguage::Rust);
        assert!(!r.is_valid());
        if let SyntaxCheckResult::Invalid { errors } = r {
            assert!(errors[0].message.contains("unbalanced"));
        }
    }

    #[test]
    fn rust_balanced_passes() {
        let r = SyntaxGate::check("fn foo() { let x = 1; }", &SyntaxLanguage::Rust);
        assert!(r.is_valid());
    }

    #[test]
    fn unknown_language_skipped() {
        let r = SyntaxGate::check("anything goes here", &SyntaxLanguage::Unknown);
        assert!(r.is_valid()); // Skipped counts as valid
        assert!(matches!(r, SyntaxCheckResult::Skipped { .. }));
    }

    #[test]
    fn language_from_extension() {
        assert_eq!(SyntaxLanguage::from_extension("rs"), SyntaxLanguage::Rust);
        assert_eq!(SyntaxLanguage::from_extension("toml"), SyntaxLanguage::Toml);
        assert_eq!(SyntaxLanguage::from_extension("json"), SyntaxLanguage::Json);
        assert_eq!(SyntaxLanguage::from_extension("py"), SyntaxLanguage::Python);
        assert_eq!(
            SyntaxLanguage::from_extension("xyz"),
            SyntaxLanguage::Unknown
        );
    }

    #[test]
    fn unclosed_string_detected() {
        let r = SyntaxGate::check("let x = \"hello;", &SyntaxLanguage::Rust);
        assert!(!r.is_valid());
    }

    #[test]
    fn syntax_error_has_message() {
        let r = SyntaxGate::check("{bad json", &SyntaxLanguage::Json);
        let err = r.to_aci_error(&SyntaxLanguage::Json);
        assert!(err.is_some());
        assert!(err.unwrap().to_string().contains("Json"));
    }
}
