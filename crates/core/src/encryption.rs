use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use anyhow::anyhow;
use base64::{engine::general_purpose, Engine as _};

pub struct EncryptionService {
    cipher: Aes256Gcm,
}

impl EncryptionService {
    pub fn new(key: &[u8]) -> anyhow::Result<Self> {
        if key.len() != 32 {
            return Err(anyhow!("Encryption key must be 32 bytes"));
        }
        let key = Key::<Aes256Gcm>::from_slice(key);
        let cipher = Aes256Gcm::new(key);
        Ok(Self { cipher })
    }

    pub fn encrypt(&self, plaintext: &str) -> anyhow::Result<String> {
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let ciphertext = self
            .cipher
            .encrypt(&nonce, plaintext.as_bytes())
            .map_err(|e| anyhow::anyhow!("Encryption failed: {:?}", e))?;

        // Combine nonce and ciphertext: nonce (12 bytes) + ciphertext
        let mut combined = nonce.to_vec();
        combined.extend_from_slice(&ciphertext);

        Ok(general_purpose::STANDARD.encode(combined))
    }

    pub fn decrypt(&self, ciphertext: &str) -> anyhow::Result<String> {
        let combined = general_purpose::STANDARD.decode(ciphertext)?;

        if combined.len() < 12 {
            return Err(anyhow!("Invalid ciphertext length"));
        }

        let nonce = Nonce::from_slice(&combined[..12]);
        let ciphertext_bytes = &combined[12..];

        let plaintext = self
            .cipher
            .decrypt(nonce, ciphertext_bytes)
            .map_err(|e| anyhow::anyhow!("Decryption failed: {:?}", e))?;
        Ok(String::from_utf8(plaintext)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn test_key() -> Vec<u8> {
        vec![0u8; 32]
    }

    #[test]
    fn test_encryption_service_new_valid_key() {
        let key = test_key();
        let service = EncryptionService::new(&key);
        assert!(service.is_ok());
    }

    #[test]
    fn test_encryption_service_new_invalid_key_length() {
        let short_key = vec![0u8; 31];
        let service = EncryptionService::new(&short_key);
        assert!(service.is_err());
        if let Err(e) = service {
            assert!(e.to_string().contains("32 bytes"));
        }

        let long_key = vec![0u8; 33];
        let service = EncryptionService::new(&long_key);
        assert!(service.is_err());
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let key = test_key();
        let service = EncryptionService::new(&key).unwrap();
        let plaintext = "test_api_key_pk_live_1234567890abcdef";

        let encrypted = service.encrypt(plaintext).unwrap();
        assert_ne!(encrypted, plaintext);
        assert!(!encrypted.is_empty());

        let decrypted = service.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_encrypt_produces_different_outputs() {
        let key = test_key();
        let service = EncryptionService::new(&key).unwrap();
        let plaintext = "same_plaintext";

        let encrypted1 = service.encrypt(plaintext).unwrap();
        let encrypted2 = service.encrypt(plaintext).unwrap();

        // Different nonces should produce different ciphertexts
        assert_ne!(encrypted1, encrypted2);

        // But both should decrypt to the same plaintext
        assert_eq!(service.decrypt(&encrypted1).unwrap(), plaintext);
        assert_eq!(service.decrypt(&encrypted2).unwrap(), plaintext);
    }

    #[test]
    fn test_decrypt_invalid_ciphertext() {
        let key = test_key();
        let service = EncryptionService::new(&key).unwrap();

        // Too short
        assert!(service.decrypt("short").is_err());

        // Invalid base64
        assert!(service.decrypt("not_base64!!!").is_err());

        // Valid base64 but wrong format
        let invalid = general_purpose::STANDARD.encode("too_short");
        assert!(service.decrypt(&invalid).is_err());
    }

    #[test]
    fn test_decrypt_wrong_key() {
        let key1 = test_key();
        let key2 = {
            let mut k = test_key();
            k[0] = 1;
            k
        };

        let service1 = EncryptionService::new(&key1).unwrap();
        let service2 = EncryptionService::new(&key2).unwrap();

        let plaintext = "test_data";
        let encrypted = service1.encrypt(plaintext).unwrap();

        // Decrypting with wrong key should fail
        assert!(service2.decrypt(&encrypted).is_err());
    }

    // Property-based test: encrypt/decrypt roundtrip for any string
    proptest! {
        #[test]
        fn test_encrypt_decrypt_any_string(s in ".*") {
            let key = test_key();
            let service = EncryptionService::new(&key).unwrap();

            let encrypted = service.encrypt(&s).map_err(|e| proptest::test_runner::TestCaseError::fail(e.to_string()))?;
            let decrypted = service.decrypt(&encrypted).map_err(|e| proptest::test_runner::TestCaseError::fail(e.to_string()))?;

            prop_assert_eq!(decrypted, s);
        }

        #[test]
        fn test_encrypt_decrypt_api_key_format(s in r"pk_(test|live)_[a-zA-Z0-9]{64}") {
            let key = test_key();
            let service = EncryptionService::new(&key).unwrap();

            let encrypted = service.encrypt(&s).map_err(|e| proptest::test_runner::TestCaseError::fail(e.to_string()))?;
            let decrypted = service.decrypt(&encrypted).map_err(|e| proptest::test_runner::TestCaseError::fail(e.to_string()))?;

            prop_assert_eq!(decrypted, s);
        }
    }
}
