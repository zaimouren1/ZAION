//! merge.rs — 致命收敛 2: AST-Level 语义冲突解决
//!
//! 核心原理：
//!   文本行级 diff 产生括号丢失/语法错误。
//!   本模块以「语义块（AstChunk）」为单位进行 diff 和合并：
//!     - 语义块 = 函数定义 / impl 块 / 结构体 / 顶层语句（按括号平衡切割）
//!     - AstDiff    — base vs branch 的块级变更列表
//!     - AstMerge   — 多分支合并：无冲突自动合并，有冲突输出 ConflictBlock
//!
//! 实现策略：
//!   不依赖 tree-sitter。使用括号平衡 + 顶层语句边界探测分割语义块。
//!   合并后强制经过 SyntaxGate 验证 → 语法错误立即检测。
use crate::{
    syntax_gate::{SyntaxGate, SyntaxLanguage},
    AciError,
};
use std::collections::HashMap;

// ── AstChunk ──────────────────────────────────────────────────────────────────

/// 一个语义块：顶层函数/impl/结构体/宏/语句。
#[derive(Debug, Clone, PartialEq)]
pub struct AstChunk {
    /// 块的文本内容（含首尾换行）
    pub text: String,
    /// SHA-256 短哈希（16 hex 字符，用于快速比较）
    pub hash: String,
    /// 块在原文件中的起始行号（0-based）
    pub line_start: usize,
}

impl AstChunk {
    pub fn new(text: String, line_start: usize) -> Self {
        let hash = short_hash(&text);
        AstChunk {
            text,
            hash,
            line_start,
        }
    }

    pub fn is_same_content(&self, other: &AstChunk) -> bool {
        self.hash == other.hash
    }
}

// ── AstChange ─────────────────────────────────────────────────────────────────

/// base → branch 的单块变化
#[derive(Debug, Clone, PartialEq)]
pub enum AstChange {
    /// 块未变化
    Unchanged { chunk: AstChunk },
    /// 块被修改（base_hash → new content）
    Modified { base: AstChunk, branch: AstChunk },
    /// 块在 branch 中被新增（base 中不存在）
    Added { chunk: AstChunk },
    /// 块在 branch 中被删除（base 中存在，branch 中没有）
    Deleted { chunk: AstChunk },
}

// ── ConflictBlock ─────────────────────────────────────────────────────────────

/// 合并冲突：同一块在多个分支中被不同方式修改
#[derive(Debug, Clone)]
pub struct ConflictBlock {
    pub base: AstChunk,
    /// branch_id → 该分支的版本
    pub variants: Vec<(String, AstChunk)>,
}

// ── MergeResult ───────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct MergeResult {
    /// 合并后的完整文本（若有冲突，冲突块用标记填充）
    pub merged: String,
    /// 自动解决的块数
    pub auto_resolved: usize,
    /// 未解决的冲突块
    pub conflicts: Vec<ConflictBlock>,
    /// 合并后语法是否通过验证
    pub syntax_valid: bool,
}

impl MergeResult {
    pub fn is_clean(&self) -> bool {
        self.conflicts.is_empty() && self.syntax_valid
    }
}

// ── AstDiff ───────────────────────────────────────────────────────────────────

pub struct AstDiff;

impl AstDiff {
    /// 将源文本按语义块切割
    pub fn chunk(source: &str) -> Vec<AstChunk> {
        chunk_source(source)
    }

