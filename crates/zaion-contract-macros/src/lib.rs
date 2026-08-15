use proc_macro::TokenStream;
use syn::visit::Visit;
use syn::{
    ExprPath, File, ImplItem, ImplItemFn, Item, ItemFn, ReturnType, TraitItem, TraitItemFn, Type,
    TypePath,
};

#[proc_macro_attribute]
pub fn must_produce(attr: TokenStream, item: TokenStream) -> TokenStream {
    let required = attr.to_string().replace(' ', "");
    if required.is_empty() {
        return compile_error("must_produce requires a type name");
    }
    let item_text = item.to_string();
    if !must_produce_contract_satisfied(&required, &item_text) {
        return compile_error(&format!(
            "Zaion architecture contract violation: implementation must produce {required}"
        ));
    }
    item
}

fn compile_error(message: &str) -> TokenStream {
    format!("compile_error!({message:?});")
        .parse()
        .expect("compile_error token stream")
}

fn must_produce_contract_satisfied(required: &str, item_text: &str) -> bool {
    let Ok(file) = syn::parse_file(item_text) else {
        return false;
    };
    let required = RequiredType::new(required);
    let mut analyzer = MustProduceAnalyzer::new(&required);
    analyzer.visit_file(&file);
    analyzer.satisfied
}

struct RequiredType {
    segments: Vec<String>,
}

impl RequiredType {
    fn new(raw: &str) -> Self {
        let segments = raw
            .split("::")
            .filter(|segment| !segment.is_empty())
            .map(str::to_string)
            .collect();
        Self { segments }
    }

    fn matches_path(&self, actual: &[String]) -> bool {
        if self.segments.is_empty() || actual.len() < self.segments.len() {
            return false;
        }
        actual[actual.len() - self.segments.len()..] == self.segments
    }
}

struct MustProduceAnalyzer<'a> {
    required: &'a RequiredType,
    satisfied: bool,
}

impl<'a> MustProduceAnalyzer<'a> {
    fn new(required: &'a RequiredType) -> Self {
        Self {
            required,
            satisfied: false,
        }
    }

    fn mark_if_return_produces(&mut self, output: &ReturnType) {
        if self.satisfied {
            return;
        }
        if let ReturnType::Type(_, ty) = output {
            if type_mentions_required(ty, self.required) {
                self.satisfied = true;
            }
        }
    }

    fn mark_if_body_produces(&mut self, func: &ImplItemFn) {
        if self.satisfied {
            return;
        }
        let mut body = BodyProductionVisitor {
            required: self.required,
            produced: false,
        };
        body.visit_block(&func.block);
        self.satisfied = body.produced;
    }

    fn mark_if_item_fn_body_produces(&mut self, func: &ItemFn) {
        if self.satisfied {
            return;
        }
        let mut body = BodyProductionVisitor {
            required: self.required,
            produced: false,
        };
        body.visit_block(&func.block);
        self.satisfied = body.produced;
    }
}

impl<'ast> Visit<'ast> for MustProduceAnalyzer<'_> {
    fn visit_file(&mut self, node: &'ast File) {
        if self.satisfied {
            return;
        }
        syn::visit::visit_file(self, node);
    }

    fn visit_item(&mut self, node: &'ast Item) {
        if self.satisfied {
            return;
        }
        match node {
            Item::Trait(item_trait) => {
                for item in &item_trait.items {
                    if let TraitItem::Fn(func) = item {
                        self.mark_if_return_produces(&func.sig.output);
                        if self.satisfied {
                            return;
                        }
                        visit_trait_default_body(func, self.required, &mut self.satisfied);
                        if self.satisfied {
                            return;
                        }
                    }
                }
            }
            Item::Impl(item_impl) => {
                for item in &item_impl.items {
                    if let ImplItem::Fn(func) = item {
                        self.mark_if_return_produces(&func.sig.output);
                        if self.satisfied {
                            return;
                        }
                        self.mark_if_body_produces(func);
                        if self.satisfied {
                            return;
                        }
                    }
                }
            }
            Item::Fn(func) => {
                self.mark_if_return_produces(&func.sig.output);
                if self.satisfied {
                    return;
                }
                self.mark_if_item_fn_body_produces(func);
            }
            _ => syn::visit::visit_item(self, node),
        }
    }
}

