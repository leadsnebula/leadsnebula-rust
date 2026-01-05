use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Publisher {
    pub id: Uuid,
    pub name: String,
    pub email: String,
    pub api_key_hash: String,
    pub api_key_prefix: String,
    pub status: String,
    pub total_requests: i32,
    pub last_request_at: Option<chrono::DateTime<chrono::Utc>>,
    pub instance_id: Uuid,
    pub instance_user_id: Option<Uuid>,
    pub is_documentation_test: bool,
    pub hmac_secret_hash: Option<String>,
    pub hmac_secret_prefix: Option<String>,
    pub hmac_required: bool,
    pub hmac_secret_encrypted: Option<String>,
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub representative_first_name: Option<String>,
    pub representative_last_name: Option<String>,
    pub address_street: Option<String>,
    pub address_city: Option<String>,
    pub address_state: Option<String>,
    pub address_zip: Option<String>,
    pub timezone: Option<String>,
    pub ein_tin: Option<String>,
}

impl Publisher {
    pub fn active(&self) -> bool {
        self.status == "active" && self.deleted_at.is_none()
    }

    pub fn require_hmac(&self) -> bool {
        self.hmac_required
    }

    pub fn is_documentation_test(&self) -> bool {
        self.is_documentation_test
    }

    pub async fn find_by_api_key(
        pool: &sqlx::PgPool,
        api_key: &str,
    ) -> Result<Option<Self>, sqlx::Error> {
        use sha2::{Digest, Sha256};

        let trimmed_key = api_key.trim();
        if trimmed_key.is_empty() {
            return Ok(None);
        }

        let mut hasher = Sha256::new();
        hasher.update(trimmed_key.as_bytes());
        let key_hash = hex::encode(hasher.finalize());

        sqlx::query_as::<_, Publisher>(
            "SELECT * FROM publishers WHERE api_key_hash = $1 AND deleted_at IS NULL",
        )
        .bind(key_hash)
        .fetch_optional(pool)
        .await
    }

    pub async fn record_request(&self, pool: &sqlx::PgPool) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE publishers SET last_request_at = NOW(), total_requests = total_requests + 1 WHERE id = $1",
        )
        .bind(self.id)
        .execute(pool)
        .await?;
        Ok(())
    }
}
