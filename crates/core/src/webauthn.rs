use anyhow::Result;
use url::Url;
use uuid::Uuid;
use webauthn_rs::prelude::*;

/// WebAuthn service for passkey authentication
/// Note: This is a simplified wrapper - full implementation will be added when WebAuthn endpoints are created
pub struct WebauthnService {
    rp: Webauthn,
}

impl WebauthnService {
    /// Create a new WebAuthn service
    pub fn new(rp_id: &str, rp_name: &str, origin: &str) -> Result<Self> {
        let rp = WebauthnBuilder::new(rp_id, &Url::parse(origin)?)?
            .rp_name(rp_name)
            .build()?;

        Ok(Self { rp })
    }

    /// Start passkey registration
    pub fn start_passkey_registration(
        &self,
        username: &str,
        user_id: Uuid,
        user_display_name: Option<&str>,
    ) -> Result<(CreationChallengeResponse, PasskeyRegistration)> {
        self.rp
            .start_passkey_registration(
                user_id,
                username,
                user_display_name.unwrap_or(username),
                None,
            )
            .map_err(|e| anyhow::anyhow!("Failed to start registration: {}", e))
    }

    /// Get the WebAuthn instance (for direct API access)
    pub fn get_webauthn(&self) -> &Webauthn {
        &self.rp
    }
}
