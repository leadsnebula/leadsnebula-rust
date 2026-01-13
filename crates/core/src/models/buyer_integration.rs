use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct BuyerIntegration {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub vertical_id: Uuid,
    pub description: Option<String>,
    pub configuration_template: serde_json::Value,
    pub default_timeout: Option<rust_decimal::Decimal>,
    pub posting_url_template: Option<String>,
    pub is_internal: bool,
    pub status: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct BuyerIntegrationCredential {
    pub id: Uuid,
    pub buyer_integration_id: Uuid,
    pub buyer_id: Option<Uuid>,
    pub api_key_encrypted: Option<String>,
    pub api_secret_encrypted: Option<String>,
    pub ping_endpoint: Option<String>,
    pub post_endpoint: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl BuyerIntegration {
    pub async fn find_by_id(pool: &sqlx::PgPool, id: &Uuid) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, BuyerIntegration>(
            "SELECT * FROM buyer_integrations WHERE id = $1 AND status = 'available'",
        )
        .bind(id)
        .fetch_optional(pool)
        .await
    }

    pub async fn find_by_slug(
        pool: &sqlx::PgPool,
        slug: &str,
    ) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, BuyerIntegration>(
            "SELECT * FROM buyer_integrations WHERE slug = $1 AND status = 'available'",
        )
        .bind(slug)
        .fetch_optional(pool)
        .await
    }
}

impl BuyerIntegrationCredential {
    pub async fn find_by_buyer_integration_id(
        pool: &sqlx::PgPool,
        buyer_integration_id: &Uuid,
        buyer_id: Option<&Uuid>,
    ) -> Result<Option<Self>, sqlx::Error> {
        let query = if let Some(bid) = buyer_id {
            sqlx::query_as::<_, BuyerIntegrationCredential>(
                "SELECT * FROM buyer_integration_credentials WHERE buyer_integration_id = $1 AND buyer_id = $2 LIMIT 1"
            )
            .bind(buyer_integration_id)
            .bind(bid)
        } else {
            sqlx::query_as::<_, BuyerIntegrationCredential>(
                "SELECT * FROM buyer_integration_credentials WHERE buyer_integration_id = $1 AND buyer_id IS NULL LIMIT 1"
            )
            .bind(buyer_integration_id)
        };
        query.fetch_optional(pool).await
    }
}
