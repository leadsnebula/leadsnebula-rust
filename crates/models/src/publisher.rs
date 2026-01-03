use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Publisher {
    pub id: Uuid,
    pub instance_id: Uuid,
    pub name: String,
    pub api_key_hash: String,
    pub api_key_prefix: String,
    pub status: String,
    pub is_documentation_test: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Publisher {
    /// Check if publisher is active
    pub fn is_active(&self) -> bool {
        self.status == "active"
    }
}
