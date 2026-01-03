use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct UserOtpSetting {
    pub id: Uuid,
    pub instance_user_id: Uuid,
    pub enabled: bool,
    pub secret_encrypted: String,
    #[serde(skip_serializing)]
    pub backup_codes_encrypted: Option<String>,
    pub last_verified_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl UserOtpSetting {
    /// Parse backup codes from encrypted JSON string
    pub fn backup_codes_array(&self) -> Vec<String> {
        if let Some(ref codes_str) = self.backup_codes_encrypted {
            serde_json::from_str(codes_str).unwrap_or_default()
        } else {
            Vec::new()
        }
    }

    /// Check if a backup code is valid and consume it
    pub async fn use_backup_code(
        &mut self,
        code: &str,
        pool: &sqlx::PgPool,
    ) -> Result<bool, sqlx::Error> {
        let mut codes = self.backup_codes_array();
        let code_upper = code.to_uppercase();

        if !codes.contains(&code_upper) {
            return Ok(false);
        }

        // Remove the used code
        codes.retain(|c| c != &code_upper);

        // Update in database
        let codes_json = serde_json::to_string(&codes).unwrap_or_default();
        sqlx::query(
            r#"
            UPDATE user_otp_settings
            SET backup_codes_encrypted = $1, updated_at = $2
            WHERE id = $3
            "#,
        )
        .bind(&codes_json)
        .bind(Utc::now())
        .bind(self.id)
        .execute(pool)
        .await?;

        // Update local state
        self.backup_codes_encrypted = Some(codes_json);
        Ok(true)
    }
}
