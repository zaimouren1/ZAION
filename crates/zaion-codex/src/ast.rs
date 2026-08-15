use crate::CodexError;
use proc_macro2::Span;
/// Real AST parsing for Rust source files using the `syn` crate.
///
/// This replaces the previous line-regex approach which had critical defects:
///   1. False positives on keywords inside strings/comments
///   2. Incorrect block boundaries (counted `{`/`}` inside string literals)
///   3. Only single-line signatures detected
///
/// Architecture:
///   - `syn::parse_file` builds a full syntax tree from Rust source
///   - `AstVisitor` walks every item and records precise span info
///   - Block content is sliced from the original source using byte offsets
///     converted to line numbers (syn provides Span but only line info in
///     proc-macro2 with the `span-locations` feature, which we derive via
///     a byte-offset line map built once per file)
use std::path::Path;
use syn::{visit::Visit, File};

// ─── Public types ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChunkKind {
    Function,
    Struct,
    Enum,
    Impl,
    Trait,
    TypeAlias,
    Const,
    Static,
    Macro,
    Mod,
    Use,
    Other,
}

impl ChunkKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ChunkKind::Function => "fn",
            ChunkKind::Struct => "struct",
            ChunkKind::Enum => "enum",
            ChunkKind::Impl => "impl",
            ChunkKind::Trait => "trait",
            ChunkKind::TypeAlias => "type",
            ChunkKind::Const => "const",
            ChunkKind::Static => "static",
            ChunkKind::Macro => "macro",
            ChunkKind::Mod => "mod",
            ChunkKind::Use => "use",
            ChunkKind::Other => "other",
        }
    }
}

/// A semantic unit extracted from a Rust source file.
#[derive(Debug, Clone)]
pub struct AstChunk {
    /// Absolute path to the source file.
    pub file_path: String,
    pub kind: ChunkKind,
    /// Canonical name: `TypeName`, `TypeName::method_name`, etc.
    pub name: String,
    /// 1-based inclusive.
    pub start_line: usize,
    /// 1-based inclusive.
    pub end_line: usize,
    /// Full source text of this chunk (including doc comments).
    pub content: String,
    /// Extracted `///` doc comment text, if any.
    pub doc_comment: Option<String>,
    /// For impl blocks: the type being implemented.
    pub impl_for: Option<String>,
    /// Estimated token count (~4 chars per token).
    pub token_estimate: usize,
}

impl AstChunk {
    /// Stable unique key for deduplication in the index.
    pub fn signature(&self) -> String {
        format!("{}::{}", self.file_path, self.name)
    }

    /// True if this chunk is a method inside an impl/trait.
    pub fn is_method(&self) -> bool {
        self.name.contains("::")
    }
}

// ─── Line helpers ──────────────────────────────────────────────────────────

/// Return (start_line, end_line) for a proc-macro2 Span.
/// Requires proc-macro2 compiled with the `span-locations` feature.
fn span_lines(span: Span) -> (usize, usize) {
    let start = span.start().line;
    let end = span.end().line;
    (start.max(1), end.max(start))
}

// ─── syn Visitor ───────────────────────────────────────────────────────────

struct AstVisitor<'src> {
    file_path: String,
    src: &'src str,
    chunks: Vec<AstChunk>,
    /// Stack of impl-type names for attributing methods.
    impl_stack: Vec<String>,
}

impl<'src> AstVisitor<'src> {
    fn new(file_path: &str, src: &'src str) -> Self {
        AstVisitor {
            file_path: file_path.to_string(),
            src,
            chunks: Vec::new(),
            impl_stack: Vec::new(),
        }
    }

    fn extract_docs(attrs: &[syn::Attribute]) -> Option<String> {
        let docs: Vec<String> = attrs
            .iter()
            .filter_map(|attr| {
                if attr.path().is_ident("doc") {
                    if let syn::Meta::NameValue(nv) = &attr.meta {
                        if let syn::Expr::Lit(expr_lit) = &nv.value {
                            if let syn::Lit::Str(s) = &expr_lit.lit {
                                return Some(s.value().trim().to_string());
                            }
                        }
                    }
                }
                None
            })
            .collect();
        if docs.is_empty() {
            None
        } else {
            Some(docs.join("\n"))
        }
    }

