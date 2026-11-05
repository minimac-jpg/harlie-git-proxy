use crate::audit::AuditPublisher;
use crate::config::Config;
use crate::scm::ScmClient;
use crate::token::TokenVerifier;

pub struct AppState {
    pub config: Config,
    pub verifier: TokenVerifier,
    pub scm: ScmClient,
    pub http: reqwest::Client,
    pub audit: AuditPublisher,
}

impl AppState {
    pub async fn new(config: Config) -> Result<Self, String> {
        let verifier = TokenVerifier::new(
            config.jwt_public_key.as_deref().unwrap_or_default(),
            &config.audience,
        )?;
        let scm = ScmClient::new(
            config.scm_url.as_deref().unwrap_or_default(),
            config.scm_token.clone(),
        );
        let audit = AuditPublisher::new(config.redis_url.as_deref()).await;
        let http = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(15))
            .timeout(std::time::Duration::from_secs(config.request_timeout_secs))
            .user_agent("harlie-git-proxy")
            .build()
            .map_err(|error| format!("http client: {error}"))?;
        Ok(Self {
            config,
            verifier,
            scm,
            http,
            audit,
        })
    }
}
