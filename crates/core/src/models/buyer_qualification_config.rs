use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct BuyerQualificationConfig {
    pub id: Uuid,
    pub buyer_id: Uuid,
    pub vertical_id: Uuid,
    pub buyer_integration_id: Option<Uuid>,
    pub rule_set_name: String,
    #[serde(default = "default_json_object")]
    #[sqlx(default)]
    pub config: serde_json::Value,
    #[sqlx(default)]
    pub rules_order: Vec<String>,
    pub enabled: bool,
    pub is_active: bool,
    pub timeout_seconds: Option<f64>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

fn default_json_object() -> serde_json::Value {
    serde_json::json!({})
}

impl BuyerQualificationConfig {
    /// Find qualification configs for multiple buyers in a single query
    /// Returns a HashMap keyed by buyer_id for fast lookup
    /// OPTIMIZED: Early exit if no configs exist (avoids unnecessary query when no rules configured)
    pub async fn find_by_buyer_ids(
        pool: &sqlx::PgPool,
        buyer_ids: &[Uuid],
    ) -> Result<std::collections::HashMap<Uuid, Option<Self>>, sqlx::Error> {
        if buyer_ids.is_empty() {
            return Ok(std::collections::HashMap::new());
        }

        // Early exit: Check if any configs exist for these buyers (fast EXISTS query)
        let has_configs: bool = sqlx::query_scalar(
            r#"
            SELECT EXISTS(
                SELECT 1 FROM buyer_qualification_configs
                WHERE buyer_id = ANY($1) AND enabled = true AND is_active = true
                LIMIT 1
            )
            "#,
        )
        .bind(buyer_ids)
        .fetch_one(pool)
        .await?;

        // If no configs exist, return empty map immediately (no need to query)
        if !has_configs {
            let mut result = std::collections::HashMap::new();
            for buyer_id in buyer_ids {
                result.insert(*buyer_id, None);
            }
            return Ok(result);
        }

        // Configs exist, fetch them
        let configs = sqlx::query_as::<_, BuyerQualificationConfig>(
            r#"
            SELECT * FROM buyer_qualification_configs
            WHERE buyer_id = ANY($1) AND enabled = true AND is_active = true
            ORDER BY created_at DESC
            "#,
        )
        .bind(buyer_ids)
        .fetch_all(pool)
        .await?;

        // Group by buyer_id, taking the first (most recent) config per buyer
        let mut result = std::collections::HashMap::new();
        for buyer_id in buyer_ids {
            result.insert(*buyer_id, None);
        }
        for config in configs {
            result.entry(config.buyer_id).and_modify(|e| {
                if e.is_none() {
                    *e = Some(config.clone());
                }
            });
        }

        Ok(result)
    }
}
