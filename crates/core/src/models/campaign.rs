use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::models::buyer::Buyer;
use crate::models::buyer_integration::BuyerIntegration;
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

    pub async fn find_by_ids(pool: &sqlx::PgPool, ids: &[Uuid]) -> Result<Vec<Self>, sqlx::Error> {
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

    /// Eager load campaigns with their associated buyers and buyer integrations in a single query
    /// Returns a vector of tuples: (Campaign, Option<Buyer>, Option<BuyerIntegration>)
    pub async fn find_by_ids_with_associations(
        pool: &sqlx::PgPool,
        ids: &[Uuid],
    ) -> Result<Vec<(Self, Option<Buyer>, Option<BuyerIntegration>)>, sqlx::Error> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        // Use explicit column selection to avoid conflicts and handle NULLs properly
        let rows = sqlx::query(
            r#"
            SELECT 
                c.id AS c_id, c.buyer_id AS c_buyer_id, c.publisher_id AS c_publisher_id,
                c.instance_id AS c_instance_id, c.name AS c_name, c.vertical AS c_vertical,
                c.campaign_token AS c_campaign_token, c.status AS c_status,
                c.is_documentation_test AS c_is_documentation_test,
                c.deleted_at AS c_deleted_at, c.created_at AS c_created_at, c.updated_at AS c_updated_at,
                b.id AS b_id, b.name AS b_name, b.instance_id AS b_instance_id,
                b.instance_user_id AS b_instance_user_id, b.vertical_id AS b_vertical_id,
                b.buyer_integration_id AS b_buyer_integration_id, b.status AS b_status,
                b.deleted_at AS b_deleted_at, b.created_at AS b_created_at, b.updated_at AS b_updated_at,
                b.contact_info AS b_contact_info, b.ein_tin AS b_ein_tin,
                b.address_street AS b_address_street, b.address_city AS b_address_city,
                b.address_state AS b_address_state, b.address_zip AS b_address_zip,
                b.email_address AS b_email_address, b.representative_first_name AS b_representative_first_name,
                b.representative_last_name AS b_representative_last_name, b.documents AS b_documents,
                b.post_type AS b_post_type, b.buyer_type AS b_buyer_type,
                bi.id AS bi_id, bi.name AS bi_name, bi.slug AS bi_slug,
                bi.vertical_id AS bi_vertical_id, bi.description AS bi_description,
                bi.configuration_template AS bi_configuration_template,
                bi.default_timeout AS bi_default_timeout, bi.posting_url_template AS bi_posting_url_template,
                bi.is_internal AS bi_is_internal, bi.status AS bi_status,
                bi.created_at AS bi_created_at, bi.updated_at AS bi_updated_at
            FROM campaigns c
            LEFT JOIN buyers b ON c.buyer_id = b.id AND b.deleted_at IS NULL
            LEFT JOIN buyer_integrations bi ON b.buyer_integration_id = bi.id AND bi.status = 'available'
            WHERE c.id = ANY($1) AND c.deleted_at IS NULL
            "#,
        )
        .bind(ids)
        .fetch_all(pool)
        .await?;

        let mut results = Vec::new();
        for row in rows {
            use sqlx::Row;

            // Extract Campaign
            let campaign = Campaign {
                id: row.get("c_id"),
                buyer_id: row.get("c_buyer_id"),
                publisher_id: row.get("c_publisher_id"),
                instance_id: row.get("c_instance_id"),
                name: row.get("c_name"),
                vertical: row.get("c_vertical"),
                campaign_token: row.get("c_campaign_token"),
                status: row.get("c_status"),
                is_documentation_test: row.get("c_is_documentation_test"),
                deleted_at: row.get("c_deleted_at"),
                created_at: row.get("c_created_at"),
                updated_at: row.get("c_updated_at"),
            };

            // Extract Buyer (if present)
            let buyer = if row.try_get::<Uuid, _>("b_id").is_ok() {
                Some(Buyer {
                    id: row.get("b_id"),
                    name: row.get("b_name"),
                    instance_id: row.get("b_instance_id"),
                    instance_user_id: row.get("b_instance_user_id"),
                    vertical_id: row.get("b_vertical_id"),
                    buyer_integration_id: row.get("b_buyer_integration_id"),
                    status: row.get("b_status"),
                    deleted_at: row.get("b_deleted_at"),
                    created_at: row.get("b_created_at"),
                    updated_at: row.get("b_updated_at"),
                    contact_info: row.get("b_contact_info"),
                    ein_tin: row.get("b_ein_tin"),
                    address_street: row.get("b_address_street"),
                    address_city: row.get("b_address_city"),
                    address_state: row.get("b_address_state"),
                    address_zip: row.get("b_address_zip"),
                    email_address: row.get("b_email_address"),
                    representative_first_name: row.get("b_representative_first_name"),
                    representative_last_name: row.get("b_representative_last_name"),
                    documents: row.get("b_documents"),
                    post_type: row.get("b_post_type"),
                    buyer_type: row.get("b_buyer_type"),
                })
            } else {
                None
            };

            // Extract BuyerIntegration (if present)
            let buyer_integration = if row.try_get::<Uuid, _>("bi_id").is_ok() {
                Some(BuyerIntegration {
                    id: row.get("bi_id"),
                    name: row.get("bi_name"),
                    slug: row.get("bi_slug"),
                    vertical_id: row.get("bi_vertical_id"),
                    description: row.get("bi_description"),
                    configuration_template: row.get("bi_configuration_template"),
                    default_timeout: row.get("bi_default_timeout"),
                    posting_url_template: row.get("bi_posting_url_template"),
                    is_internal: row.get("bi_is_internal"),
                    status: row.get("bi_status"),
                    created_at: row.get("bi_created_at"),
                    updated_at: row.get("bi_updated_at"),
                })
            } else {
                None
            };

            results.push((campaign, buyer, buyer_integration));
        }

        Ok(results)
    }
}
