//! Static codebase scanner — finds improvement opportunities without LLM.

use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::ast_scanner::AstScanner;

/// The kind of improvement opportunity detected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FindingKind {
    /// `// TODO`, `// FIXME`, `// HACK` comment in source
    TodoComment,
    /// `unwrap()` call outside of test code (Rust-only)
    UnwrapInProd,
    /// Public function with no doc comment
    UndocumentedPubFn,
    /// File exceeding the line budget (default 800)
    OversizedFile,
    /// Function exceeding the line budget (default 50)
    OversizedFunction,
    /// `panic!()` call in non-test code (Rust-only)
    PanicInProd,
    /// `clone()` on a large type (heuristic: `.clone()` in hot-path loop)
    ExpensiveClone,
    /// Bare `except: pass` or `except Exception: pass` in Python
    BareExcept,
    /// `console.log(` in non-test TypeScript/JavaScript files
    ConsoleLog,
    /// `any` type annotation in TypeScript (`: any` or `as any`)
    AnyType,
}

impl std::fmt::Display for FindingKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TodoComment => write!(f, "TODO/FIXME comment"),
            Self::UnwrapInProd => write!(f, "unwrap() in production code"),
            Self::UndocumentedPubFn => write!(f, "undocumented public function"),
            Self::OversizedFile => write!(f, "oversized file (>800 lines)"),
            Self::OversizedFunction => write!(f, "oversized function (>50 lines)"),
            Self::PanicInProd => write!(f, "panic!() in production code"),
            Self::ExpensiveClone => write!(f, "potentially expensive clone"),
            Self::BareExcept => write!(f, "bare except clause"),
            Self::ConsoleLog => write!(f, "console.log in production code"),
            Self::AnyType => write!(f, "TypeScript 'any' type annotation"),
        }
    }
}

/// A single improvement opportunity found in the codebase.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub kind: FindingKind,
    pub file: String,
    pub line: usize,
    /// Relevant source snippet (up to 5 lines).
    pub snippet: String,
    /// Priority: 0=low, 1=medium, 2=high
    pub priority: u8,
}

impl Finding {
    pub fn summary(&self) -> String {
        format!(
            "[{}] {}:{} — {}",
            self.kind,
            self.file,
            self.line,
            self.snippet.lines().next().unwrap_or("")
        )
    }
}

/// Scanner configuration.
#[derive(Debug, Clone)]
pub struct ScanConfig {
    pub max_file_lines: usize,
    pub max_fn_lines: usize,
    /// File extensions to scan.
    pub extensions: Vec<String>,
    /// Directories to skip.
    pub skip_dirs: Vec<String>,
}

impl Default for ScanConfig {
    fn default() -> Self {
        Self {
            max_file_lines: 800,
            max_fn_lines: 50,
            extensions: vec!["rs".to_string()],
            skip_dirs: vec!["target".to_string(), ".git".to_string()],
        }
    }
}

/// Codebase scanner.
pub struct Scanner {
    config: ScanConfig,
}

impl Scanner {
    pub fn new(config: ScanConfig) -> Self {
        Self { config }
    }

    /// Scan all matching files under `workspace_root` and return findings.
    pub fn scan(&self, workspace_root: &Path) -> Vec<Finding> {
        let mut findings = Vec::new();
        self.scan_dir(workspace_root, workspace_root, &mut findings);
        // Sort: high priority first, then by file+line.
        findings.sort_by(|a, b| b.priority.cmp(&a.priority).then(a.file.cmp(&b.file)));
        findings
    }

    fn scan_dir(&self, root: &Path, dir: &Path, out: &mut Vec<Finding>) {
        // Attempt to initialise the AST scanner once per scan_dir invocation.
        // If tree-sitter is unavailable the heuristic path is used as fallback.
        let mut ast = AstScanner::new();
        self.scan_dir_inner(root, dir, &mut ast, out);
    }

