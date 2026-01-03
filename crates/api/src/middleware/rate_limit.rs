use axum::{
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use leadsnebula_core::RedisClient;
use std::sync::Arc;
use tracing::{debug, warn};

/// Rate limit configuration
#[derive(Clone, Debug)]
pub struct RateLimitConfig {
    /// Maximum requests per window
    pub max_requests: u64,
    /// Window duration in seconds
    pub window_seconds: u64,
    /// Whether to use Redis (true) or in-memory (false)
    #[allow(dead_code)] // Informational field for future use
    pub use_redis: bool,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_requests: 100,
            window_seconds: 60,
            use_redis: true,
        }
    }
}

/// Rate limit state
#[derive(Clone)]
pub struct RateLimitState {
    pub redis: Option<Arc<RedisClient>>,
    pub config: RateLimitConfig,
    // In-memory fallback (simple HashMap-based counter)
    pub in_memory_store:
        Arc<parking_lot::RwLock<std::collections::HashMap<String, (u64, std::time::Instant)>>>,
}

impl RateLimitState {
    pub fn new(redis: Option<Arc<RedisClient>>, config: RateLimitConfig) -> Self {
        Self {
            redis,
            config,
            in_memory_store: Arc::new(parking_lot::RwLock::new(std::collections::HashMap::new())),
        }
    }
}

/// Extract client identifier from request (IP address or API key)
fn get_client_id(headers: &HeaderMap) -> String {
    // Try to get API key first (more reliable for authenticated requests)
    if let Some(api_key) = headers.get("x-api-key") {
        if let Ok(key_str) = api_key.to_str() {
            return format!("api_key:{}", key_str);
        }
    }

    // Fall back to IP address
    // In production behind a proxy, you'd check X-Forwarded-For or X-Real-IP
    if let Some(forwarded_for) = headers.get("x-forwarded-for") {
        if let Ok(ip) = forwarded_for.to_str() {
            // Take the first IP if there are multiple
            let ip = ip.split(',').next().unwrap_or("unknown").trim();
            return format!("ip:{}", ip);
        }
    }

    if let Some(real_ip) = headers.get("x-real-ip") {
        if let Ok(ip) = real_ip.to_str() {
            return format!("ip:{}", ip);
        }
    }

    // Fallback to a default identifier
    "unknown".to_string()
}

/// Check rate limit using Redis
async fn check_rate_limit_redis(
    redis: &RedisClient,
    key: &str,
    config: &RateLimitConfig,
) -> Result<bool, anyhow::Error> {
    let redis_key = format!("rate_limit:{}", key);
    let count = redis
        .increment(&redis_key, Some(config.window_seconds))
        .await?;

    Ok(count <= config.max_requests)
}

/// Check rate limit using in-memory store
fn check_rate_limit_memory(
    store: &parking_lot::RwLock<std::collections::HashMap<String, (u64, std::time::Instant)>>,
    key: &str,
    config: &RateLimitConfig,
) -> bool {
    let mut store = store.write();
    let now = std::time::Instant::now();

    // Clean up expired entries
    store.retain(|_, (_, timestamp)| {
        now.duration_since(*timestamp).as_secs() < config.window_seconds
    });

    let entry = store.entry(key.to_string()).or_insert_with(|| (0, now));

    // Check if window has expired
    if now.duration_since(entry.1).as_secs() >= config.window_seconds {
        entry.0 = 1;
        entry.1 = now;
        return true;
    }

    // Increment and check
    entry.0 += 1;
    entry.1 = now;

    entry.0 <= config.max_requests
}