    /// 计算 base → branch 的块级变更
    ///
    /// 算法：LCS（最长公共子序列）哈希对齐，O(n²) 但块数通常 < 200。
    pub fn diff(base: &str, branch: &str) -> Vec<AstChange> {
        let base_chunks = chunk_source(base);
        let branch_chunks = chunk_source(branch);

        // 构建 base hash 索引
        let base_index: HashMap<&str, usize> = base_chunks
            .iter()
            .enumerate()
            .map(|(i, c)| (c.hash.as_str(), i))
            .collect();

        let _branch_index: HashMap<&str, usize> = branch_chunks
            .iter()
            .enumerate()
            .map(|(i, c)| (c.hash.as_str(), i))
            .collect();

        let mut changes = Vec::new();
        let mut base_seen = vec![false; base_chunks.len()];
        let mut branch_seen = vec![false; branch_chunks.len()];

        // Pass 1: match identical chunks
        for (bi, bc) in branch_chunks.iter().enumerate() {
            if let Some(&base_idx) = base_index.get(bc.hash.as_str()) {
                if !base_seen[base_idx] {
                    base_seen[base_idx] = true;
                    branch_seen[bi] = true;
                    changes.push((base_idx, bi, AstChange::Unchanged { chunk: bc.clone() }));
                }
            }
        }

        // Pass 2: unmatched base chunks → Deleted or Modified
        for (bi, bc) in base_chunks.iter().enumerate() {
            if !base_seen[bi] {
                // Find unmatched branch chunk nearby (simple heuristic: first unmatched)
                let modified = branch_chunks
                    .iter()
                    .enumerate()
                    .find(|(bri, _)| !branch_seen[*bri]);

                if let Some((bri, brc)) = modified {
                    branch_seen[bri] = true;
                    changes.push((
                        bi,
                        bri,
                        AstChange::Modified {
                            base: bc.clone(),
                            branch: brc.clone(),
                        },
                    ));
                } else {
                    changes.push((bi, usize::MAX, AstChange::Deleted { chunk: bc.clone() }));
                }
            }
        }

        // Pass 3: unmatched branch chunks → Added
        for (bri, brc) in branch_chunks.iter().enumerate() {
            if !branch_seen[bri] {
                changes.push((usize::MAX, bri, AstChange::Added { chunk: brc.clone() }));
            }
        }

        // Sort by base index then branch index for deterministic output
        changes.sort_by_key(|(bi, bri, _)| (*bi, *bri));
        changes.into_iter().map(|(_, _, c)| c).collect()
    }
}

// ── AstMergeResolver ─────────────────────────────────────────────────────────

pub struct AstMergeResolver;

