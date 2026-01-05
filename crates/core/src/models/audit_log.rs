use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AuditLog {
    pub id: Uuid,
    pub instance_id: Option<Uuid>,
    pub instance_user_id: Option<Uuid>,
    pub action_type: String,
    pub resource_type: Option<String>,
    pub resource_id: Option<Uuid>,
    pub details: serde_json::Value,
    pub affected_resources: serde_json::Value,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}
