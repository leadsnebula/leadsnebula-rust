use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use anyhow::anyhow;
use base64::{engine::general_purpose, Engine as _};
use hmac::{Hmac, Mac};
use pbkdf2::pbkdf2;
use rand::RngCore;
use serde_json::json;
use sha1::Sha1;
use sha2::Sha256;

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
        // Legacy simple envelope: base64(nonce + ciphertext_with_tag)
        let mut nonce = [0u8; 12];
        OsRng.fill_bytes(&mut nonce);
        let nonce = Nonce::from_slice(&nonce);

        let ciphertext = self
            .cipher
            .encrypt(nonce, plaintext.as_bytes())
            .map_err(|e| anyhow::anyhow!("Encryption failed: {:?}", e))?;

        // Compose combined blob: nonce + ciphertext_with_tag
        let mut combined = nonce.as_slice().to_vec();
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

    // Derive a 32-byte AES key using PBKDF2-HMAC-SHA1 to match Rails ActiveSupport::KeyGenerator
    // iterations: 65536 (as used in Ruby code)
    pub fn derive_key_from_secret(secret: &str, salt: &str) -> [u8; 32] {
        let mut out = [0u8; 32];
        // Use pbkdf2 with HMAC-SHA1 to match Rails' ActiveSupport::KeyGenerator default
        pbkdf2::<Hmac<Sha1>>(secret.as_bytes(), salt.as_bytes(), 65536, &mut out);
        out
    }

    // Encrypt into a Rails-like JSON envelope: {"p":"<base64_payload>","h":{"iv":"...","at":"...","c":false}}
    // For deterministic: derive IV from HMAC-SHA256(payload, key) to produce deterministic nonce (12 bytes)
    pub fn encrypt_envelope(
        key_bytes: &[u8],
        plaintext: &str,
        deterministic: bool,
    ) -> anyhow::Result<String> {
        if key_bytes.len() != 32 {
            return Err(anyhow!("Encryption key must be 32 bytes"));
        }

        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key_bytes));

        // Determine IV
        let iv: [u8; 12] = if deterministic {
            // HMAC-SHA256 of plaintext with key_bytes, take first 12 bytes
            type HmacSha256 = Hmac<Sha256>;
            let mut mac = <HmacSha256 as hmac::Mac>::new_from_slice(key_bytes)
                .map_err(|e| anyhow!(format!("HMAC init error: {:?}", e)))?;
            mac.update(plaintext.as_bytes());
            let result = mac.finalize().into_bytes();
            let mut iv = [0u8; 12];
            iv.copy_from_slice(&result[..12]);
            iv
        } else {
            let mut iv = [0u8; 12];
            OsRng.fill_bytes(&mut iv);
            iv
        };

        let nonce = Nonce::from_slice(&iv);
        let ciphertext_with_tag = cipher
            .encrypt(nonce, plaintext.as_bytes())
            .map_err(|e| anyhow!(format!("encrypt failed: {:?}", e)))?;

        // AES-GCM appends auth tag at the end; split it off
        if ciphertext_with_tag.len() < 16 {
            return Err(anyhow!("ciphertext too short"));
        }
        let tag_pos = ciphertext_with_tag.len() - 16;
        let payload = &ciphertext_with_tag[..tag_pos];
        let tag = &ciphertext_with_tag[tag_pos..];

        let envelope = json!({
            "p": general_purpose::STANDARD.encode(payload),
            "h": {
                "iv": general_purpose::STANDARD.encode(iv),
                "at": general_purpose::STANDARD.encode(tag),
                "c": false
            }
        });

        Ok(envelope.to_string())
    }

    pub fn decrypt_envelope(key_bytes: &[u8], envelope: &str) -> anyhow::Result<String> {
        let v: serde_json::Value = serde_json::from_str(envelope)?;
        let payload_b64 = v
            .get("p")
            .and_then(|x| x.as_str())
            .ok_or_else(|| anyhow!("missing payload"))?;
        let headers = v.get("h").ok_or_else(|| anyhow!("missing headers"))?;
        let iv_b64 = headers
            .get("iv")
            .and_then(|x| x.as_str())
            .ok_or_else(|| anyhow!("missing iv"))?;
        let at_b64 = headers
            .get("at")
            .and_then(|x| x.as_str())
            .ok_or_else(|| anyhow!("missing at"))?;

        let payload = general_purpose::STANDARD.decode(payload_b64)?;
        let iv = general_purpose::STANDARD.decode(iv_b64)?;
        let tag = general_purpose::STANDARD.decode(at_b64)?;

        if iv.len() != 12 || tag.len() != 16 {
            return Err(anyhow!("invalid iv/tag lengths"));
        }

        // Reconstruct ciphertext_with_tag
        let mut ciphertext_with_tag = payload.clone();
        ciphertext_with_tag.extend_from_slice(&tag);

        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key_bytes));
        let nonce = Nonce::from_slice(&iv);
        let plaintext = cipher
            .decrypt(nonce, &ciphertext_with_tag[..])
            .map_err(|e| anyhow!(format!("decrypt failed: {:?}", e)))?;
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
