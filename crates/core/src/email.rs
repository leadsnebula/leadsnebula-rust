//! AWS SES v2 email sending. Uses verified From address and standard SendEmail API.
//! Best practices: single region, UTF-8 charset, text+html body, clear error mapping.

use aws_sdk_sesv2::types::{Body, Content, Destination, EmailContent, Message};
use aws_sdk_sesv2::Client as SesClient;
use tracing::{error, info};

pub struct EmailService {
    client: SesClient,
    from_email: String,
}

impl EmailService {
    /// Create EmailService. Uses default AWS config (env/instance profile); From address must be SES-verified.
    pub async fn new(from_email: String) -> anyhow::Result<Self> {
        let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .load()
            .await;
        let client = SesClient::new(&config);
        Ok(Self { client, from_email })
    }

    /// Send a single email via SES SendEmail. Returns Ok(()) on success or Err with context.
    /// Best practice: always provide both text and HTML when possible; use UTF-8 charset.
    pub async fn send_email(
        &self,
        to: &str,
        subject: &str,
        body_text: &str,
        body_html: Option<&str>,
    ) -> anyhow::Result<()> {
        let subject_content = Content::builder().data(subject).charset("UTF-8").build()?;

        let text_content = Content::builder()
            .data(body_text)
            .charset("UTF-8")
            .build()?;

        let body_builder = Body::builder().text(text_content);
        let body = if let Some(html) = body_html {
            let html_content = Content::builder().data(html).charset("UTF-8").build()?;
            body_builder.html(html_content).build()
        } else {
            body_builder.build()
        };

        let message = Message::builder()
            .subject(subject_content)
            .body(body)
            .build();

        let content = EmailContent::builder().simple(message).build();

        let destination = Destination::builder().to_addresses(to).build();

        self.client
            .send_email()
            .from_email_address(&self.from_email)
            .destination(destination)
            .content(content)
            .send()
            .await
            .map_err(|e| {
                let msg = e.to_string();
                error!("SES send_email failed: {}", msg);
                anyhow::anyhow!("Email send failed: {}", msg)
            })?;

        info!(to = %to, "SES email sent successfully");
        Ok(())
    }
}