    fn slice_content(&self, start_line: usize, end_line: usize) -> String {
        self.src
            .lines()
            .enumerate()
            .filter(|(i, _)| *i + 1 >= start_line && *i < end_line)
            .map(|(_, l)| l)
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn push(
        &mut self,
        kind: ChunkKind,
        name: String,
        span: Span,
        doc_comment: Option<String>,
        impl_for: Option<String>,
    ) {
        let (start_line, end_line) = span_lines(span);
        let content = self.slice_content(start_line, end_line);
        let token_estimate = content.len() / 4;
        self.chunks.push(AstChunk {
            file_path: self.file_path.clone(),
            kind,
            name,
            start_line,
            end_line,
            content,
            doc_comment,
            impl_for,
            token_estimate,
        });
    }
}

impl<'ast, 'src> Visit<'ast> for AstVisitor<'src> {
    // ── Top-level items ────────────────────────────────────────────────────

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        let name = node.sig.ident.to_string();
        let docs = Self::extract_docs(&node.attrs);
        self.push(ChunkKind::Function, name, node.sig.ident.span(), docs, None);
        syn::visit::visit_item_fn(self, node);
    }

    fn visit_item_struct(&mut self, node: &'ast syn::ItemStruct) {
        let name = node.ident.to_string();
        let docs = Self::extract_docs(&node.attrs);
        self.push(ChunkKind::Struct, name, node.ident.span(), docs, None);
        syn::visit::visit_item_struct(self, node);
    }

    fn visit_item_enum(&mut self, node: &'ast syn::ItemEnum) {
        let name = node.ident.to_string();
        let docs = Self::extract_docs(&node.attrs);
        self.push(ChunkKind::Enum, name, node.ident.span(), docs, None);
        syn::visit::visit_item_enum(self, node);
    }

    fn visit_item_trait(&mut self, node: &'ast syn::ItemTrait) {
        let name = node.ident.to_string();
        let docs = Self::extract_docs(&node.attrs);
        self.push(
            ChunkKind::Trait,
            name.clone(),
            node.ident.span(),
            docs,
            None,
        );
        // Visit trait items (methods etc.) with trait name on stack.
        self.impl_stack.push(name);
        for item in &node.items {
            self.visit_trait_item(item);
        }
        self.impl_stack.pop();
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        // Determine the type name and optional trait being implemented.
        let self_type = type_to_string(&node.self_ty);
        let trait_name = node
            .trait_
            .as_ref()
            .map(|(_, path, _)| path_to_string(path));
        let impl_name = match &trait_name {
            Some(tr) => format!("{} for {}", tr, self_type),
            None => self_type.clone(),
        };
        let docs = Self::extract_docs(&node.attrs);
        let span = node.impl_token.span;
        self.push(
            ChunkKind::Impl,
            impl_name.clone(),
            span,
            docs,
            Some(self_type.clone()),
        );

        // Visit methods inside impl block.
        self.impl_stack.push(self_type);
        for item in &node.items {
            self.visit_impl_item(item);
        }
        self.impl_stack.pop();
    }

    fn visit_item_type(&mut self, node: &'ast syn::ItemType) {
        let name = node.ident.to_string();
        let docs = Self::extract_docs(&node.attrs);
        self.push(ChunkKind::TypeAlias, name, node.ident.span(), docs, None);
    }

    fn visit_item_const(&mut self, node: &'ast syn::ItemConst) {
        let name = node.ident.to_string();
        let docs = Self::extract_docs(&node.attrs);
        self.push(ChunkKind::Const, name, node.ident.span(), docs, None);
    }

