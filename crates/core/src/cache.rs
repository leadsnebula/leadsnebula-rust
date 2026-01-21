use crate::redis::RedisClient;
use moka::future::Cache;
use redis::{cmd, AsyncCommands};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub struct CacheService {
    redis: Option<Arc<RedisClient>>,
    l1_cache: Cache<String, String>, // moka in-memory L1 cache
    env: String,
    // Cache operation tracking
    hits: Arc<AtomicU64>,
    misses: Arc<AtomicU64>,
}

impl CacheService {
    pub fn new(redis: Option<Arc<RedisClient>>, env: String) -> Self {
        // Create moka L1 cache with 10,000 entries max and 1 hour TTL
        let l1_cache = Cache::builder()
            .max_capacity(10_000)
            .time_to_live(Duration::from_secs(3600))
            .build();
        Self {
            redis,
            l1_cache,
            env,
            hits: Arc::new(AtomicU64::new(0)),
            misses: Arc::new(AtomicU64::new(0)),
        }
    }

    fn get_ttl(&self, is_dev: bool) -> u64 {
        if is_dev {
            300 // 5 minutes for dev
        } else {
            3600 // 1 hour for prod
        }
    }

    pub async fn get(&self, key: &str) -> anyhow::Result<Option<String>> {
        if let Some(redis) = &self.redis {
            let cache_key = format!("cache:{}", key);
            let result = redis.get(&cache_key).await;
            match &result {
                Ok(Some(_)) => {
                    self.hits.fetch_add(1, Ordering::Relaxed);
                    tracing::info!(
                        cache_key = %key,
                        cache_result = "hit",
                        "Cache hit"
                    );
                }
                Ok(None) => {
                    self.misses.fetch_add(1, Ordering::Relaxed);
                    tracing::info!(
                        cache_key = %key,
                        cache_result = "miss",
                        "Cache miss"
                    );
                }
                Err(e) => {
                    self.misses.fetch_add(1, Ordering::Relaxed);
                    tracing::warn!(
                        cache_key = %key,
                        cache_result = "error",
                        error = %e,
                        "Cache get error"
                    );
                }
            }
            result
        } else {
            self.misses.fetch_add(1, Ordering::Relaxed);
            tracing::info!(
                cache_key = %key,
                cache_result = "miss",
                reason = "redis_not_configured",
                "Cache miss (Redis not configured)"
            );
            Ok(None)
        }
    }

    pub async fn set(&self, key: &str, value: &str) -> anyhow::Result<()> {
        let start = std::time::Instant::now();
        if let Some(redis) = &self.redis {
            let cache_key = format!("cache:{}", key);
            let is_dev = self.env == "dev" || self.env == "development";
            let ttl = self.get_ttl(is_dev);
            let result = redis.set_with_ttl(&cache_key, value, ttl).await;
            let duration_ms = start.elapsed().as_millis() as u64;
            match &result {
                Ok(_) => {
                    tracing::info!(
                        operation = "cache_set",
                        cache_key = %key,
                        full_key = %cache_key,
                        value_length = value.len(),
                        ttl_seconds = ttl,
                        duration_ms = duration_ms,
                        "Cache SET operation"
                    );
                }
                Err(e) => {
                    tracing::error!(
                        operation = "cache_set",
                        cache_key = %key,
                        full_key = %cache_key,
                        value_length = value.len(),
                        ttl_seconds = ttl,
                        duration_ms = duration_ms,
                        error = %e,
                        "Cache SET error"
                    );
                }
            }
            result
        } else {
            tracing::debug!(
                operation = "cache_set",
                cache_key = %key,
                reason = "redis_not_configured",
                duration_ms = start.elapsed().as_millis() as u64,
                "Cache SET skipped (Redis not configured)"
            );
            Ok(())
        }
    }

    pub async fn delete(&self, key: &str) -> anyhow::Result<()> {
        let start = std::time::Instant::now();
        if let Some(redis) = &self.redis {
            let cache_key = format!("cache:{}", key);
            let result = redis.delete(&cache_key).await;
            let duration_ms = start.elapsed().as_millis() as u64;
            match &result {
                Ok(_) => {
                    tracing::info!(
                        operation = "cache_delete",
                        cache_key = %key,
                        full_key = %cache_key,
                        duration_ms = duration_ms,
                        "Cache DELETE operation"
                    );
                }
                Err(e) => {
                    tracing::error!(
                        operation = "cache_delete",
                        cache_key = %key,
                        full_key = %cache_key,
                        duration_ms = duration_ms,
                        error = %e,
                        "Cache DELETE error"
                    );
                }
            }
            result
        } else {
            tracing::debug!(
                operation = "cache_delete",
                cache_key = %key,
                reason = "redis_not_configured",
                duration_ms = start.elapsed().as_millis() as u64,
                "Cache DELETE skipped (Redis not configured)"
            );
            Ok(())
        }
    }

