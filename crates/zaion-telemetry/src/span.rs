//! Span — the fundamental unit of telemetry tracing.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::convert::Infallible;
use std::str::FromStr;
use uuid::Uuid;

/// Unique span identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SpanId(pub String);

impl SpanId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}

impl FromStr for SpanId {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.to_string()))
    }
}

impl From<&str> for SpanId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl Default for SpanId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for SpanId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Key-value attributes attached to a span.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SpanAttributes {
    pub entries: HashMap<String, serde_json::Value>,
}

impl SpanAttributes {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    pub fn set(&mut self, key: impl Into<String>, value: impl Into<serde_json::Value>) {
        self.entries.insert(key.into(), value.into());
    }

    pub fn get(&self, key: &str) -> Option<&serde_json::Value> {
        self.entries.get(key)
    }
}

/// A span represents a single operation in the trace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Span {
    pub span_id: SpanId,
    pub parent_span_id: Option<SpanId>,
    pub name: String,
    pub start_time: DateTime<Utc>,
    pub end_time: Option<DateTime<Utc>>,
    pub attributes: SpanAttributes,
}

impl Span {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            span_id: SpanId::new(),
            parent_span_id: None,
            name: name.into(),
            start_time: Utc::now(),
            end_time: None,
            attributes: SpanAttributes::new(),
        }
    }

    pub fn with_parent(name: impl Into<String>, parent_id: SpanId) -> Self {
        Self {
            span_id: SpanId::new(),
            parent_span_id: Some(parent_id),
            name: name.into(),
            start_time: Utc::now(),
            end_time: None,
            attributes: SpanAttributes::new(),
        }
    }

    pub fn end(&mut self) {
        self.end_time = Some(Utc::now());
    }

    pub fn duration_ms(&self) -> Option<i64> {
        self.end_time
            .map(|end| (end - self.start_time).num_milliseconds())
    }

    pub fn set_attribute(&mut self, key: impl Into<String>, value: impl Into<serde_json::Value>) {
        self.attributes.set(key, value);
    }
}
