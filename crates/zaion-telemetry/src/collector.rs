//! TelemetryCollector — captures spans and builds traces in real-time.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::error::{TelemetryError, TelemetryResult};
use crate::span::{Span, SpanId};
use crate::trace::{Trace, TraceId};

/// Thread-safe telemetry collector that captures spans and builds traces.
#[derive(Clone)]
pub struct TelemetryCollector {
    inner: Arc<Mutex<CollectorInner>>,
}

struct CollectorInner {
    active_traces: HashMap<TraceId, Trace>,
    completed_traces: Vec<Trace>,
    active_spans: HashMap<SpanId, TraceId>,
}

impl TelemetryCollector {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(CollectorInner {
                active_traces: HashMap::new(),
                completed_traces: Vec::new(),
                active_spans: HashMap::new(),
            })),
        }
    }

    /// Start a new trace and return its ID.
    pub fn start_trace(&self) -> TraceId {
        let trace = Trace::new();
        let trace_id = trace.trace_id.clone();
        let mut inner = self.inner.lock().unwrap();
        inner.active_traces.insert(trace_id.clone(), trace);
        trace_id
    }

    /// Start a root span within a trace.
    pub fn start_span(
        &self,
        trace_id: &TraceId,
        name: impl Into<String>,
    ) -> TelemetryResult<SpanId> {
        let span = Span::new(name);
        let span_id = span.span_id.clone();
        let mut inner = self.inner.lock().unwrap();
        let trace = inner
            .active_traces
            .get_mut(trace_id)
            .ok_or_else(|| TelemetryError::TraceNotFound(trace_id.0.clone()))?;
        trace.add_span(span);
        inner.active_spans.insert(span_id.clone(), trace_id.clone());
        Ok(span_id)
    }

    /// End a span by recording its end time.
    pub fn end_span(&self, span_id: &SpanId) -> TelemetryResult<()> {
        let mut inner = self.inner.lock().unwrap();
        let trace_id = inner
            .active_spans
            .get(span_id)
            .ok_or_else(|| TelemetryError::SpanNotFound(span_id.0.clone()))?
            .clone();
        let trace = inner
            .active_traces
            .get_mut(&trace_id)
            .ok_or_else(|| TelemetryError::TraceNotFound(trace_id.0.clone()))?;
        if let Some(span) = trace.spans.iter_mut().find(|s| s.span_id == *span_id) {
            span.end();
        }
        inner.active_spans.remove(span_id);
        Ok(())
    }

    /// Complete a trace and move it to the completed list.
    pub fn complete_trace(&self, trace_id: &TraceId) -> TelemetryResult<Trace> {
        let mut inner = self.inner.lock().unwrap();
        let trace = inner
            .active_traces
            .remove(trace_id)
            .ok_or_else(|| TelemetryError::TraceNotFound(trace_id.0.clone()))?;
        inner.completed_traces.push(trace.clone());
        Ok(trace)
    }

    /// Get all completed traces.
    pub fn completed_traces(&self) -> Vec<Trace> {
        let inner = self.inner.lock().unwrap();
        inner.completed_traces.clone()
    }

    /// Get count of active traces.
    pub fn active_trace_count(&self) -> usize {
        let inner = self.inner.lock().unwrap();
        inner.active_traces.len()
    }
}

impl Default for TelemetryCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collector_start_trace() {
        let collector = TelemetryCollector::new();
        let trace_id = collector.start_trace();
        assert_eq!(collector.active_trace_count(), 1);
        assert!(!trace_id.0.is_empty());
    }

    #[test]
    fn test_collector_start_and_end_span() {
        let collector = TelemetryCollector::new();
        let trace_id = collector.start_trace();
        let span_id = collector.start_span(&trace_id, "test_span").unwrap();
        collector.end_span(&span_id).unwrap();
        let trace = collector.complete_trace(&trace_id).unwrap();
        assert_eq!(trace.spans.len(), 1);
        assert!(trace.spans[0].end_time.is_some());
    }

    #[test]
    fn test_collector_complete_trace() {
        let collector = TelemetryCollector::new();
        let trace_id = collector.start_trace();
        collector.start_span(&trace_id, "span1").unwrap();
        let trace = collector.complete_trace(&trace_id).unwrap();
        assert_eq!(trace.spans.len(), 1);
        assert_eq!(collector.active_trace_count(), 0);
        assert_eq!(collector.completed_traces().len(), 1);
    }

    #[test]
    fn test_collector_span_not_found() {
        let collector = TelemetryCollector::new();
        let result = collector.end_span(&SpanId("nonexistent".to_string()));
        assert!(result.is_err());
    }

    #[test]
    fn test_collector_trace_not_found() {
        let collector = TelemetryCollector::new();
        let result = collector.start_span(&TraceId("nonexistent".to_string()), "span");
        assert!(result.is_err());
    }
}
