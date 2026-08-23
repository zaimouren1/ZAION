//! CSRF protection middleware (M1 S6).
//!
//! For a bearer-token API without cookies, the standard CSRF mitigation is
//! requiring a credential the browser cannot attach cross-origin: either the
//! Authorization header or a custom X-CSRF-Token header on mutating requests.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::body::Body;
use axum::extract::Request;
use axum::http::Method;
use tower::{Layer, Service};

/// Layer attaching CsrfMiddleware.
#[derive(Clone, Debug, Default)]
pub struct CsrfLayer;

impl CsrfLayer {
    pub fn new() -> Self {
        Self
    }
}

impl<S> Layer<S> for CsrfLayer {
    type Service = CsrfMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        CsrfMiddleware { inner }
    }
}

/// Middleware rejecting mutating requests without a browser-unaidable
/// credential (Authorization or X-CSRF-Token).
#[derive(Clone, Debug)]
pub struct CsrfMiddleware<S> {
    inner: S,
}

fn is_mutating(method: &Method) -> bool {
    method == Method::POST
        || method == Method::PUT
        || method == Method::PATCH
        || method == Method::DELETE
}

impl<S> Service<Request<Body>> for CsrfMiddleware<S>
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
        if is_mutating(req.method()) {
            let has_auth = req
                .headers()
                .contains_key(axum::http::header::AUTHORIZATION);
            let has_csrf = req.headers().contains_key("x-csrf-token");
            if !has_auth && !has_csrf {
                let mut res = axum::response::Response::new(Body::from(
                    serde_json::json!({"error": "CSRF protection: mutating request requires Authorization or X-CSRF-Token header"}).to_string(),
                ));
                *res.status_mut() = axum::http::StatusCode::FORBIDDEN;
                return Box::pin(async move { Ok(res) });
            }
        }
        let mut inner = self.inner.clone();
        Box::pin(async move { inner.call(req).await })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn router_with_csrf() -> axum::Router {
        use axum::routing::{get, post};
        axum::Router::new()
            .route("/read", get(|| async { "ok" }))
            .route(
                "/write",
                post(|| async { (axum::http::StatusCode::CREATED, "created") }),
            )
            .layer(CsrfLayer::new())
    }

    async fn call(router: axum::Router, method: Method, path: &str, auth: bool) -> u16 {
        use tower::ServiceExt;
        let mut req = axum::http::Request::builder().method(method).uri(path);
        if auth {
            req = req.header("authorization", "Bearer secret");
        }
        let req = req.body(Body::empty()).unwrap();
        router.oneshot(req).await.unwrap().status().as_u16()
    }

    #[tokio::test]
    async fn mutation_without_credential_rejected() {
        let router = router_with_csrf().await;
        assert_eq!(
            call(router.clone(), Method::POST, "/write", false).await,
            403
        );
    }

    #[tokio::test]
    async fn mutation_with_auth_allowed() {
        let router = router_with_csrf().await;
        assert_eq!(
            call(router.clone(), Method::POST, "/write", true).await,
            201
        );
    }

    #[tokio::test]
    async fn reads_always_allowed() {
        let router = router_with_csrf().await;
        assert_eq!(call(router.clone(), Method::GET, "/read", false).await, 200);
    }

    #[tokio::test]
    async fn csrf_header_alone_allowed() {
        use tower::ServiceExt;
        let router = router_with_csrf().await;
        let req = axum::http::Request::builder()
            .method(Method::POST)
            .uri("/write")
            .header("x-csrf-token", "t0ken")
            .body(Body::empty())
            .unwrap();
        assert_eq!(router.oneshot(req).await.unwrap().status().as_u16(), 201);
    }
}
