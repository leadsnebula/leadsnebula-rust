use crate::normalize_env_for_redis;
use bb8::{ManageConnection, Pool};
use redis::aio::ConnectionManager;
use redis::{AsyncCommands, Client, RedisError};
use std::error::Error;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, info};

/// Wrapper around redis::aio::ConnectionManager to implement bb8::ManageConnection
#[derive(Clone)]
pub struct RedisConnectionManager {
    client: Client,
}

impl ManageConnection for RedisConnectionManager {
    type Connection = ConnectionManager;
    type Error = RedisError;

    async fn connect(&self) -> Result<Self::Connection, Self::Error> {
        ConnectionManager::new(self.client.clone())
            .await
            .map_err(|e| {
                // Log detailed error information for debugging
                error!(
                    "Redis ConnectionManager::new() failed: {} (kind: {:?}, is_connection_refusal: {}, is_timeout: {}, is_io_error: {})",
                    e,
                    e.kind(),
                    e.is_connection_refusal(),
                    e.is_timeout(),
                    e.is_io_error()
                );
                if let Some(source) = e.source() {
                    error!("Redis error source: {}", source);
                }
                e
            })
    }

    async fn is_valid(&self, conn: &mut Self::Connection) -> Result<(), Self::Error> {
        // Test connection with a PING
        let _: String = redis::cmd("PING").query_async(conn).await?;
        Ok(())
    }

    fn has_broken(&self, _: &mut Self::Connection) -> bool {
        // Redis errors are typically recoverable
        false
    }
}

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

        // Create Redis client from URL - this handles TLS automatically for rediss:// URLs
        let client = Client::open(redis_url).map_err(|e| {
            error!(
                "Failed to create Redis client: {} (kind: {:?}, is_connection_refusal: {}, is_timeout: {}, is_io_error: {})",
                e,
                e.kind(),
                e.is_connection_refusal(),
                e.is_timeout(),
                e.is_io_error()
            );
            if let Some(source) = e.source() {
                error!("Redis Client::open() error source: {}", source);
            }
            anyhow::anyhow!("Redis client error: {}", e)
        })?;

        info!(
            "Creating Redis connection manager (TLS: {})...",
            redis_url.starts_with("rediss://")
        );

        // Create our custom ManageConnection wrapper
        let manager = RedisConnectionManager { client };

        info!(
            "Building Redis connection pool (size: {}, min_idle: 2)...",
            pool_size
        );
        let pool = Pool::builder()
            .max_size(pool_size)
            .min_idle(Some(2))
            .connection_timeout(Duration::from_secs(30)) // Increased to 30s for TLS handshake
            .test_on_check_out(true)
            .idle_timeout(Some(Duration::from_secs(60)))
            .build(manager)
            .await
            .map_err(|e| {
                let error_source = e
                    .source()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                error!(
                    "Failed to build Redis connection pool: {} (source: {})",
                    e, error_source
                );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires Redis URL in environment
    async fn test_redis_tls_connection() {
        let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL must be set for this test");

        // Test that we can create a client and connection manager
        let client = Client::open(redis_url.as_str()).expect("Failed to create Redis client");
        let mut conn = ConnectionManager::new(client)
            .await
            .expect("Failed to create connection manager");

        // Test PING
        let result: String = redis::cmd("PING")
            .query_async(&mut conn)
            .await
            .expect("PING failed");
        assert_eq!(result, "PONG");

        // Test SET/GET
        let _: () = conn
            .set("test_key", "test_value")
            .await
            .expect("SET failed");
        let value: Option<String> = conn.get("test_key").await.expect("GET failed");
        assert_eq!(value, Some("test_value".to_string()));

        // Cleanup
        let _: () = conn.del("test_key").await.expect("DEL failed");
    }
}
