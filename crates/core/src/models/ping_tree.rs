use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PingTree {
    pub id: Uuid,
    pub instance_id: Uuid,
    pub publisher_id: Uuid,
    pub name: String,
    pub vertical: String,
    pub strategy: String, // 'ping_post' or 'fullpost'
    pub status: String,   // 'active' or 'paused'
    pub priority: Option<i32>,
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl PingTree {
    pub fn is_active(&self) -> bool {
        self.status == "active" && self.deleted_at.is_none()
    }

    pub async fn find_for_routing(
        pool: &sqlx::PgPool,
        publisher_id: &Uuid,
        vertical: &str,
    ) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, PingTree>(
            r#"
            SELECT * FROM ping_trees
            WHERE publisher_id = $1
              AND vertical = $2
              AND status = 'active'
              AND deleted_at IS NULL
            ORDER BY priority ASC NULLS LAST, created_at ASC
            LIMIT 1
            "#,
        )
        .bind(publisher_id)
        .bind(vertical)
        .fetch_optional(pool)
        .await
    }

    pub async fn find_by_id(pool: &sqlx::PgPool, id: &Uuid) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, PingTree>(
            "SELECT * FROM ping_trees WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_optional(pool)
        .await
    }
}
