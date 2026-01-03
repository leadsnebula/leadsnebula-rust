use crate::ssm::SsmClient;
use anyhow::Result;
use aws_config::BehaviorVersion;
use aws_sdk_sesv2::types::{Body, Content, Destination, EmailContent, Message};
use aws_sdk_sesv2::Client as SesClient;
use tracing::{info, warn};

/// Utility function to store SES credentials in SSM Parameter Store
pub async fn store_ses_credentials_in_ssm(
    ssm_client: &SsmClient,
    environment: &str,
    access_key_id: &str,
    secret_access_key: &str,
    region: Option<&str>,
) -> Result<()> {
    let ses_path = format!("/leadsnebula/{}/shared/aws/ses", environment);
    let region = region.unwrap_or("us-east-1");

    ssm_client
        .put_parameter(
            &format!("{}/access_key_id", ses_path),
            access_key_id,
            Some("SES access key ID for email sending"),
        )
        .await?;

    ssm_client
        .put_parameter(
            &format!("{}/secret_access_key", ses_path),
            secret_access_key,
            Some("SES secret access key for email sending"),
        )
        .await?;

    ssm_client
        .put_parameter(
            &format!("{}/region", ses_path),
            region,
            Some("AWS region for SES"),
        )
        .await?;

    info!("Successfully stored SES credentials in SSM at {}", ses_path);
    Ok(())
}

/// Email service for sending emails via AWS SES
pub struct EmailService {
    client: SesClient,
    from_email: String,
    frontend_url: String,
}

