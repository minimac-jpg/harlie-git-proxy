use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use uuid::Uuid;

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct Claims {
    pub iss: String,
    pub sub: String,
    pub aud: String,
    pub tenant_id: Uuid,
    pub build_id: String,
    pub task_id: String,
    pub repo_id: Uuid,
    pub full_name: String,
    #[serde(default)]
    pub repo_allowlist: Vec<String>,
    #[serde(default = "default_permission")]
    pub perm: String,
    pub iat: i64,
    pub exp: i64,
    pub jti: Uuid,
}

fn default_permission() -> String {
    "read".to_string()
}

pub struct TokenVerifier {
    key: DecodingKey,
    validation: Validation,
}

impl TokenVerifier {
    pub fn new(public_key_pem: &str, audience: &str) -> Result<Self, String> {
        let key = DecodingKey::from_rsa_pem(public_key_pem.as_bytes())
            .map_err(|error| format!("invalid JWT public key: {error}"))?;
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_audience(&[audience]);
        validation.set_required_spec_claims(&["exp", "aud", "sub"]);
        Ok(Self { key, validation })
    }

    pub fn verify(&self, token: &str) -> Result<Claims, String> {
        decode::<Claims>(token, &self.key, &self.validation)
            .map(|data| data.claims)
            .map_err(|_| "invalid or expired git proxy token".to_string())
    }
}

pub fn token_from_headers(headers: &axum::http::HeaderMap) -> Option<String> {
    use base64::Engine as _;
    let value = headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .trim();
    if let Some(token) = value.strip_prefix("Bearer ") {
        return Some(token.trim().to_string());
    }
    let encoded = value.strip_prefix("Basic ")?;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    let decoded = String::from_utf8(decoded).ok()?;
    let (_, password) = decoded.split_once(':')?;
    let token = if password.trim().is_empty() {
        decoded.as_str()
    } else {
        password
    };
    Some(token.trim().to_string())
}
