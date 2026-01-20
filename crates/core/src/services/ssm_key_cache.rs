use crate::ssm::SsmService;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

struct CachedKey {
    key: String,
    expires_at: Instant,
}

static ENCRYPTION_KEY_CACHE: Lazy<Mutex<HashMap<String, CachedKey>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

const CACHE_TTL_SECONDS: u64 = 3600; // 1 hour

/// Get SSM parameter with in-memory caching (1 hour TTL)
/// This provides a fast in-memory cache layer on top of SSM service's Redis cache
pub async fn get_ssm_parameter_cached(
    ssm: &SsmService,
    path: &str,
    decrypt: bool,
) -> anyhow::Result<Option<String>> {
    let cache_key = format!("{}:{}", path, decrypt);

    // Check in-memory cache first
    {
        let cache = ENCRYPTION_KEY_CACHE.lock().unwrap();
        if let Some(cached) = cache.get(&cache_key) {
            if cached.expires_at > Instant::now() {
                tracing::debug!("SSM in-memory cache hit: {}", path);
                return Ok(Some(cached.key.clone()));
            }
        }
    }

    // Cache miss or expired - fetch from SSM (which may use Redis cache)
    tracing::debug!("SSM in-memory cache miss: {}", path);
    let key = ssm.get_parameter(path, decrypt).await?;

    if let Some(ref key_value) = key {
        // Store in in-memory cache
        let mut cache = ENCRYPTION_KEY_CACHE.lock().unwrap();
        cache.insert(
            cache_key,
            CachedKey {
                key: key_value.clone(),
                expires_at: Instant::now() + Duration::from_secs(CACHE_TTL_SECONDS),
            },
        );
    }

    Ok(key)
}
