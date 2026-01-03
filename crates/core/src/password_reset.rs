use crate::email::EmailService;
use crate::models::user::User;

pub struct PasswordResetService {
    email_service: EmailService,
    #[allow(dead_code)]
    reset_token_ttl_seconds: u64,
}

impl PasswordResetService {
    pub fn new(email_service: EmailService) -> Self {
        Self {
            email_service,
            reset_token_ttl_seconds: 3600, // 1 hour
        }
    }

    pub async fn send_reset_email(
        &self,
        user: &User,
        reset_token: &str,
    ) -> anyhow::Result<()> {
        let reset_url = format!("https://app.leadsnebula.com/reset-password?token={}", reset_token);
        
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

