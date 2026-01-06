use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

pub fn verify_hmac(secret: &str, message: &str, signature: &str) -> anyhow::Result<bool> {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())?;
    mac.update(message.as_bytes());
    // Decode hex-encoded signature to bytes
    let signature_bytes = hex::decode(signature)?;
    mac.verify_slice(&signature_bytes)?;
    Ok(true)
}

pub fn generate_hmac(secret: &str, message: &str) -> anyhow::Result<String> {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())?;
    mac.update(message.as_bytes());
    let result = mac.finalize();
    Ok(hex::encode(result.into_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn test_generate_hmac_produces_hex_string() {
        let secret = "test_secret";
        let message = "test_message";
        let signature = generate_hmac(secret, message).unwrap();

        // Should be hex-encoded (64 chars for SHA256)
        assert_eq!(signature.len(), 64);
        assert!(signature.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_verify_hmac_correct_signature() {
        let secret = "test_secret";
        let message = "test_message";
        let signature = generate_hmac(secret, message).unwrap();

        let result = verify_hmac(secret, message, &signature).unwrap();
        assert!(result);
    }

    #[test]
    fn test_verify_hmac_incorrect_signature() {
        let secret = "test_secret";
        let message = "test_message";
        let _signature = generate_hmac(secret, message).unwrap();
        let wrong_signature = "a".repeat(64);

        let result = verify_hmac(secret, message, &wrong_signature);
        assert!(result.is_err());
    }

    #[test]
    fn test_verify_hmac_wrong_secret() {
        let secret1 = "secret1";
        let secret2 = "secret2";
        let message = "test_message";
        let signature = generate_hmac(secret1, message).unwrap();

        let result = verify_hmac(secret2, message, &signature);
        assert!(result.is_err());
    }

    #[test]
    fn test_verify_hmac_wrong_message() {
        let secret = "test_secret";
        let message1 = "message1";
        let message2 = "message2";
        let signature = generate_hmac(secret, message1).unwrap();

        let result = verify_hmac(secret, message2, &signature);
        assert!(result.is_err());
    }

    #[test]
    fn test_hmac_deterministic() {
        let secret = "test_secret";
        let message = "test_message";

        let sig1 = generate_hmac(secret, message).unwrap();
        let sig2 = generate_hmac(secret, message).unwrap();

        // Same secret + message should produce same signature
        assert_eq!(sig1, sig2);
    }

    #[test]
    fn test_verify_hmac_invalid_signature_format() {
        let secret = "test_secret";
        let message = "test_message";

        // Invalid hex string (odd length)
        assert!(verify_hmac(secret, message, "abc").is_err());

        // Invalid hex string (non-hex chars)
        assert!(verify_hmac(secret, message, "g".repeat(64).as_str()).is_err());
    }

    // Property-based test: generate/verify roundtrip
    proptest! {
        #[test]
        fn test_hmac_generate_verify_roundtrip(
            secret in ".*",
            message in ".*"
        ) {
            if !secret.is_empty() {
                let signature = generate_hmac(&secret, &message).map_err(|e| proptest::test_runner::TestCaseError::fail(e.to_string()))?;
                let result = verify_hmac(&secret, &message, &signature).map_err(|e| proptest::test_runner::TestCaseError::fail(e.to_string()))?;
                prop_assert!(result);
            }
        }
    }
}
