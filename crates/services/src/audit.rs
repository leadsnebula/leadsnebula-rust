use anyhow::Result;
use chrono::Utc;
use serde_json::{json, Value};
use sqlx::PgPool;
use tracing::debug;
use uuid::Uuid;

/// Audit service for logging security and compliance events
pub struct AuditService;

impl AuditService {
    /// Log an audit event (async, non-blocking)
    /// This follows the Ruby audit log structure for consistency
    #[allow(clippy::too_many_arguments)]
    pub async fn log_event(
        pool: &PgPool,
        instance_id: Option<Uuid>,
        user_id: Option<Uuid>,
        action_type: &str,
        resource_type: Option<&str>,
        resource_id: Option<Uuid>,
        details: Value,
        ip_address: Option<&str>,
        user_agent: Option<&str>,
        affected_resources: Option<Value>,
    ) -> Result<()> {
        let affected = affected_resources.unwrap_or_else(|| json!({}));

        sqlx::query(
            r#"
            INSERT INTO audit_logs (
                instance_id, instance_user_id, action_type, resource_type, resource_id,
                details, affected_resources, ip_address, user_agent, created_at, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            "#,
        )
        .bind(instance_id)
        .bind(user_id)
        .bind(action_type)
        .bind(resource_type)
        .bind(resource_id)
        .bind(details)
        .bind(affected)
        .bind(ip_address)
        .bind(user_agent)
        .bind(Utc::now())
        .bind(Utc::now())
        .execute(pool)
        .await?;

        debug!(
            "Audit event logged: action_type={}, user_id={:?}",
            action_type, user_id
        );
        Ok(())
    }

    /// Log a login attempt (success or failure)
    #[allow(clippy::too_many_arguments)]
    pub async fn log_login_attempt(
        pool: &PgPool,
        user_id: Option<Uuid>,
        email: Option<&str>,
        success: bool,
        failure_reason: Option<&str>,
        ip_address: Option<&str>,
        user_agent: Option<&str>,
        instance_id: Option<Uuid>,
    ) -> Result<()> {
        let action_type = if success {
            "login_success"
        } else {
            "login_failed"
        };

        let details = if success {
            json!({
                "action": "login",
                "outcome": "success",
                "target_type": "InstanceUser",
                "target_id": user_id,
                "target_name": email,
                "timestamp": Utc::now().to_rfc3339()
            })
        } else {
            json!({
                "action": "login",
                "outcome": "failure",
                "target_type": user_id.map(|_| "InstanceUser").unwrap_or("UnknownUser"),
                "target_id": user_id,
                "target_name": email,
                "failed_attempts": [{
                    "reason": failure_reason,
                    "timestamp": Utc::now().to_rfc3339(),
                    "ip_address": ip_address
                }],
                "attempt_count": 1,
                "first_attempt_at": Utc::now().to_rfc3339(),
                "last_attempt_at": Utc::now().to_rfc3339(),
                "timestamp": Utc::now().to_rfc3339()
            })
        };

        Self::log_event(
            pool,
            instance_id,
            user_id,
            action_type,
            Some("InstanceUser"),
            user_id,
            details,
            ip_address,
            user_agent,
            None,
        )
        .await
    }

    /// Log a password change event
    pub async fn log_password_change(
        pool: &PgPool,
        user_id: Uuid,
        instance_id: Option<Uuid>,
        ip_address: Option<&str>,
        user_agent: Option<&str>,
    ) -> Result<()> {
        let details = json!({
            "action": "update",
            "target_type": "InstanceUser",
            "target_id": user_id,
            "property_name": "password",
            "outcome": "success",
            "timestamp": Utc::now().to_rfc3339()
        });

        Self::log_event(
            pool,
            instance_id,
            Some(user_id),
            "pwd_changed",
            Some("InstanceUser"),
            Some(user_id),
            details,
            ip_address,
            user_agent,
            None,
        )
        .await
    }

    /// Log a password policy update
    #[allow(clippy::too_many_arguments)]
    pub async fn log_password_policy_update(
        pool: &PgPool,
        user_id: Uuid,
        instance_id: Option<Uuid>,
        property_name: &str,
        old_value: &str,
        new_value: &str,
        ip_address: Option<&str>,
        user_agent: Option<&str>,
    ) -> Result<()> {
        let action_type = match property_name {
            "password_expiry_days" => "pwd_expiry_changed",
            "password_reuse_count" => "pwd_reuse_changed",
            "password_min_length" => "pwd_len_changed",
            "password_require_uppercase" => "pwd_upper_changed",
            "password_require_lowercase" => "pwd_lower_changed",
            "password_require_numbers" => "pwd_num_changed",
            "password_require_special_chars" => "pwd_special_changed",
            _ => "password_policy_update_failed",
        };

        let details = json!({
            "action": "update",
            "target_type": "PasswordPolicyConfig",
            "target_name": property_name,
            "changes": {
                property_name: {
                    "before": old_value,
                    "after": new_value
                }
            },
            "outcome": "success",
            "timestamp": Utc::now().to_rfc3339()
        });

        Self::log_event(
            pool,
            instance_id,
            Some(user_id),
            action_type,
            Some("PasswordPolicyConfig"),
            None,
            details,
            ip_address,
            user_agent,
            None,
        )
        .await
    }

    /// Log a password change request
    pub async fn log_password_change_request(
        pool: &PgPool,
        user_id: Uuid,
        instance_id: Option<Uuid>,
    ) -> Result<()> {
        let details = json!({
            "action": "request",
            "target_type": "InstanceUser",
            "target_id": user_id,
            "outcome": "success",
            "timestamp": Utc::now().to_rfc3339()
        });

        Self::log_event(
            pool,
            instance_id,
            Some(user_id),
            "pwd_change_req",
            Some("InstanceUser"),
            Some(user_id),
            details,
            None,
            None,
            None,
        )
        .await
    }

    /// Log OTP enabled event
    pub async fn log_otp_enabled(
        pool: &PgPool,
        user_id: Uuid,
        instance_id: Option<Uuid>,
    ) -> Result<()> {
        let details = json!({
            "action": "create",
            "target_type": "UserOtpSetting",
            "target_id": user_id,
            "changes": {
                "otp_enabled": {
                    "before": false,
                    "after": true
                }
            },
            "outcome": "success",
            "timestamp": Utc::now().to_rfc3339()
        });

        Self::log_event(
            pool,
            instance_id,
            Some(user_id),
            "otp_enabled",
            Some("UserOtpSetting"),
            Some(user_id),
            details,
            None,
            None,
            None,
        )
        .await
    }

    /// Log OTP disabled event
    pub async fn log_otp_disabled(
        pool: &PgPool,
        user_id: Uuid,
        instance_id: Option<Uuid>,
    ) -> Result<()> {
        let details = json!({
            "action": "update",
            "target_type": "UserOtpSetting",
            "target_id": user_id,
            "changes": {
                "otp_enabled": {
                    "before": true,
                    "after": false
                }
            },
            "outcome": "success",
            "timestamp": Utc::now().to_rfc3339()
        });

        Self::log_event(
            pool,
            instance_id,
            Some(user_id),
            "otp_disabled",
            Some("UserOtpSetting"),
            Some(user_id),
            details,
            None,
            None,
            None,
        )
        .await
    }
}