/// Rate limiting middleware
pub async fn rate_limit_middleware(
    State(state): State<RateLimitState>,
    request: Request,
    next: Next,
) -> Response {
    let client_id = get_client_id(request.headers());
    let key = format!("{}:{}", client_id, state.config.window_seconds);

    let allowed = if let Some(redis) = &state.redis {
        // Use Redis if available
        match check_rate_limit_redis(redis, &key, &state.config).await {
            Ok(allowed) => allowed,
            Err(e) => {
                warn!(
                    "Redis rate limit check failed: {}. Falling back to in-memory.",
                    e
                );
                check_rate_limit_memory(&state.in_memory_store, &key, &state.config)
            }
        }
    } else {
        // Fall back to in-memory
        check_rate_limit_memory(&state.in_memory_store, &key, &state.config)
    };

    if !allowed {
        debug!("Rate limit exceeded for client: {}", client_id);
        return (
            StatusCode::TOO_MANY_REQUESTS,
            axum::Json(serde_json::json!({
                "success": false,
                "error": "Rate limit exceeded. Please try again later.",
                "status_code": 429
            })),
        )
            .into_response();
    }

    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn test_get_client_id_from_api_key() {
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", HeaderValue::from_static("test-key-123"));
        let client_id = get_client_id(&headers);
        assert_eq!(client_id, "api_key:test-key-123");
    }

    #[test]
    fn test_get_client_id_from_ip() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("192.168.1.1"));
        let client_id = get_client_id(&headers);
        assert_eq!(client_id, "ip:192.168.1.1");
    }

    #[test]
    fn test_in_memory_rate_limit() {
        let config = RateLimitConfig {
            max_requests: 5,
            window_seconds: 60,
            use_redis: false,
        };
        let store = Arc::new(parking_lot::RwLock::new(std::collections::HashMap::new()));

        // First 5 requests should be allowed
        for i in 1..=5 {
            assert!(
                check_rate_limit_memory(&store, "test_client", &config),
                "Request {} should be allowed",
                i
            );
        }

        // 6th request should be blocked
        assert!(!check_rate_limit_memory(&store, "test_client", &config));
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        routing::get,
        Router,
    };
    use tower::ServiceExt;

    async fn test_handler() -> &'static str {
        "OK"
    }

    #[tokio::test]
    async fn test_rate_limit_allows_requests_under_limit() {
        let config = RateLimitConfig {
            max_requests: 5,
            window_seconds: 60,
            use_redis: false, // Use in-memory for tests
        };
        let state = RateLimitState::new(None, config);

        let app = Router::new().route("/test", get(test_handler)).layer(
            axum::middleware::from_fn_with_state(state.clone(), rate_limit_middleware),
        );

        // First 5 requests should succeed
        for i in 1..=5 {
            let req = Request::builder()
                .uri("/test")
                .header("x-forwarded-for", "192.168.1.1")
                .body(Body::empty())
                .unwrap();

            let response = app.clone().oneshot(req).await.unwrap();
            assert_eq!(
                response.status(),
                StatusCode::OK,
                "Request {} should succeed",
                i
            );
        }
    }

    #[tokio::test]
    async fn test_rate_limit_blocks_exceeding_requests() {
        let config = RateLimitConfig {
            max_requests: 3,
            window_seconds: 60,
            use_redis: false,
        };
        let state = RateLimitState::new(None, config);

        let app = Router::new().route("/test", get(test_handler)).layer(
            axum::middleware::from_fn_with_state(state.clone(), rate_limit_middleware),
        );

        // First 3 requests should succeed
        for _i in 1..=3 {
            let req = Request::builder()
                .uri("/test")
                .header("x-forwarded-for", "192.168.1.2")
                .body(Body::empty())
                .unwrap();

            let response = app.clone().oneshot(req).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }

        // 4th request should be blocked
        let req = Request::builder()
            .uri("/test")
            .header("x-forwarded-for", "192.168.1.2")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn test_rate_limit_separate_counters_per_ip() {
        let config = RateLimitConfig {
            max_requests: 2,
            window_seconds: 60,
            use_redis: false,
        };
        let state = RateLimitState::new(None, config);

        let app = Router::new().route("/test", get(test_handler)).layer(
            axum::middleware::from_fn_with_state(state.clone(), rate_limit_middleware),
        );

        // IP 1: 2 requests (should succeed)
        for _ in 1..=2 {
            let req = Request::builder()
                .uri("/test")
                .header("x-forwarded-for", "192.168.1.10")
                .body(Body::empty())
                .unwrap();
            let response = app.clone().oneshot(req).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }

        // IP 2: 2 requests (should also succeed - separate counter)
        for _ in 1..=2 {
            let req = Request::builder()
                .uri("/test")
                .header("x-forwarded-for", "192.168.1.20")
                .body(Body::empty())
                .unwrap();
            let response = app.clone().oneshot(req).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }

        // IP 1: 3rd request (should be blocked)
        let req = Request::builder()
            .uri("/test")
            .header("x-forwarded-for", "192.168.1.10")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn test_rate_limit_uses_api_key_when_available() {
        let config = RateLimitConfig {
            max_requests: 2,
            window_seconds: 60,
            use_redis: false,
        };
        let state = RateLimitState::new(None, config);

        let app = Router::new().route("/test", get(test_handler)).layer(
            axum::middleware::from_fn_with_state(state.clone(), rate_limit_middleware),
        );

        // Same IP but different API keys should have separate counters
        for api_key in ["key1", "key2"] {
            for _ in 1..=2 {
                let req = Request::builder()
                    .uri("/test")
                    .header("x-forwarded-for", "192.168.1.1")
                    .header("x-api-key", api_key)
                    .body(Body::empty())
                    .unwrap();
                let response = app.clone().oneshot(req).await.unwrap();
                assert_eq!(response.status(), StatusCode::OK);
            }
        }
    }
}
