use crate::provider::{CompletionRequest, CompletionResponse, LlmProvider, ProviderType};
use crate::AdapterError;
use std::time::Duration;

/// Callback fired before each retry attempt: `(attempt, delay_ms, error)`.
pub type OnRetryCallback = Box<dyn Fn(u32, u64, &str) + Send + Sync>;

// ── Configuration ─────────────────────────────────────────────────────────────

/// Configuration for exponential backoff retry.
#[derive(Clone, Debug)]
pub struct RetryConfig {
    /// Maximum number of attempts (including the first).
    pub max_attempts: u32,
    /// Initial delay before the first retry.
    pub initial_delay: Duration,
    /// Multiplier applied to the delay on each successive attempt.
    pub backoff_factor: f64,
    /// Upper bound on the computed delay.
    pub max_delay: Duration,
    /// Whether to retry on HTTP 429 (rate-limit) errors.
    pub retry_on_rate_limit: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_delay: Duration::from_millis(500),
            backoff_factor: 2.0,
            max_delay: Duration::from_secs(30),
            retry_on_rate_limit: true,
        }
    }
}

// ── RetryProvider ─────────────────────────────────────────────────────────────

/// Wraps any `LlmProvider` with exponential-backoff retry logic.
pub struct RetryProvider<P: LlmProvider> {
    inner: P,
    config: RetryConfig,
    /// Fired before each retry: `(attempt_number, delay_ms, error_message)`.
    on_retry: Option<OnRetryCallback>,
}

impl<P: LlmProvider> RetryProvider<P> {
    /// Create a new `RetryProvider` wrapping `inner` with the given config.
    pub fn new(inner: P, config: RetryConfig) -> Self {
        Self {
            inner,
            config,
            on_retry: None,
        }
    }

    /// Attach a callback invoked before each retry.
    ///
    /// Arguments: `(attempt, delay_ms, error_description)`.
    pub fn with_on_retry(mut self, f: impl Fn(u32, u64, &str) + Send + Sync + 'static) -> Self {
        self.on_retry = Some(Box::new(f));
        self
    }

    /// Return `true` if this error class should be retried.
    ///
    /// Retryable: network errors, HTTP 429, HTTP 5xx.
    /// NOT retryable: HTTP 400, 401, 403, 404 (client errors).
    pub fn is_retryable(error: &AdapterError) -> bool {
        let msg = error.to_string().to_lowercase();
        // Explicit non-retryable HTTP client error codes.
        if msg.contains("http 400")
            || msg.contains("http 401")
            || msg.contains("http 403")
            || msg.contains("http 404")
        {
            return false;
        }
        // Rate limit (HTTP 429).
        if msg.contains("429") || msg.contains("rate limit") {
            return true;
        }
        // HTTP 5xx server errors.
        if msg.contains("http 5") {
            return true;
        }
        // Network / connectivity errors (reqwest surface these without an HTTP code).
        if msg.contains("connection refused")
            || msg.contains("timed out")
            || msg.contains("timeout")
            || msg.contains("network")
            || msg.contains("connect error")
            || msg.contains("dns")
        {
            return true;
        }
        false
    }

    /// Compute the sleep duration before attempt `attempt` (0-indexed).
    ///
    /// `delay = min(initial_delay * backoff_factor^attempt, max_delay)`
    pub fn delay_for_attempt(config: &RetryConfig, attempt: u32) -> Duration {
        let base_ms = config.initial_delay.as_millis() as f64;
        let raw_ms = base_ms * config.backoff_factor.powi(attempt as i32);
        let capped_ms = raw_ms.min(config.max_delay.as_millis() as f64) as u64;
        Duration::from_millis(capped_ms)
    }
}

// ── LlmProvider impl ──────────────────────────────────────────────────────────

impl<P: LlmProvider> LlmProvider for RetryProvider<P> {
    fn provider_type(&self) -> ProviderType {
        self.inner.provider_type()
    }

    /// Call `complete` with exponential-backoff retries.
    fn complete(&self, req: &CompletionRequest) -> Result<CompletionResponse, AdapterError> {
        let max = self.config.max_attempts;
        for attempt in 0..max {
            match self.inner.complete(req) {
                Ok(resp) => return Ok(resp),
                Err(e) if !Self::is_retryable(&e) => return Err(e),
                Err(e) if attempt == max - 1 => return Err(e),
                Err(e) => {
                    let delay = Self::delay_for_attempt(&self.config, attempt);
                    if let Some(ref cb) = self.on_retry {
                        cb(attempt + 1, delay.as_millis() as u64, &e.to_string());
                    }
                    std::thread::sleep(delay);
                }
            }
        }
        // Unreachable: the loop always returns inside the last iteration.
        Err(AdapterError::Provider(
            "retry loop exhausted unexpectedly".into(),
        ))
    }