    pub async fn set_with_ttl(
        &self,
        key: &str,
        value: &str,
        ttl_seconds: u64,
    ) -> anyhow::Result<()> {
        let start = std::time::Instant::now();
        if let Some(redis) = &self.redis {
            let cache_key = format!("cache:{}", key);
            let result = redis.set_with_ttl(&cache_key, value, ttl_seconds).await;
            let duration_ms = start.elapsed().as_millis() as u64;
            match &result {
                Ok(_) => {
                    tracing::info!(
                        operation = "cache_set_with_ttl",
                        cache_key = %key,
                        full_key = %cache_key,
                        value_length = value.len(),
                        ttl_seconds = ttl_seconds,
                        duration_ms = duration_ms,
                        "Cache SET with TTL operation"
                    );
                }
                Err(e) => {
                    tracing::error!(
                        operation = "cache_set_with_ttl",
                        cache_key = %key,
                        full_key = %cache_key,
                        value_length = value.len(),
                        ttl_seconds = ttl_seconds,
                        duration_ms = duration_ms,
                        error = %e,
                        "Cache SET with TTL error"
                    );
                }
            }
            result
        } else {
            tracing::debug!(
                operation = "cache_set_with_ttl",
                cache_key = %key,
                reason = "redis_not_configured",
                ttl_seconds = ttl_seconds,
                duration_ms = start.elapsed().as_millis() as u64,
                "Cache SET with TTL skipped (Redis not configured)"
            );
            Ok(())
        }
    }

    /// Typed cache get-or-insert pattern with automatic serialization
    /// Uses hybrid cache: moka L1 (in-memory, <1ms) + Redis L2 (2-5ms)
    pub async fn get_or_insert_with<T, F, Fut>(
        &self,
        key: &str,
        ttl_seconds: u64,
        f: F,
    ) -> anyhow::Result<T>
    where
        T: Serialize + for<'de> Deserialize<'de>,
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = anyhow::Result<T>>,
    {
        // Check L1 (moka) first - <1ms
        if let Some(cached) = self.l1_cache.get(key).await {
            // Use simd-json for faster deserialization (requires mutable bytes)
            let mut bytes = cached.clone().into_bytes();
            match simd_json::from_slice::<T>(&mut bytes) {
                Ok(value) => {
                    self.hits.fetch_add(1, Ordering::Relaxed);
                    tracing::info!(
                        cache_key = %key,
                        cache_result = "hit",
                        cache_level = "L1",
                        ttl_seconds = ttl_seconds,
                        "Cache get_or_insert_with: L1 hit"
                    );
                    return Ok(value);
                }
                Err(_) => {
                    // Fallback to serde_json if simd-json fails (e.g., invalid JSON)
                    let bytes = cached.into_bytes();
                    if let Ok(value) = serde_json::from_slice::<T>(&bytes) {
                        self.hits.fetch_add(1, Ordering::Relaxed);
                        tracing::info!(
                            cache_key = %key,
                            cache_result = "hit",
                            cache_level = "L1",
                            ttl_seconds = ttl_seconds,
                            "Cache get_or_insert_with: L1 hit (fallback)"
                        );
                        return Ok(value);
                    }
                }
            }
        }

        // Check L2 (Redis) - 2-5ms
        // self.get() already handles None case for redis
        if let Some(cached) = self.get(key).await? {
            // Use simd-json for faster deserialization (requires mutable bytes)
            let mut bytes = cached.clone().into_bytes();
            match simd_json::from_slice::<T>(&mut bytes) {
                Ok(value) => {
                    // Store in L1 for next time
                    self.l1_cache.insert(key.to_string(), cached.clone()).await;
                    self.hits.fetch_add(1, Ordering::Relaxed);
                    tracing::info!(
                        cache_key = %key,
                        cache_result = "hit",
                        cache_level = "L2",
                        ttl_seconds = ttl_seconds,
                        "Cache get_or_insert_with: L2 hit"
                    );
                    return Ok(value);
                }
                Err(_) => {
                    // Fallback to serde_json if simd-json fails
                    let bytes = cached.clone().into_bytes();
                    if let Ok(value) = serde_json::from_slice::<T>(&bytes) {
                        // Store in L1 for next time
                        self.l1_cache.insert(key.to_string(), cached.clone()).await;
                        self.hits.fetch_add(1, Ordering::Relaxed);
                        tracing::info!(
                            cache_key = %key,
                            cache_result = "hit",
                            cache_level = "L2",
                            ttl_seconds = ttl_seconds,
                            "Cache get_or_insert_with: L2 hit (fallback)"
                        );
                        return Ok(value);
                    }
                }
            }
        }

        // Cache miss - fetch from source
        self.misses.fetch_add(1, Ordering::Relaxed);
        tracing::info!(
            cache_key = %key,
            cache_result = "miss",
            ttl_seconds = ttl_seconds,
            "Cache get_or_insert_with: miss, fetching from source"
        );
        let value = f().await?;
        // Use simd-json for faster serialization
        let serialized = match simd_json::to_vec(&value) {
            Ok(bytes) => {
                String::from_utf8(bytes).unwrap_or_else(|_| serde_json::to_string(&value).unwrap())
            }
            Err(_) => serde_json::to_string(&value)?, // Fallback to serde_json
        };

        // Store in both L1 and L2
        self.l1_cache
            .insert(key.to_string(), serialized.clone())
            .await;
        self.set_with_ttl(key, &serialized, ttl_seconds).await?;
        tracing::info!(
            cache_key = %key,
            cache_result = "set",
            ttl_seconds = ttl_seconds,
            "Cache get_or_insert_with: value cached in L1 and L2"
        );
        Ok(value)
    }

