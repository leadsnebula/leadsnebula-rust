use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PingTreeCampaign {
    pub id: Uuid,
    pub ping_tree_id: Uuid,
    pub campaign_id: Uuid,
    pub priority: Option<i32>,
    pub min_price: Option<f64>,
    pub max_price: Option<f64>,
    pub enabled: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl PingTreeCampaign {
    pub async fn find_enabled_for_ping_tree(
        pool: &sqlx::PgPool,
        ping_tree_id: &Uuid,
    ) -> Result<Vec<Self>, sqlx::Error> {
        sqlx::query_as::<_, PingTreeCampaign>(
            r#"
            SELECT ptc.* FROM ping_tree_campaigns ptc
            INNER JOIN campaigns c ON ptc.campaign_id = c.id
            WHERE ptc.ping_tree_id = $1
              AND ptc.enabled = true
              AND c.status = 'active'
              AND c.deleted_at IS NULL
            ORDER BY ptc.priority ASC NULLS LAST, ptc.created_at ASC
            "#,
        )
        .bind(ping_tree_id)
        .fetch_all(pool)
        .await
    }

    pub async fn find_by_ping_tree_and_campaign(
        pool: &sqlx::PgPool,
        ping_tree_id: &Uuid,
        campaign_id: &Uuid,
    ) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, PingTreeCampaign>(
            "SELECT * FROM ping_tree_campaigns WHERE ping_tree_id = $1 AND campaign_id = $2",
        )
        .bind(ping_tree_id)
        .bind(campaign_id)
        .fetch_optional(pool)
        .await
    }
}