    /// Call `complete_stream` with retry — but only before the first token.
    ///
    /// Once streaming has started (i.e. `on_token` was called at least once),
    /// an error is NOT retried because the partial stream cannot be replayed.
    fn complete_stream(
        &self,
        req: &CompletionRequest,
        on_token: &mut dyn FnMut(&str),
    ) -> Result<CompletionResponse, AdapterError> {
        let max = self.config.max_attempts;
        for attempt in 0..max {
            let mut token_received = false;
            // Wrap the caller's callback so we can track whether any token arrived.
            let mut guarded = |t: &str| {
                token_received = true;
                on_token(t);
            };
            match self.inner.complete_stream(req, &mut guarded) {
                Ok(resp) => return Ok(resp),
                Err(e) if token_received => return Err(e), // mid-stream: no retry
                Err(e) if !Self::is_retryable(&e) => return Err(e),
                Err(e) if attempt == max - 1 => return Err(e),
                Err(e) => {
                    let delay = Self::delay_for_attempt(&self.config, attempt);
                    if let Some(ref cb) = self.on_retry {
                        cb(attempt + 1, delay.as_millis() as u64, &e.to_string());
                    }
                    std::thread::sleep(delay);
                }
            }
        }
        Err(AdapterError::Provider(
            "retry loop exhausted unexpectedly".into(),
        ))
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ChatMessage, CompletionResponse, FinishReason};
    use std::sync::{Arc, Mutex};

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn make_request() -> CompletionRequest {
        CompletionRequest {
            model: "test".into(),
            messages: vec![ChatMessage::text("user", "hi")],
            max_tokens: Some(16),
            temperature: None,
            tools: None,
            tool_choice: None,
            enable_cache: false,
        }
    }

    fn ok_response() -> CompletionResponse {
        CompletionResponse {
            content: "ok".into(),
            model: "test".into(),
            input_tokens: 1,
            output_tokens: 1,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            tool_calls: vec![],
            finish_reason: FinishReason::Stop,
            reasoning_content: String::new(),
        }
    }

    fn net_error() -> AdapterError {
        AdapterError::Provider("connection refused".into())
    }

    fn auth_error() -> AdapterError {
        AdapterError::Provider("HTTP 401: unauthorized".into())
    }

    fn rate_limit_error() -> AdapterError {
        AdapterError::Provider("HTTP 429: rate limit exceeded".into())
    }

    fn server_error() -> AdapterError {
        AdapterError::Provider("HTTP 500: internal server error".into())
    }

    // ── Mock provider ─────────────────────────────────────────────────────────

    /// Fails for the first `fail_count` calls then succeeds.
    struct MockProvider {
        fail_count: u32,
        calls: Arc<Mutex<u32>>,
        error_factory: fn() -> AdapterError,
    }

    impl MockProvider {
        fn new(fail_count: u32, error_factory: fn() -> AdapterError) -> (Self, Arc<Mutex<u32>>) {
            let calls = Arc::new(Mutex::new(0u32));
            let p = Self {
                fail_count,
                calls: Arc::clone(&calls),
                error_factory,
            };
            (p, calls)
        }

        fn always_ok() -> (Self, Arc<Mutex<u32>>) {
            Self::new(0, net_error)
        }

        fn fail_n_times(n: u32, err: fn() -> AdapterError) -> (Self, Arc<Mutex<u32>>) {
            Self::new(n, err)
        }
    }

    impl LlmProvider for MockProvider {
        fn provider_type(&self) -> ProviderType {
            ProviderType::OpenAiCompatible
        }

        fn complete(&self, _req: &CompletionRequest) -> Result<CompletionResponse, AdapterError> {
            let mut c = self.calls.lock().unwrap();
            *c += 1;
            if *c <= self.fail_count {
                Err((self.error_factory)())
            } else {
                Ok(ok_response())
            }
        }
    }

    fn fast_config(max_attempts: u32) -> RetryConfig {
        RetryConfig {
            max_attempts,
            initial_delay: Duration::from_millis(1),
            backoff_factor: 2.0,
            max_delay: Duration::from_millis(10),
            retry_on_rate_limit: true,
        }
    }

    // ── Tests ─────────────────────────────────────────────────────────────────

