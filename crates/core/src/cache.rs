use crate::redis::RedisClient;
use redis::{cmd, AsyncCommands};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

pub struct CacheService {
    redis: Option<Arc<RedisClient>>,
    env: String,
    // Cache operation tracking
    hits: Arc<AtomicU64>,
    misses: Arc<AtomicU64>,
}

impl CacheService {
    pub fn new(redis: Option<Arc<RedisClient>>, env: String) -> Self {
        Self {
            redis,
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
                    tracing::debug!("Cache hit: {}", key);
                }
                Ok(None) => {
                    self.misses.fetch_add(1, Ordering::Relaxed);
                    tracing::debug!("Cache miss: {}", key);
                }
                Err(_) => {
                    self.misses.fetch_add(1, Ordering::Relaxed);
                }
            }
            result
        } else {
            self.misses.fetch_add(1, Ordering::Relaxed);
            Ok(None)
        }
    }

    pub async fn set(&self, key: &str, value: &str) -> anyhow::Result<()> {
        if let Some(redis) = &self.redis {
            let cache_key = format!("cache:{}", key);
            let is_dev = self.env == "dev" || self.env == "development";
            let ttl = self.get_ttl(is_dev);
            redis.set_with_ttl(&cache_key, value, ttl).await
        } else {
            Ok(())
        }
    }

    pub async fn delete(&self, key: &str) -> anyhow::Result<()> {
        if let Some(redis) = &self.redis {
            let cache_key = format!("cache:{}", key);
            redis.delete(&cache_key).await
        } else {
            Ok(())
        }
    }

    pub async fn set_with_ttl(
        &self,
        key: &str,
        value: &str,
        ttl_seconds: u64,
    ) -> anyhow::Result<()> {
        if let Some(redis) = &self.redis {
            let cache_key = format!("cache:{}", key);
            redis.set_with_ttl(&cache_key, value, ttl_seconds).await
        } else {
            Ok(())
        }
    }

    /// Typed cache get-or-insert pattern with automatic serialization
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
        // Try cache first
        if let Some(cached) = self.get(key).await? {
            if let Ok(value) = serde_json::from_str::<T>(&cached) {
                return Ok(value);
            }
            // If deserialization fails, treat as cache miss and continue
        }

        // Cache miss - fetch from DB
        let value = f().await?;
        let serialized = serde_json::to_string(&value)?;
        self.set_with_ttl(key, &serialized, ttl_seconds).await?;
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
