#[cfg(feature = "otp")]
use totp_rs::{Secret, TOTP};

#[cfg(feature = "otp")]
pub struct OtpService {
    totp: TOTP,
}

#[cfg(feature = "otp")]
impl OtpService {
    pub fn new(secret: &str) -> anyhow::Result<Self> {
        let secret = Secret::Encoded(secret.to_string());
        let totp = TOTP::new(totp_rs::Algorithm::SHA1, 6, 1, 30, secret.to_bytes()?)?;
        Ok(Self { totp })
    }

    pub fn generate(&self) -> anyhow::Result<String> {
        Ok(self.totp.generate_current()?)
    }

    pub fn verify(&self, code: &str) -> bool {
        self.totp.check_current(code).unwrap_or(false)
    }
}
