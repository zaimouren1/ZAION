use zaion_types::session::SessionKey;

#[derive(Debug, Clone)]
pub struct SlimmedContext {
    pub layers: Vec<ContextLayer>,
    pub compressed_ratio: f64,
}

#[derive(Debug, Clone)]
pub struct ContextLayer {
    pub layer: u8,
    pub content: serde_json::Value,
    pub compressed: bool,
}

pub struct ContextSlimmer {
    pub max_tokens: usize,
}

impl ContextSlimmer {
    pub fn new(max_tokens: usize) -> Self {
        Self { max_tokens }
    }

    pub fn slim(&self, layers: Vec<ContextLayer>) -> SlimmedContext {
        let total = layers.len();
        let mut result = Vec::with_capacity(total);
        for layer in layers {
            let compressed = layer.layer >= 5;
            result.push(ContextLayer {
                layer: layer.layer,
                content: layer.content,
                compressed,
            });
        }
        let compressed_count = result.iter().filter(|l| l.compressed).count();
        let ratio = if total > 0 {
            compressed_count as f64 / total as f64
        } else {
            0.0
        };
        SlimmedContext {
            layers: result,
            compressed_ratio: ratio,
        }
    }

    pub fn build_context_messages(
        &self,
        _session_key: &SessionKey,
        slimmed: &SlimmedContext,
    ) -> Vec<serde_json::Value> {
        let mut messages = Vec::new();
        for layer in &slimmed.layers {
            messages.push(serde_json::json!({
                "role": "system",
                "content": format!("[Memory L{}{}] {}",
                    layer.layer,
                    if layer.compressed { " (compressed)" } else { "" },
                    layer.content
                )
            }));
        }
        messages
    }
}