    #[test]
    fn test_retry_succeeds_on_first_attempt() {
        let (mock, calls) = MockProvider::always_ok();
        let p = RetryProvider::new(mock, fast_config(3));
        let result = p.complete(&make_request());
        assert!(result.is_ok());
        assert_eq!(
            *calls.lock().unwrap(),
            1,
            "should call provider exactly once"
        );
    }

    #[test]
    fn test_retry_retries_on_network_error() {
        // Fails once with a network error, then succeeds on second attempt.
        let (mock, calls) = MockProvider::fail_n_times(1, net_error);
        let retried = Arc::new(Mutex::new(0u32));
        let retried_clone = Arc::clone(&retried);
        let p = RetryProvider::new(mock, fast_config(3)).with_on_retry(move |_, _, _| {
            *retried_clone.lock().unwrap() += 1;
        });
        let result = p.complete(&make_request());
        assert!(
            result.is_ok(),
            "expected success after retry, got: {:?}",
            result
        );
        assert_eq!(
            *calls.lock().unwrap(),
            2,
            "should have called provider twice"
        );
        assert_eq!(*retried.lock().unwrap(), 1, "callback should fire once");
    }

    #[test]
    fn test_retry_does_not_retry_auth_error() {
        // 401 is a client error — must not retry.
        let (mock, calls) = MockProvider::fail_n_times(99, auth_error);
        let p = RetryProvider::new(mock, fast_config(3));
        let result = p.complete(&make_request());
        assert!(result.is_err());
        assert_eq!(*calls.lock().unwrap(), 1, "401 must not be retried");
    }

    #[test]
    fn test_retry_exhausts_max_attempts() {
        // Always fails with a network error — should exhaust all 3 attempts.
        let (mock, calls) = MockProvider::fail_n_times(99, net_error);
        let p = RetryProvider::new(mock, fast_config(3));
        let result = p.complete(&make_request());
        assert!(result.is_err(), "should fail after exhausting retries");
        assert_eq!(
            *calls.lock().unwrap(),
            3,
            "should have made exactly max_attempts calls"
        );
    }

    #[test]
    fn test_delay_calculation() {
        let cfg = RetryConfig {
            max_attempts: 5,
            initial_delay: Duration::from_millis(100),
            backoff_factor: 2.0,
            max_delay: Duration::from_millis(1000),
            retry_on_rate_limit: true,
        };
        assert_eq!(
            RetryProvider::<crate::provider::OpenAiProvider>::delay_for_attempt(&cfg, 0),
            Duration::from_millis(100)
        );
        assert_eq!(
            RetryProvider::<crate::provider::OpenAiProvider>::delay_for_attempt(&cfg, 1),
            Duration::from_millis(200)
        );
        assert_eq!(
            RetryProvider::<crate::provider::OpenAiProvider>::delay_for_attempt(&cfg, 2),
            Duration::from_millis(400)
        );
        // Capped at max_delay.
        assert_eq!(
            RetryProvider::<crate::provider::OpenAiProvider>::delay_for_attempt(&cfg, 4),
            Duration::from_millis(1000)
        );
    }

    #[test]
    fn test_is_retryable_classifies_correctly() {
        // Retryable.
        assert!(RetryProvider::<crate::provider::OpenAiProvider>::is_retryable(&net_error()));
        assert!(
            RetryProvider::<crate::provider::OpenAiProvider>::is_retryable(&rate_limit_error())
        );
        assert!(RetryProvider::<crate::provider::OpenAiProvider>::is_retryable(&server_error()));
        assert!(
            RetryProvider::<crate::provider::OpenAiProvider>::is_retryable(
                &AdapterError::Provider("HTTP 503: service unavailable".into())
            )
        );
        assert!(
            RetryProvider::<crate::provider::OpenAiProvider>::is_retryable(
                &AdapterError::Provider("timed out waiting for response".into())
            )
        );
        // NOT retryable.
        assert!(!RetryProvider::<crate::provider::OpenAiProvider>::is_retryable(&auth_error()));
        assert!(
            !RetryProvider::<crate::provider::OpenAiProvider>::is_retryable(
                &AdapterError::Provider("HTTP 400: bad request".into())
            )
        );
        assert!(
            !RetryProvider::<crate::provider::OpenAiProvider>::is_retryable(
                &AdapterError::Provider("HTTP 403: forbidden".into())
            )
        );
        assert!(
            !RetryProvider::<crate::provider::OpenAiProvider>::is_retryable(
                &AdapterError::Provider("HTTP 404: not found".into())
            )
        );
    }
}
