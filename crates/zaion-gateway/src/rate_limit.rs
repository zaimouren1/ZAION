//! Fixed-window rate limiting (M1 S6).
//!
//! A simple, testable in-memory limiter: at most N requests per window.
//! Enforced via RateLimitLayer middleware (429 on overflow).

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::Request;
use tower::{Layer, Service};

/// Fixed-window rate limiter (thread-safe).
#[derive(Clone, Debug)]
pub struct RateLimiter {
    max_requests: u32,
    window: Duration,
    state: Arc<Mutex<WindowState>>,
}

#[derive(Clone, Debug)]
struct WindowState {
    window_start: Instant,
    count: u32,
}

impl RateLimiter {
    /// Allow at most max_requests per window duration.
    pub fn new(max_requests: u32, window: Duration) -> Self {
        Self {
            max_requests: max_requests.max(1),
            window,
            state: Arc::new(Mutex::new(WindowState {
                window_start: Instant::now(),
                count: 0,
            })),
        }
    }

    /// Allow if under the limit for the current window; else deny.
    pub fn check(&self) -> bool {
        let mut guard = match self.state.lock() {
            Ok(g) => g,
            Err(_) => return false,
        };
        if guard.window_start.elapsed() >= self.window {
            guard.window_start = Instant::now();
            guard.count = 0;
        }
        if guard.count < self.max_requests {
            guard.count += 1;
            true
        } else {
            false
        }
    }

    /// Number of requests admitted in the current window.
    pub fn count(&self) -> u32 {
        self.state.lock().map(|g| g.count).unwrap_or(0)
    }
}

/// Layer attaching RateLimitMiddleware.
#[derive(Clone, Debug)]
pub struct RateLimitLayer {
    limiter: Arc<RateLimiter>,
}

impl RateLimitLayer {
    pub fn new(limiter: RateLimiter) -> Self {
        Self {
            limiter: Arc::new(limiter),
        }
    }
}

impl<S> Layer<S> for RateLimitLayer {
    type Service = RateLimitMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RateLimitMiddleware {
            inner,
            limiter: self.limiter.clone(),
        }
    }
}

/// Tower middleware denying requests over the rate limit with 429.
#[derive(Clone, Debug)]
pub struct RateLimitMiddleware<S> {
    inner: S,
    limiter: Arc<RateLimiter>,
}

impl<S> Service<Request<Body>> for RateLimitMiddleware<S>
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
        if !self.limiter.check() {
            let mut res = axum::response::Response::new(Body::from(
                serde_json::json!({"error": "rate limit exceeded"}).to_string(),
            ));
            *res.status_mut() = axum::http::StatusCode::TOO_MANY_REQUESTS;
            return Box::pin(async move { Ok(res) });
        }
        let mut inner = self.inner.clone();
        Box::pin(async move { inner.call(req).await })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limiter_allows_under_limit() {
        let limiter = RateLimiter::new(3, Duration::from_secs(60));
        assert!(limiter.check());
        assert!(limiter.check());
        assert!(limiter.check());
        assert_eq!(limiter.count(), 3);
    }

    #[test]
    fn limiter_denies_over_limit() {
        let limiter = RateLimiter::new(2, Duration::from_secs(60));
        assert!(limiter.check());
        assert!(limiter.check());
        assert!(!limiter.check(), "third request denied");
        assert_eq!(limiter.count(), 2);
    }

    #[test]
    fn limiter_window_resets() {
        let limiter = RateLimiter::new(1, Duration::from_millis(20));
        assert!(limiter.check());
        assert!(!limiter.check());
        std::thread::sleep(Duration::from_millis(30));
        assert!(limiter.check(), "window reset allows again");
    }

    async fn router_with_limit(limiter: RateLimiter) -> axum::Router {
        use axum::routing::get;
        axum::Router::new()
            .route("/probe", get(|| async { "ok" }))
            .layer(RateLimitLayer::new(limiter))
    }

    async fn get_status(router: axum::Router) -> u16 {
        use tower::ServiceExt;
        let req = Request::builder()
            .uri("/probe")
            .body(Body::empty())
            .unwrap();
        router.oneshot(req).await.unwrap().status().as_u16()
    }

    #[tokio::test]
    async fn middleware_returns_429_on_overflow() {
        let router = router_with_limit(RateLimiter::new(2, Duration::from_secs(60))).await;
        assert_eq!(get_status(router.clone()).await, 200);
        assert_eq!(get_status(router.clone()).await, 200);
        assert_eq!(get_status(router).await, 429);
    }
}
