use anyhow::Result;
use redis::aio::ConnectionManager;
use redis::{AsyncCommands, Client};
use std::sync::Arc;
use tracing::{info, warn};

/// Redis client wrapper with connection pooling
#[derive(Clone)]
pub struct RedisClient {
    #[allow(dead_code)] // Kept for potential reconnection logic
    client: Arc<Client>,
    connection_manager: ConnectionManager,
}

impl RedisClient {
    /// Create a new Redis client from a connection URL
    pub async fn new(redis_url: &str) -> Result<Self> {
        let client = Client::open(redis_url)?;
        let connection_manager = ConnectionManager::new(client.clone()).await?;

        // Test the connection
        let mut conn = connection_manager.clone();
        let _: String = redis::cmd("PING").query_async(&mut conn).await?;
        info!("Redis connection established successfully");

        Ok(Self {
            client: Arc::new(client),
            connection_manager,
        })
    }

    /// Set a key-value pair with optional TTL
    pub async fn set(&self, key: &str, value: &str, ttl_seconds: Option<u64>) -> Result<()> {
        let mut conn = self.connection_manager.clone();
        if let Some(ttl) = ttl_seconds {
            conn.set_ex::<_, _, ()>(key, value, ttl).await?;
        } else {
            conn.set::<_, _, ()>(key, value).await?;
        }
        Ok(())
    }

    /// Get a value by key
    pub async fn get(&self, key: &str) -> Result<Option<String>> {
        let mut conn = self.connection_manager.clone();
        let value: Option<String> = conn.get(key).await?;
        Ok(value)
    }

    /// Delete a key
    pub async fn delete(&self, key: &str) -> Result<()> {
        let mut conn = self.connection_manager.clone();
        conn.del::<_, ()>(key).await?;
        Ok(())
    }

    /// Increment a counter (useful for rate limiting)
    pub async fn increment(&self, key: &str, ttl_seconds: Option<u64>) -> Result<u64> {
        let mut conn = self.connection_manager.clone();
        let count: u64 = conn.incr(key, 1).await?;

        // Set TTL on first increment
        if count == 1 {
            if let Some(ttl) = ttl_seconds {
                conn.expire::<_, ()>(key, ttl as i64).await?;
            }
        }

        Ok(count)
    }

    /// Check if a key exists
    pub async fn exists(&self, key: &str) -> Result<bool> {
        let mut conn = self.connection_manager.clone();
        let exists: bool = conn.exists(key).await?;
        Ok(exists)
    }

    /// Set a key with JSON serialization
    pub async fn set_json<T: serde::Serialize>(
        &self,
        key: &str,
        value: &T,
        ttl_seconds: Option<u64>,
    ) -> Result<()> {
        let json = serde_json::to_string(value)?;
        self.set(key, &json, ttl_seconds).await
    }

    /// Get a value and deserialize from JSON
    pub async fn get_json<T: serde::de::DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        if let Some(json) = self.get(key).await? {
            let value: T = serde_json::from_str(&json)?;
            Ok(Some(value))
        } else {
            Ok(None)
        }
    }

    /// Get TTL for a key
    pub async fn ttl(&self, key: &str) -> Result<Option<i64>> {
        let mut conn = self.connection_manager.clone();
        let ttl: i64 = conn.ttl(key).await?;
        // -2: key doesn't exist, -1: key exists but has no expiration
        Ok(if ttl < 0 { None } else { Some(ttl) })
    }
}

/// Create a Redis client from environment or SSM
/// Returns None if Redis is not configured (for graceful degradation)
pub async fn create_redis_client(
    ssm_client: Option<&crate::ssm::SsmClient>,
    environment: &str,
) -> Result<Option<RedisClient>> {
    // Try to get Redis URL from SSM first
    let redis_url = if let Some(ssm) = ssm_client {
        let path = format!("/leadsnebula/{}/rust/redis/connection_url", environment);
        if let Ok(Some(url)) = ssm.get_parameter(&path).await {
            Some(url)
        } else {
            None
        }
    } else {
        None
    }
    .or_else(|| std::env::var("REDIS_URL").ok());

    match redis_url {
        Some(url) => match RedisClient::new(&url).await {
            Ok(client) => {
                info!("Redis client initialized successfully");
                Ok(Some(client))
            }
            Err(e) => {
                warn!(
                    "Failed to initialize Redis client: {}. Continuing without Redis.",
                    e
                );
                Ok(None)
            }
        },
        None => {
            warn!("Redis URL not configured. Continuing without Redis (rate limiting and caching disabled).");
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_redis_operations() {
        // Skip if Redis is not available
        let redis_url =
            std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".to_string());

        // Try to connect with a short timeout
        let client = match tokio::time::timeout(
            std::time::Duration::from_secs(2),
            RedisClient::new(&redis_url),
        )
        .await
        {
            Ok(Ok(client)) => client,
            _ => {
                // Redis not available, skip test
                return;
            }
        };

        // Test set/get
        client
            .set("test:key", "test_value", Some(60))
            .await
            .unwrap();
        let value = client.get("test:key").await.unwrap();
        assert_eq!(value, Some("test_value".to_string()));

        // Test increment
        let count1 = client.increment("test:counter", Some(60)).await.unwrap();
        let count2 = client.increment("test:counter", Some(60)).await.unwrap();
        assert_eq!(count2, count1 + 1);

        // Test exists
        assert!(client.exists("test:key").await.unwrap());

        // Test delete
        client.delete("test:key").await.unwrap();
        assert!(!client.exists("test:key").await.unwrap());

        // Cleanup
        let _ = client.delete("test:counter").await;
    }

    #[tokio::test]
    async fn test_redis_json_operations() {
        #[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq)]
        struct TestData {
            name: String,
            count: u32,
        }

        let redis_url =
            std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".to_string());

        // Try to connect with a short timeout
        let client = match tokio::time::timeout(
            std::time::Duration::from_secs(2),
            RedisClient::new(&redis_url),
        )
        .await
        {
            Ok(Ok(client)) => client,
            _ => {
                // Redis not available, skip test
                return;
            }
        };

        let data = TestData {
            name: "test".to_string(),
            count: 42,
        };

        client.set_json("test:json", &data, Some(60)).await.unwrap();

        let retrieved: Option<TestData> = client.get_json("test:json").await.unwrap();
        assert_eq!(retrieved, Some(data));

        // Cleanup
        let _ = client.delete("test:json").await;
    }
}