impl AstMergeResolver {
    /// 合并 base 与一个或多个 branch 版本。
    ///
    /// 规则：
    ///   - 所有分支均未修改该块 → Unchanged
    ///   - 只有一个分支修改了该块 → 自动采用该分支版本
    ///   - 多个分支对同一块做了不同修改 → ConflictBlock
    ///   - 一个分支删除、另一个修改 → ConflictBlock
    ///
    /// 合并完成后强制经 SyntaxGate 验证。
    pub fn merge(
        base: &str,
        branches: &[(&str, &str)], // (branch_id, branch_content)
        language: &str,
    ) -> MergeResult {
        let lang = SyntaxLanguage::parse_name(language);
        let base_chunks = chunk_source(base);

        // 每个 base chunk hash → 各分支的处理决策
        // key: base chunk hash
        // value: Vec<(branch_id, AstChange)>
        let mut chunk_decisions: HashMap<String, Vec<(String, AstChange)>> = HashMap::new();

        // 跟踪各分支新增的块
        let mut additions: Vec<AstChunk> = Vec::new();

        for (branch_id, branch_content) in branches {
            let changes = AstDiff::diff(base, branch_content);
            for change in changes {
                match &change {
                    AstChange::Unchanged { chunk } => {
                        chunk_decisions
                            .entry(chunk.hash.clone())
                            .or_default()
                            .push((branch_id.to_string(), change));
                    }
                    AstChange::Modified { base, .. } => {
                        chunk_decisions
                            .entry(base.hash.clone())
                            .or_default()
                            .push((branch_id.to_string(), change));
                    }
                    AstChange::Deleted { chunk } => {
                        chunk_decisions
                            .entry(chunk.hash.clone())
                            .or_default()
                            .push((branch_id.to_string(), change));
                    }
                    AstChange::Added { chunk } => {
                        additions.push(chunk.clone());
                    }
                }
            }
        }

        let mut merged_chunks: Vec<String> = Vec::new();
        let mut conflicts: Vec<ConflictBlock> = Vec::new();
        let mut auto_resolved = 0usize;

        for base_chunk in &base_chunks {
            let decisions = chunk_decisions
                .get(&base_chunk.hash)
                .map(|v| v.as_slice())
                .unwrap_or(&[]);

            // Collect unique outcomes (branch may duplicate Unchanged)
            let mut modifications: Vec<(&str, &AstChunk)> = Vec::new();
            let mut deletions: Vec<&str> = Vec::new();
            let mut unchanged_count = 0usize;

            for (bid, change) in decisions {
                match change {
                    AstChange::Unchanged { .. } => unchanged_count += 1,
                    AstChange::Modified { branch, .. } => modifications.push((bid, branch)),
                    AstChange::Deleted { .. } => deletions.push(bid),
                    AstChange::Added { .. } => {}
                }
            }

            let total_branches = branches.len();
            let touched = modifications.len() + deletions.len();

            if touched == 0 {
                // All branches left it unchanged (or not seen = also unchanged)
                merged_chunks.push(base_chunk.text.clone());
                auto_resolved += 1;
            } else if deletions.is_empty() && modifications.len() == 1 {
                // Exactly one branch modified it, rest unchanged
                merged_chunks.push(modifications[0].1.text.clone());
                auto_resolved += 1;
            } else if modifications.len() == 1
                && deletions.is_empty()
                && unchanged_count + 1 == total_branches
            {
                // One branch modified, all others unchanged
                merged_chunks.push(modifications[0].1.text.clone());
                auto_resolved += 1;
            } else {
                // Real conflict: multiple branches changed this chunk differently
                let variants: Vec<(String, AstChunk)> = modifications
                    .iter()
                    .map(|(bid, chunk)| (bid.to_string(), (*chunk).clone()))
                    .collect();

                // If all modifications are identical → auto-resolve
                if !variants.is_empty() {
                    let first_hash = &variants[0].1.hash;
                    if variants.iter().all(|(_, c)| &c.hash == first_hash) && deletions.is_empty() {
                        merged_chunks.push(variants[0].1.text.clone());
                        auto_resolved += 1;
                        continue;
                    }
                }

                // Mark conflict in output
                merged_chunks.push(format!(
                    "<<<<<<< base\n{}\n=======\n{}\n>>>>>>> conflict\n",
                    base_chunk.text,
                    variants
                        .first()
                        .map(|(_, c)| c.text.as_str())
                        .unwrap_or("(deleted)"),
                ));
                conflicts.push(ConflictBlock {
                    base: base_chunk.clone(),
                    variants,
                });
            }
        }

        // Append additions (all branches' new chunks, deduplicated by hash)
        let mut seen_hashes: std::collections::HashSet<String> = std::collections::HashSet::new();
        for chunk in &additions {
            if seen_hashes.insert(chunk.hash.clone()) {
                merged_chunks.push(chunk.text.clone());
            }
        }

        let merged = merged_chunks.join("");

        // Syntax validation (only if no conflict markers present)
        let syntax_valid = if conflicts.is_empty() {
            let check = SyntaxGate::check(&merged, &lang);
            check.is_valid()
        } else {
            false // conflict markers always fail syntax
        };

        MergeResult {
            merged,
            auto_resolved,
            conflicts,
            syntax_valid,
        }
    }

    /// 应用 LLM 决策解决单个冲突块。
    ///
    /// resolved_text: LLM 输出的最终版本。
    /// 经 SyntaxGate 验证后替换 conflict marker。
    pub fn apply_resolution(
        merged_with_markers: &str,
        conflict: &ConflictBlock,
        resolved_text: &str,
        language: &str,
    ) -> Result<String, AciError> {
        let lang = SyntaxLanguage::parse_name(language);

        // Build the conflict marker that appears in the merged text
        let marker = format!(
            "<<<<<<< base\n{}\n=======\n{}\n>>>>>>> conflict\n",
            conflict.base.text,
            conflict
                .variants
                .first()
                .map(|(_, c)| c.text.as_str())
                .unwrap_or("(deleted)"),
        );

        if !merged_with_markers.contains(&marker) {
            return Err(AciError::PatchFailed(
                "conflict marker not found in merged text".into(),
            ));
        }

        let result = merged_with_markers.replacen(&marker, resolved_text, 1);

        // Syntax check after resolution
        let check = SyntaxGate::check(&result, &lang);
        if !check.is_valid() {
            if let Some(err) = check.to_aci_error(&lang) {
                return Err(err);
            }
        }

        Ok(result)
    }
}

