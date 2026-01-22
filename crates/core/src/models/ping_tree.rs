use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Row};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PingTree {
    pub id: Uuid,
    pub instance_id: Uuid,
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

    /// Find ping tree for routing with revshare info
    /// Returns (PingTree, revshare_percentage, revshare_flat_amount) or None
    pub async fn find_for_routing(
        pool: &sqlx::PgPool,
        publisher_id: &Uuid,
        vertical: &str,
    ) -> Result<
        Option<(
            Self,
            Option<rust_decimal::Decimal>,
            Option<rust_decimal::Decimal>,
        )>,
        sqlx::Error,
    > {
        // Use a custom query to join with ping_tree_publishers
        // We need to manually construct the result since FromRow doesn't handle tuples well
        let row = sqlx::query(
            r#"
            SELECT pt.*, ptp.revshare_percentage, ptp.revshare_flat_amount
            FROM ping_trees pt
            INNER JOIN ping_tree_publishers ptp ON pt.id = ptp.ping_tree_id
            WHERE ptp.publisher_id = $1
              AND pt.vertical = $2
              AND pt.deleted_at IS NULL
            ORDER BY pt.priority ASC NULLS LAST, pt.created_at ASC
            LIMIT 1
            "#,
        )
        .bind(publisher_id)
        .bind(vertical)
        .fetch_optional(pool)
        .await?;

        if let Some(row) = row {
            let ping_tree = PingTree {
                id: row.get("id"),
                instance_id: row.get("instance_id"),
                name: row.get("name"),
                vertical: row.get("vertical"),
                strategy: row.get("strategy"),
                status: row.get("status"),
                priority: row.get("priority"),
                deleted_at: row.get("deleted_at"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
            };
            let revshare_percentage: Option<rust_decimal::Decimal> = row.get("revshare_percentage");
            let revshare_flat_amount: Option<rust_decimal::Decimal> =
                row.get("revshare_flat_amount");
            Ok(Some((ping_tree, revshare_percentage, revshare_flat_amount)))
        } else {
            Ok(None)
        }
    }

    /// Find all publishers assigned to a ping tree
    pub async fn find_publishers(
        pool: &sqlx::PgPool,
        ping_tree_id: &Uuid,
    ) -> Result<Vec<crate::models::ping_tree_publisher::PingTreePublisher>, sqlx::Error> {
        crate::models::ping_tree_publisher::PingTreePublisher::find_by_ping_tree(pool, ping_tree_id)
        .await
    }

    pub async fn find_by_id(pool: &sqlx::PgPool, id: &Uuid) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, PingTree>(
            r#"
            SELECT id, instance_id, name, vertical, strategy, status, priority, 
                   deleted_at, created_at, updated_at
            FROM ping_trees 
            WHERE id = $1 AND deleted_at IS NULL
            "#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await
    }
}
