use anyhow::Result;
use serde::Deserialize;
use std::env;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub environment: String,
    pub port: u16,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let environment = env::var("ENVIRONMENT")
            .or_else(|_| env::var("ENV"))
            .unwrap_or_else(|_| "development".to_string());

        let port = env::var("PORT")
            .unwrap_or_else(|_| "8080".to_string())
            .parse::<u16>()?;

        Ok(Self { environment, port })
    }

    pub fn is_production(&self) -> bool {
        self.environment == "production" || self.environment == "prod"
    }

    pub fn is_staging(&self) -> bool {
        self.environment == "staging"
    }

    pub fn is_development(&self) -> bool {
        self.environment == "development" || self.environment == "dev"
    }
}
