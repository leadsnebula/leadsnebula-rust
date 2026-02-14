use crate::email::EmailService;
use crate::models::user::User;
use std::sync::Arc;

pub struct PasswordResetService {
    email_service: Arc<EmailService>,
    reset_url_base: String,
    #[allow(dead_code)]
    reset_token_ttl_seconds: u64,
}

impl PasswordResetService {
    /// * `reset_url_base`: base URL for the reset link (dashboard host, e.g. https://dashboard.leadsnebula.com or https://dev.dashboard.leadsnebula.com). No trailing slash.
    pub fn new(email_service: Arc<EmailService>, reset_url_base: String) -> Self {
        Self {
            email_service,
            reset_url_base,
            reset_token_ttl_seconds: 3600, // 1 hour
        }
    }

    pub async fn send_reset_email(&self, user: &User, reset_token: &str) -> anyhow::Result<()> {
        let reset_url = format!(
            "{}/reset-password?token={}",
            self.reset_url_base.trim_end_matches('/'),
            reset_token
        );

        let subject = "Reset your password";
        let body_text = format!(
            "Click the following link to reset your password: {}",
            reset_url
        );
        let body_html = format!(
            "<p>Click the following link to reset your password:</p><p><a href=\"{}\">{}</a></p>",
            reset_url, reset_url
        );

        self.email_service
            .send_email(&user.email, subject, &body_text, Some(&body_html))
            .await
    }
}
