//! Time tool handlers: time_now / time_parse / time_diff.

use chrono::{DateTime, Utc};
use serde_json::json;

use crate::{McpParam, McpParamType, McpSchema, McpTool, McpToolMeta, McpToolRegistry};

pub(super) fn time_now_handler(_input: serde_json::Value) -> Result<serde_json::Value, String> {
    let now = Utc::now();
    Ok(json!({
        "rfc3339": now.to_rfc3339(),
        "unix_secs": now.timestamp(),
        "unix_millis": now.timestamp_millis(),
        "year": now.format("%Y").to_string(),
        "date": now.format("%Y-%m-%d").to_string(),
        "time": now.format("%H:%M:%S").to_string()
    }))
}

pub(super) fn time_parse_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    let text = input
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'text' parameter".to_string())?;

    // Try RFC3339 first, then fall back to a Unix timestamp (seconds).
    let parsed: DateTime<Utc> = match DateTime::parse_from_rfc3339(text) {
        Ok(dt) => dt.with_timezone(&Utc),
        Err(_) => {
            let secs = text
                .trim()
                .parse::<i64>()
                .map_err(|_| format!("could not parse '{}' as RFC3339 or unix timestamp", text))?;
            DateTime::<Utc>::from_timestamp(secs, 0)
                .ok_or_else(|| format!("unix timestamp out of range: {}", secs))?
        }
    };

    Ok(json!({
        "input": text,
        "rfc3339": parsed.to_rfc3339(),
        "unix_secs": parsed.timestamp(),
        "unix_millis": parsed.timestamp_millis()
    }))
}

pub(super) fn time_diff_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    let from = input
        .get("from")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'from' parameter".to_string())?;
    let to = input
        .get("to")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'to' parameter".to_string())?;

    let parse = |s: &str| -> Result<DateTime<Utc>, String> {
        match DateTime::parse_from_rfc3339(s) {
            Ok(dt) => Ok(dt.with_timezone(&Utc)),
            Err(_) => {
                let secs = s
                    .trim()
                    .parse::<i64>()
                    .map_err(|_| format!("could not parse '{}' as RFC3339 or unix timestamp", s))?;
                DateTime::<Utc>::from_timestamp(secs, 0)
                    .ok_or_else(|| format!("unix timestamp out of range: {}", secs))
            }
        }
    };

    let from_dt = parse(from)?;
    let to_dt = parse(to)?;
    let delta = to_dt.signed_duration_since(from_dt);

    Ok(json!({
        "from": from_dt.to_rfc3339(),
        "to": to_dt.to_rfc3339(),
        "seconds": delta.num_seconds(),
        "minutes": delta.num_minutes(),
        "hours": delta.num_hours(),
        "days": delta.num_days()
    }))
}

/// Register the time tools into `registry`.
pub(super) fn register(registry: &mut McpToolRegistry) {
    registry.register(McpTool::new(
        McpToolMeta::new(
            "time_now",
            "1.0",
            "Get the current UTC time in RFC3339 and Unix timestamp forms.",
            McpSchema::new(vec![]),
            "utility",
        ),
        time_now_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "time_parse",
            "1.0",
            "Parse an RFC3339 string or Unix timestamp into normalized time fields.",
            McpSchema::new(vec![McpParam::required(
                "text",
                McpParamType::String,
                "RFC3339 timestamp or Unix seconds to parse",
            )]),
            "utility",
        ),
        time_parse_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "time_diff",
            "1.0",
            "Compute the difference between two timestamps (RFC3339 or Unix seconds).",
            McpSchema::new(vec![
                McpParam::required(
                    "from",
                    McpParamType::String,
                    "start timestamp (RFC3339 or Unix seconds)",
                ),
                McpParam::required(
                    "to",
                    McpParamType::String,
                    "end timestamp (RFC3339 or Unix seconds)",
                ),
            ]),
            "utility",
        ),
        time_diff_handler,
    ));
}
