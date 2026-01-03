use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// HMAC verifier for request signature verification
pub struct HmacVerifier;

impl HmacVerifier {
    /// Compute HMAC-SHA256 signature for a message
    pub fn compute_signature(message: &[u8], secret: &str) -> String {
        let mut mac =
            HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC can take key of any size");
        mac.update(message);
        let result = mac.finalize();
        hex::encode(result.into_bytes())
    }

    /// Verify HMAC signature with constant-time comparison
    pub fn verify_signature(message: &[u8], secret: &str, provided_signature: &str) -> bool {
        let expected = Self::compute_signature(message, secret);
        Self::constant_time_compare(&expected, provided_signature)
    }

    /// Parse HMAC signature from header value
    /// Supports formats: "sha256=<hex>" or just "<hex>"
    pub fn parse_signature(header_value: &str) -> String {
        header_value
            .trim()
            .strip_prefix("sha256=")
            .unwrap_or(header_value.trim())
            .to_string()
    }

    /// Constant-time string comparison to prevent timing attacks
    fn constant_time_compare(a: &str, b: &str) -> bool {
        if a.len() != b.len() {
            return false;
        }

        let a_bytes = a.as_bytes();
        let b_bytes = b.as_bytes();
        let mut result = 0u8;

        for (x, y) in a_bytes.iter().zip(b_bytes.iter()) {
            result |= x ^ y;
        }

        result == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hmac_computation() {
        let secret = "test-secret";
        let message = b"test message";
        let signature = HmacVerifier::compute_signature(message, secret);

        assert_eq!(signature.len(), 64); // SHA-256 produces 64 hex chars
    }

    #[test]
    fn test_hmac_verification() {
        let secret = "test-secret";
        let message = b"test message";
        let signature = HmacVerifier::compute_signature(message, secret);

        assert!(HmacVerifier::verify_signature(message, secret, &signature));
        assert!(!HmacVerifier::verify_signature(message, secret, "invalid"));
    }

    #[test]
    fn test_parse_signature() {
        assert_eq!(HmacVerifier::parse_signature("sha256=abc123"), "abc123");
        assert_eq!(HmacVerifier::parse_signature("abc123"), "abc123");
    }
}
