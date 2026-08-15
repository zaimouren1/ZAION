use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextCompileInput {
    pub memory_atoms: Vec<String>,
    pub turn_history: Vec<String>,
    pub activity_state: String,
    pub token_budget: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompiledContextPack {
    pub strategy_id: &'static str,
    pub source_layer_ids: Vec<String>,
    pub token_budget: usize,
    pub content: String,
    pub evidence_hash: String,
}

pub trait ContextStrategy: Send + Sync {
    fn id(&self) -> &'static str;
    fn maturity(&self) -> &'static str;
    fn compile(&self, input: ContextCompileInput) -> CompiledContextPack;
}

#[derive(Debug)]
pub struct MinimalContext;

impl ContextStrategy for MinimalContext {
    fn id(&self) -> &'static str {
        "minimal"
    }

    fn maturity(&self) -> &'static str {
        "stable"
    }

    fn compile(&self, input: ContextCompileInput) -> CompiledContextPack {
        let content = input.turn_history.last().cloned().unwrap_or_default();
        pack(
            self.id(),
            input.memory_atoms,
            input.token_budget.min(1024),
            content,
        )
    }
}

#[derive(Debug)]
pub struct FullContext;

impl ContextStrategy for FullContext {
    fn id(&self) -> &'static str {
        "full"
    }

    fn maturity(&self) -> &'static str {
        "stable"
    }

    fn compile(&self, input: ContextCompileInput) -> CompiledContextPack {
        let content = input.turn_history.join("\n");
        pack(self.id(), input.memory_atoms, input.token_budget, content)
    }
}

pub struct ContextStrategyRegistry {
    strategies: Vec<Box<dyn ContextStrategy>>,
}

impl ContextStrategyRegistry {
    pub fn stable_default() -> Self {
        Self {
            strategies: vec![Box::new(MinimalContext), Box::new(FullContext)],
        }
    }

    pub fn get(&self, id: &str) -> Option<&dyn ContextStrategy> {
        self.strategies
            .iter()
            .find(|strategy| strategy.id() == id)
            .map(|strategy| strategy.as_ref())
    }

    pub fn stable_strategy_ids(&self) -> Vec<&'static str> {
        self.strategies
            .iter()
            .filter(|strategy| strategy.maturity() == "stable")
            .map(|strategy| strategy.id())
            .collect()
    }
}

fn pack(
    strategy_id: &'static str,
    source_layer_ids: Vec<String>,
    token_budget: usize,
    content: String,
) -> CompiledContextPack {
    let mut hasher = Sha256::new();
    hasher.update(strategy_id.as_bytes());
    hasher.update(content.as_bytes());
    for id in &source_layer_ids {
        hasher.update(id.as_bytes());
    }
    CompiledContextPack {
        strategy_id,
        source_layer_ids,
        token_budget,
        content,
        evidence_hash: format!("sha256:{}", hex::encode(hasher.finalize())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_exposes_minimal_and_full_context() {
        let registry = ContextStrategyRegistry::stable_default();
        assert!(registry.get("minimal").is_some());
        assert!(registry.get("full").is_some());
        assert_eq!(registry.stable_strategy_ids(), vec!["minimal", "full"]);
    }

    #[test]
    fn minimal_context_records_strategy_id_and_budget() {
        let strategy = MinimalContext;
        let pack = strategy.compile(ContextCompileInput {
            memory_atoms: vec!["m1".to_string()],
            turn_history: vec!["user: hi".to_string()],
            activity_state: "chat".to_string(),
            token_budget: 1024,
        });

        assert_eq!(pack.strategy_id, "minimal");
        assert!(pack.token_budget <= 1024);
        assert!(pack.evidence_hash.starts_with("sha256:"));
    }
}
