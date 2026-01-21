use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tracing::{debug, warn};

/// Retry configuration
#[derive(Debug, Clone)]
pub struct RetryConfig {
    pub max_retries: u32,
    pub initial_delay_ms: u64,
    pub max_delay_ms: u64,
    pub multiplier: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay_ms: 100,
            max_delay_ms: 1000,
            multiplier: 2.0,
        }
    }
}

/// Execute a function with exponential backoff retry
pub async fn retry_with_backoff<F, Fut, T, E>(
    config: &RetryConfig,
    operation: &str,
    f: F,
) -> Result<T, E>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
    E: std::fmt::Display,
{
    let mut attempt = 0;
    let mut delay_ms = config.initial_delay_ms;

    loop {
        match f().await {
            Ok(result) => {
                if attempt > 0 {
                    debug!(
                        operation = %operation,
                        attempt = attempt + 1,
                        "Operation succeeded after retry"
                    );
                }
                return Ok(result);
            }
            Err(e) => {
                if attempt >= config.max_retries {
                    warn!(
                        operation = %operation,
                        attempts = attempt + 1,
                        error = %e,
                        "Operation failed after all retries"
                    );
                    return Err(e);
                }

                attempt += 1;
                warn!(
                    operation = %operation,
                    attempt = attempt,
                    max_retries = config.max_retries,
                    delay_ms = delay_ms,
                    error = %e,
                    "Operation failed, retrying with exponential backoff"
                );

                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                delay_ms = (delay_ms as f64 * config.multiplier) as u64;
                delay_ms = delay_ms.min(config.max_delay_ms);
            }
        }
    }
}

/// Simple circuit breaker state
#[derive(Debug, Clone, Copy, PartialEq)]
enum CircuitState {
    Closed,   // Normal operation
    Open,     // Circuit is open, failing fast
    HalfOpen, // Testing if service recovered
}

/// Simple circuit breaker for protecting against cascading failures
pub struct CircuitBreaker {
    state: Arc<Mutex<CircuitState>>,
    failure_count: Arc<AtomicU64>,
    success_count: Arc<AtomicU64>,
    last_failure_time: Arc<Mutex<Option<Instant>>>,
    failure_threshold: u64,
    success_threshold: u64,
    timeout: Duration,
}

impl CircuitBreaker {
    pub fn new(failure_threshold: u64, success_threshold: u64, timeout: Duration) -> Self {
        Self {
            state: Arc::new(Mutex::new(CircuitState::Closed)),
            failure_count: Arc::new(AtomicU64::new(0)),
            success_count: Arc::new(AtomicU64::new(0)),
            last_failure_time: Arc::new(Mutex::new(None)),
            failure_threshold,
            success_threshold,
            timeout,
        }
    }

    /// Check if the circuit allows the operation
    pub async fn call<F, Fut, T>(&self, f: F) -> Result<T, anyhow::Error>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<T, anyhow::Error>>,
    {
        let state = *self.state.lock().await;

        match state {
            CircuitState::Open => {
                // Check if timeout has passed, transition to half-open
                let last_failure = *self.last_failure_time.lock().await;
                if let Some(last) = last_failure {
                    if last.elapsed() >= self.timeout {
                        *self.state.lock().await = CircuitState::HalfOpen;
                        self.success_count.store(0, Ordering::Relaxed);
                        debug!("Circuit breaker transitioning to half-open");
                    } else {
                        return Err(anyhow::anyhow!("Circuit breaker is open"));
                    }
                } else {
                    return Err(anyhow::anyhow!("Circuit breaker is open"));
                }
            }
            CircuitState::HalfOpen => {
                // Already in half-open, proceed
            }
            CircuitState::Closed => {
                // Normal operation
            }
        }

        // Execute the operation
        match f().await {
            Ok(result) => {
                self.on_success().await;
                Ok(result)
            }
            Err(e) => {
                self.on_failure().await;
                Err(e)
            }
        }
    }

    async fn on_success(&self) {
        let state = *self.state.lock().await;

        match state {
            CircuitState::HalfOpen => {
                let success = self.success_count.fetch_add(1, Ordering::Relaxed) + 1;
                if success >= self.success_threshold {
                    *self.state.lock().await = CircuitState::Closed;
                    self.failure_count.store(0, Ordering::Relaxed);
                    debug!("Circuit breaker closed after successful recovery");
                }
            }
            CircuitState::Closed => {
                // Reset failure count on success
                self.failure_count.store(0, Ordering::Relaxed);
            }
            CircuitState::Open => {
                // Should not happen, but handle gracefully
            }
        }
    }

    async fn on_failure(&self) {
        let state = *self.state.lock().await;

        match state {
            CircuitState::HalfOpen => {
                // Failure in half-open, go back to open
                *self.state.lock().await = CircuitState::Open;
                *self.last_failure_time.lock().await = Some(Instant::now());
                self.success_count.store(0, Ordering::Relaxed);
                warn!("Circuit breaker reopened after failure in half-open state");
            }
            CircuitState::Closed => {
                let failures = self.failure_count.fetch_add(1, Ordering::Relaxed) + 1;
                *self.last_failure_time.lock().await = Some(Instant::now());

                if failures >= self.failure_threshold {
                    *self.state.lock().await = CircuitState::Open;
                    warn!(
                        failures = failures,
                        threshold = self.failure_threshold,
                        "Circuit breaker opened after too many failures"
                    );
                }
            }
            CircuitState::Open => {
                // Already open, update failure time
                *self.last_failure_time.lock().await = Some(Instant::now());
            }
        }
    }

    pub async fn is_open(&self) -> bool {
        *self.state.lock().await == CircuitState::Open
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;

    #[tokio::test]
    async fn test_retry_success_on_first_attempt() {
        let config = RetryConfig::default();
        let call_count = Arc::new(AtomicU64::new(0));

        let call_count_clone = call_count.clone();
        let result = retry_with_backoff(&config, "test", || {
            let call_count = call_count_clone.clone();
            async move {
                call_count.fetch_add(1, Ordering::Relaxed);
                Ok::<(), String>(())
            }
        })
        .await;

        assert!(result.is_ok());
        assert_eq!(call_count.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn test_retry_success_after_retries() {
        let config = RetryConfig {
            max_retries: 3,
            initial_delay_ms: 10,
            max_delay_ms: 100,
            multiplier: 2.0,
        };
        let call_count = Arc::new(AtomicU64::new(0));

        let call_count_clone = call_count.clone();
        let result = retry_with_backoff(&config, "test", || {
            let call_count = call_count_clone.clone();
            async move {
                let count = call_count.fetch_add(1, Ordering::Relaxed) + 1;
                if count < 3 {
                    Err::<(), String>("error".to_string())
                } else {
                    Ok(())
                }
            }
        })
        .await;

        assert!(result.is_ok());
        assert_eq!(call_count.load(Ordering::Relaxed), 3);
    }

    #[tokio::test]
    async fn test_circuit_breaker_opens_after_threshold() {
        let breaker = CircuitBreaker::new(3, 2, Duration::from_secs(1));

        // Fail 3 times
        for _ in 0..3 {
            let _ = breaker
                .call(|| async { Err::<(), _>(anyhow::anyhow!("error")) })
                .await;
        }

        assert!(breaker.is_open().await);
    }
}