// ── Chunk splitter ────────────────────────────────────────────────────────────

/// 将源代码按顶层语义块切割。
///
/// 策略：
///   1. 空行序列 + 括号平衡 → 块边界
///   2. 每个顶层 `fn`/`impl`/`struct`/`enum`/`type`/`mod`/`#[` 开头 → 新块起点
///   3. 保持括号平衡（避免在函数体中间切割）
fn chunk_source(source: &str) -> Vec<AstChunk> {
    if source.trim().is_empty() {
        return Vec::new();
    }

    let lines: Vec<&str> = source.lines().collect();
    let mut chunks: Vec<AstChunk> = Vec::new();
    let mut current_lines: Vec<&str> = Vec::new();
    let mut current_start = 0usize;
    let mut depth: i32 = 0; // brace/bracket depth

    let top_level_keywords = [
        "fn ",
        "pub fn ",
        "async fn ",
        "pub async fn ",
        "impl ",
        "pub impl ",
        "struct ",
        "pub struct ",
        "enum ",
        "pub enum ",
        "type ",
        "pub type ",
        "mod ",
        "pub mod ",
        "trait ",
        "pub trait ",
        "const ",
        "pub const ",
        "static ",
        "pub static ",
        "macro_rules!",
        "use ",
        "pub use ",
        "#[",
    ];

    for (line_idx, &line) in lines.iter().enumerate() {
        let trimmed = line.trim();

        // Detect start of a new top-level block (only when depth == 0)
        let is_new_block_start = depth == 0
            && !current_lines.is_empty()
            && !trimmed.is_empty()
            && top_level_keywords.iter().any(|kw| trimmed.starts_with(kw));

        if is_new_block_start {
            let text = current_lines.join("\n") + "\n";
            if !text.trim().is_empty() {
                chunks.push(AstChunk::new(text, current_start));
            }
            current_lines = Vec::new();
            current_start = line_idx;
        }

        current_lines.push(line);

        // Track brace depth
        for ch in line.chars() {
            match ch {
                '{' | '(' | '[' => depth += 1,
                '}' | ')' | ']' => depth -= 1,
                _ => {}
            }
        }

        // When depth returns to 0 after content, consider flushing
        if depth == 0 && !current_lines.is_empty() {
            let text_so_far = current_lines.join("\n");
            // Only flush if this looks like a complete unit (non-trivial)
            if text_so_far.trim().len() > 1 {
                // Look ahead: if next line is blank or new keyword → flush
                let next = lines.get(line_idx + 1).copied().unwrap_or("").trim();
                if next.is_empty() || top_level_keywords.iter().any(|kw| next.starts_with(kw)) {
                    let text = text_so_far + "\n";
                    chunks.push(AstChunk::new(text, current_start));
                    current_lines = Vec::new();
                    current_start = line_idx + 1;
                }
            }
        }
    }

    // Flush remaining
    if !current_lines.is_empty() {
        let text = current_lines.join("\n") + "\n";
        if !text.trim().is_empty() {
            chunks.push(AstChunk::new(text, current_start));
        }
    }

    // If no chunks produced (e.g. flat text), return whole source as one chunk
    if chunks.is_empty() {
        chunks.push(AstChunk::new(source.to_string(), 0));
    }

    chunks
}

// ── Hash helper ───────────────────────────────────────────────────────────────

