use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::models::enums::LeadStatus;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Lead {
    pub uuid: Uuid,
    pub event_id: String,
    pub lead_id: Option<String>,
    pub publisher_id: Option<Uuid>,
    pub vertical_id: Uuid,
    pub campaign_id: Option<Uuid>,
    pub buyer_id: Option<Uuid>,
    pub request_type: String,
    pub strategy: String,
    pub status: LeadStatus,
    pub promise_id: Option<String>,
    pub ping_id: Option<String>,
    pub post_id: Option<String>,
    pub session_id: Option<String>,
    pub request_stage: Option<String>,
    pub first_name_encrypted: Option<String>,
    pub last_name_encrypted: Option<String>,
    pub email_encrypted: Option<String>,
    pub cell_phone_encrypted: Option<String>,
    pub street_address_encrypted: Option<String>,
    pub city_encrypted: Option<String>,
    pub state_encrypted: Option<String>,
    pub zip_encrypted: Option<String>,
    pub ip_address_encrypted: Option<String>,
    pub email_sha256: Option<String>,
    pub phone_sha256: Option<String>,
    pub ip_address_hash: Option<String>,
    pub email_domain: Option<String>,
    pub tcpa_consent: bool,
    pub tcpa_language: String,
    pub is_test: bool,
    pub user_agent: Option<String>,
    pub referrer: Option<String>,
    pub website_url: Option<String>,
    pub click_id: Option<String>,
    pub url_consent: Option<String>,
    pub best_call_time: Option<String>,
    pub date_of_birth: Option<chrono::NaiveDate>,
    pub home_phone: Option<String>,
    pub jornaya_lead_id: Option<String>,
    pub trusted_form_url: Option<String>,
    pub fbp_cookie: Option<String>,
    pub fbc_cookie: Option<String>,
    pub utm_params: Option<serde_json::Value>,
    pub submitted_at: Option<chrono::DateTime<chrono::Utc>>,
    pub sold_at: Option<chrono::DateTime<chrono::Utc>>,
    pub retry_count: i32,
    pub next_retry_at: Option<chrono::DateTime<chrono::Utc>>,
    pub vertical_data: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl Lead {
    pub async fn find_by_lead_id(
        pool: &sqlx::PgPool,
        lead_id: &str,
    ) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, Lead>("SELECT * FROM leads WHERE lead_id = $1")
            .bind(lead_id)
            .fetch_optional(pool)
            .await
    }

    pub async fn find_by_promise_id(
        pool: &sqlx::PgPool,
        promise_id: &str,
    ) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, Lead>("SELECT * FROM leads WHERE promise_id = $1")
            .bind(promise_id)
            .fetch_optional(pool)
            .await
    }
}
