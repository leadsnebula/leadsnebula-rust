use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use rust_decimal::Decimal;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PingTreePublisher {
    pub id: Uuid,
    pub ping_tree_id: Uuid,
    pub publisher_id: Uuid,
    pub vertical: String,
    pub revshare_percentage: Option<Decimal>,
    pub revshare_flat_amount: Option<Decimal>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug)]
pub enum ValidationError {
    BothRevshareSet,
    PercentageOutOfRange,
    FlatAmountNegative,
    VerticalMismatch,
}

impl PingTreePublisher {
    /// Validate revshare values
    /// Returns Ok(()) if valid, Err(ValidationError) if invalid
    pub fn validate_revshare(
        revshare_percentage: Option<Decimal>,
        revshare_flat_amount: Option<Decimal>,
    ) -> Result<(), ValidationError> {
        // Check mutual exclusivity
        if revshare_percentage.is_some() && revshare_flat_amount.is_some() {
            return Err(ValidationError::BothRevshareSet);
        }

        // Validate percentage range
        if let Some(percentage) = revshare_percentage {
            if percentage < Decimal::ZERO || percentage > Decimal::from(100) {
                return Err(ValidationError::PercentageOutOfRange);
            }
        }

        // Validate flat amount is non-negative
        if let Some(flat) = revshare_flat_amount {
            if flat < Decimal::ZERO {
                return Err(ValidationError::FlatAmountNegative);
            }
        }

        Ok(())
    }

    /// Apply defaults: if both null, set revshare_percentage = 80.0
    /// Returns (revshare_percentage, revshare_flat_amount) with defaults applied
    pub fn apply_defaults(
        revshare_percentage: Option<Decimal>,
        revshare_flat_amount: Option<Decimal>,
    ) -> (Option<Decimal>, Option<Decimal>) {
        if revshare_percentage.is_none() && revshare_flat_amount.is_none() {
            (Some(Decimal::from(80)), None)
        } else {
            (revshare_percentage, revshare_flat_amount)
        }
    }

    /// Create a new ping tree publisher assignment
    pub async fn create(
        pool: &sqlx::PgPool,
        ping_tree_id: Uuid,
        publisher_id: Uuid,
        vertical: String,
        revshare_percentage: Option<Decimal>,
        revshare_flat_amount: Option<Decimal>,
    ) -> Result<Self, sqlx::Error> {
        // Apply defaults
        let (revshare_percentage, revshare_flat_amount) =
            Self::apply_defaults(revshare_percentage, revshare_flat_amount);

        // Validate
        Self::validate_revshare(revshare_percentage, revshare_flat_amount)
            .map_err(|_| sqlx::Error::RowNotFound)?;

        // Verify vertical matches ping_tree.vertical
        let ping_tree_vertical: String = sqlx::query_scalar(
            "SELECT vertical FROM ping_trees WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(ping_tree_id)
        .fetch_one(pool)
        .await?;

        if vertical != ping_tree_vertical {
            return Err(sqlx::Error::RowNotFound);
        }

        let id = Uuid::new_v4();
        sqlx::query_as::<_, PingTreePublisher>(
            r#"
            INSERT INTO ping_tree_publishers (
                id, ping_tree_id, publisher_id, vertical,
                revshare_percentage, revshare_flat_amount,
                created_at, updated_at
            ) VALUES ($1, $2, $3, $4, $5, $6, NOW(), NOW())
            RETURNING id, ping_tree_id, publisher_id, vertical, 
                      revshare_percentage, revshare_flat_amount,
                      created_at, updated_at
            "#,
        )
        .bind(id)
        .bind(ping_tree_id)
        .bind(publisher_id)
        .bind(&vertical)
        .bind(revshare_percentage)
        .bind(revshare_flat_amount)
        .fetch_one(pool)
        .await
    }

    /// Find all publishers for a ping tree
    pub async fn find_by_ping_tree(
        pool: &sqlx::PgPool,
        ping_tree_id: &Uuid,
    ) -> Result<Vec<Self>, sqlx::Error> {
        sqlx::query_as::<_, PingTreePublisher>(
            r#"
            SELECT id, ping_tree_id, publisher_id, vertical, 
                   revshare_percentage, revshare_flat_amount,
                   created_at, updated_at
            FROM ping_tree_publishers 
            WHERE ping_tree_id = $1 
            ORDER BY created_at ASC
            "#,
        )
        .bind(ping_tree_id)
        .fetch_all(pool)
        .await
    }

    /// Find assignment by publisher and vertical
    pub async fn find_by_publisher_and_vertical(
        pool: &sqlx::PgPool,
        publisher_id: &Uuid,
        vertical: &str,
    ) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, PingTreePublisher>(
            r#"
            SELECT id, ping_tree_id, publisher_id, vertical, 
                   revshare_percentage, revshare_flat_amount,
                   created_at, updated_at
            FROM ping_tree_publishers 
            WHERE publisher_id = $1 AND vertical = $2
            "#,
        )
        .bind(publisher_id)
        .bind(vertical)
        .fetch_optional(pool)
        .await
    }

    /// Update revshare for an assignment
    pub async fn update_revshare(
        pool: &sqlx::PgPool,
        id: &Uuid,
        revshare_percentage: Option<Decimal>,
        revshare_flat_amount: Option<Decimal>,
    ) -> Result<Self, sqlx::Error> {
        // Apply defaults
        let (revshare_percentage, revshare_flat_amount) =
            Self::apply_defaults(revshare_percentage, revshare_flat_amount);

        // Validate
        Self::validate_revshare(revshare_percentage, revshare_flat_amount)
            .map_err(|_| sqlx::Error::RowNotFound)?;

        sqlx::query_as::<_, PingTreePublisher>(
            r#"
            UPDATE ping_tree_publishers
            SET revshare_percentage = $1,
                revshare_flat_amount = $2,
                updated_at = NOW()
            WHERE id = $3
            RETURNING id, ping_tree_id, publisher_id, vertical, 
                      revshare_percentage, revshare_flat_amount,
                      created_at, updated_at
            "#,
        )
        .bind(revshare_percentage)
        .bind(revshare_flat_amount)
        .bind(id)
        .fetch_one(pool)
        .await
    }

    /// Delete an assignment
    pub async fn delete(
        pool: &sqlx::PgPool,
        ping_tree_id: &Uuid,
        publisher_id: &Uuid,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM ping_tree_publishers WHERE ping_tree_id = $1 AND publisher_id = $2")
            .bind(ping_tree_id)
            .bind(publisher_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Find by ping_tree_id and publisher_id
    pub async fn find_by_ping_tree_and_publisher(
        pool: &sqlx::PgPool,
        ping_tree_id: &Uuid,
        publisher_id: &Uuid,
    ) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, PingTreePublisher>(
            r#"
            SELECT id, ping_tree_id, publisher_id, vertical, 
                   revshare_percentage, revshare_flat_amount,
                   created_at, updated_at
            FROM ping_tree_publishers 
            WHERE ping_tree_id = $1 AND publisher_id = $2
            "#,
        )
        .bind(ping_tree_id)
        .bind(publisher_id)
        .fetch_optional(pool)
        .await
    }
}
