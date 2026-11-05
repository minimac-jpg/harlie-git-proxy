use serde::Deserialize;
use uuid::Uuid;

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct GitToken {
    pub full_name: String,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default = "default_permission")]
    pub permission: String,
    #[serde(default)]
    pub anonymous: bool,
}

fn default_permission() -> String {
    "read".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResolvedRepo {
    pub repo_id: Uuid,
    pub full_name: String,
}

pub struct ScmClient {
    http: reqwest::Client,
    base_url: String,
    service_token: Option<String>,
}

impl ScmClient {
    pub fn new(base_url: &str, service_token: Option<String>) -> Self {
        // The deployment sources this from `scm_callers`, which is a
        // comma-separated inbound allowlist. Outbound calls must present a
        // single caller token.
        let service_token = service_token
            .map(|raw| {
                raw.split(',')
                    .map(str::trim)
                    .find(|token| !token.is_empty())
                    .unwrap_or("")
                    .to_string()
            })
            .filter(|token| !token.is_empty());
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            service_token,
        }
    }

    pub async fn git_token(
        &self,
        tenant_id: Uuid,
        repo_id: Uuid,
        permission: &str,
    ) -> Result<GitToken, String> {
        let url = format!(
            "{}/api/internal/repositories/{repo_id}/git-token",
            self.base_url
        );
        let mut request = self.http.post(&url).json(&serde_json::json!({
            "tenant_id": tenant_id,
            "permission": permission,
        }));
        if let Some(token) = &self.service_token {
            request = request.bearer_auth(token);
        }
        let response = request
            .send()
            .await
            .map_err(|error| format!("scm git-token request failed: {error}"))?;
        if !response.status().is_success() {
            return Err(format!("scm git-token returned {}", response.status()));
        }
        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|error| format!("bad scm git-token response: {error}"))?;
        let data = body.get("data").cloned().unwrap_or(body);
        serde_json::from_value(data).map_err(|error| format!("bad scm git-token payload: {error}"))
    }

    pub async fn resolve_repository(
        &self,
        tenant_id: Uuid,
        full_name: &str,
    ) -> Result<Option<ResolvedRepo>, String> {
        let url = format!(
            "{}/api/internal/repositories/resolve?tenant_id={tenant_id}&full_name={}",
            self.base_url,
            urlencode(full_name)
        );
        let mut request = self.http.get(&url);
        if let Some(token) = &self.service_token {
            request = request.bearer_auth(token);
        }
        let response = request
            .send()
            .await
            .map_err(|error| format!("scm resolve request failed: {error}"))?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !response.status().is_success() {
            return Err(format!("scm resolve returned {}", response.status()));
        }
        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|error| format!("bad scm resolve response: {error}"))?;
        let data = body.get("data").cloned().unwrap_or(body);
        serde_json::from_value(data)
            .map(Some)
            .map_err(|error| format!("bad scm resolve payload: {error}"))
    }
}

fn urlencode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}
