//! Shared gateway authentication (M1 S1).
//!
//! The auth core extracted for reuse across every ingress (HTTP/WS/SSE/stdio):
//! - BearerAuth parses an Authorization: Bearer <token> header.
//! - AuthPolicy decides allow/deny given the policy (token + loopback rule).
//! - AuthLayer/AuthMiddleware is a tower middleware enforcing the policy.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::body::Body;
use axum::extract::Request;
use tower::{Layer, Service};

/// A parsed bearer credential.
#[derive(Clone, Debug, PartialEq)]
pub struct BearerAuth {
    pub token: String,
}

/// Constant-time string comparison (token equality must not leak timing).
pub fn constant_time_eq(a: &str, b: &str) -> bool {
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    if a_bytes.len() != b_bytes.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a_bytes.iter().zip(b_bytes) {
        diff |= x ^ y;
    }
    diff == 0
}

impl BearerAuth {
    /// Extract a bearer token from an Authorization header value.
    pub fn extract(authorization: Option<&str>) -> Option<Self> {
        let token = authorization?.strip_prefix("Bearer ")?.trim();
        if token.is_empty() {
            return None;
        }
        Some(Self {
            token: token.to_string(),
        })
    }
}

/// Authentication decision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthDecision {
    /// Request is authenticated/authorized.
    Allowed,
    /// No credential was supplied.
    MissingToken,
    /// A credential was supplied but is invalid.
    InvalidToken,
}

/// Authorization roles (M1 minimal set; M6 expands to Admin/Operator/Approver/Viewer).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthRole {
    Admin,
    Operator,
}

impl AuthRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Operator => "operator",
        }
    }
}

/// The gateway authentication policy.
///
/// Mirrors the CLI GatewayAccessPolicy contract: a non-loopback bind MUST
/// carry a bearer token; loopback may allow anonymous (generated-credential)
/// operation.
#[derive(Clone, Debug)]
pub struct AuthPolicy {
    bearer_token: Option<Arc<str>>,
    allow_loopback_anonymous: bool,
    roles: std::collections::HashMap<String, AuthRole>,
}

impl AuthPolicy {
    /// Build a policy.
    ///
    /// - bearer_token: the expected token. None means no token required.
    /// - allow_loopback_anonymous: permit anonymous requests when no token is
    ///   configured (loopback-only mode).
    pub fn new(bearer_token: Option<String>, allow_loopback_anonymous: bool) -> Self {
        Self {
            bearer_token: bearer_token.map(Arc::from),
            allow_loopback_anonymous,
            roles: std::collections::HashMap::new(),
        }
    }

    /// Register a role for a specific token (RBAC minimal set).
    pub fn with_role(mut self, token: impl Into<String>, role: AuthRole) -> Self {
        self.roles.insert(token.into(), role);
        self
    }

    /// Role of an authenticated credential, if registered.
    pub fn role_of(&self, auth: &BearerAuth) -> Option<AuthRole> {
        self.roles.get(&auth.token).copied()
    }

    /// Role for a raw authorization header value, if authenticated and registered.
    pub fn role_of_header(&self, authorization: Option<&str>) -> Option<AuthRole> {
        let auth = BearerAuth::extract(authorization)?;
        if self.authorize(Some(&auth)) == AuthDecision::Allowed {
            self.role_of(&auth)
        } else {
            None
        }
    }

    /// The deny-by-default policy: token required unless loopback anonymous is
    /// explicitly allowed and no token is configured.
    pub fn deny_by_default() -> Self {
        Self::new(None, false)
    }

    /// Decide access for an optional credential.
    pub fn authorize(&self, auth: Option<&BearerAuth>) -> AuthDecision {
        match (&self.bearer_token, auth) {
            (None, _) if self.allow_loopback_anonymous => AuthDecision::Allowed,
            (None, _) => AuthDecision::MissingToken,
            (Some(expected), Some(given)) if constant_time_eq(expected.as_ref(), &given.token) => {
                AuthDecision::Allowed
            }
            (Some(_), Some(_)) => AuthDecision::InvalidToken,
            (Some(_), None) => AuthDecision::MissingToken,
        }
    }

    /// Convenience: is the request allowed?
    pub fn is_allowed(&self, authorization: Option<&str>) -> bool {
        self.authorize(BearerAuth::extract(authorization).as_ref()) == AuthDecision::Allowed
    }
}

/// Layer that attaches AuthMiddleware to a service.
#[derive(Clone, Debug)]
pub struct AuthLayer {
    policy: Arc<AuthPolicy>,
}

impl AuthLayer {
    pub fn new(policy: AuthPolicy) -> Self {
        Self {
            policy: Arc::new(policy),
        }
    }
}

impl<S> Layer<S> for AuthLayer {
    type Service = AuthMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        AuthMiddleware {
            inner,
            policy: self.policy.clone(),
        }
    }
}

/// Tower middleware enforcing the auth policy on every request.
///
/// On denial the response is a 401 with a JSON error body; otherwise the inner
/// service runs unchanged. The authenticated principal is not yet attached to
/// extensions — that wiring lands in S2/S3 alongside the unified server.
#[derive(Clone, Debug)]
pub struct AuthMiddleware<S> {
    inner: S,
    policy: Arc<AuthPolicy>,
}

