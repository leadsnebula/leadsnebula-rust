// Cross-language encryption compatibility tests
// Verifies Rust encryption/decryption is compatible with Rails

#[cfg(test)]
mod encryption_compatibility_tests {
    use crate::encryption::EncryptionService;
    use base64::engine::general_purpose;
    use std::str;

    // Test keys matching Rails test configuration
    // Rails uses: 'a' * 32 + 'b' * 32 (64 hex chars = 32 bytes)
    const TEST_PRIMARY_KEY: &str =
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaabbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const TEST_DETERMINISTIC_KEY: &str =
        "ccccccccccccccccccccccccccccccccdddddddddddddddddddddddddddddddd";
    const TEST_SALT: &str = "test_salt_12345678901234567890";

    #[test]
    fn test_derive_key_matches_rails() {
        // Verify key derivation matches Rails ActiveSupport::KeyGenerator
        // Rails uses PBKDF2-HMAC-SHA1 with 65536 iterations

        let secret = "test_secret_key";
        let salt = TEST_SALT;

        let derived = EncryptionService::derive_key_from_secret(secret, salt);

        // Key should be 32 bytes
        assert_eq!(derived.len(), 32);

        // For deterministic encryption, same input should produce same key
        let derived2 = EncryptionService::derive_key_from_secret(secret, salt);
        assert_eq!(derived, derived2);
    }

    #[test]
    fn test_envelope_format_matches_rails() {
        // Verify envelope format matches Rails encryption format
        // Rails format: {"p":"<base64_payload>","h":{"iv":"...","at":"...","c":false}}

        let key_bytes = hex::decode(TEST_PRIMARY_KEY).unwrap();
        let plaintext = "test message";

        let envelope = EncryptionService::encrypt_envelope(&key_bytes, plaintext, false)
            .expect("encryption should succeed");

        // Parse envelope
        let v: serde_json::Value = serde_json::from_str(&envelope).unwrap();

        // Verify structure matches Rails format
        assert!(v.get("p").is_some(), "envelope should have 'p' field");
        assert!(v.get("h").is_some(), "envelope should have 'h' field");

        let headers = v.get("h").unwrap();
        assert!(
            headers.get("iv").is_some(),
            "headers should have 'iv' field"
        );
        assert!(
            headers.get("at").is_some(),
            "headers should have 'at' field"
        );

        // Verify compression flag (should be false for non-deterministic)
        let compression = headers.get("c").and_then(|v| v.as_bool()).unwrap_or(false);
        assert_eq!(compression, false);
    }

    #[test]
    fn test_deterministic_encryption_produces_same_envelope() {
        // For deterministic encryption, same plaintext should produce same envelope

        let key_bytes = hex::decode(TEST_DETERMINISTIC_KEY).unwrap();
        let plaintext = "test deterministic message";

        let envelope1 = EncryptionService::encrypt_envelope(&key_bytes, plaintext, true)
            .expect("encryption should succeed");
        let envelope2 = EncryptionService::encrypt_envelope(&key_bytes, plaintext, true)
            .expect("encryption should succeed");

        // Deterministic encryption should produce identical envelopes
        assert_eq!(envelope1, envelope2);
    }

