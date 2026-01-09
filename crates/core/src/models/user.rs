use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Type};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[sqlx(type_name = "instance_user_status_enum", rename_all = "lowercase")]
pub enum InstanceUserStatus {
    Active,
    Inactive,
    Suspended,
}

impl InstanceUserStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            InstanceUserStatus::Active => "active",
            InstanceUserStatus::Inactive => "inactive",
            InstanceUserStatus::Suspended => "suspended",
        }
    }
}

impl std::fmt::Display for InstanceUserStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub encrypted_password: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub status: InstanceUserStatus,
    pub confirmed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl User {
    pub fn is_active(&self) -> bool {
        matches!(self.status, InstanceUserStatus::Active)
    }

    pub fn is_confirmed(&self) -> bool {
        self.confirmed_at.is_some()
    }
}