impl<S> Service<Request<Body>> for AuthMiddleware<S>
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
        let auth = BearerAuth::extract(
            req.headers()
                .get("authorization")
                .and_then(|v| v.to_str().ok()),
        );
        let decision = self.policy.authorize(auth.as_ref());
        if decision != AuthDecision::Allowed {
            let status = axum::http::StatusCode::UNAUTHORIZED;
            let body = Body::from(
                serde_json::json!({"error": "missing or invalid gateway bearer token"}).to_string(),
            );
            let mut res = axum::response::Response::new(body);
            *res.status_mut() = status;
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
    fn extract_parses_bearer() {
        let auth = BearerAuth::extract(Some("Bearer abc123"));
        assert_eq!(
            auth,
            Some(BearerAuth {
                token: "abc123".into()
            })
        );
    }

    #[test]
    fn extract_rejects_missing_and_empty() {
        assert_eq!(BearerAuth::extract(None), None);
        assert_eq!(BearerAuth::extract(Some("")), None);
        assert_eq!(BearerAuth::extract(Some("Bearer ")), None);
        assert_eq!(BearerAuth::extract(Some("Basic xyz")), None);
    }

    #[test]
    fn policy_allows_valid_token() {
        let p = AuthPolicy::new(Some("secret".into()), false);
        assert_eq!(
            p.authorize(Some(&BearerAuth {
                token: "secret".into()
            })),
            AuthDecision::Allowed
        );
        assert!(p.is_allowed(Some("Bearer secret")));
    }

    #[test]
    fn policy_rejects_wrong_token() {
        let p = AuthPolicy::new(Some("secret".into()), false);
        assert_eq!(
            p.authorize(Some(&BearerAuth {
                token: "wrong".into()
            })),
            AuthDecision::InvalidToken
        );
        assert!(!p.is_allowed(Some("Bearer wrong")));
    }

    #[test]
    fn policy_rejects_missing_token_when_required() {
        let p = AuthPolicy::new(Some("secret".into()), false);
        assert_eq!(p.authorize(None), AuthDecision::MissingToken);
        assert!(!p.is_allowed(None));
    }

    #[test]
    fn policy_deny_by_default() {
        let p = AuthPolicy::deny_by_default();
        assert_eq!(p.authorize(None), AuthDecision::MissingToken);
        assert_eq!(
            p.authorize(Some(&BearerAuth {
                token: "anything".into()
            })),
            AuthDecision::MissingToken
        );
    }

    #[test]
    fn policy_loopback_anonymous() {
        let p = AuthPolicy::new(None, true);
        assert_eq!(p.authorize(None), AuthDecision::Allowed);
        assert!(p.is_allowed(None));
    }

    #[test]
    fn constant_time_eq_works() {
        assert!(constant_time_eq("abc", "abc"));
        assert!(!constant_time_eq("abc", "abd"));
        assert!(!constant_time_eq("abc", "abcd"));
        assert!(!constant_time_eq("", "a"));
    }

    #[test]
    fn policy_loopback_with_token_still_enforced() {
        let p = AuthPolicy::new(Some("t".into()), true);
        assert_eq!(p.authorize(None), AuthDecision::MissingToken);
        assert_eq!(
            p.authorize(Some(&BearerAuth { token: "t".into() })),
            AuthDecision::Allowed
        );
    }

    // --- AuthLayer middleware integration tests ---

    fn test_router(policy: AuthPolicy) -> axum::Router {
        use axum::routing::get;
        axum::Router::new()
            .route("/protected", get(|| async { "ok" }))
            .layer(AuthLayer::new(policy))
    }

    async fn get_status(router: axum::Router, auth: Option<&str>) -> u16 {
        use tower::ServiceExt;
        let mut req = axum::http::Request::builder().uri("/protected");
        if let Some(token) = auth {
            req = req.header("authorization", token);
        }
        let res = router
            .oneshot(req.body(axum::body::Body::empty()).unwrap())
            .await
            .unwrap();
        res.status().as_u16()
    }

    #[tokio::test]
    async fn middleware_rejects_missing_token() {
        let router = test_router(AuthPolicy::new(Some("secret".into()), false));
        assert_eq!(get_status(router, None).await, 401);
    }

    #[tokio::test]
    async fn middleware_rejects_wrong_token() {
        let router = test_router(AuthPolicy::new(Some("secret".into()), false));
        assert_eq!(get_status(router, Some("Bearer wrong")).await, 401);
    }

    #[tokio::test]
    async fn middleware_allows_valid_token() {
        let router = test_router(AuthPolicy::new(Some("secret".into()), false));
        assert_eq!(get_status(router, Some("Bearer secret")).await, 200);
    }

    #[tokio::test]
    async fn middleware_deny_by_default_even_with_token() {
        let router = test_router(AuthPolicy::deny_by_default());
        assert_eq!(get_status(router, Some("Bearer anything")).await, 401);
    }

    #[test]
    fn rbac_role_for_registered_token() {
        let p = AuthPolicy::new(Some("admin-token".into()), false)
            .with_role("admin-token", AuthRole::Admin)
            .with_role("op-token", AuthRole::Operator);
        let admin = BearerAuth {
            token: "admin-token".into(),
        };
        let op = BearerAuth {
            token: "op-token".into(),
        };
        assert_eq!(p.role_of(&admin), Some(AuthRole::Admin));
        assert_eq!(p.role_of(&op), Some(AuthRole::Operator));
        assert_eq!(
            p.role_of_header(Some("Bearer admin-token")),
            Some(AuthRole::Admin)
        );
    }

    #[test]
    fn rbac_unregistered_token_no_role() {
        let p = AuthPolicy::new(Some("only-token".into()), false);
        let other = BearerAuth {
            token: "only-token".into(),
        };
        // authenticated but no role registered -> None (no privilege beyond auth)
        assert_eq!(p.role_of(&other), None);
    }

    #[test]
    fn rbac_role_str() {
        assert_eq!(AuthRole::Admin.as_str(), "admin");
        assert_eq!(AuthRole::Operator.as_str(), "operator");
    }
}
