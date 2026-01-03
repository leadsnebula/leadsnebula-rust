use crate::normalize_env_for_ssm;
use aws_sdk_ssm::Client as SsmClient;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, error, warn};

const MAX_RETRIES: u32 = 3;
const INITIAL_RETRY_DELAY_MS: u64 = 500;

pub struct SsmService {
    client: SsmClient,
    redis: Option<Arc<crate::redis::RedisClient>>,
    env: String,
}

impl SsmService {
    pub async fn new(env: String, redis: Option<Arc<crate::redis::RedisClient>>) -> anyhow::Result<Self> {
        // Configure AWS SDK - unset AWS_WEB_IDENTITY_TOKEN_FILE to skip web identity token provider
        // which requires sleep_impl. We'll use environment variables for credentials instead.
        std::env::remove_var("AWS_WEB_IDENTITY_TOKEN_FILE");
        std::env::remove_var("AWS_ROLE_ARN");
        std::env::remove_var("AWS_ROLE_SESSION_NAME");
        
        // Load config with explicit sleep_impl
        use aws_smithy_async::rt::sleep::TokioSleep;
        let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .sleep_impl(TokioSleep::new())
            .load()
            .await;
        let client = SsmClient::new(&config);

        Ok(Self {
            client,
            redis,
            env: normalize_env_for_ssm(&env).to_string(),
        })
    }

    pub async fn get_parameter(&self, path: &str, with_decryption: bool) -> anyhow::Result<Option<String>> {
        // Try cache first
        if let Some(redis) = &self.redis {
            let cache_key = format!("{}:ssm:{}:{}", self.env, path, with_decryption);
            if let Ok(Some(cached)) = redis.get(&cache_key).await {
                debug!("SSM cache hit: {}", path);
                return Ok(Some(cached));
            }
        }

        // Fetch from SSM with retry
        let mut retries = 0;
        let mut delay_ms = INITIAL_RETRY_DELAY_MS;

        loop {
            match self.client
                .get_parameter()
                .name(path)
                .with_decryption(with_decryption)
                .send()
                .await
            {
                Ok(response) => {
                    if let Some(param) = response.parameter() {
                        let value = param.value().unwrap_or("").to_string();

                        // Cache the value
                        if let Some(redis) = &self.redis {
                            let cache_key = format!("{}:ssm:{}:{}", self.env, path, with_decryption);
                            let ttl = if path.contains("/encryption/") { 604800 } else { 86400 }; // 7 days for encryption keys, 1 day for others
                            let _ = redis.set_with_ttl(&cache_key, &value, ttl).await;
                        }

                        return Ok(Some(value));
                    }
                    return Ok(None);
                }
                Err(e) => {
                    if retries < MAX_RETRIES {
                        retries += 1;
                        warn!("SSM retry {}/{} for {}: {}", retries, MAX_RETRIES, path, e);
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                        delay_ms *= 2;
                        continue;
                    }
                    error!("SSM error for {}: {}", path, e);
                    return Err(anyhow::anyhow!("Failed to fetch SSM parameter {}: {}", path, e));
                }
            }
        }
    }

    pub async fn get_parameters_by_path(&self, path_prefix: &str) -> anyhow::Result<HashMap<String, String>> {
        // Try cache first
        if let Some(redis) = &self.redis {
            let cache_key = format!("{}:ssm:path:{}", self.env, path_prefix);
            if let Ok(Some(cached)) = redis.get(&cache_key).await {
                debug!("SSM path cache hit: {}", path_prefix);
                if let Ok(parsed) = serde_json::from_str::<HashMap<String, String>>(&cached) {
                    return Ok(parsed);
                }
            }
        }

        // Fetch from SSM
        let mut all_params = HashMap::new();
        let mut next_token: Option<String> = None;

        loop {
            let mut request = self.client
                .get_parameters_by_path()
                .path(path_prefix)
                .recursive(true)
                .with_decryption(true);

            if let Some(token) = next_token {
                request = request.next_token(token);
            }

            match request.send().await {
                Ok(response) => {
                    let parameters = response.parameters();
                    for param in parameters {
                        if let (Some(name), Some(value)) = (param.name(), param.value()) {
                            all_params.insert(name.to_string(), value.to_string());
                        }
                    }

                    next_token = response.next_token().map(|s| s.to_string());
                    if next_token.is_none() {
                        break;
                    }
                }
                Err(e) => {
                    error!("SSM get_parameters_by_path error: {}", e);
                    return Err(anyhow::anyhow!("Failed to fetch SSM parameters: {}", e));
                }
            }
        }

        // Cache the result
        if let Some(redis) = &self.redis {
            let cache_key = format!("{}:ssm:path:{}", self.env, path_prefix);
            if let Ok(json) = serde_json::to_string(&all_params) {
                let _ = redis.set_with_ttl(&cache_key, &json, 86400).await; // 1 day cache
            }
        }

        Ok(all_params)
    }

    pub fn build_path(&self, component: &str, category: &str, key_name: Option<&str>) -> String {
        let base = format!("/leadsnebula/{}/{}/{}", self.env, component, category);
        if let Some(key) = key_name {
            format!("{}/{}", base, key)
        } else {
            base
        }
    }
}