impl EmailService {
    /// Create a new email service
    /// Fetches credentials from SSM Parameter Store or environment variables
    pub async fn new() -> Result<Self> {
        // Determine environment for SSM path
        let environment = if let Ok(env) = std::env::var("SES_SSM_ENV") {
            env
        } else {
            let current_env = std::env::var("ENVIRONMENT")
                .or_else(|_| std::env::var("ENV"))
                .unwrap_or_else(|_| "development".to_string());
            match current_env.as_str() {
                "production" => "prod".to_string(),
                "staging" => "staging".to_string(),
                _ => "prod".to_string(), // Development defaults to 'prod' in Ruby
            }
        };

        // Try to fetch credentials from SSM
        let (access_key_id, secret_access_key, region) = match SsmClient::new().await {
            Ok(ssm_client) => {
                let ses_path = format!("/leadsnebula/{}/shared/aws/ses", environment);
                let ak_path = format!("{}/access_key_id", ses_path);
                let sk_path = format!("{}/secret_access_key", ses_path);
                let reg_path = format!("{}/region", ses_path);

                let ak = ssm_client.get_parameter(&ak_path).await.ok().flatten();
                let sk = ssm_client.get_parameter(&sk_path).await.ok().flatten();
                let reg = ssm_client.get_parameter(&reg_path).await.ok().flatten();

                (ak, sk, reg)
            }
            Err(_) => (None, None, None),
        };

        // Determine if we're in production/staging (no env var fallback) or development
        let is_production = matches!(environment.as_str(), "prod" | "production" | "staging");

        // Fall back to environment variables only in development
        let access_key_id = if access_key_id.is_some() {
            access_key_id
        } else if !is_production {
            std::env::var("SES_ACCESS_KEY_ID")
                .ok()
                .or_else(|| std::env::var("AWS_ACCESS_KEY_ID").ok())
        } else {
            None
        };

        let secret_access_key = if secret_access_key.is_some() {
            secret_access_key
        } else if !is_production {
            std::env::var("SES_SECRET_ACCESS_KEY")
                .ok()
                .or_else(|| std::env::var("AWS_SECRET_ACCESS_KEY").ok())
        } else {
            None
        };

        let region = region
            .or_else(|| {
                if !is_production {
                    std::env::var("SES_REGION")
                        .ok()
                        .or_else(|| std::env::var("AWS_REGION").ok())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "us-east-1".to_string());

        // Validate we have credentials
        if access_key_id.is_none() || secret_access_key.is_none() {
            if is_production {
                return Err(anyhow::anyhow!("SES credentials not found in SSM Parameter Store at /leadsnebula/{}/shared/aws/ses/. Production environments must use SSM Parameter Store.", environment));
            } else {
                return Err(anyhow::anyhow!("SES credentials not found. Please set SES_ACCESS_KEY_ID and SES_SECRET_ACCESS_KEY environment variables or store them in SSM Parameter Store at /leadsnebula/{}/shared/aws/ses/", environment));
            }
        }

        let ak = access_key_id.unwrap();
        let sk = secret_access_key.unwrap();

        // Set environment variables so AWS SDK can pick them up
        std::env::set_var("AWS_ACCESS_KEY_ID", &ak);
        std::env::set_var("AWS_SECRET_ACCESS_KEY", &sk);
        std::env::set_var("AWS_REGION", &region);

        // Load AWS config - it will pick up the environment variables we just set
        let config = aws_config::load_defaults(BehaviorVersion::latest()).await;
        let client = SesClient::new(&config);

        let from_email = std::env::var("MAILER_FROM_EMAIL")
            .unwrap_or_else(|_| "noreply@leadsnebula.com".to_string());

        // Determine frontend URL based on environment (matching config.rs pattern)
        let current_env = std::env::var("ENVIRONMENT")
            .or_else(|_| std::env::var("ENV"))
            .unwrap_or_else(|_| "development".to_string());

        let frontend_url = std::env::var("FRONTEND_URL").unwrap_or_else(|_| {
            if current_env == "development" {
                "http://localhost:3000".to_string()
            } else {
                "https://dashboard.leadsnebula.com".to_string()
            }
        });

        Ok(Self {
            client,
            from_email,
            frontend_url,
        })
    }

    /// Send a password reset email
    pub async fn send_password_reset_email(
        &self,
        to_email: &str,
        to_name: Option<&str>,
        reset_token: &str,
    ) -> Result<()> {
        let reset_url = format!(
            "{}/reset-password?reset_password_token={}",
            self.frontend_url,
            urlencoding::encode(reset_token)
        );

        let display_name = to_name.map(|n| format!("{} ", n)).unwrap_or_default();

        let subject = "Change Your Password - LeadsNebula";
        let html_body = format!(
            r#"
            <!DOCTYPE html>
            <html>
            <head>
                <meta charset="utf-8">
                <meta name="viewport" content="width=device-width, initial-scale=1.0">
                <title>Change Your Password</title>
            </head>
            <body style="font-family: Arial, sans-serif; line-height: 1.6; color: #333; max-width: 600px; margin: 0 auto; padding: 20px;">
                <h1 style="color: #2c3e50;">Change Your Password</h1>
                <p>Hello{},</p>
                <p>You have requested to change your password. Click the link below to reset your password:</p>
                <p style="margin: 30px 0;">
                    <a href="{}" style="background-color: #3498db; color: white; padding: 12px 24px; text-decoration: none; border-radius: 5px; display: inline-block;">Reset Password</a>
                </p>
                <p>Or copy and paste this URL into your browser:</p>
                <p style="word-break: break-all; color: #7f8c8d;">{}</p>
                <p>This link will expire in 1 hour.</p>
                <p>If you did not request this password change, please ignore this email.</p>
                <hr style="border: none; border-top: 1px solid #eee; margin: 30px 0;">
                <p style="color: #7f8c8d; font-size: 12px;">This is an automated message from LeadsNebula. Please do not reply to this email.</p>
            </body>
            </html>
            "#,
            display_name, reset_url, reset_url
        );

        let text_body = format!(
            r#"
Change Your Password - LeadsNebula

Hello{},

You have requested to change your password. Click the link below to reset your password:

{}

This link will expire in 1 hour.

If you did not request this password change, please ignore this email.

---
This is an automated message from LeadsNebula. Please do not reply to this email.
            "#,
            display_name, reset_url
        );

        self.send_email(to_email, subject, &html_body, &text_body)
            .await
    }

    /// Send a generic email
    async fn send_email(
        &self,
        to_email: &str,
        subject: &str,
        html_body: &str,
        text_body: &str,
    ) -> Result<()> {
        let destination = Destination::builder().to_addresses(to_email).build();

        let message = Message::builder()
            .subject(Content::builder().data(subject).charset("UTF-8").build()?)
            .body(
                Body::builder()
                    .html(
                        Content::builder()
                            .data(html_body)
                            .charset("UTF-8")
                            .build()?,
                    )
                    .text(
                        Content::builder()
                            .data(text_body)
                            .charset("UTF-8")
                            .build()?,
                    )
                    .build(),
            )
            .build();

        let content = EmailContent::builder().simple(message).build();

        let result = self
            .client
            .send_email()
            .from_email_address(&self.from_email)
            .destination(destination)
            .content(content)
            .send()
            .await;

        match result {
            Ok(_) => {
                info!("Password reset email sent successfully to: {}", to_email);
                Ok(())
            }
            Err(e) => {
                warn!("Failed to send password reset email to {}: {}", to_email, e);
                Err(anyhow::anyhow!("Failed to send email: {}", e))
            }
        }
    }
}
