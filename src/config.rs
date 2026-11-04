use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(default)]
    pub scm_url: Option<String>,
    #[serde(default)]
    pub scm_token: Option<String>,
    #[serde(default)]
    pub jwt_public_key: Option<String>,
    #[serde(default = "default_audience")]
    pub audience: String,
    #[serde(default = "default_upstream")]
    pub upstream_base: String,
    #[serde(default = "default_max_body")]
    pub max_body_bytes: usize,
    #[serde(default = "default_timeout")]
    pub request_timeout_secs: u64,
    /// Optional PEM certificate/key for direct in-cluster TLS.
    #[serde(default)]
    pub redis_url: Option<String>,
    #[serde(default)]
    pub tls_cert_path: Option<String>,
    #[serde(default)]
    pub tls_key_path: Option<String>,
}

fn default_bind() -> String {
    "0.0.0.0:3010".to_string()
}
fn default_audience() -> String {
    "harlie-git-proxy".to_string()
}
fn default_upstream() -> String {
    "https://github.com".to_string()
}
fn default_max_body() -> usize {
    64 * 1024 * 1024
}
fn default_timeout() -> u64 {
    900
}

impl Config {
    pub fn load() -> Result<Self, String> {
        let _ = dotenvy::dotenv();
        let mut cfg: Config =
            toml::from_str(&std::fs::read_to_string("config.toml").unwrap_or_default())
                .map_err(|error| format!("invalid config.toml: {error}"))?;
        if let Ok(value) = std::env::var("HARLIE_GIT_PROXY_BIND") {
            cfg.bind = value;
        }
        if let Ok(value) = std::env::var("HARLIE_GIT_PROXY_SCM_URL") {
            cfg.scm_url = Some(value);
        }
        if let Ok(value) = std::env::var("HARLIE_GIT_PROXY_SCM_TOKEN") {
            cfg.scm_token = Some(value);
        }
        if let Ok(value) = std::env::var("HARLIE_GIT_PROXY_JWT_PUBLIC_KEY") {
            cfg.jwt_public_key = Some(normalize_pem(&value));
        }
        if let Ok(value) = std::env::var("HARLIE_GIT_PROXY_AUDIENCE") {
            cfg.audience = value;
        }
        if let Ok(value) = std::env::var("HARLIE_GIT_PROXY_UPSTREAM_BASE") {
            cfg.upstream_base = value;
        }
        if let Ok(value) = std::env::var("HARLIE_GIT_PROXY_MAX_BODY_BYTES") {
            if let Ok(parsed) = value.parse() {
                cfg.max_body_bytes = parsed;
            }
        }
        if let Ok(value) = std::env::var("HARLIE_GIT_PROXY_REQUEST_TIMEOUT_SECS") {
            if let Ok(parsed) = value.parse() {
                cfg.request_timeout_secs = parsed;
            }
        }
        if let Ok(value) = std::env::var("HARLIE_GIT_PROXY_REDIS_URL") {
            cfg.redis_url = Some(value);
        }
        if let Ok(value) = std::env::var("HARLIE_GIT_PROXY_TLS_CERT_PATH") {
            cfg.tls_cert_path = Some(value);
        }
        if let Ok(value) = std::env::var("HARLIE_GIT_PROXY_TLS_KEY_PATH") {
            cfg.tls_key_path = Some(value);
        }
        if cfg
            .jwt_public_key
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
        {
            return Err("HARLIE_GIT_PROXY_JWT_PUBLIC_KEY is required".to_string());
        }
        if cfg
            .scm_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
        {
            return Err("HARLIE_GIT_PROXY_SCM_URL is required".to_string());
        }
        Ok(cfg)
    }
}

pub fn normalize_pem(value: &str) -> String {
    value.replace("\\n", "\n")
}
