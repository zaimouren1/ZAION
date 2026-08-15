//! Trace — a collection of spans forming a complete operation tree.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::str::FromStr;
use uuid::Uuid;

use crate::span::{Span, SpanId};

/// Unique trace identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TraceId(pub String);

impl TraceId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}

impl FromStr for TraceId {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.to_string()))
    }
}

impl From<&str> for TraceId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl Default for TraceId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for TraceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A trace is a tree of spans representing a complete operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trace {
    pub trace_id: TraceId,
    pub root_span_id: Option<SpanId>,
    pub spans: Vec<Span>,
    pub created_at: DateTime<Utc>,
}

impl Trace {
    pub fn new() -> Self {
        Self {
            trace_id: TraceId::new(),
            root_span_id: None,
            spans: Vec::new(),
            created_at: Utc::now(),
        }
    }

    pub fn add_span(&mut self, span: Span) {
        if self.root_span_id.is_none() && span.parent_span_id.is_none() {
            self.root_span_id = Some(span.span_id.clone());
        }
        self.spans.push(span);
    }

    pub fn get_span(&self, span_id: &SpanId) -> Option<&Span> {
        self.spans.iter().find(|s| &s.span_id == span_id)
    }

    pub fn get_span_mut(&mut self, span_id: &SpanId) -> Option<&mut Span> {
        self.spans.iter_mut().find(|s| &s.span_id == span_id)
    }

    pub fn total_duration_ms(&self) -> Option<i64> {
        let root_id = self.root_span_id.as_ref()?;
        self.get_span(root_id)?.duration_ms()
    }

    pub fn span_count(&self) -> usize {
        self.spans.len()
    }

    /// Export trace as OpenTelemetry-compatible JSON.
    pub fn to_otlp_json(&self) -> serde_json::Value {
        let resource_spans = self
            .spans
            .iter()
            .map(|span| {
                let mut attrs = Vec::new();
                for (k, v) in &span.attributes.entries {
                    attrs.push(serde_json::json!({
                        "key": k,
                        "value": { "stringValue": v.to_string() }
                    }));
                }
                serde_json::json!({
                    "traceId": self.trace_id.0,
                    "spanId": span.span_id.0,
                    "parentSpanId": span.parent_span_id.as_ref().map(|p| &p.0),
                    "name": span.name,
                    "startTimeUnixNano": span.start_time.timestamp_nanos_opt().unwrap_or(0),
                    "endTimeUnixNano": span.end_time.map(|e| e.timestamp_nanos_opt().unwrap_or(0)),
                    "attributes": attrs,
                })
            })
            .collect::<Vec<_>>();

        serde_json::json!({
            "resourceSpans": [{
                "resource": {
                    "attributes": [{
                        "key": "service.name",
                        "value": { "stringValue": "zaion-agent" }
                    }]
                },
                "scopeSpans": [{
                    "scope": { "name": "zaion-telemetry" },
                    "spans": resource_spans
                }]
            }]
        })
    }
}

impl Default for Trace {
    fn default() -> Self {
        Self::new()
    }
}
