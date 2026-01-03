use anyhow::{Context, Result};
use std::time::{SystemTime, UNIX_EPOCH};
use totp_rs::{Algorithm, Secret, TOTP};

/// OTP/TOTP helper for two-factor authentication
pub struct OtpHelper;

impl OtpHelper {
    /// Generate a new TOTP secret (base32 encoded)
    pub fn generate_secret() -> String {
        // Use to_encoded() to get the base32 string representation
        // This ensures we get a valid base32 string that can be decoded later
        Secret::generate_secret().to_encoded().to_string()
    }

    /// Create a TOTP instance for a secret (base32 encoded string)
    pub fn create_totp(secret: &str, issuer: &str, account_name: &str) -> Result<TOTP> {
        // In totp-rs 5.7, TOTP::new expects Vec<u8> directly
        // Decode the base32 string to bytes
        let decoded = base32::decode(base32::Alphabet::RFC4648 { padding: false }, secret)
            .ok_or_else(|| anyhow::anyhow!("Invalid base32 secret"))?;

        // TOTP::new signature: new(algorithm, digits, skew, secret_bytes, issuer, account_name)
        let totp = TOTP::new(
            Algorithm::SHA1,
            6,  // 6-digit codes
            30, // 30-second window
            1,  // skew (1 step tolerance)
            decoded,
            Some(issuer.to_string()),
            account_name.to_string(),
        )?;

        Ok(totp)
    }

    /// Generate a TOTP code for the current time
    pub fn generate_code(secret: &str, issuer: &str, account_name: &str) -> Result<String> {
        let totp = Self::create_totp(secret, issuer, account_name)?;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("Time went backwards")?
            .as_secs();

        Ok(totp.generate(timestamp))
    }

    /// Verify a TOTP code with drift tolerance
    pub fn verify_code(
        secret: &str,
        code: &str,
        issuer: &str,
        account_name: &str,
        drift_behind: u64,
        drift_ahead: u64,
    ) -> Result<bool> {
        let totp = Self::create_totp(secret, issuer, account_name)?;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("Time went backwards")?
            .as_secs();

        // Check current time window
        if totp.check(code, timestamp) {
            return Ok(true);
        }

        // Check previous windows (drift behind) - each window is 30 seconds
        for i in 1..=drift_behind {
            let check_time = timestamp.saturating_sub(i * 30);
            if totp.check(code, check_time) {
                return Ok(true);
            }
        }

        // Check future windows (drift ahead)
        for i in 1..=drift_ahead {
            if totp.check(code, timestamp + i * 30) {
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Generate backup codes (10 codes, 8 characters each)
    pub fn generate_backup_codes() -> Vec<String> {
        use rand::Rng;
        let mut rng = rand::thread_rng();

        (0..10)
            .map(|_| {
                let bytes: [u8; 4] = rng.gen();
                hex::encode(bytes).to_uppercase()
            })
            .collect()
    }

    /// Generate provisioning URI for QR code
    pub fn generate_provisioning_uri(
        secret: &str,
        issuer: &str,
        account_name: &str,
    ) -> Result<String> {
        let totp = Self::create_totp(secret, issuer, account_name)?;
        Ok(totp.get_url())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_totp_generation_and_verification() {
        let secret = OtpHelper::generate_secret();
        let issuer = "LeadsNebula";
        let account_name = "test@example.com";

        // Generate code
        let code = OtpHelper::generate_code(&secret, issuer, account_name).unwrap();

        // Verify code
        let verified = OtpHelper::verify_code(&secret, &code, issuer, account_name, 1, 1).unwrap();
        assert!(verified);
    }

    #[test]
    fn test_backup_codes() {
        let codes = OtpHelper::generate_backup_codes();
        assert_eq!(codes.len(), 10);
        for code in &codes {
            assert_eq!(code.len(), 8);
        }
    }
}
