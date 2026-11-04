use chrono::Utc;
use redis::aio::ConnectionManager;
use serde_json::{json, Value};
use uuid::Uuid;

pub struct AuditPublisher {
    conn: Option<ConnectionManager>,
}

impl AuditPublisher {
    pub async fn new(redis_url: Option<&str>) -> Result<Self, String> {
        let conn = match redis_url.map(str::trim).filter(|url| !url.is_empty()) {
            Some(url) => {
                let client = redis::Client::open(url)
                    .map_err(|error| format!("invalid audit redis url: {error}"))?;
                Some(
                    client
                        .get_connection_manager()
                        .await
                        .map_err(|error| format!("audit redis connect failed: {error}"))?,
                )
            }
            None => None,
        };
        Ok(Self { conn })
    }

    pub async fn publish(&self, payload: Value, tenant_id: Uuid) {
        let Some(mut conn) = self.conn.clone() else {
            return;
        };
        let envelope = json!({
            "event_id": Uuid::new_v4(),
            "event_type": "audit.record",
            "schema_version": 1,
            "occurred_at": Utc::now().to_rfc3339(),
            "tenant_id": tenant_id.to_string(),
            "source": "harlie-git-proxy",
            "payload": payload,
        });
        let body = match serde_json::to_string(&envelope) {
            Ok(body) => body,
            Err(error) => {
                tracing::warn!(error = %error, "failed to encode git proxy audit event");
                return;
            }
        };
        if let Err(error) = redis::cmd("XADD")
            .arg("events.audit")
            .arg("*")
            .arg("data")
            .arg(&body)
            .query_async::<()>(&mut conn)
            .await
        {
            tracing::warn!(error = %error, "failed to publish git proxy audit event");
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn access_record(
    tenant_id: Uuid,
    build_id: &str,
    task_id: &str,
    repo_id: Uuid,
    full_name: &str,
    permission: &str,
    path: &str,
    status: u16,
) -> Value {
    let success = (200..400).contains(&status);
    json!({
        "id": Uuid::new_v4(),
        "tenant_id": tenant_id.to_string(),
        "event": { "name": "git.proxy.access", "version": "1" },
        "actor": {
            "actor_id": task_id,
            "actor_type": "service_account",
            "display_name": null,
            "ip_address": null,
            "user_agent": null,
        },
        "action": { "category": "repository", "verb": if permission == "write" { "write" } else { "read" } },
        "resource": {
            "resource_type": "repository",
            "resource_id": repo_id.to_string(),
            "resource_name": full_name,
            "parent_resource": null,
        },
        "outcome": if success { "success" } else { "failure" },
        "scope": { "type": "project", "id": repo_id.to_string() },
        "correlation": {
            "request_id": Uuid::new_v4().to_string(),
            "trace_id": Uuid::new_v4().to_string(),
            "build_id": build_id,
            "deployment_id": null,
            "approval_id": null,
        },
        "metadata": {
            "exportable": true,
            "tags": [],
            "retention": null,
            "details": {
                "task_id": task_id,
                "permission": permission,
                "path": path,
                "status": status,
            },
        },
        "occurred_at": Utc::now().to_rfc3339(),
    })
}
