use bcrypt::{hash, verify, DEFAULT_COST};
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

pub fn hash_password(password: &str) -> anyhow::Result<String> {
    hash(password, DEFAULT_COST).map_err(Into::into)
}

pub fn verify_password(password: &str, hash: &str) -> anyhow::Result<bool> {
    verify(password, hash).map_err(Into::into)
}
