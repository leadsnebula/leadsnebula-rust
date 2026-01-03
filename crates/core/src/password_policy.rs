use anyhow::Result;
use sqlx::PgPool;
use uuid::Uuid;

/// Password policy validation helper
pub struct PasswordPolicyHelper;

#[derive(Debug, Clone)]
pub struct PasswordPolicy {
    pub min_length: i32,
    pub require_uppercase: bool,
    pub require_lowercase: bool,
    pub require_numbers: bool,
    pub require_special_chars: bool,
    pub password_reuse_count: i32,
}

impl PasswordPolicyHelper {
    /// Load password policy for an instance
    pub async fn load_policy(pool: &PgPool, instance_id: Uuid) -> Result<PasswordPolicy> {
        // Load policy from password_policy_config table
        let min_length: Option<i32> = sqlx::query_scalar(
            r#"
            SELECT config_value::int
            FROM password_policy_config
            WHERE config_key = 'password_min_length'
            AND instance_id = $1
            LIMIT 1
            "#,
        )
        .bind(instance_id)
        .fetch_optional(pool)
        .await?
        .flatten();

        let require_uppercase: Option<bool> = sqlx::query_scalar(
            r#"
            SELECT config_value::bool
            FROM password_policy_config
            WHERE config_key = 'password_require_uppercase'
            AND instance_id = $1
            LIMIT 1
            "#,
        )
        .bind(instance_id)
        .fetch_optional(pool)
        .await?
        .flatten();

        let require_lowercase: Option<bool> = sqlx::query_scalar(
            r#"
            SELECT config_value::bool
            FROM password_policy_config
            WHERE config_key = 'password_require_lowercase'
            AND instance_id = $1
            LIMIT 1
            "#,
        )
        .bind(instance_id)
        .fetch_optional(pool)
        .await?
        .flatten();

        let require_numbers: Option<bool> = sqlx::query_scalar(
            r#"
            SELECT config_value::bool
            FROM password_policy_config
            WHERE config_key = 'password_require_numbers'
            AND instance_id = $1
            LIMIT 1
            "#,
        )
        .bind(instance_id)
        .fetch_optional(pool)
        .await?
        .flatten();

        let require_special_chars: Option<bool> = sqlx::query_scalar(
            r#"
            SELECT config_value::bool
            FROM password_policy_config
            WHERE config_key = 'password_require_special_chars'
            AND instance_id = $1
            LIMIT 1
            "#,
        )
        .bind(instance_id)
        .fetch_optional(pool)
        .await?
        .flatten();

        let password_reuse_count: Option<i32> = sqlx::query_scalar(
            r#"
            SELECT config_value::int
            FROM password_policy_config
            WHERE config_key = 'password_reuse_count'
            AND instance_id = $1
            LIMIT 1
            "#,
        )
        .bind(instance_id)
        .fetch_optional(pool)
        .await?
        .flatten();

        Ok(PasswordPolicy {
            min_length: min_length.unwrap_or(8),
            require_uppercase: require_uppercase.unwrap_or(true),
            require_lowercase: require_lowercase.unwrap_or(true),
            require_numbers: require_numbers.unwrap_or(true),
            require_special_chars: require_special_chars.unwrap_or(false),
            password_reuse_count: password_reuse_count.unwrap_or(5),
        })
    }

    /// Validate password against policy
    pub fn validate_password(password: &str, policy: &PasswordPolicy) -> Result<Vec<String>> {
        let mut errors = Vec::new();

        // Check minimum length
        if password.len() < policy.min_length as usize {
            errors.push(format!(
                "Password must be at least {} characters long",
                policy.min_length
            ));
        }

        // Check for uppercase
        if policy.require_uppercase && !password.chars().any(|c| c.is_uppercase()) {
            errors.push("Password must contain at least one uppercase letter".to_string());
        }

        // Check for lowercase
        if policy.require_lowercase && !password.chars().any(|c| c.is_lowercase()) {
            errors.push("Password must contain at least one lowercase letter".to_string());
        }

        // Check for numbers
        if policy.require_numbers && !password.chars().any(|c| c.is_ascii_digit()) {
            errors.push("Password must contain at least one number".to_string());
        }

        // Check for special characters
        if policy.require_special_chars {
            let special_chars = "!@#$%^&*()_+-=[]{}|;:,.<>?";
            if !password.chars().any(|c| special_chars.contains(c)) {
                errors.push("Password must contain at least one special character".to_string());
            }
        }

        Ok(errors)
    }

    /// Check if password has been reused recently
    /// Note: This checks if the new password matches any password in history
    /// In practice, you'd verify the new password against each hash in history
    pub async fn check_password_reuse(
        pool: &PgPool,
        user_id: Uuid,
        _new_password_hash: &str, // Will be used when we implement proper verification
        _policy: &PasswordPolicy,
    ) -> Result<bool> {
        // Load recent password history count
        let _history_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM password_histories
            WHERE instance_user_id = $1
            "#,
        )
        .bind(user_id)
        .fetch_one(pool)
        .await?;

        // If history is full, we need to verify against all hashes
        // For now, return false (password not reused) - proper implementation would verify each hash
        // TODO: Implement proper password verification against history hashes
        Ok(false)
    }
}
