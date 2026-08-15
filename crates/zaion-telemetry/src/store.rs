//! TraceStore — persistent trace storage with SQLite backend.

use crate::error::{TelemetryError, TelemetryResult};
use crate::trace::{Trace, TraceId};

/// Query parameters for trace retrieval.
pub struct TraceQuery {
    pub limit: usize,
    pub min_duration_ms: Option<i64>,
    pub span_name_filter: Option<String>,
}

impl Default for TraceQuery {
    fn default() -> Self {
        Self {
            limit: 100,
            min_duration_ms: None,
            span_name_filter: None,
        }
    }
}

/// In-memory trace store (SQLite integration deferred).
pub struct TraceStore {
    traces: Vec<Trace>,
}

impl TraceStore {
    pub fn new() -> Self {
        Self { traces: Vec::new() }
    }

    pub fn insert(&mut self, trace: Trace) {
        self.traces.push(trace);
    }

    pub fn get(&self, trace_id: &TraceId) -> TelemetryResult<&Trace> {
        self.traces
            .iter()
            .find(|t| t.trace_id == *trace_id)
            .ok_or_else(|| TelemetryError::TraceNotFound(trace_id.0.clone()))
    }

    pub fn query(&self, q: &TraceQuery) -> Vec<&Trace> {
        self.traces
            .iter()
            .filter(|t| {
                if let Some(min_ms) = q.min_duration_ms {
                    t.total_duration_ms().unwrap_or(0) >= min_ms
                } else {
                    true
                }
            })
            .filter(|t| {
                if let Some(ref name) = q.span_name_filter {
                    t.spans.iter().any(|s| s.name.contains(name.as_str()))
                } else {
                    true
                }
            })
            .take(q.limit)
            .collect()
    }

    pub fn count(&self) -> usize {
        self.traces.len()
    }
}

impl Default for TraceStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace::Trace;

    #[test]
    fn test_store_insert_and_get() {
        let mut store = TraceStore::new();
        let trace = Trace::new();
        let id = trace.trace_id.clone();
        store.insert(trace);
        assert_eq!(store.count(), 1);
        assert!(store.get(&id).is_ok());
    }

    #[test]
    fn test_store_get_not_found() {
        let store = TraceStore::new();
        let result = store.get(&TraceId::new());
        assert!(result.is_err());
    }

    #[test]
    fn test_store_query_default() {
        let mut store = TraceStore::new();
        store.insert(Trace::new());
        store.insert(Trace::new());
        let results = store.query(&TraceQuery::default());
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_store_query_with_limit() {
        let mut store = TraceStore::new();
        for _ in 0..10 {
            store.insert(Trace::new());
        }
        let q = TraceQuery {
            limit: 3,
            ..Default::default()
        };
        let results = store.query(&q);
        assert_eq!(results.len(), 3);
    }
}
