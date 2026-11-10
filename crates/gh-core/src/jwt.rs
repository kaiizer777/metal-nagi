use chrono::Utc;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};

use crate::error::GhCoreError;

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    iat: i64,
    exp: i64,
    iss: String,
}

pub fn generate_app_jwt(app_id: u64, private_key_pem: &SecretString) -> Result<String, GhCoreError> {
    let now = Utc::now().timestamp();
    // 60 seconds leeway for clock drift
    let iat = now - 60;
    // Expiration: 10 minutes from iat (maximum allowed by GitHub)
    let exp = iat + (10 * 60);

    let claims = Claims {
        iat,
        exp,
        iss: app_id.to_string(),
    };

    let header = Header::new(Algorithm::RS256);
    let key = EncodingKey::from_rsa_pem(private_key_pem.expose_secret().as_bytes())?;

    let token = encode(&header, &claims, &key)?;
    Ok(token)
}
