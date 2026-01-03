#[cfg(feature = "webauthn")]
use webauthn_rs::prelude::*;

#[cfg(feature = "webauthn")]
pub struct WebAuthnService {
    #[allow(dead_code)] // Placeholder implementation
    webauthn: Webauthn,
}

#[cfg(feature = "webauthn")]
impl WebAuthnService {
    pub fn new(rp_id: &str, rp_name: &str, origin: &str) -> anyhow::Result<Self> {
        let url = url::Url::parse(origin)?;
        let webauthn = WebauthnBuilder::new(rp_id, &url)?
            .rp_name(rp_name)
            .build()?;
        Ok(Self { webauthn })
    }
}

