use crate::normalize_env_for_redis;
use bb8::Pool;
use bb8_redis::{redis::AsyncCommands, RedisConnectionManager};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, info};

pub type RedisPool = Pool<RedisConnectionManager>;

pub struct RedisClient {
    pool: Arc<RedisPool>,
    env: String,
}

impl RedisClient {
    pub async fn new(redis_url: &str, env: String, pool_size: u32) -> anyhow::Result<Self> {
        debug!(
            "Creating Redis connection manager with URL scheme: {}",
            if redis_url.starts_with("rediss://") {
                "TLS (rediss://)"
            } else if redis_url.starts_with("redis://") {
                "Plain (redis://)"
            } else {
                "Unknown"
            }
        );

        let manager = RedisConnectionManager::new(redis_url).map_err(|e| {
            error!("Failed to create Redis connection manager: {}", e);
            anyhow::anyhow!("Redis connection manager error: {}", e)
        })?;

        info!(
            "Building Redis connection pool (size: {}, min_idle: 2)...",
            pool_size
        );
        let pool = Pool::builder()
            .max_size(pool_size)
            .min_idle(Some(2))
            .connection_timeout(Duration::from_secs(15)) // Increased from 10 to 15 seconds
            .test_on_check_out(true)
            .idle_timeout(Some(Duration::from_secs(60)))
            .build(manager)
            .await
            .map_err(|e| {
                error!("Failed to build Redis connection pool: {}", e);
                anyhow::anyhow!("Redis pool build error: {}", e)
            })?;

        info!(
            "Redis connection pool created successfully (max_size: {})",
            pool_size
        );
        Ok(Self {
            pool: Arc::new(pool),
            env: normalize_env_for_redis(&env).to_string(),
        })
    }

    pub fn pool(&self) -> Arc<RedisPool> {
        Arc::clone(&self.pool)
    }

    pub async fn ping(&self) -> anyhow::Result<String> {
        debug!("Pinging Redis...");
        let mut conn = self.pool.get().await.map_err(|e| {
            error!("Failed to get Redis connection from pool: {}", e);
            anyhow::anyhow!("Redis pool error: {}", e)
        })?;
        let result: String = conn.ping().await.map_err(|e| {
            error!("Redis PING command failed: {}", e);
            anyhow::anyhow!("Redis PING error: {}", e)
        })?;
        debug!("Redis PING successful: {}", result);
        Ok(result)
    }

    fn prefix_key(&self, key: &str) -> String {
        format!("{}:{}", self.env, key)
    }

    pub async fn get(&self, key: &str) -> anyhow::Result<Option<String>> {
        let prefixed_key = self.prefix_key(key);
        let mut conn = self.pool.get().await?;

        match conn.get::<_, Option<String>>(&prefixed_key).await {
            Ok(value) => Ok(value),
            Err(e) => {
                error!("Redis GET error for {}: {}", prefixed_key, e);
                Err(anyhow::anyhow!("Redis error: {}", e))
            }
        }
    }

    pub async fn set(&self, key: &str, value: &str) -> anyhow::Result<()> {
        self.set_with_ttl(key, value, 0).await
    }

    pub async fn set_with_ttl(
        &self,
        key: &str,
        value: &str,
        ttl_seconds: u64,
    ) -> anyhow::Result<()> {
        let prefixed_key = self.prefix_key(key);
        let mut conn = self.pool.get().await?;

        if ttl_seconds > 0 {
            conn.set_ex::<_, _, ()>(&prefixed_key, value, ttl_seconds)
                .await?;
        } else {
            conn.set::<_, _, ()>(&prefixed_key, value).await?;
        }

        Ok(())
    }

    pub async fn delete(&self, key: &str) -> anyhow::Result<()> {
        let prefixed_key = self.prefix_key(key);
        let mut conn = self.pool.get().await?;
        conn.del::<_, ()>(&prefixed_key).await?;
        Ok(())
    }

    pub async fn exists(&self, key: &str) -> anyhow::Result<bool> {
        let prefixed_key = self.prefix_key(key);
        let mut conn = self.pool.get().await?;
        let result: u64 = conn.exists(&prefixed_key).await?;
        Ok(result > 0)
    }

    pub async fn increment(&self, key: &str) -> anyhow::Result<u64> {
        let prefixed_key = self.prefix_key(key);
        let mut conn = self.pool.get().await?;
        let result: u64 = conn.incr(&prefixed_key, 1).await?;
        Ok(result)
    }

    pub async fn increment_with_ttl(&self, key: &str, ttl_seconds: u64) -> anyhow::Result<u64> {
        let result = self.increment(key).await?;
        if result == 1 && ttl_seconds > 0 {
            // Set TTL on first increment
            let prefixed_key = self.prefix_key(key);
            let mut conn = self.pool.get().await?;
            let _: () = conn.expire(&prefixed_key, ttl_seconds as i64).await?;
        }
        Ok(result)
    }
}
