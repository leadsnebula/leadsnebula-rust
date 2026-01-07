use argon2::password_hash::{rand_core::OsRng, SaltString};
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(test)]
use argon2::{Algorithm, Params, Version};

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

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn test_jwt_encode_decode_roundtrip() {
        let secret = "test_secret_key_for_jwt_encoding".to_string();
        let service = JwtService::new(secret);
        let user_id = "123e4567-e89b-12d3-a456-426614174000".to_string();
        let email = "test@example.com".to_string();

        let token = service.encode(user_id.clone(), email.clone()).unwrap();
        assert!(!token.is_empty());

        let claims = service.decode(&token).unwrap();
        assert_eq!(claims.user_id, user_id);
        assert_eq!(claims.email, email);
        assert!(claims.iat > 0);
        assert!(claims.exp > claims.iat);
    }

    #[test]
    fn test_jwt_decode_invalid_token() {
        let secret = "test_secret_key".to_string();
        let service = JwtService::new(secret);

        // Invalid token format
        assert!(service.decode("invalid.token.here").is_err());
        assert!(service.decode("not_a_jwt").is_err());
    }

    #[test]
    fn test_jwt_decode_wrong_secret() {
        let secret1 = "secret1".to_string();
        let secret2 = "secret2".to_string();

        let service1 = JwtService::new(secret1);
        let service2 = JwtService::new(secret2);

        let token = service1
            .encode("user123".to_string(), "test@example.com".to_string())
            .unwrap();

        // Decoding with wrong secret should fail
        assert!(service2.decode(&token).is_err());
    }

    #[test]
    fn test_hash_password_produces_different_hashes() {
        let password = "TestPassword123!";
        let hash1 = hash_password(password).unwrap();
        let hash2 = hash_password(password).unwrap();

        // Different salts should produce different hashes
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_verify_password_correct() {
        let password = "TestPassword123!";
        let hash = hash_password(password).unwrap();

        let result = verify_password(password, &hash).unwrap();
        assert!(result);
    }

    #[test]
    fn test_verify_password_incorrect() {
        let password = "TestPassword123!";
        let wrong_password = "WrongPassword123!";
        let hash = hash_password(password).unwrap();

        let result = verify_password(wrong_password, &hash).unwrap();
        assert!(!result);
    }

    #[test]
    fn test_verify_password_invalid_hash() {
        let password = "TestPassword123!";
        let invalid_hash = "invalid_hash_format";

        assert!(verify_password(password, invalid_hash).is_err());
    }

    /// Test-only fast password hasher for property-based tests
    /// Uses lower cost parameters (1 MiB memory, 1 iteration) to speed up tests
    /// while still validating the hash/verify roundtrip logic
    fn hash_password_fast(password: &str) -> anyhow::Result<String> {
        let salt = SaltString::generate(&mut OsRng);

        // Use faster parameters for tests: 1 MiB memory, 1 iteration (vs 19 MiB, 2 iterations in production)
        // This reduces test time from ~38s to ~1s while maintaining test coverage
        let params = Params::new(1024, 1, 1, None)
            .map_err(|e| anyhow::anyhow!("Argon2 params error: {}", e))?;
        let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

        let password_hash = argon2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| anyhow::anyhow!("Argon2 hashing error: {}", e))?;

        Ok(password_hash.to_string())
    }

    // Property-based test: hash/verify roundtrip for any password
    // Optimized: Uses fast hasher (64 cases) instead of production hasher (256 cases)
    // This reduces test time from 38+ seconds to ~1 second while maintaining coverage
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]
        #[test]
        fn test_hash_verify_password_roundtrip(
            password in "[a-zA-Z0-9!@#$%^&*()_+\\-=\\[\\]{};':\"\\\\|,.<>\\/?]{8,128}"
        ) {
            // Use fast hasher for generation (speeds up test)
            let hash = hash_password_fast(&password)
                .map_err(|e| proptest::test_runner::TestCaseError::fail(e.to_string()))?;

            // Still verify with production function to ensure compatibility
            // The production verify_password can handle hashes created with different parameters
            let result = verify_password(&password, &hash)
                .map_err(|e| proptest::test_runner::TestCaseError::fail(e.to_string()))?;

            prop_assert!(result);
        }
    }
}
