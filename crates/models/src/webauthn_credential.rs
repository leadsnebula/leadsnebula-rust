use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct WebauthnCredential {
    pub id: Uuid,
    pub instance_user_id: Uuid,
    pub external_id: String,
    pub public_key: String,
    pub sign_count: i32,
    pub name: Option<String>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub passkey_type: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl WebauthnCredential {
    /// Check if this is a physical security key
    pub fn is_physical(&self) -> bool {
        self.passkey_type.as_deref() == Some("physical")
    }

    /// Check if this is a software passkey
    pub fn is_soft(&self) -> bool {
        self.passkey_type.as_deref() == Some("soft")
    }

    /// Get display name for passkey type
    pub fn type_display_name(&self) -> &str {
        if self.is_physical() {
            "Physical Security Key"
        } else {
            "Software Passkey"
        }
    }
}
