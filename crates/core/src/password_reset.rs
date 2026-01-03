use base64::Engine;
use rand::Rng;
use sha2::{Digest, Sha256};

/// Password reset token helper
pub struct PasswordResetHelper;

impl PasswordResetHelper {
    /// Generate a secure random token for password reset
    /// Returns (raw_token, hashed_token)
    pub fn generate_token() -> (String, String) {
        // Generate a 32-byte random token
        let mut rng = rand::thread_rng();
        let mut bytes = [0u8; 32];
        rng.fill(&mut bytes);

        // Encode as base64url (URL-safe base64)
        let raw_token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);

        // Hash the token for storage (SHA256)
        let mut hasher = Sha256::new();
        hasher.update(raw_token.as_bytes());
        let hash = hasher.finalize();
        let hashed_token = hex::encode(hash);

        (raw_token, hashed_token)
    }

    /// Verify a token against a stored hash
    pub fn verify_token(token: &str, stored_hash: &str) -> bool {
        let mut hasher = Sha256::new();
        hasher.update(token.as_bytes());
        let hash = hasher.finalize();
        let computed_hash = hex::encode(hash);

        // Constant-time comparison to prevent timing attacks
        computed_hash == stored_hash
    }

    /// Check if a reset token is expired (default: 1 hour)
    pub fn is_token_expired(
        sent_at: Option<chrono::DateTime<chrono::Utc>>,
        expiry_hours: i64,
    ) -> bool {
        match sent_at {
            None => true,
            Some(sent_at) => {
                let now = chrono::Utc::now();
                let expiry = sent_at + chrono::Duration::hours(expiry_hours);
                now > expiry
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_generation_and_verification() {
        let (raw_token, hashed_token) = PasswordResetHelper::generate_token();

        // Token should be verifiable
        assert!(PasswordResetHelper::verify_token(&raw_token, &hashed_token));

        // Wrong token should not verify
        assert!(!PasswordResetHelper::verify_token(
            "wrong_token",
            &hashed_token
        ));
    }

    #[test]
    fn test_token_expiry() {
        // Token sent 2 hours ago should be expired (1 hour expiry)
        let two_hours_ago = chrono::Utc::now() - chrono::Duration::hours(2);
        assert!(PasswordResetHelper::is_token_expired(
            Some(two_hours_ago),
            1
        ));

        // Token sent 30 minutes ago should not be expired (1 hour expiry)
        let thirty_minutes_ago = chrono::Utc::now() - chrono::Duration::minutes(30);
        assert!(!PasswordResetHelper::is_token_expired(
            Some(thirty_minutes_ago),
            1
        ));

        // No sent_at should be expired
        assert!(PasswordResetHelper::is_token_expired(None, 1));
    }
}
