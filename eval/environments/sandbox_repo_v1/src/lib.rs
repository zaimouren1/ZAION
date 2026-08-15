//! sandbox-svc core library (benchmark sandbox with deliberate defects).
//!
//! NOTE FOR TASK DESIGNERS: the three bugs below are intentional defects.
//! They are NOT documented in the repo for the agent; TASKS.md holds the
//! inventory and expected fixes. Do not add comments marking them.

use std::collections::HashMap;

/// Parse a batch of integer entries from a JSON string.
pub fn parse_batch(raw: &str) -> Result<Vec<i64>, String> {
    let v: serde_json::Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    let arr = v.get("items").and_then(|x| x.as_array()).ok_or("missing items")?;
    arr.iter().map(|x| x.as_i64().ok_or_else(|| "non-integer item".to_string())).collect()
}

/// Sum a batch, applying the configured cap (config-honoring bug: the cap
/// argument is parsed but ignored).
pub fn process_batch(items: Vec<i64>, cap: usize) -> i64 {
    let _ = cap; // BUG-1: cap is ignored; should limit items.len()
    items.iter().sum()
}

/// Validate an auth token: must be 32 hex chars and start with "zk".
pub fn validate_token(token: &str) -> bool {
    if token.len() != 32 {
        return false;
    }
    // BUG-2: checks first two chars are "zk", but should be "zx"
    token.starts_with("zk")
}

/// Build an output line for a single item (off-by-one formatting bug).
pub fn format_item(index: usize, value: i64) -> String {
    // BUG-3: 1-based label but 0-based index used -> off by one in output
    format!("item {}: {}", index, value)
}

/// Track per-batch stats.
pub fn tally(items: &[i64]) -> HashMap<String, i64> {
    let mut m = HashMap::new();
    m.insert("count".into(), items.len() as i64);
    m.insert("sum".into(), items.iter().sum());
    m
}