    fn scan_dir_inner(
        &self,
        root: &Path,
        dir: &Path,
        ast: &mut Option<AstScanner>,
        out: &mut Vec<Finding>,
    ) {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

            if path.is_dir() {
                if !self.config.skip_dirs.iter().any(|s| s == name) {
                    self.scan_dir_inner(root, &path, ast, out);
                }
                continue;
            }

            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if !self.config.extensions.iter().any(|e| e == ext) {
                continue;
            }

            if let Ok(content) = std::fs::read_to_string(&path) {
                let rel = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");

                // Skip test files for certain checks
                let is_test_file = rel.contains("/tests/")
                    || rel.ends_with("_test.rs")
                    || content.contains("#[cfg(test)]");

                self.scan_content(&rel, ext, &content, is_test_file, ast, out);
            }
        }
    }

    fn scan_content(
        &self,
        rel_path: &str,
        file_ext: &str,
        content: &str,
        is_test: bool,
        ast: &mut Option<AstScanner>,
        out: &mut Vec<Finding>,
    ) {
        let lines: Vec<&str> = content.lines().collect();
        let total = lines.len();

        // OversizedFile
        if total > self.config.max_file_lines {
            out.push(Finding {
                kind: FindingKind::OversizedFile,
                file: rel_path.to_string(),
                line: total,
                snippet: format!("{} lines (limit {})", total, self.config.max_file_lines),
                priority: 1,
            });
        }

        let mut fn_start: Option<usize> = None;
        let mut brace_depth: i32 = 0;
        let mut fn_start_depth: i32 = 0;

        for (i, &line) in lines.iter().enumerate() {
            let ln = i + 1;
            let trimmed = line.trim();

            // --- Language-agnostic: TODO/FIXME/HACK ---
            // Rust single-line comment
            let is_line_comment_rs = trimmed.starts_with("//");
            // Python single-line comment
            let is_line_comment_py = trimmed.starts_with('#');

            if is_line_comment_rs || is_line_comment_py {
                let upper = trimmed.to_uppercase();
                if upper.contains("TODO") || upper.contains("FIXME") || upper.contains("HACK") {
                    out.push(Finding {
                        kind: FindingKind::TodoComment,
                        file: rel_path.to_string(),
                        line: ln,
                        snippet: trimmed.chars().take(80).collect(),
                        priority: 0,
                    });
                }
            }

            // --- Rust-specific checks ---
            if file_ext == "rs" {
                // unwrap() in prod code
                if !is_test && trimmed.contains(".unwrap()") && !trimmed.starts_with("//") {
                    out.push(Finding {
                        kind: FindingKind::UnwrapInProd,
                        file: rel_path.to_string(),
                        line: ln,
                        snippet: trimmed.chars().take(80).collect(),
                        priority: 2,
                    });
                }

                // panic! in prod code
                if !is_test && trimmed.starts_with("panic!(") && !trimmed.starts_with("//") {
                    out.push(Finding {
                        kind: FindingKind::PanicInProd,
                        file: rel_path.to_string(),
                        line: ln,
                        snippet: trimmed.chars().take(80).collect(),
                        priority: 2,
                    });
                }

                // UndocumentedPubFn + OversizedFunction:
                // Use AST-backed scanner when available; fall back to heuristic otherwise.
                // These are emitted once per file (below the per-line loop), so we skip
                // them here — see the post-loop block.

                // Track brace depth for heuristic function size (fallback path only)
                for ch in trimmed.chars() {
                    match ch {
                        '{' => brace_depth += 1,
                        '}' => brace_depth -= 1,
                        _ => {}
                    }
                }

                // Heuristic: detect function boundaries (used only when ast is None)
                if ast.is_none() {
                    if trimmed.starts_with("pub fn ") || trimmed.starts_with("pub async fn ") {
                        let prev_is_doc = i > 0 && lines[i - 1].trim().starts_with("///");
                        let prev_is_attr = i > 0 && lines[i - 1].trim().starts_with("#[");
                        let preceded_by_doc = prev_is_doc
                            || (prev_is_attr && i > 1 && lines[i - 2].trim().starts_with("///"));
                        if !preceded_by_doc && !is_test {
                            out.push(Finding {
                                kind: FindingKind::UndocumentedPubFn,
                                file: rel_path.to_string(),
                                line: ln,
                                snippet: trimmed.chars().take(80).collect(),
                                priority: 0,
                            });
                        }
                        fn_start = Some(ln);
                        fn_start_depth = brace_depth - 1; // brace already counted above
                    }

                    if let Some(start) = fn_start {
                        if brace_depth <= fn_start_depth && ln > start {
                            let fn_len = ln - start;
                            if fn_len > self.config.max_fn_lines {
                                out.push(Finding {
                                    kind: FindingKind::OversizedFunction,
                                    file: rel_path.to_string(),
                                    line: start,
                                    snippet: format!(
                                        "function ~{} lines (limit {})",
                                        fn_len, self.config.max_fn_lines
                                    ),
                                    priority: 1,
                                });
                            }
                            fn_start = None;
                        }
                    }
                }

                // ExpensiveClone: .clone() inside a loop context
                // Check if there is a `for ` or `while ` within the preceding 10 lines
                if !trimmed.starts_with("//") && trimmed.contains(".clone()") {
                    let look_back = i.saturating_sub(10);
                    let in_loop = lines[look_back..i].iter().any(|prev| {
                        let p = prev.trim();
                        (p.contains("for ") || p.contains("while ")) && !p.starts_with("//")
                    });
                    if in_loop {
                        out.push(Finding {
                            kind: FindingKind::ExpensiveClone,
                            file: rel_path.to_string(),
                            line: ln,
                            snippet: trimmed.chars().take(80).collect(),
                            priority: 1,
                        });
                    }
                }
            }

            // --- Python-specific checks ---
            if file_ext == "py" {
                // Bare except: `except:` or `except Exception:` followed by `pass`
                // We detect the pattern on the except line itself combined with next line
                let trimmed_lower = trimmed.to_lowercase();
                let is_bare_except = trimmed_lower == "except:"
                    || trimmed_lower.starts_with("except exception:")
                    || trimmed_lower.starts_with("except baseexception:");
                if is_bare_except {
                    // Check next line for `pass`
                    let next_is_pass = lines
                        .get(i + 1)
                        .map(|l| l.trim().to_lowercase() == "pass")
                        .unwrap_or(false);
                    // Also detect single-line: `except: pass` or `except Exception: pass`
                    let same_line_pass =
                        trimmed_lower.contains(": pass") || trimmed_lower.ends_with(":pass");
                    if next_is_pass || same_line_pass {
                        out.push(Finding {
                            kind: FindingKind::BareExcept,
                            file: rel_path.to_string(),
                            line: ln,
                            snippet: trimmed.chars().take(80).collect(),
                            priority: 1,
                        });
                    }
                }
            }

            // --- TypeScript/JavaScript-specific checks ---
            if file_ext == "ts" || file_ext == "tsx" || file_ext == "js" || file_ext == "jsx" {
                // console.log in non-test files
                if !is_test && trimmed.contains("console.log(") && !trimmed.starts_with("//") {
                    out.push(Finding {
                        kind: FindingKind::ConsoleLog,
                        file: rel_path.to_string(),
                        line: ln,
                        snippet: trimmed.chars().take(80).collect(),
                        priority: 0,
                    });
                }

                // `any` type annotation: `: any` or `as any`
                if !trimmed.starts_with("//")
                    && (trimmed.contains(": any") || trimmed.contains("as any"))
                {
                    out.push(Finding {
                        kind: FindingKind::AnyType,
                        file: rel_path.to_string(),
                        line: ln,
                        snippet: trimmed.chars().take(80).collect(),
                        priority: 1,
                    });
                }
            }
        }

        // --- AST-backed Rust checks (post-loop, once per file) ---
        // When tree-sitter is available, use it for accurate OversizedFunction and
        // UndocumentedPubFn detection.  The per-line heuristic above already handles
        // these when `ast` is None.
        if file_ext == "rs" && !is_test {
            if let Some(ref mut ast_scanner) = ast {
                let oversized = ast_scanner.scan_oversized_functions(
                    content,
                    rel_path,
                    self.config.max_fn_lines,
                );
                out.extend(oversized);

                let undoc = ast_scanner.scan_undocumented_pub_fns(content, rel_path);
                out.extend(undoc);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn write(dir: &Path, name: &str, content: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, content).unwrap();
        p
    }

    #[test]
    fn detects_todo_comment() {
        let dir = tempdir().unwrap();
        write(dir.path(), "a.rs", "fn foo() {\n    // TODO: fix this\n}\n");
        let s = Scanner::new(ScanConfig::default());
        let f = s.scan(dir.path());
        assert!(f.iter().any(|x| x.kind == FindingKind::TodoComment));
    }

    #[test]
    fn detects_unwrap_in_prod() {
        let dir = tempdir().unwrap();
        write(
            dir.path(),
            "a.rs",
            "fn foo() {\n    let x = bar().unwrap();\n}\n",
        );
        let s = Scanner::new(ScanConfig::default());
        let f = s.scan(dir.path());
        assert!(f.iter().any(|x| x.kind == FindingKind::UnwrapInProd));
    }

    #[test]
    fn no_unwrap_in_test_files() {
        let dir = tempdir().unwrap();
        write(
            dir.path(),
            "a.rs",
            "#[cfg(test)]\nfn foo() {\n    let x = bar().unwrap();\n}\n",
        );
        let s = Scanner::new(ScanConfig::default());
        let f = s.scan(dir.path());
        assert!(!f.iter().any(|x| x.kind == FindingKind::UnwrapInProd));
    }

    #[test]
    fn detects_oversized_file() {
        let dir = tempdir().unwrap();
        let big: String = (0..850).map(|i| format!("// line {}\n", i)).collect();
        write(dir.path(), "big.rs", &big);
        let s = Scanner::new(ScanConfig::default());
        let f = s.scan(dir.path());
        assert!(f.iter().any(|x| x.kind == FindingKind::OversizedFile));
    }

    #[test]
    fn skips_target_dir() {
        let dir = tempdir().unwrap();
        std::fs::create_dir(dir.path().join("target")).unwrap();
        write(
            &dir.path().join("target"),
            "a.rs",
            "fn foo() { let x = bar().unwrap(); }\n",
        );
        let s = Scanner::new(ScanConfig::default());
        let f = s.scan(dir.path());
        assert!(!f.iter().any(|x| x.kind == FindingKind::UnwrapInProd));
    }

    #[test]
    fn scan_empty_dir_returns_empty() {
        let dir = tempdir().unwrap();
        let s = Scanner::new(ScanConfig::default());
        assert!(s.scan(dir.path()).is_empty());
    }

    #[test]
    fn detects_expensive_clone_in_loop() {
        let dir = tempdir().unwrap();
        let content = "\
fn process(items: &[String]) {
    for item in items {
        let owned = item.clone();
        println!(\"{}\", owned);
    }
}
";
        write(dir.path(), "a.rs", content);
        let s = Scanner::new(ScanConfig::default());
        let f = s.scan(dir.path());
        assert!(
            f.iter().any(|x| x.kind == FindingKind::ExpensiveClone),
            "expected ExpensiveClone finding but got: {:?}",
            f.iter().map(|x| &x.kind).collect::<Vec<_>>()
        );
    }

    #[test]
    fn no_expensive_clone_outside_loop() {
        let dir = tempdir().unwrap();
        let content = "\
fn setup(item: &String) -> String {
    item.clone()
}
";
        write(dir.path(), "a.rs", content);
        let s = Scanner::new(ScanConfig::default());
        let f = s.scan(dir.path());
        assert!(!f.iter().any(|x| x.kind == FindingKind::ExpensiveClone));
    }

    #[test]
    fn detects_bare_except_in_python() {
        let dir = tempdir().unwrap();
        let content = "\
def risky():
    try:
        do_something()
    except Exception:
        pass
";
        write(dir.path(), "a.py", content);
        let config = ScanConfig {
            extensions: vec!["py".to_string()],
            ..ScanConfig::default()
        };
        let s = Scanner::new(config);
        let f = s.scan(dir.path());
        assert!(
            f.iter().any(|x| x.kind == FindingKind::BareExcept),
            "expected BareExcept finding but got: {:?}",
            f.iter().map(|x| &x.kind).collect::<Vec<_>>()
        );
    }

    #[test]
    fn detects_bare_except_plain() {
        let dir = tempdir().unwrap();
        let content = "\
def risky():
    try:
        do_something()
    except:
        pass
";
        write(dir.path(), "a.py", content);
        let config = ScanConfig {
            extensions: vec!["py".to_string()],
            ..ScanConfig::default()
        };
        let s = Scanner::new(config);
        let f = s.scan(dir.path());
        assert!(f.iter().any(|x| x.kind == FindingKind::BareExcept));
    }

    #[test]
    fn detects_console_log_in_ts() {
        let dir = tempdir().unwrap();
        let content = "\
export function greet(name: string): string {
    console.log(\"hello\", name);
    return `Hello, ${name}`;
}
";
        write(dir.path(), "greet.ts", content);
        let config = ScanConfig {
            extensions: vec!["ts".to_string()],
            ..ScanConfig::default()
        };
        let s = Scanner::new(config);
        let f = s.scan(dir.path());
        assert!(
            f.iter().any(|x| x.kind == FindingKind::ConsoleLog),
            "expected ConsoleLog finding but got: {:?}",
            f.iter().map(|x| &x.kind).collect::<Vec<_>>()
        );
    }

    #[test]
    fn detects_any_type_in_ts() {
        let dir = tempdir().unwrap();
        let content = "\
function process(data: any): void {
    const result = data as any;
    console.log(result);
}
";
        write(dir.path(), "proc.ts", content);
        let config = ScanConfig {
            extensions: vec!["ts".to_string()],
            ..ScanConfig::default()
        };
        let s = Scanner::new(config);
        let f = s.scan(dir.path());
        let any_findings: Vec<_> = f
            .iter()
            .filter(|x| x.kind == FindingKind::AnyType)
            .collect();
        assert!(
            any_findings.len() >= 2,
            "expected at least 2 AnyType findings (': any' and 'as any') but got: {:?}",
            any_findings
        );
    }

    #[test]
    fn no_console_log_in_commented_line_ts() {
        let dir = tempdir().unwrap();
        let content = "\
// console.log(\"this is commented out\");
export function noop(): void {}
";
        write(dir.path(), "noop.ts", content);
        let config = ScanConfig {
            extensions: vec!["ts".to_string()],
            ..ScanConfig::default()
        };
        let s = Scanner::new(config);
        let f = s.scan(dir.path());
        assert!(!f.iter().any(|x| x.kind == FindingKind::ConsoleLog));
    }
}