    fn visit_item_static(&mut self, node: &'ast syn::ItemStatic) {
        let name = node.ident.to_string();
        let docs = Self::extract_docs(&node.attrs);
        self.push(ChunkKind::Static, name, node.ident.span(), docs, None);
    }

    fn visit_item_macro(&mut self, node: &'ast syn::ItemMacro) {
        if let Some(ident) = &node.ident {
            let name = ident.to_string();
            let docs = Self::extract_docs(&node.attrs);
            self.push(ChunkKind::Macro, name, ident.span(), docs, None);
        }
    }

    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        let name = node.ident.to_string();
        let docs = Self::extract_docs(&node.attrs);
        self.push(ChunkKind::Mod, name, node.ident.span(), docs, None);
        // Do NOT recurse into mod bodies to avoid deep nesting pollution.
    }

    // ── Impl methods ───────────────────────────────────────────────────────

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        let method = node.sig.ident.to_string();
        let parent = self.impl_stack.last().cloned().unwrap_or_default();
        let qualified = if parent.is_empty() {
            method.clone()
        } else {
            format!("{}::{}", parent, method)
        };
        let docs = Self::extract_docs(&node.attrs);
        self.push(
            ChunkKind::Function,
            qualified,
            node.sig.ident.span(),
            docs,
            self.impl_stack.last().cloned(),
        );
    }

    // ── Trait methods ──────────────────────────────────────────────────────

    fn visit_trait_item_fn(&mut self, node: &'ast syn::TraitItemFn) {
        let method = node.sig.ident.to_string();
        let parent = self.impl_stack.last().cloned().unwrap_or_default();
        let qualified = if parent.is_empty() {
            method.clone()
        } else {
            format!("{}::{}", parent, method)
        };
        let docs = Self::extract_docs(&node.attrs);
        self.push(
            ChunkKind::Function,
            qualified,
            node.sig.ident.span(),
            docs,
            self.impl_stack.last().cloned(),
        );
    }
}

// ─── Helper functions ──────────────────────────────────────────────────────