fn short_hash(text: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    text.hash(&mut h);
    format!("{:016x}", h.finish())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── chunk tests ──────────────────────────────────────────────────────────

    #[test]
    fn chunk_single_function() {
        let src = "fn foo() {\n    let x = 1;\n}\n";
        let chunks = AstDiff::chunk(src);
        assert!(!chunks.is_empty());
        assert!(chunks[0].text.contains("fn foo()"));
    }

    #[test]
    fn chunk_two_functions() {
        let src = "fn foo() {}\n\nfn bar() {}\n";
        let chunks = AstDiff::chunk(src);
        assert_eq!(
            chunks.len(),
            2,
            "expected 2 chunks, got {}: {:?}",
            chunks.len(),
            chunks.iter().map(|c| &c.text).collect::<Vec<_>>()
        );
    }

    #[test]
    fn chunk_struct_and_impl() {
        let src = "struct Foo {\n    x: i32,\n}\n\nimpl Foo {\n    fn new() -> Self { Foo { x: 0 } }\n}\n";
        let chunks = AstDiff::chunk(src);
        assert!(chunks.len() >= 2);
    }

    #[test]
    fn same_content_has_same_hash() {
        let c1 = AstChunk::new("fn foo() {}\n".into(), 0);
        let c2 = AstChunk::new("fn foo() {}\n".into(), 5);
        assert!(c1.is_same_content(&c2));
    }

    #[test]
    fn different_content_has_different_hash() {
        let c1 = AstChunk::new("fn foo() {}\n".into(), 0);
        let c2 = AstChunk::new("fn bar() {}\n".into(), 0);
        assert!(!c1.is_same_content(&c2));
    }

    // ── diff tests ───────────────────────────────────────────────────────────

    #[test]
    fn diff_identical_sources_all_unchanged() {
        let src = "fn foo() {}\n\nfn bar() {}\n";
        let changes = AstDiff::diff(src, src);
        assert!(changes
            .iter()
            .all(|c| matches!(c, AstChange::Unchanged { .. })));
    }

    #[test]
    fn diff_detects_modification() {
        let base = "fn foo() { let x = 1; }\n\nfn bar() {}\n";
        let branch = "fn foo() { let x = 42; }\n\nfn bar() {}\n";
        let changes = AstDiff::diff(base, branch);
        let has_modified = changes
            .iter()
            .any(|c| matches!(c, AstChange::Modified { .. }));
        assert!(has_modified, "expected Modified change");
    }

    #[test]
    fn diff_detects_addition() {
        let base = "fn foo() {}\n";
        let branch = "fn foo() {}\n\nfn new_fn() {}\n";
        let changes = AstDiff::diff(base, branch);
        let has_added = changes.iter().any(|c| matches!(c, AstChange::Added { .. }));
        assert!(has_added, "expected Added change");
    }

    #[test]
    fn diff_detects_deletion() {
        let base = "fn foo() {}\n\nfn bar() {}\n";
        let branch = "fn foo() {}\n";
        let changes = AstDiff::diff(base, branch);
        let has_deleted = changes
            .iter()
            .any(|c| matches!(c, AstChange::Deleted { .. }));
        assert!(has_deleted, "expected Deleted change");
    }

    // ── merge tests ──────────────────────────────────────────────────────────

    #[test]
    fn merge_no_changes_is_clean() {
        let base = "fn foo() {}\n\nfn bar() {}\n";
        let result = AstMergeResolver::merge(base, &[("b1", base), ("b2", base)], "rust");
        assert!(result.conflicts.is_empty());
        assert_eq!(result.auto_resolved, AstDiff::chunk(base).len());
    }

    #[test]
    fn merge_single_branch_modification_auto_resolves() {
        let base = "fn foo() { let x = 1; }\n\nfn bar() {}\n";
        let branch1 = "fn foo() { let x = 42; }\n\nfn bar() {}\n"; // modified foo
        let branch2 = base; // unchanged

        let result = AstMergeResolver::merge(base, &[("b1", branch1), ("b2", branch2)], "rust");
        assert!(
            result.conflicts.is_empty(),
            "expected no conflicts, got: {:?}",
            result
                .conflicts
                .iter()
                .map(|c| &c.base.text)
                .collect::<Vec<_>>()
        );
        assert!(
            result.merged.contains("let x = 42;"),
            "modified version should win"
        );
    }

    #[test]
    fn merge_two_branches_modify_same_chunk_creates_conflict() {
        let base = "fn foo() { let x = 1; }\n\nfn bar() {}\n";
        let branch1 = "fn foo() { let x = 42; }\n\nfn bar() {}\n";
        let branch2 = "fn foo() { let x = 99; }\n\nfn bar() {}\n";

        let result = AstMergeResolver::merge(base, &[("b1", branch1), ("b2", branch2)], "rust");
        assert!(!result.conflicts.is_empty(), "expected conflict");
        assert!(!result.is_clean());
    }

    #[test]
    fn merge_identical_modifications_auto_resolves() {
        let base = "fn foo() { 1 }\n\nfn bar() {}\n";
        let branch1 = "fn foo() { 42 }\n\nfn bar() {}\n";
        let branch2 = "fn foo() { 42 }\n\nfn bar() {}\n"; // same change

        let result = AstMergeResolver::merge(base, &[("b1", branch1), ("b2", branch2)], "rust");
        assert!(
            result.conflicts.is_empty(),
            "identical modifications should auto-resolve"
        );
        assert!(result.merged.contains("42"));
    }

    #[test]
    fn merge_additions_from_single_branch_included() {
        let base = "fn foo() {}\n";
        let branch1 = "fn foo() {}\n\nfn new_fn() {}\n";
        let branch2 = base;

        let result = AstMergeResolver::merge(base, &[("b1", branch1), ("b2", branch2)], "rust");
        assert!(result.conflicts.is_empty());
        assert!(
            result.merged.contains("new_fn"),
            "added function should appear in merge"
        );
    }

    #[test]
    fn apply_resolution_fixes_conflict_and_validates_syntax() {
        let base = "fn foo() { let x = 1; }\n\nfn bar() {}\n";
        let branch1 = "fn foo() { let x = 42; }\n\nfn bar() {}\n";
        let branch2 = "fn foo() { let x = 99; }\n\nfn bar() {}\n";

        let result = AstMergeResolver::merge(base, &[("b1", branch1), ("b2", branch2)], "rust");
        assert!(!result.conflicts.is_empty());
        let conflict = &result.conflicts[0];

        // LLM "decides" to use x = 42
        let resolution = "fn foo() { let x = 42; }\n";
        let resolved =
            AstMergeResolver::apply_resolution(&result.merged, conflict, resolution, "rust")
                .unwrap();
        assert!(resolved.contains("let x = 42;"));
        assert!(!resolved.contains("<<<<<<<"));
    }

    #[test]
    fn apply_resolution_rejects_invalid_syntax() {
        let base = "fn foo() { let x = 1; }\n\nfn bar() {}\n";
        let branch1 = "fn foo() { let x = 42; }\n\nfn bar() {}\n";
        let branch2 = "fn foo() { let x = 99; }\n\nfn bar() {}\n";

        let result = AstMergeResolver::merge(base, &[("b1", branch1), ("b2", branch2)], "rust");
        let conflict = &result.conflicts[0];

        // LLM outputs broken syntax
        let bad_resolution = "fn foo() { let x = {{{ }\n"; // unbalanced
        let err =
            AstMergeResolver::apply_resolution(&result.merged, conflict, bad_resolution, "rust");
        assert!(err.is_err(), "invalid syntax should be rejected");
    }

    #[test]
    fn merge_five_shadow_processes_no_overlap_clean() {
        // Simulate 5 shadow processes each modifying a different function
        let base = concat!(
            "fn f1() { 1 }\n\n",
            "fn f2() { 2 }\n\n",
            "fn f3() { 3 }\n\n",
            "fn f4() { 4 }\n\n",
            "fn f5() { 5 }\n",
        );

        let b1 = base.replace("fn f1() { 1 }", "fn f1() { 10 }");
        let b2 = base.replace("fn f2() { 2 }", "fn f2() { 20 }");
        let b3 = base.replace("fn f3() { 3 }", "fn f3() { 30 }");
        let b4 = base.replace("fn f4() { 4 }", "fn f4() { 40 }");
        let b5 = base.replace("fn f5() { 5 }", "fn f5() { 50 }");

        let result = AstMergeResolver::merge(
            base,
            &[
                ("shadow-0", b1.as_str()),
                ("shadow-1", b2.as_str()),
                ("shadow-2", b3.as_str()),
                ("shadow-3", b4.as_str()),
                ("shadow-4", b5.as_str()),
            ],
            "rust",
        );

        // All 5 modifications should auto-resolve without conflicts
        assert!(
            result.conflicts.is_empty(),
            "5 non-overlapping modifications should produce zero conflicts, got: {:?}",
            result
                .conflicts
                .iter()
                .map(|c| &c.base.text)
                .collect::<Vec<_>>()
        );
    }
}
