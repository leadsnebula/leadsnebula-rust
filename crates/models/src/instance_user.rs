use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// InstanceUser model - represents a user in the system
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct InstanceUser {
    pub id: Uuid,
    pub email: String,
    #[serde(skip_serializing)]
    pub encrypted_password: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub status: String, // active, suspended, revoked, pending_verification
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_password_change_at: Option<DateTime<Utc>>,
    pub preferred_2fa_method: Option<String>,
    pub passwordless_login_enabled: Option<bool>,
}

impl InstanceUser {
    /// Check if user is active
    pub fn is_active(&self) -> bool {
        self.status == "active"
    }
}