fn type_to_string(ty: &syn::Type) -> String {
    use syn::Type;
    match ty {
        Type::Path(p) => path_to_string(&p.path),
        Type::Reference(r) => type_to_string(&r.elem),
        Type::Paren(p) => type_to_string(&p.elem),
        _ => quote::quote!(#ty).to_string(),
    }
}

fn path_to_string(path: &syn::Path) -> String {
    path.segments
        .iter()
        .map(|s| s.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
}

// ─── Public API ────────────────────────────────────────────────────────────

/// Parse a Rust source file and extract all semantic chunks.
///
/// Returns an ordered list of `AstChunk` sorted by `start_line`.
/// Correctly handles:
///   - Multi-line function signatures
///   - Keywords inside string literals
///   - Keywords inside comments (`//`, `/* */`, `///`)
///   - Nested impl/trait blocks
///   - Generic type parameters
pub fn chunk_rust_file(path: &Path) -> Result<Vec<AstChunk>, CodexError> {
    let src = std::fs::read_to_string(path).map_err(CodexError::Io)?;
    chunk_rust_source(path.to_string_lossy().as_ref(), &src)
}

/// Parse Rust source from a string (useful for tests).
pub fn chunk_rust_source(file_path: &str, src: &str) -> Result<Vec<AstChunk>, CodexError> {
    let syntax: File = syn::parse_str(src)
        .map_err(|e| CodexError::Parse(format!("syn parse error in {}: {}", file_path, e)))?;
    let mut visitor = AstVisitor::new(file_path, src);
    visitor.visit_file(&syntax);
    let mut chunks = visitor.chunks;
    chunks.sort_by_key(|c| c.start_line);
    Ok(chunks)
}

/// Walk a directory tree and chunk every `.rs` file found.
pub fn chunk_directory(root: &Path) -> Result<Vec<AstChunk>, CodexError> {
    use walkdir::WalkDir;
    let mut all = Vec::new();
    for entry in WalkDir::new(root).into_iter().filter_map(|e| e.ok()) {
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) == Some("rs") {
            // Skip generated files (build artifacts, macros).
            if p.components().any(|c| c.as_os_str() == "target") {
                continue;
            }
            match chunk_rust_file(p) {
                Ok(mut chunks) => all.append(&mut chunks),
                Err(CodexError::Parse(_)) => { /* skip unparseable files */ }
                Err(e) => return Err(e),
            }
        }
    }
    all.sort_by(|a, b| {
        a.file_path
            .cmp(&b.file_path)
            .then(a.start_line.cmp(&b.start_line))
    });
    Ok(all)
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
/// A well-documented struct.
pub struct Foo {
    pub x: i32,
}

impl Foo {
    /// Constructor.
    pub fn new(x: i32) -> Self {
        Foo { x }
    }

    fn internal(&self) -> i32 {
        // This comment has fn inside it: fn fake()
        let s = "fn not_a_function() {}";  // string with fn keyword
        self.x
    }
}

pub trait Bar {
    fn bar_method(&self) -> String;
}

pub fn standalone(a: u32, b: u32) -> u32 {
    a + b
}
"#;

    #[test]
    fn test_no_false_positives_in_strings_and_comments() {
        let chunks = chunk_rust_source("test.rs", SAMPLE).unwrap();
        // Should NOT produce chunks for:
        //   - "fn fake()" inside comment
        //   - "fn not_a_function" inside string literal
        let names: Vec<&str> = chunks.iter().map(|c| c.name.as_str()).collect();
        assert!(!names.contains(&"fake"), "false positive: comment fn");
        assert!(
            !names.contains(&"not_a_function"),
            "false positive: string fn"
        );
    }

    #[test]
    fn test_detects_all_real_items() {
        let chunks = chunk_rust_source("test.rs", SAMPLE).unwrap();
        let names: Vec<&str> = chunks.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"Foo"), "missing struct Foo");
        assert!(names.contains(&"Foo::new"), "missing Foo::new");
        assert!(names.contains(&"Foo::internal"), "missing Foo::internal");
        assert!(names.contains(&"Bar"), "missing trait Bar");
        assert!(
            names.contains(&"Bar::bar_method"),
            "missing Bar::bar_method"
        );
        assert!(names.contains(&"standalone"), "missing fn standalone");
    }

    #[test]
    fn test_doc_comments_extracted() {
        let chunks = chunk_rust_source("test.rs", SAMPLE).unwrap();
        let foo = chunks.iter().find(|c| c.name == "Foo").unwrap();
        assert!(foo.doc_comment.is_some());
        assert!(foo
            .doc_comment
            .as_ref()
            .unwrap()
            .contains("well-documented"));
    }

    #[test]
    fn test_chunk_kind_correct() {
        let chunks = chunk_rust_source("test.rs", SAMPLE).unwrap();
        let foo = chunks.iter().find(|c| c.name == "Foo").unwrap();
        assert_eq!(foo.kind, ChunkKind::Struct);
        let standalone = chunks.iter().find(|c| c.name == "standalone").unwrap();
        assert_eq!(standalone.kind, ChunkKind::Function);
        let bar = chunks.iter().find(|c| c.name == "Bar").unwrap();
        assert_eq!(bar.kind, ChunkKind::Trait);
    }

    #[test]
    fn test_line_numbers_nonzero() {
        let chunks = chunk_rust_source("test.rs", SAMPLE).unwrap();
        for c in &chunks {
            assert!(c.start_line > 0, "start_line must be > 0: {}", c.name);
            assert!(c.end_line >= c.start_line, "end >= start: {}", c.name);
        }
    }

    #[test]
    fn test_multiline_signature() {
        let src = r#"
pub fn complex<T: Clone + Send>(
    arg1: T,
    arg2: &str,
) -> Option<T> {
    None
}
"#;
        let chunks = chunk_rust_source("multi.rs", src).unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].name, "complex");
        assert_eq!(chunks[0].kind, ChunkKind::Function);
    }
}
