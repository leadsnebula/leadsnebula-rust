use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::models::enums::CampaignStatus;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Campaign {
    pub id: Uuid,
    pub buyer_id: Uuid,
    pub publisher_id: Uuid,
    pub instance_id: Uuid,
    pub name: Option<String>,
    pub vertical: String,
    pub campaign_token: String,
    pub status: CampaignStatus,
    pub is_documentation_test: bool,
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl Campaign {
    pub fn active(&self) -> bool {
        matches!(self.status, CampaignStatus::Active) && self.deleted_at.is_none()
    }

    pub fn is_documentation_test(&self) -> bool {
        self.is_documentation_test
    }

    pub async fn find_by_token(
        pool: &sqlx::PgPool,
        token: &str,
    ) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, Campaign>(
            "SELECT * FROM campaigns WHERE campaign_token = $1 AND deleted_at IS NULL",
        )
        .bind(token)
        .fetch_optional(pool)
        .await
    }

    pub async fn find_by_token_and_publisher(
        pool: &sqlx::PgPool,
        token: &str,
        publisher_id: Uuid,
    ) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, Campaign>(
            "SELECT * FROM campaigns WHERE campaign_token = $1 AND publisher_id = $2 AND deleted_at IS NULL",
        )
        .bind(token)
        .bind(publisher_id)
        .fetch_optional(pool)
        .await
    }

    pub async fn find_by_ids(
        pool: &sqlx::PgPool,
        ids: &[Uuid],
    ) -> Result<Vec<Self>, sqlx::Error> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        sqlx::query_as::<_, Campaign>(
            "SELECT * FROM campaigns WHERE id = ANY($1) AND deleted_at IS NULL",
        )
        .bind(ids)
        .fetch_all(pool)
        .await
    }
}
