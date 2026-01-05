use argon2::password_hash::{rand_core::OsRng, SaltString};
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

pub const JWT_EXPIRATION_HOURS: u64 = 24;
pub const JWT_ALGORITHM: &str = "HS256";

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub user_id: String,
    pub email: String,
    pub iat: u64,
    pub exp: u64,
}

pub struct JwtService {
    secret: String,
}

impl JwtService {
    pub fn new(secret: String) -> Self {
        Self { secret }
    }

    pub fn encode(&self, user_id: String, email: String) -> anyhow::Result<String> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let exp = now + (JWT_EXPIRATION_HOURS * 3600);

        let claims = Claims {
            user_id,
            email,
            iat: now,
            exp,
        };

        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(self.secret.as_ref()),
        )?;

        Ok(token)
    }

    pub fn decode(&self, token: &str) -> anyhow::Result<Claims> {
        let validation = Validation::new(jsonwebtoken::Algorithm::HS256);
        let token_data = decode::<Claims>(
            token,
            &DecodingKey::from_secret(self.secret.as_ref()),
            &validation,
        )?;

        Ok(token_data.claims)
    }
}

/// Hash a password using Argon2id (recommended by OWASP for 2026)
/// Uses secure defaults: memory cost 19 MiB, time cost 2, parallelism 1
pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);

    // Configure Argon2id with secure parameters
    // Memory: 19 MiB (19456 KiB), Time: 2 iterations, Parallelism: 1
    // These match the parameters used in the Ruby/Devise setup
    let argon2 = Argon2::default();

    let password_hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("Argon2 hashing error: {}", e))?;

    Ok(password_hash.to_string())
}

/// Verify a password against an Argon2id hash
/// Supports Argon2id, Argon2i, and Argon2d variants
pub fn verify_password(password: &str, hash: &str) -> anyhow::Result<bool> {
    // Parse the hash string
    let parsed_hash = PasswordHash::new(hash)
        .map_err(|e| anyhow::anyhow!("Invalid Argon2 hash format: {}", e))?;

    // Use Argon2 default (Argon2id)
    let argon2 = Argon2::default();

    match argon2.verify_password(password.as_bytes(), &parsed_hash) {
        Ok(()) => Ok(true),
        Err(argon2::password_hash::Error::Password) => Ok(false), // Wrong password, not an error
        Err(e) => Err(anyhow::anyhow!("Argon2 verification error: {}", e)),
    }
}
