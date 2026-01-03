use anyhow::{Context, Result};
use aws_config::SdkConfig;
use aws_sdk_ssm::Client as AwsSsmClient;
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, warn};

use crate::redis::RedisClient;

// Cache entry with TTL
struct CacheEntry {
    value: String,
    expires_at: Instant,
}

// In-memory cache for SSM parameters
static SSM_CACHE: Lazy<RwLock<HashMap<String, CacheEntry>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

// Default cache TTL: 5 minutes
const DEFAULT_CACHE_TTL: Duration = Duration::from_secs(300);
// Encryption keys cache longer: 1 hour
const ENCRYPTION_KEY_CACHE_TTL: Duration = Duration::from_secs(3600);

pub struct SsmClient {
    client: Option<AwsSsmClient>,
    #[allow(dead_code)] // Kept for potential future use
    config: Option<Arc<SdkConfig>>,
    redis: Option<Arc<RedisClient>>,
}

impl SsmClient {
    pub async fn new() -> Result<Self> {
        let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
        let client = AwsSsmClient::new(&config);
        Ok(Self {
            client: Some(client),
            config: Some(Arc::new(config)),
            redis: None,
        })
    }

    /// Create a new SSM client with Redis caching
    pub async fn new_with_redis(redis: Option<Arc<RedisClient>>) -> Result<Self> {
        let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
        let client = AwsSsmClient::new(&config);
        Ok(Self {
            client: Some(client),
            config: Some(Arc::new(config)),
            redis,
        })
    }

    /// Create a dummy client that always returns None (for local dev when AWS is unavailable)
    pub fn dummy() -> Self {
        Self {
            client: None,
            config: None,
            redis: None,
        }
    }

    /// Set Redis client for caching (can be called after creation)
    pub fn with_redis(mut self, redis: Option<Arc<RedisClient>>) -> Self {
        self.redis = redis;
        self
    }

    /// Get a parameter from SSM with caching (in-memory + Redis)
    pub async fn get_parameter(&self, path: &str) -> Result<Option<String>> {
        // If no client (dummy mode), return None to trigger env var fallback
        let client = match &self.client {
            Some(c) => c,
            None => {
                debug!("SSM client not available, returning None for: {}", path);
                return Ok(None);
            }
        };

        // Check Redis cache first (if available)
        if let Some(redis) = &self.redis {
            let redis_key = format!("ssm:{}", path);
            if let Ok(Some(cached)) = redis.get(&redis_key).await {
                debug!("SSM Redis cache hit: {}", path);
                // Also update in-memory cache
                let ttl = if path.contains("/encryption/") {
                    ENCRYPTION_KEY_CACHE_TTL
                } else {
                    DEFAULT_CACHE_TTL
                };
                self.set_cache(path, cached.clone(), ttl);
                return Ok(Some(cached));
            }
        }

        // Check in-memory cache
        if let Some(cached) = self.get_from_cache(path) {
            debug!("SSM in-memory cache hit: {}", path);
            return Ok(Some(cached));
        }

        // Fetch from SSM
        debug!("SSM cache miss, fetching from AWS: {}", path);
        match self.fetch_from_ssm_internal(client, path).await {
            Ok(Some(value)) => {
                // Determine cache TTL based on path
                let ttl = if path.contains("/encryption/") {
                    ENCRYPTION_KEY_CACHE_TTL
                } else {
                    DEFAULT_CACHE_TTL
                };
                let ttl_seconds = ttl.as_secs();

                // Store in both caches
                self.set_cache(path, value.clone(), ttl);

                // Store in Redis if available
                if let Some(redis) = &self.redis {
                    let redis_key = format!("ssm:{}", path);
                    if let Err(e) = redis.set(&redis_key, &value, Some(ttl_seconds)).await {
                        warn!("Failed to cache SSM parameter in Redis: {}", e);
                    }
                }

                Ok(Some(value))
            }
            Ok(None) => Ok(None),
            Err(e) => {
                warn!("Failed to fetch SSM parameter {}: {}", path, e);
                // Return None instead of error to allow env var fallback
                Ok(None)
            }
        }
    }

