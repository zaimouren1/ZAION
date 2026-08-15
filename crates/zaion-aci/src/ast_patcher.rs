//! AstPatcher — AST 级别代码替换 + 语法校验熔断
//!
//! replace_ast_node(path, old_text, new_text, language)：
//!   1. 读取文件
//!   2. 定位 old_text（精确字符串匹配，要求全局唯一）
//!   3. 替换为 new_text
//!   4. 经 SyntaxGate 校验新内容 → 语法错误直接打回，文件不落盘
//!   5. 原子写（写临时文件 → rename）
//!   6. 返回 AstPatchResult
use crate::{
    syntax_gate::{SyntaxGate, SyntaxLanguage},
    AciError,
};
use std::path::Path;

// ── AstPatchResult ────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct AstPatchResult {
    /// 替换发生的字节偏移量
    pub offset: usize,
    /// 替换后的完整文件内容
    pub new_content: String,
    /// 是否实际有内容变化
    pub changed: bool,
}

// ── AstPatcher ────────────────────────────────────────────────────────────────

pub struct AstPatcher;

impl AstPatcher {
    /// 在文件中用 new_text 替换 old_text，经语法校验后原子写入。
    pub fn replace_node(
        path: &Path,
        old_text: &str,
        new_text: &str,
        language: &str,
    ) -> Result<AstPatchResult, AciError> {
        let lang = SyntaxLanguage::parse_name(language);
        let original = std::fs::read_to_string(path)
            .map_err(|_| AciError::FileNotFound(path.display().to_string()))?;

        // 精确唯一性检查
        let occurrences = original.matches(old_text).count();
        if occurrences == 0 {
            return Err(AciError::PatchFailed(format!(
                "old_text not found in {}",
                path.display()
            )));
        }
        if occurrences > 1 {
            return Err(AciError::PatchFailed(format!(
                "old_text is ambiguous ({occurrences} occurrences) in {} — please provide more context",
                path.display()
            )));
        }

        let offset = original.find(old_text).unwrap();
        let new_content = original.replacen(old_text, new_text, 1);

        // 语法校验熔断 — 少一个括号直接打回重写
        let check = SyntaxGate::check(&new_content, &lang);
        if !check.is_valid() {
            if let Some(err) = check.to_aci_error(&lang) {
                return Err(err);
            }
        }

        // 原子写入（临时文件 → rename）
        let changed = new_content != original;
        if changed {
            Self::atomic_write(path, &new_content)?;
        }

        Ok(AstPatchResult {
            offset,
            new_content,
            changed,
        })
    }

    /// 在文件指定行后插入内容，经语法校验后原子写入。
    pub fn insert_after_line(
        path: &Path,
        line_number: usize,
        content_to_insert: &str,
        language: &str,
    ) -> Result<AstPatchResult, AciError> {
        let lang = SyntaxLanguage::parse_name(language);
        let original = std::fs::read_to_string(path)
            .map_err(|_| AciError::FileNotFound(path.display().to_string()))?;

        let mut lines: Vec<&str> = original.lines().collect();
        let insert_at = line_number.min(lines.len());
        let new_lines: Vec<&str> = content_to_insert.lines().collect();
        lines.splice(insert_at..insert_at, new_lines.iter().copied());

        let new_content = lines.join("\n") + if original.ends_with('\n') { "\n" } else { "" };

        let check = SyntaxGate::check(&new_content, &lang);
        if !check.is_valid() {
            if let Some(err) = check.to_aci_error(&lang) {
                return Err(err);
            }
        }

        let changed = new_content != original;
        if changed {
            Self::atomic_write(path, &new_content)?;
        }

        Ok(AstPatchResult {
            offset: insert_at,
            new_content,
            changed,
        })
    }

    /// 删除文件中精确匹配的文本块（唯一），经语法校验后写入。
    pub fn delete_text_block(
        path: &Path,
        target_text: &str,
        language: &str,
    ) -> Result<AstPatchResult, AciError> {
        Self::replace_node(path, target_text, "", language)
    }

