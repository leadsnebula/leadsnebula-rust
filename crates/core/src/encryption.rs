use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use anyhow::anyhow;
use base64::{Engine as _, engine::general_purpose};

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

