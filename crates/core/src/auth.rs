use anyhow::{Context, Result};
use argon2::password_hash::{rand_core::OsRng, SaltString};
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

/// JWT secret key - loaded from SSM or environment
#[derive(Clone)]
pub struct JwtSecret(String);

impl JwtSecret {
    pub fn new(secret: String) -> Self {
        Self(secret)
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

/// JWT claims structure
#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    #[serde(rename = "sub")]
    pub sub: String, // Subject (user ID as string)
    pub email: String,
    pub iat: usize, // Issued at
    pub exp: usize, // Expiration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password_verified: Option<bool>,
}

impl Claims {
    /// Create new claims for a user
    pub fn new(user_id: Uuid, email: String, expires_in_secs: u64) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs() as usize;

        Self {
            sub: user_id.to_string(),
            email,
            iat: now,
            exp: now + expires_in_secs as usize,
            password_verified: None,
        }
    }

    /// Check if claims are expired
    pub fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs() as usize;
        now > self.exp
    }
}

/// JWT helper for token generation and verification
pub struct JwtHelper;

impl JwtHelper {
    /// Default token expiration (7 days to match cookie expiration)
    pub const EXPIRATION_TIME: u64 = 7 * 24 * 60 * 60;

    /// Generate JWT token for a user
    pub fn encode(claims: &Claims, secret: &JwtSecret) -> Result<String> {
        let header = Header::default();
        let encoding_key = EncodingKey::from_secret(secret.as_bytes());

        encode(&header, claims, &encoding_key).context("Failed to encode JWT token")
    }

    /// Decode and verify JWT token
    pub fn decode(token: &str, secret: &JwtSecret) -> Result<Claims> {
        let decoding_key = DecodingKey::from_secret(secret.as_bytes());
        let validation = Validation::default();

        let token_data = decode::<Claims>(token, &decoding_key, &validation)
            .context("Failed to decode JWT token")?;

        Ok(token_data.claims)
    }
}

/// Argon2 password hashing helper
pub struct PasswordHelper;

impl PasswordHelper {
    /// Hash a password using Argon2
    pub fn hash_password(password: &str) -> Result<String> {
        let argon2 = Argon2::default();
        let salt = SaltString::generate(&mut OsRng);
        let password_hash = argon2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| anyhow::anyhow!("Failed to hash password: {}", e))?;

        Ok(password_hash.to_string())
    }

    /// Verify a password against a hash
    pub fn verify_password(password: &str, hash: &str) -> Result<bool> {
        let parsed_hash = PasswordHash::parse(hash, argon2::password_hash::Encoding::default())
            .map_err(|e| anyhow::anyhow!("Invalid password hash format: {}", e))?;

        let argon2 = Argon2::default();
        match argon2.verify_password(password.as_bytes(), &parsed_hash) {
            Ok(()) => Ok(true),
            Err(_) => Ok(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_password_hashing() {
        let password = "test_password_123";
        let hash = PasswordHelper::hash_password(password).unwrap();

        assert!(PasswordHelper::verify_password(password, &hash).unwrap());
        assert!(!PasswordHelper::verify_password("wrong_password", &hash).unwrap());
    }

    #[test]
    fn test_jwt_encode_decode() {
        let user_id = Uuid::new_v4();
        let email = "test@example.com".to_string();
        let claims = Claims::new(user_id, email.clone(), 3600);

        let secret = JwtSecret::new("test_secret_key_12345".to_string());
        let token = JwtHelper::encode(&claims, &secret).unwrap();

        let decoded = JwtHelper::decode(&token, &secret).unwrap();
        assert_eq!(decoded.sub, user_id.to_string());
        assert_eq!(decoded.email, email);
    }
}
