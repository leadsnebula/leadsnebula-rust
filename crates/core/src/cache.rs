use crate::redis::RedisClient;
use std::sync::Arc;

pub struct CacheService {
    redis: Option<Arc<RedisClient>>,
    env: String,
}

impl CacheService {
    pub fn new(redis: Option<Arc<RedisClient>>, env: String) -> Self {
        Self { redis, env }
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
            redis.get(&cache_key).await
        } else {
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
}

