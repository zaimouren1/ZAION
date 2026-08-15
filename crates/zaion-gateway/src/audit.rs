//! Write-audit middleware (M1 S4).
//!
//! Records mutating requests (POST/PUT/PATCH/DELETE) with their outcome into a
//! bounded, inspectable audit log - the foundation for the M1 complete-write-audit gate.

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use axum::body::Body;
use axum::extract::Request;
use axum::http::Method;
use tower::{Layer, Service};

/// A single audited mutation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditEntry {
    pub method: String,
    pub path: String,
    pub status: u16,
    pub at: String,
}

/// Bounded, thread-safe audit log.
#[derive(Clone, Debug, Default)]
pub struct WriteAudit {
    entries: Arc<Mutex<VecDeque<AuditEntry>>>,
    capacity: usize,
}

impl WriteAudit {
    /// Create an audit log with a bounded capacity (oldest entries dropped).
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: Arc::new(Mutex::new(VecDeque::new())),
            capacity: capacity.max(1),
        }
    }

    /// Record a mutation.
    pub fn record(&self, method: &str, path: &str, status: u16) {
        let entry = AuditEntry {
            method: method.to_string(),
            path: path.to_string(),
            status,
            at: timestamp(),
        };
        if let Ok(mut guard) = self.entries.lock() {
            if guard.len() >= self.capacity {
                guard.pop_front();
            }
            guard.push_back(entry);
        }
    }

    /// Snapshot of all entries.
    pub fn entries(&self) -> Vec<AuditEntry> {
        self.entries
            .lock()
            .map(|guard| guard.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Number of recorded entries.
    pub fn len(&self) -> usize {
        self.entries.lock().map(|g| g.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Millisecond epoch timestamp without external deps.
fn timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis().to_string())
        .unwrap_or_else(|_| "0".into())
}

/// Layer attaching AuditMiddleware to a service.
#[derive(Clone, Debug)]
pub struct AuditLayer {
    audit: Arc<WriteAudit>,
}

impl AuditLayer {
    pub fn new(audit: WriteAudit) -> Self {
        Self { audit: Arc::new(audit) }
    }
}

impl<S> Layer<S> for AuditLayer {
    type Service = AuditMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        AuditMiddleware { inner, audit: self.audit.clone() }
    }
}

/// Tower middleware recording mutating requests.
#[derive(Clone, Debug)]
pub struct AuditMiddleware<S> {
    inner: S,
    audit: Arc<WriteAudit>,
}

impl<S> Service<Request<Body>> for AuditMiddleware<S>
where
    S: Service<Request<Body>, Response = axum::response::Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let method_ref = req.method();
        let is_write = method_ref == Method::POST
            || method_ref == Method::PUT
            || method_ref == Method::PATCH
            || method_ref == Method::DELETE;
        let method = req.method().to_string();
        let path = req.uri().path().to_string();
        let audit = self.audit.clone();
        let mut inner = self.inner.clone();
        Box::pin(async move {
            let res = inner.call(req).await?;
            if is_write {
                audit.record(&method, &path, res.status().as_u16());
            }
            Ok(res)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_records_and_bounds() {
        let audit = WriteAudit::new(3);
        audit.record("POST", "/a", 201);
        audit.record("PUT", "/b", 200);
        audit.record("DELETE", "/c", 204);
        assert_eq!(audit.len(), 3);
        audit.record("POST", "/d", 200);
        assert_eq!(audit.len(), 3, "capacity bound");
        let entries = audit.entries();
        assert_eq!(entries[0].path, "/b", "oldest dropped");
        assert_eq!(entries[2].path, "/d");
    }

    #[test]
    fn audit_records_method_path_status() {
        let audit = WriteAudit::new(10);
        audit.record("POST", "/run", 401);
        let e = &audit.entries()[0];
        assert_eq!(e.method, "POST");
        assert_eq!(e.path, "/run");
        assert_eq!(e.status, 401);
        assert!(!e.at.is_empty());
    }

    async fn router_with_audit() -> (axum::Router, WriteAudit) {
        use axum::routing::{get, post};
        let audit = WriteAudit::new(64);
        let router = axum::Router::new()
            .route("/read", get(|| async { "ok" }))
            .route("/write", post(|| async { (axum::http::StatusCode::CREATED, "created") }))
            .layer(AuditLayer::new(audit.clone()));
        (router, audit)
    }

    async fn call(router: axum::Router, method: Method, path: &str) -> u16 {
        use tower::ServiceExt;
        let req = Request::builder()
            .method(method)
            .uri(path)
            .body(Body::empty())
            .unwrap();
        router.oneshot(req).await.unwrap().status().as_u16()
    }

    #[tokio::test]
    async fn middleware_audits_writes_not_reads() {
        let (router, audit) = router_with_audit().await;
        assert_eq!(call(router.clone(), Method::POST, "/write").await, 201);
        assert_eq!(call(router.clone(), Method::GET, "/read").await, 200);
        assert_eq!(audit.len(), 1, "only the POST is audited");
        let e = &audit.entries()[0];
        assert_eq!(e.method, "POST");
        assert_eq!(e.status, 201);
    }

    #[tokio::test]
    async fn middleware_audits_failed_writes() {
        let (router, audit) = router_with_audit().await;
        // POST to a GET-only route -> 405, still a write attempt
        assert_eq!(call(router.clone(), Method::POST, "/read").await, 405);
        assert_eq!(audit.len(), 1);
        assert_eq!(audit.entries()[0].status, 405);
    }
}