use anyhow::Result;
use leadsnebula_core::services::database::create_pool;
use leadsnebula_core::ssm::SsmService;
use serde_json::json;
use sqlx::Row;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let env_loaded = dotenvy::from_filename(".env.local").is_ok();
    if !env_loaded {
        let _ = dotenvy::dotenv();
    }

    let environment = std::env::var("ENVIRONMENT")
        .or_else(|_| std::env::var("ENV"))
        .unwrap_or_else(|_| "development".to_string());

    let env_normalized = leadsnebula_core::normalize_env_for_ssm(&environment);
    let ssm = Arc::new(SsmService::new(environment.clone(), None).await?);
    let config_path = format!("/leadsnebula/{}/rust/", env_normalized);
    let params = ssm.get_parameters_by_path(&config_path).await?;

    let database_url = params
        .get(&format!(
            "/leadsnebula/{}/rust/db/connection_url",
            env_normalized
        ))
        .cloned()
        .or_else(|| std::env::var("DATABASE_URL").ok())
        .ok_or_else(|| anyhow::anyhow!("DATABASE_URL not found"))?;

    let pool = create_pool(&database_url).await?;

    let sql = r#"
SELECT l.uuid::text as uuid, l.lead_id, l.promise_id, l.ping_id, l.post_id, l.buyer_id, l.campaign_id, l.submitted_at,
       p.id AS ping_row_id, p.buyer_id AS ping_buyer_id, p.campaign_id AS ping_campaign_id,
       pp.payload AS ping_payload,
       pos.id AS post_row_id, pos.buyer_id AS post_buyer_id, pos.campaign_id AS post_campaign_id,
       postp.payload AS post_payload
FROM leads l
LEFT JOIN pings p ON p.ping_id = l.ping_id
LEFT JOIN ping_payloads pp ON pp.ping_id = p.id
LEFT JOIN posts pos ON pos.post_id = l.post_id
LEFT JOIN post_payloads postp ON postp.post_id = pos.id
WHERE l.buyer_id IS NULL OR l.campaign_id IS NULL OR l.post_id IS NULL
LIMIT 200
"#;

    let rows = sqlx::query(sql).fetch_all(&pool).await?;

    for r in rows {
        let obj = json!({
            "uuid": r.try_get::<Option<String>, _>("uuid")?,
            "lead_id": r.try_get::<Option<String>, _>("lead_id")?,
            "promise_id": r.try_get::<Option<String>, _>("promise_id")?,
            "ping_id": r.try_get::<Option<String>, _>("ping_id")?,
            "post_id": r.try_get::<Option<String>, _>("post_id")?,
            "lead_buyer_id": r.try_get::<Option<uuid::Uuid>, _>("buyer_id").ok().map(|o| o.map(|u| u.to_string())),
            "lead_campaign_id": r.try_get::<Option<uuid::Uuid>, _>("campaign_id").ok().map(|o| o.map(|u| u.to_string())),
            "submitted_at": r.try_get::<Option<chrono::NaiveDateTime>, _>("submitted_at").ok().map(|o| o.map(|d| d.to_string())),
            "ping_row_id": r.try_get::<Option<i64>, _>("ping_row_id").ok().map(|o| o.map(|v| v.to_string())),
            "ping_buyer_id": r.try_get::<Option<uuid::Uuid>, _>("ping_buyer_id").ok().map(|o| o.map(|u| u.to_string())),
            "ping_campaign_id": r.try_get::<Option<uuid::Uuid>, _>("ping_campaign_id").ok().map(|o| o.map(|u| u.to_string())),
            "ping_payload": r.try_get::<Option<serde_json::Value>, _>("ping_payload").ok().flatten(),
            "post_row_id": r.try_get::<Option<i64>, _>("post_row_id").ok().map(|o| o.map(|v| v.to_string())),
            "post_buyer_id": r.try_get::<Option<uuid::Uuid>, _>("post_buyer_id").ok().map(|o| o.map(|u| u.to_string())),
            "post_campaign_id": r.try_get::<Option<uuid::Uuid>, _>("post_campaign_id").ok().map(|o| o.map(|u| u.to_string())),
            "post_payload": r.try_get::<Option<serde_json::Value>, _>("post_payload").ok().flatten(),
        });

        println!("{}", serde_json::to_string_pretty(&obj)?);
    }

    Ok(())
}