    /// 原子写：先写临时文件，再 rename → 不产生半写状态
    fn atomic_write(path: &Path, content: &str) -> Result<(), AciError> {
        let tmp = path.with_extension("_aci_tmp");
        std::fs::write(&tmp, content)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// 纯内存替换（不写文件，用于 dry-run / 测试）
    pub fn replace_in_memory(
        original: &str,
        old_text: &str,
        new_text: &str,
        language: &str,
    ) -> Result<String, AciError> {
        let lang = SyntaxLanguage::parse_name(language);

        let occurrences = original.matches(old_text).count();
        if occurrences == 0 {
            return Err(AciError::PatchFailed("old_text not found".into()));
        }
        if occurrences > 1 {
            return Err(AciError::PatchFailed(format!(
                "old_text is ambiguous ({occurrences} occurrences)"
            )));
        }

        let new_content = original.replacen(old_text, new_text, 1);

        let check = SyntaxGate::check(&new_content, &lang);
        if !check.is_valid() {
            if let Some(err) = check.to_aci_error(&lang) {
                return Err(err);
            }
        }

        Ok(new_content)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(content: &str, ext: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("zaion_aci_{}.{}", uuid::Uuid::new_v4(), ext));
        std::fs::write(&p, content).unwrap();
        p
    }

    #[test]
    fn replace_node_in_memory_valid_rust() {
        let src = "fn foo() { let x = 1; }";
        let result =
            AstPatcher::replace_in_memory(src, "let x = 1;", "let x = 42;", "rust").unwrap();
        assert!(result.contains("let x = 42;"));
    }

    #[test]
    fn replace_node_rejects_syntax_error() {
        let src = "fn foo() { let x = 1; }";
        // 替换后产生不平衡括号
        let err = AstPatcher::replace_in_memory(src, "}", "// unclosed", "rust");
        assert!(err.is_err());
        assert!(matches!(err.unwrap_err(), AciError::SyntaxError { .. }));
    }

    #[test]
    fn replace_node_rejects_ambiguous_match() {
        let src = "x x x";
        let err = AstPatcher::replace_in_memory(src, "x", "y", "rust");
        assert!(err.is_err());
        assert!(err.unwrap_err().to_string().contains("ambiguous"));
    }

    #[test]
    fn replace_node_rejects_missing_match() {
        let src = "fn foo() {}";
        let err = AstPatcher::replace_in_memory(src, "fn bar()", "fn baz()", "rust");
        assert!(err.is_err());
        assert!(err.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn replace_node_file_patches_and_writes() {
        let f = temp_file("[core]\nkey = \"old\"", "toml");
        let result =
            AstPatcher::replace_node(&f, "key = \"old\"", "key = \"new\"", "toml").unwrap();
        assert!(result.changed);
        let content = std::fs::read_to_string(&f).unwrap();
        assert!(content.contains("key = \"new\""));
        std::fs::remove_file(&f).ok();
    }

    #[test]
    fn replace_node_file_rejects_invalid_toml_result() {
        let f = temp_file("[core]\nkey = \"value\"", "toml");
        // 替换会产生无效 TOML
        let err = AstPatcher::replace_node(&f, "key = \"value\"", "key = !!bad", "toml");
        assert!(err.is_err());
        // 文件内容未被修改（语法校验熔断）
        let content = std::fs::read_to_string(&f).unwrap();
        assert!(content.contains("key = \"value\""));
        std::fs::remove_file(&f).ok();
    }

    #[test]
    fn insert_after_line_appends_correctly() {
        let f = temp_file("{\"a\": 1}", "json");
        // JSON 插入行会破坏格式（预期语法错误），用 unknown 跳过校验
        let f2 = temp_file("line one\nline two\nline three", "txt");
        let result = AstPatcher::insert_after_line(&f2, 1, "INSERTED", "unknown").unwrap();
        assert!(result.changed);
        let content = std::fs::read_to_string(&f2).unwrap();
        assert!(content.contains("INSERTED"));
        std::fs::remove_file(&f).ok();
        std::fs::remove_file(&f2).ok();
    }

    #[test]
    fn delete_text_block_removes_unique_text() {
        let src = "fn foo() {}\nfn bar() {}\n";
        let f = temp_file(src, "rs");
        let result = AstPatcher::delete_text_block(&f, "fn bar() {}\n", "rust").unwrap();
        assert!(result.changed);
        let content = std::fs::read_to_string(&f).unwrap();
        assert!(!content.contains("fn bar()"));
        std::fs::remove_file(&f).ok();
    }
}