struct BodyProductionVisitor<'a> {
    required: &'a RequiredType,
    produced: bool,
}

impl<'ast> Visit<'ast> for BodyProductionVisitor<'_> {
    fn visit_expr_path(&mut self, node: &'ast ExprPath) {
        if self.produced {
            return;
        }
        let path = path_segments_to_strings(&node.path);
        if self.required.matches_path(&path) {
            self.produced = true;
            return;
        }
        syn::visit::visit_expr_path(self, node);
    }

    fn visit_type_path(&mut self, node: &'ast TypePath) {
        if self.produced {
            return;
        }
        let path = path_segments_to_strings(&node.path);
        if self.required.matches_path(&path) {
            self.produced = true;
            return;
        }
        syn::visit::visit_type_path(self, node);
    }

    fn visit_lit_str(&mut self, _node: &'ast syn::LitStr) {}
}

fn visit_trait_default_body(func: &TraitItemFn, required: &RequiredType, satisfied: &mut bool) {
    let Some(block) = &func.default else {
        return;
    };
    let mut body = BodyProductionVisitor {
        required,
        produced: false,
    };
    body.visit_block(block);
    *satisfied = body.produced;
}

fn type_mentions_required(ty: &Type, required: &RequiredType) -> bool {
    let mut visitor = TypeProductionVisitor {
        required,
        mentioned: false,
    };
    visitor.visit_type(ty);
    visitor.mentioned
}

struct TypeProductionVisitor<'a> {
    required: &'a RequiredType,
    mentioned: bool,
}

impl<'ast> Visit<'ast> for TypeProductionVisitor<'_> {
    fn visit_type_path(&mut self, node: &'ast TypePath) {
        if self.mentioned {
            return;
        }
        let path = path_segments_to_strings(&node.path);
        if self.required.matches_path(&path) {
            self.mentioned = true;
            return;
        }
        syn::visit::visit_type_path(self, node);
    }
}

fn path_segments_to_strings(path: &syn::Path) -> Vec<String> {
    path.segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::must_produce_contract_satisfied;

    #[test]
    fn message_mentions_zaion_contract() {
        let message = "Zaion architecture contract violation";
        assert!(message.contains("Zaion architecture contract"));
    }

    #[test]
    fn semantic_gate_accepts_trait_method_return_type() {
        let item = r#"
            pub trait ToolExecutor {
                fn execute(&self) -> Result<ToolReceipt, ToolError>;
            }
        "#;
        assert!(must_produce_contract_satisfied("ToolReceipt", item));
    }

    #[test]
    fn semantic_gate_accepts_impl_method_return_type() {
        let item = r#"
            impl ToolExecutor for StableExecutor {
                fn execute(&self) -> Result<ToolReceipt, ToolError> {
                    Ok(ToolReceipt::new())
                }
            }
        "#;
        assert!(must_produce_contract_satisfied("ToolReceipt", item));
    }

    #[test]
    fn semantic_gate_rejects_comment_only_mentions() {
        let item = r#"
            impl ToolExecutor for StableExecutor {
                // ToolReceipt
                fn execute(&self) -> Result<(), ToolError> {
                    Ok(())
                }
            }
        "#;
        assert!(!must_produce_contract_satisfied("ToolReceipt", item));
    }

    #[test]
    fn semantic_gate_rejects_string_literal_only_mentions() {
        let item = r#"
            impl ToolExecutor for StableExecutor {
                fn execute(&self) -> Result<(), ToolError> {
                    let _ = "ToolReceipt";
                    Ok(())
                }
            }
        "#;
        assert!(!must_produce_contract_satisfied("ToolReceipt", item));
    }
}
