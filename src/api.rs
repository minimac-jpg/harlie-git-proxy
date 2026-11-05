use axum::body::{Body, Bytes};
use axum::extract::{DefaultBodyLimit, Path, RawQuery, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use axum::Router;
use base64::Engine as _;
use serde_json::json;
use uuid::Uuid;

use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    let max_body = state.config.max_body_bytes;
    Router::new()
        .route("/health", axum::routing::get(health))
        .route("/git/by-name/{owner}/{repo}", any(by_name_root))
        .route("/git/by-name/{owner}/{repo}/{*path}", any(by_name))
        .route("/git/{repo}", any(by_id_root))
        .route("/git/{repo}/{*path}", any(by_id))
        .layer(DefaultBodyLimit::max(max_body))
        .with_state(std::sync::Arc::new(state))
}

async fn health() -> impl IntoResponse {
    axum::Json(json!({ "status": "ok" }))
}

enum RepoRef {
    Id(String),
    Name(String),
}

async fn by_id_root(
    State(state): State<std::sync::Arc<AppState>>,
    Path(repo): Path<String>,
    method: Method,
    headers: HeaderMap,
    raw_query: RawQuery,
    body: Bytes,
) -> Response {
    handle(
        state,
        RepoRef::Id(repo),
        String::new(),
        method,
        headers,
        raw_query,
        body,
    )
    .await
}

async fn by_id(
    State(state): State<std::sync::Arc<AppState>>,
    Path((repo, path)): Path<(String, String)>,
    method: Method,
    headers: HeaderMap,
    raw_query: RawQuery,
    body: Bytes,
) -> Response {
    handle(
        state,
        RepoRef::Id(repo),
        path,
        method,
        headers,
        raw_query,
        body,
    )
    .await
}

async fn by_name_root(
    State(state): State<std::sync::Arc<AppState>>,
    Path((owner, repo)): Path<(String, String)>,
    method: Method,
    headers: HeaderMap,
    raw_query: RawQuery,
    body: Bytes,
) -> Response {
    handle(
        state,
        RepoRef::Name(format!("{owner}/{repo}")),
        String::new(),
        method,
        headers,
        raw_query,
        body,
    )
    .await
}

async fn by_name(
    State(state): State<std::sync::Arc<AppState>>,
    Path((owner, repo, path)): Path<(String, String, String)>,
    method: Method,
    headers: HeaderMap,
    raw_query: RawQuery,
    body: Bytes,
) -> Response {
    handle(
        state,
        RepoRef::Name(format!("{owner}/{repo}")),
        path,
        method,
        headers,
        raw_query,
        body,
    )
    .await
}

async fn handle(
    state: std::sync::Arc<AppState>,
    repo_ref: RepoRef,
    path: String,
    method: Method,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
    body: Bytes,
) -> Response {
    let token = match crate::token::token_from_headers(&headers) {
        Some(token) => token,
        None => {
            tracing::warn!("git proxy request rejected: missing credential");
            return error(StatusCode::UNAUTHORIZED, "missing git proxy credential");
        }
    };
    let claims = match state.verifier.verify(&token) {
        Ok(claims) => claims,
        Err(message) => {
            tracing::warn!(error = %message, "git proxy request rejected: invalid credential");
            return error(
                StatusCode::UNAUTHORIZED,
                "invalid or expired git proxy credential",
            );
        }
    };
    tracing::debug!(
        tenant_id = %claims.tenant_id,
        task_id = %claims.task_id,
        repo_id = %claims.repo_id,
        audience = %claims.aud,
        "git proxy credential accepted"
    );

    let (repo_id, full_name) = match resolve_repo(&state, &claims, &repo_ref).await {
        Ok(value) => value,
        Err(response) => return response,
    };

    let path = path.trim_start_matches('/').to_string();
    let query = query.unwrap_or_default();
    let body_upload = String::from_utf8_lossy(&body).contains("\"operation\"");
    let needs_write = path.contains("receive-pack")
        || query.contains("service=git-receive-pack")
        || method == Method::PUT
        || method == Method::PATCH
        || method == Method::DELETE
        || (path.contains("info/lfs")
            && body_upload
            && String::from_utf8_lossy(&body).contains("upload"));
    if needs_write && claims.perm != "write" {
        return error(
            StatusCode::FORBIDDEN,
            "git write access requires an explicit contents: write permission",
        );
    }

    let upstream = {
        let base = state.config.upstream_base.trim_end_matches('/');
        let mut url = format!("{base}/{full_name}.git");
        if !path.is_empty() {
            url.push('/');
            url.push_str(&path);
        }
        if !query.is_empty() {
            url.push('?');
            url.push_str(&query);
        }
        url
    };

    // Public repos are cloned anonymously. Private repos (or writes) get a
    // repo-scoped token minted only inside the broker.
    let mut token: Option<String> = None;
    let mut as_write = needs_write;
    if needs_write {
        token = mint_token(&state, claims.tenant_id, repo_id, "write").await;
    }
    let mut upstream_response = match send(
        &state,
        &method,
        &upstream,
        &headers,
        &body,
        token.as_deref(),
    )
    .await
    {
        Ok(response) => response,
        Err(message) => return error(StatusCode::BAD_GATEWAY, message),
    };
    if !as_write
        && matches!(
            upstream_response.status(),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::NOT_FOUND
        )
    {
        if let Some(minted) = mint_token(&state, claims.tenant_id, repo_id, "read").await {
            token = Some(minted);
            as_write = true;
            match send(
                &state,
                &method,
                &upstream,
                &headers,
                &body,
                token.as_deref(),
            )
            .await
            {
                Ok(response) => upstream_response = response,
                Err(message) => return error(StatusCode::BAD_GATEWAY, message),
            }
        }
    }

    let status = upstream_response.status();
    tracing::info!(
        tenant_id = %claims.tenant_id,
        build_id = %claims.build_id,
        task_id = %claims.task_id,
        repo = %full_name,
        path = %path,
        status = status.as_u16(),
        authenticated = as_write,
        "git proxy request"
    );
    let record = crate::audit::access_record(
        claims.tenant_id,
        &claims.build_id,
        &claims.task_id,
        repo_id,
        &full_name,
        &claims.perm,
        &path,
        status.as_u16(),
    );
    state.audit.publish(record, claims.tenant_id).await;

    let mut response = Response::builder().status(status);
    for (name, value) in upstream_response.headers() {
        if safe_response_header(name.as_str()) {
            response = response.header(name, value);
        }
    }
    match response.body(Body::from_stream(upstream_response.bytes_stream())) {
        Ok(response) => response,
        Err(err) => error(StatusCode::BAD_GATEWAY, format!("proxy response: {err}")),
    }
}