    /// Delete all keys matching a prefix pattern
    /// Uses SCAN to find matching keys, then deletes them in batches
    pub async fn delete_by_prefix(&self, prefix: &str) -> anyhow::Result<usize> {
        if let Some(redis) = &self.redis {
            // Construct the full key pattern: {env}:cache:{prefix}*
            let cache_prefix = format!("cache:{}", prefix);
            let full_pattern = format!("{}*", redis.prefix_key(&cache_prefix));

            // Use SCAN to find all keys matching the prefix
            let pool = redis.pool();
            let mut conn = pool.get().await?;
            let mut cursor = 0u64;
            let mut deleted_count = 0usize;
            let mut keys_to_delete = Vec::new();

            loop {
                let (next_cursor, keys): (u64, Vec<String>) = cmd("SCAN")
                    .arg(cursor)
                    .arg("MATCH")
                    .arg(&full_pattern)
                    .arg("COUNT")
                    .arg(100)
                    .query_async(&mut *conn)
                    .await?;

                keys_to_delete.extend(keys);
                cursor = next_cursor;

                if cursor == 0 {
                    break;
                }
            }

            // Delete keys in batches
            if !keys_to_delete.is_empty() {
                for key_batch in keys_to_delete.chunks(100) {
                    let deleted: u64 = conn.del::<_, u64>(key_batch).await?;
                    deleted_count += deleted as usize;
                }
            }

            tracing::debug!("Deleted {} keys with prefix: {}", deleted_count, prefix);
            Ok(deleted_count)
        } else {
            Ok(0)
        }
    }

    /// Get cache statistics
    pub fn get_stats(&self) -> CacheStats {
        CacheStats {
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
        }
    }

    /// Get cache hit rate as percentage
    pub fn get_hit_rate(&self) -> f64 {
        let hits = self.hits.load(Ordering::Relaxed);
        let misses = self.misses.load(Ordering::Relaxed);
        let total = hits + misses;
        if total == 0 {
            0.0
        } else {
            (hits as f64) / (total as f64) * 100.0
        }
    }
}

#[derive(Debug, Clone)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_ttl_dev() {
        let service = CacheService::new(None, "dev".to_string());
        let ttl = service.get_ttl(true);
        assert_eq!(ttl, 300); // 5 minutes
    }

    #[test]
    fn test_get_ttl_prod() {
        let service = CacheService::new(None, "prod".to_string());
        let ttl = service.get_ttl(false);
        assert_eq!(ttl, 3600); // 1 hour
    }

    #[test]
    fn test_get_ttl_development() {
        let service = CacheService::new(None, "development".to_string());
        let ttl = service.get_ttl(true);
        assert_eq!(ttl, 300); // 5 minutes
    }

    #[tokio::test]
    async fn test_get_without_redis() {
        let service = CacheService::new(None, "dev".to_string());
        let result = service.get("test_key").await.unwrap();
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn test_set_without_redis() {
        let service = CacheService::new(None, "dev".to_string());
        // Should not panic when Redis is None
        let result = service.set("test_key", "test_value").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_delete_without_redis() {
        let service = CacheService::new(None, "dev".to_string());
        // Should not panic when Redis is None
        let result = service.delete("test_key").await;
        assert!(result.is_ok());
    }
}
