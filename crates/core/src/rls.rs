use anyhow::Result;
use sqlx::PgPool;
use tracing::debug;
use uuid::Uuid;

/// RLS Context Setter - Sets PostgreSQL session variables for Row Level Security
/// Similar to Ruby's RlsContextSetter concern
pub struct RlsContext;

impl RlsContext {
    /// Set RLS context for a publisher (API key authentication)
    pub async fn set_publisher_context(
        pool: &PgPool,
        publisher_id: Uuid,
        instance_id: Uuid,
        is_documentation_test: bool,
    ) -> Result<()> {
        // Set publisher ID
        sqlx::query("SET app.current_publisher_id = $1")
            .bind(publisher_id)
            .execute(pool)
            .await?;

        // Set instance ID
        sqlx::query("SET app.current_instance_id = $1")
            .bind(instance_id)
            .execute(pool)
            .await?;

        // Set user role
        let user_role = if is_documentation_test {
            "documentation_test"
        } else {
            "publisher"
        };
        sqlx::query("SET app.user_role = $1")
            .bind(user_role)
            .execute(pool)
            .await?;

        debug!(
            "RLS context set: publisher_id={}, instance_id={}, role={}",
            publisher_id, instance_id, user_role
        );

        Ok(())
    }

    /// Set RLS context for an authenticated user (JWT authentication)
    pub async fn set_user_context(
        pool: &PgPool,
        user_id: Uuid,
        instance_id: Option<Uuid>,
        is_admin: bool,
        publisher_id: Option<Uuid>,
        is_documentation_test: bool,
    ) -> Result<()> {
        // Set instance ID if available
        if let Some(inst_id) = instance_id {
            sqlx::query("SET app.current_instance_id = $1")
                .bind(inst_id)
                .execute(pool)
                .await?;
        }

        // Set user role and publisher context
        if is_admin {
            sqlx::query("SET app.user_role = 'admin'")
                .execute(pool)
                .await?;
        } else if let Some(pub_id) = publisher_id {
            // User is a publisher
            sqlx::query("SET app.current_publisher_id = $1")
                .bind(pub_id)
                .execute(pool)
                .await?;

            let user_role = if is_documentation_test {
                "documentation_test"
            } else {
                "publisher"
            };
            sqlx::query("SET app.user_role = $1")
                .bind(user_role)
                .execute(pool)
                .await?;
        } else {
            // Default to system role
            sqlx::query("SET app.user_role = 'system'")
                .execute(pool)
                .await?;
        }

        debug!(
            "RLS context set: user_id={}, instance_id={:?}, role={}",
            user_id,
            instance_id,
            if is_admin {
                "admin"
            } else if publisher_id.is_some() {
                if is_documentation_test {
                    "documentation_test"
                } else {
                    "publisher"
                }
            } else {
                "system"
            }
        );

        Ok(())
    }

    /// Set RLS context for system/background jobs
    pub async fn set_system_context(pool: &PgPool) -> Result<()> {
        sqlx::query("SET app.user_role = 'system'")
            .execute(pool)
            .await?;

        debug!("RLS context set: system role");
        Ok(())
    }

    /// Clear all RLS context variables
    pub async fn clear_context(pool: &PgPool) -> Result<()> {
        sqlx::query("RESET app.current_publisher_id")
            .execute(pool)
            .await?;
        sqlx::query("RESET app.current_instance_id")
            .execute(pool)
            .await?;
        sqlx::query("RESET app.user_role").execute(pool).await?;

        debug!("RLS context cleared");
        Ok(())
    }
}
