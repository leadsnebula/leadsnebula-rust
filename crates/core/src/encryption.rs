use anyhow::Result;
use ring::aead::{
    Aad, BoundKey, Nonce, NonceSequence, OpeningKey, SealingKey, UnboundKey, AES_256_GCM, NONCE_LEN,
};
use ring::rand::{SecureRandom, SystemRandom};
use std::sync::Arc;
use thiserror::Error;

/// Encryption service using ring (AES-256-GCM)
pub struct EncryptionService {
    key_bytes: Arc<[u8; 32]>, // Store key bytes, create UnboundKey on demand
    rng: SystemRandom,
}

/// Wrapper for encrypted data
#[derive(Debug, Clone)]
pub struct Encrypted {
    ciphertext: Vec<u8>,
    nonce: Vec<u8>,
}

/// Wrapper for decrypted data
#[derive(Debug, Clone)]
pub struct Decrypted {
    plaintext: Vec<u8>,
}

#[derive(Error, Debug)]
pub enum EncryptionError {
    #[error("Encryption failed: {0}")]
    EncryptionFailed(String),
    #[error("Decryption failed: {0}")]
    DecryptionFailed(String),
    #[error("Invalid key length: expected 32 bytes, got {0}")]
    InvalidKeyLength(usize),
}

// Simple nonce sequence for single-use nonces
struct SingleNonce {
    nonce_bytes: [u8; NONCE_LEN],
    used: bool,
}

impl NonceSequence for SingleNonce {
    fn advance(&mut self) -> Result<Nonce, ring::error::Unspecified> {
        if self.used {
            Err(ring::error::Unspecified)
        } else {
            self.used = true;
            Ok(Nonce::assume_unique_for_key(self.nonce_bytes))
        }
    }
}

impl EncryptionService {
    /// Create a new encryption service with a 256-bit key
    pub fn new(key: &[u8]) -> Result<Self, EncryptionError> {
        if key.len() != 32 {
            return Err(EncryptionError::InvalidKeyLength(key.len()));
        }

        let mut key_array = [0u8; 32];
        key_array.copy_from_slice(key);

        Ok(Self {
            key_bytes: Arc::new(key_array),
            rng: SystemRandom::new(),
        })
    }

    /// Create UnboundKey from stored key bytes
    fn create_key(&self) -> Result<UnboundKey, EncryptionError> {
        UnboundKey::new(&AES_256_GCM, &*self.key_bytes)
            .map_err(|e| EncryptionError::EncryptionFailed(format!("Key creation failed: {:?}", e)))
    }

    /// Encrypt plaintext (non-deterministic)
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<Encrypted, EncryptionError> {
        // Generate random nonce (12 bytes for GCM)
        let mut nonce_bytes = [0u8; NONCE_LEN];
        self.rng.fill(&mut nonce_bytes).map_err(|e| {
            EncryptionError::EncryptionFailed(format!("Nonce generation failed: {:?}", e))
        })?;

        let unbound_key = self.create_key()?;
        let nonce_sequence = SingleNonce {
            nonce_bytes,
            used: false,
        };
        let mut sealing_key = SealingKey::new(unbound_key, nonce_sequence);

        let mut ciphertext = plaintext.to_vec();
        sealing_key
            .seal_in_place_append_tag(Aad::empty(), &mut ciphertext)
            .map_err(|e| {
                EncryptionError::EncryptionFailed(format!("Encryption failed: {:?}", e))
            })?;

        Ok(Encrypted {
            ciphertext,
            nonce: nonce_bytes.to_vec(),
        })
    }

    /// Decrypt ciphertext
    pub fn decrypt(&self, encrypted: &Encrypted) -> Result<Decrypted, EncryptionError> {
        if encrypted.nonce.len() != NONCE_LEN {
            return Err(EncryptionError::DecryptionFailed(
                "Invalid nonce length".to_string(),
            ));
        }

        let mut nonce_bytes = [0u8; NONCE_LEN];
        nonce_bytes.copy_from_slice(&encrypted.nonce);

        let unbound_key = self.create_key()?;
        let nonce_sequence = SingleNonce {
            nonce_bytes,
            used: false,
        };
        let mut opening_key = OpeningKey::new(unbound_key, nonce_sequence);

        let mut plaintext = encrypted.ciphertext.clone();
        let plaintext_len = opening_key
            .open_in_place(Aad::empty(), &mut plaintext)
            .map_err(|e| EncryptionError::DecryptionFailed(format!("Decryption failed: {:?}", e)))?
            .len();

        plaintext.truncate(plaintext_len);

        Ok(Decrypted { plaintext })
    }

    /// Encrypt string (convenience method)
    pub fn encrypt_string(&self, plaintext: &str) -> Result<Encrypted, EncryptionError> {
        self.encrypt(plaintext.as_bytes())
    }

    /// Decrypt to string (convenience method)
    pub fn decrypt_string(&self, encrypted: &Encrypted) -> Result<String, EncryptionError> {
        let decrypted = self.decrypt(encrypted)?;
        String::from_utf8(decrypted.plaintext)
            .map_err(|e| EncryptionError::DecryptionFailed(format!("UTF-8 decode failed: {}", e)))
    }
}

// Serialization for Encrypted type
impl serde::Serialize for Encrypted {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("Encrypted", 2)?;
        state.serialize_field("ciphertext", &hex::encode(&self.ciphertext))?;
        state.serialize_field("nonce", &hex::encode(&self.nonce))?;
        state.end()
    }
}

impl<'de> serde::Deserialize<'de> for Encrypted {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{self, MapAccess, Visitor};
        use std::fmt;

        struct EncryptedVisitor;

        impl<'de> Visitor<'de> for EncryptedVisitor {
            type Value = Encrypted;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("struct Encrypted")
            }

            fn visit_map<V>(self, mut map: V) -> Result<Encrypted, V::Error>
            where
                V: MapAccess<'de>,
            {
                let mut ciphertext_hex = None;
                let mut nonce_hex = None;

                while let Some(key) = map.next_key()? {
                    match key {
                        "ciphertext" => {
                            if ciphertext_hex.is_some() {
                                return Err(de::Error::duplicate_field("ciphertext"));
                            }
                            ciphertext_hex = Some(map.next_value()?);
                        }
                        "nonce" => {
                            if nonce_hex.is_some() {
                                return Err(de::Error::duplicate_field("nonce"));
                            }
                            nonce_hex = Some(map.next_value()?);
                        }
                        _ => {
                            let _ = map.next_value::<de::IgnoredAny>()?;
                        }
                    }
                }

                let ciphertext_hex: String =
                    ciphertext_hex.ok_or_else(|| de::Error::missing_field("ciphertext"))?;
                let nonce_hex: String =
                    nonce_hex.ok_or_else(|| de::Error::missing_field("nonce"))?;

                let ciphertext = hex::decode(ciphertext_hex)
                    .map_err(|e| de::Error::custom(format!("Invalid hex ciphertext: {}", e)))?;
                let nonce = hex::decode(nonce_hex)
                    .map_err(|e| de::Error::custom(format!("Invalid hex nonce: {}", e)))?;

                Ok(Encrypted { ciphertext, nonce })
            }
        }

        const FIELDS: &[&str] = &["ciphertext", "nonce"];
        deserializer.deserialize_struct("Encrypted", FIELDS, EncryptedVisitor)
    }
}