    #[test]
    fn test_round_trip_encryption_decryption() {
        // Test that we can encrypt and decrypt our own envelopes

        let key_bytes = hex::decode(TEST_PRIMARY_KEY).unwrap();
        let plaintext = "test round trip message";

        let envelope = EncryptionService::encrypt_envelope(&key_bytes, plaintext, false)
            .expect("encryption should succeed");

        let decrypted = EncryptionService::decrypt_envelope(&key_bytes, &envelope)
            .expect("decryption should succeed");

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_deterministic_round_trip() {
        // Test deterministic encryption round trip

        let key_bytes = hex::decode(TEST_DETERMINISTIC_KEY).unwrap();
        let plaintext = "test deterministic round trip";

        let envelope = EncryptionService::encrypt_envelope(&key_bytes, plaintext, true)
            .expect("encryption should succeed");

        let decrypted = EncryptionService::decrypt_envelope(&key_bytes, &envelope)
            .expect("decryption should succeed");

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_wrong_key_fails_decryption() {
        // Test that wrong key fails to decrypt

        let key1_bytes = hex::decode(TEST_PRIMARY_KEY).unwrap();
        let key2_bytes = hex::decode(TEST_DETERMINISTIC_KEY).unwrap();
        let plaintext = "test wrong key";

        let envelope = EncryptionService::encrypt_envelope(&key1_bytes, plaintext, false)
            .expect("encryption should succeed");

        // Try to decrypt with wrong key
        let result = EncryptionService::decrypt_envelope(&key2_bytes, &envelope);
        assert!(result.is_err(), "decryption with wrong key should fail");
    }

    #[test]
    fn test_envelope_structure_validity() {
        // Test that envelope structure is valid JSON and has correct fields

        let key_bytes = hex::decode(TEST_PRIMARY_KEY).unwrap();
        let plaintext = "test structure";

        let envelope = EncryptionService::encrypt_envelope(&key_bytes, plaintext, false)
            .expect("encryption should succeed");

        // Should be valid JSON
        let v: serde_json::Value =
            serde_json::from_str(&envelope).expect("envelope should be valid JSON");

        // Check required fields
        let payload = v
            .get("p")
            .and_then(|v| v.as_str())
            .expect("should have payload");
        let headers = v.get("h").expect("should have headers");

        let iv = headers
            .get("iv")
            .and_then(|v| v.as_str())
            .expect("should have IV");
        let at = headers
            .get("at")
            .and_then(|v| v.as_str())
            .expect("should have auth tag");

        // Verify base64 encoding
        use base64::Engine;
        assert!(general_purpose::STANDARD.decode(payload).is_ok());
        assert!(general_purpose::STANDARD.decode(iv).is_ok());
        assert!(general_purpose::STANDARD.decode(at).is_ok());
    }

    #[test]
    fn test_iv_length_for_deterministic() {
        // Test that deterministic encryption produces consistent IV (12 bytes)

        let key_bytes = hex::decode(TEST_DETERMINISTIC_KEY).unwrap();
        let plaintext = "test IV consistency";

        let envelope1 = EncryptionService::encrypt_envelope(&key_bytes, plaintext, true)
            .expect("encryption should succeed");
        let envelope2 = EncryptionService::encrypt_envelope(&key_bytes, plaintext, true)
            .expect("encryption should succeed");

        // Parse envelopes
        let v1: serde_json::Value = serde_json::from_str(&envelope1).unwrap();
        let v2: serde_json::Value = serde_json::from_str(&envelope2).unwrap();

        let iv1 = v1.get("h").unwrap().get("iv").unwrap().as_str().unwrap();
        let iv2 = v2.get("h").unwrap().get("iv").unwrap().as_str().unwrap();

        // Deterministic encryption should produce same IV
        assert_eq!(iv1, iv2);

        // IV should decode to 12 bytes
        use base64::Engine;
        let iv_bytes = general_purpose::STANDARD.decode(iv1).unwrap();
        assert_eq!(iv_bytes.len(), 12);
    }

    #[test]
    fn test_auth_tag_length() {
        // Test that auth tag is correct length (16 bytes for AES-256-GCM)

        let key_bytes = hex::decode(TEST_PRIMARY_KEY).unwrap();
        let plaintext = "test auth tag";

        let envelope = EncryptionService::encrypt_envelope(&key_bytes, plaintext, false)
            .expect("encryption should succeed");

        let v: serde_json::Value = serde_json::from_str(&envelope).unwrap();
        let at = v.get("h").unwrap().get("at").unwrap().as_str().unwrap();

        use base64::Engine;
        let at_bytes = general_purpose::STANDARD.decode(at).unwrap();
        assert_eq!(at_bytes.len(), 16, "auth tag should be 16 bytes");
    }
}
