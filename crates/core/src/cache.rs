use crate::redis::RedisClient;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, warn};

/// Cache service for expensive operations
#[derive(Clone)]
pub struct CacheService {
    redis: Option<Arc<RedisClient>>,
    // In-memory fallback cache
    in_memory:
        Arc<parking_lot::RwLock<std::collections::HashMap<String, (String, std::time::Instant)>>>,
}

impl CacheService {
    pub fn new(redis: Option<Arc<RedisClient>>) -> Self {
        Self {
            redis,
            in_memory: Arc::new(parking_lot::RwLock::new(std::collections::HashMap::new())),
        }
    }

    /// Get a cached value by key
    pub async fn get<T: for<'de> Deserialize<'de>>(&self, key: &str) -> Result<Option<T>> {
        // Try Redis first
        if let Some(redis) = &self.redis {
            let redis_key = format!("cache:{}", key);
            if let Ok(Some(json_str)) = redis.get(&redis_key).await {
                if let Ok(value) = serde_json::from_str::<T>(&json_str) {
                    debug!("Cache hit (Redis): {}", key);
                    // Also update in-memory cache
                    self.set_in_memory(key, &json_str, Duration::from_secs(300));
                    return Ok(Some(value));
                }
            }
        }

        // Fall back to in-memory
        if let Some(cached) = self.get_in_memory(key) {
            debug!("Cache hit (in-memory): {}", key);
            let value: T = serde_json::from_str(&cached)?;
            return Ok(Some(value));
        }

        debug!("Cache miss: {}", key);
        Ok(None)
    }

    /// Set a cached value with TTL
    pub async fn set<T: Serialize>(&self, key: &str, value: &T, ttl_seconds: u64) -> Result<()> {
        let json = serde_json::to_string(value)?;

        // Store in Redis if available
        if let Some(redis) = &self.redis {
            let redis_key = format!("cache:{}", key);
            if let Err(e) = redis.set_json(&redis_key, value, Some(ttl_seconds)).await {
                warn!("Failed to cache in Redis: {}", e);
            }
        }

        // Also store in-memory
        self.set_in_memory(key, &json, Duration::from_secs(ttl_seconds));

        Ok(())
    }

    /// Invalidate a cache key
    pub async fn invalidate(&self, key: &str) -> Result<()> {
        // Invalidate in Redis
        if let Some(redis) = &self.redis {
            let redis_key = format!("cache:{}", key);
            let _ = redis.delete(&redis_key).await;
        }

        // Invalidate in-memory
        let mut cache = self.in_memory.write();
        cache.remove(key);

        Ok(())
    }

    /// Invalidate all keys matching a pattern (prefix)
    pub async fn invalidate_pattern(&self, pattern: &str) -> Result<()> {
        // For Redis, we'd need SCAN which is more complex
        // For now, just invalidate in-memory matching keys
        let mut cache = self.in_memory.write();
        cache.retain(|k, _| !k.starts_with(pattern));

        // Note: Redis pattern invalidation would require SCAN command
        // which is more complex. For now, we rely on TTL expiration.
        warn!(
            "Pattern invalidation only works for in-memory cache. Redis keys will expire via TTL."
        );

        Ok(())
    }

    fn get_in_memory(&self, key: &str) -> Option<String> {
        let cache = self.in_memory.read();
        if let Some((value, expires_at)) = cache.get(key) {
            if *expires_at > std::time::Instant::now() {
                return Some(value.clone());
            } else {
                // Expired, remove it
                drop(cache);
                let mut cache = self.in_memory.write();
                cache.remove(key);
            }
        }
        None
    }

    fn set_in_memory(&self, key: &str, value: &str, ttl: Duration) {
        let mut cache = self.in_memory.write();
        cache.insert(
            key.to_string(),
            (value.to_string(), std::time::Instant::now() + ttl),
        );
    }
}

/// Cache key builders for common operations
pub mod keys {
    /// Cache key for buyer data
    pub fn buyer(buyer_id: &uuid::Uuid) -> String {
        format!("buyer:{}", buyer_id)
    }

    /// Cache key for campaign data
    pub fn campaign(campaign_id: &uuid::Uuid) -> String {
        format!("campaign:{}", campaign_id)
    }

    /// Cache key for publisher data
    pub fn publisher(publisher_id: &uuid::Uuid) -> String {
        format!("publisher:{}", publisher_id)
    }

    /// Cache key for user data
    pub fn user(user_id: &uuid::Uuid) -> String {
        format!("user:{}", user_id)
    }

    /// Cache key for instance data
    pub fn instance(instance_id: &uuid::Uuid) -> String {
        format!("instance:{}", instance_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
    struct TestData {
        id: u32,
        name: String,
    }

    #[tokio::test]
    async fn test_cache_operations() {
        let cache = CacheService::new(None);

        let data = TestData {
            id: 1,
            name: "test".to_string(),
        };

        // Set and get
        cache.set("test:key", &data, 60).await.unwrap();
        let retrieved: Option<TestData> = cache.get("test:key").await.unwrap();
        assert_eq!(retrieved, Some(data.clone()));

        // Invalidate
        cache.invalidate("test:key").await.unwrap();
        let retrieved: Option<TestData> = cache.get("test:key").await.unwrap();
        assert_eq!(retrieved, None);
    }

    #[test]
    fn test_cache_keys() {
        let buyer_id = uuid::Uuid::new_v4();
        let key = keys::buyer(&buyer_id);
        assert!(key.starts_with("buyer:"));
        assert!(key.contains(&buyer_id.to_string()));
    }
}