async fn resolve_repo(
    state: &AppState,
    claims: &crate::token::Claims,
    repo_ref: &RepoRef,
) -> Result<(Uuid, String), Response> {
    match repo_ref {
        RepoRef::Id(raw) => {
            let id = raw
                .trim_end_matches(".git")
                .parse::<Uuid>()
                .map_err(|_| error(StatusCode::BAD_REQUEST, "invalid repository path"))?;
            if id != claims.repo_id {
                return Err(error(
                    StatusCode::FORBIDDEN,
                    "git proxy token does not authorize this repository",
                ));
            }
            let full_name = claims.full_name.trim_end_matches(".git").to_string();
            if full_name.is_empty() {
                return Err(error(
                    StatusCode::BAD_REQUEST,
                    "git proxy token has no repository",
                ));
            }
            Ok((id, full_name))
        }
        RepoRef::Name(raw) => {
            let full_name = raw
                .trim_end_matches(".git")
                .trim_matches('/')
                .to_ascii_lowercase();
            if !claims
                .repo_allowlist
                .iter()
                .any(|allowed| allowed.eq_ignore_ascii_case(&full_name))
            {
                return Err(error(
                    StatusCode::FORBIDDEN,
                    "submodule repository is not in this task's clone grant",
                ));
            }
            match state
                .scm
                .resolve_repository(claims.tenant_id, &full_name)
                .await
                .map_err(|message| error(StatusCode::BAD_GATEWAY, message))?
            {
                Some(resolved) => Ok((resolved.repo_id, resolved.full_name)),
                // Public submodules that are not connected to the platform can
                // still be cloned anonymously through the proxy.
                None => Ok((Uuid::nil(), full_name)),
            }
        }
    }
}

async fn mint_token(
    state: &AppState,
    tenant_id: Uuid,
    repo_id: Uuid,
    permission: &str,
) -> Option<String> {
    if repo_id.is_nil() {
        return None;
    }
    match state.scm.git_token(tenant_id, repo_id, permission).await {
        Ok(token) => token.token,
        Err(error) => {
            tracing::warn!(%repo_id, error = %error, "scm token mint failed");
            None
        }
    }
}

async fn send(
    state: &AppState,
    method: &Method,
    url: &str,
    headers: &HeaderMap,
    body: &Bytes,
    token: Option<&str>,
) -> Result<reqwest::Response, String> {
    let mut request = state.http.request(method.clone(), url).body(body.clone());
    for (name, value) in headers {
        if safe_request_header(name.as_str()) {
            if let Ok(value) = value.to_str() {
                request = request.header(name.as_str(), value);
            }
        }
    }
    if let Some(token) = token {
        let credentials =
            base64::engine::general_purpose::STANDARD.encode(format!("x-access-token:{token}"));
        request = request.header("authorization", format!("Basic {credentials}"));
    }
    request
        .send()
        .await
        .map_err(|error| format!("upstream request failed: {error}"))
}

fn safe_request_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "accept" | "content-type" | "git-protocol" | "user-agent" | "x-git-lfs"
    )
}

fn safe_response_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "content-type"
            | "content-length"
            | "content-encoding"
            | "cache-control"
            | "etag"
            | "last-modified"
            | "www-authenticate"
            | "location"
            | "x-git-lfs"
    )
}

fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (status, axum::Json(json!({ "error": message.into() }))).into_response()
}