    /// Get multiple parameters by path prefix
    pub async fn get_parameters_by_path(
        &self,
        path_prefix: &str,
    ) -> Result<HashMap<String, String>> {
        let client = match &self.client {
            Some(c) => c,
            None => {
                debug!(
                    "SSM client not available, returning empty map for: {}",
                    path_prefix
                );
                return Ok(HashMap::new());
            }
        };

        let mut result = HashMap::new();

        let mut paginator = client
            .get_parameters_by_path()
            .path(path_prefix)
            .recursive(true)
            .with_decryption(true)
            .into_paginator()
            .send();

        while let Some(page) = paginator.next().await {
            let page = page.context("Failed to fetch SSM parameters page")?;
            for param in page.parameters() {
                if let (Some(name), Some(value)) = (param.name(), param.value()) {
                    result.insert(name.to_string(), value.to_string());
                    // Cache individual parameters
                    let ttl = if name.contains("/encryption/") {
                        ENCRYPTION_KEY_CACHE_TTL
                    } else {
                        DEFAULT_CACHE_TTL
                    };
                    self.set_cache(name, value.to_string(), ttl);
                }
            }
        }

        Ok(result)
    }

    /// Store a parameter in SSM
    pub async fn put_parameter(
        &self,
        path: &str,
        value: &str,
        description: Option<&str>,
    ) -> Result<()> {
        let client = match &self.client {
            Some(c) => c,
            None => {
                return Err(anyhow::anyhow!("SSM client not available"));
            }
        };

        let mut request = client
            .put_parameter()
            .name(path)
            .value(value)
            .set_type(Some(aws_sdk_ssm::types::ParameterType::SecureString))
            .overwrite(true);

        if let Some(desc) = description {
            request = request.description(desc);
        }

        request
            .send()
            .await
            .context("Failed to store SSM parameter")?;

        // Invalidate cache
        self.invalidate_cache(path).await;

        Ok(())
    }

    /// Fetch parameter from SSM (no cache) - internal method
    async fn fetch_from_ssm_internal(
        &self,
        client: &AwsSsmClient,
        path: &str,
    ) -> Result<Option<String>> {
        match client
            .get_parameter()
            .name(path)
            .with_decryption(true)
            .send()
            .await
        {
            Ok(response) => {
                if let Some(param) = response.parameter() {
                    Ok(param.value().map(|s| s.to_string()))
                } else {
                    Ok(None)
                }
            }
            Err(aws_sdk_ssm::error::SdkError::ServiceError(service_err)) => {
                if service_err.err().is_parameter_not_found() {
                    Ok(None)
                } else {
                    Err(anyhow::anyhow!("SSM error: {}", service_err.err()))
                }
            }
            Err(e) => Err(anyhow::anyhow!("SSM request failed: {}", e)),
        }
    }

    /// Get from cache if not expired
    fn get_from_cache(&self, path: &str) -> Option<String> {
        let cache = SSM_CACHE.read();
        if let Some(entry) = cache.get(path) {
            if entry.expires_at > Instant::now() {
                return Some(entry.value.clone());
            } else {
                // Expired, remove it
                drop(cache);
                let mut cache = SSM_CACHE.write();
                cache.remove(path);
            }
        }
        None
    }

    /// Set cache entry
    fn set_cache(&self, path: &str, value: String, ttl: Duration) {
        let mut cache = SSM_CACHE.write();
        cache.insert(
            path.to_string(),
            CacheEntry {
                value,
                expires_at: Instant::now() + ttl,
            },
        );
    }

    /// Invalidate cache for a path and all parent paths (both in-memory and Redis)
    async fn invalidate_cache(&self, path: &str) {
        // Collect paths to invalidate first (drop lock before await)
        let paths_to_invalidate = {
            let mut cache = SSM_CACHE.write();
            cache.remove(path);

            // Also remove parent path caches
            let parts: Vec<&str> = path.split('/').collect();
            let mut paths = vec![path.to_string()];
            for i in 1..parts.len() {
                let parent_path = parts[0..=i].join("/");
                cache.remove(&parent_path);
                paths.push(parent_path);
            }
            paths
        };

        // Invalidate Redis cache (lock is dropped, safe to await)
        if let Some(redis) = &self.redis {
            for path_to_invalidate in &paths_to_invalidate {
                let redis_key = format!("ssm:{}", path_to_invalidate);
                if let Err(e) = redis.delete(&redis_key).await {
                    warn!(
                        "Failed to invalidate Redis cache for {}: {}",
                        path_to_invalidate, e
                    );
                }
            }
        }
    }
}
