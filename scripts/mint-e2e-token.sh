#!/usr/bin/env bash
set -euo pipefail

# Mints a staging proxy JWT with the engine's private key for the E2E harness.
#
# Required:
#   HARLIE_E2E_PRIVATE_KEY_PATH  path to git_proxy_private_key PEM
#   HARLIE_E2E_TENANT_ID         tenant UUID
#   HARLIE_E2E_BUILD_ID          build id string
#   HARLIE_E2E_TASK_ID           task id string
#   HARLIE_E2E_REPO_ID           main repo UUID
#   HARLIE_E2E_FULL_NAME         owner/repo
# Optional:
#   HARLIE_E2E_ALLOWLIST         comma-separated owner/repo submodule allowlist
#   HARLIE_E2E_PERMISSION        read|write (default read)
#   HARLIE_E2E_TTL_SECS          default 3600
#   HARLIE_E2E_KID               default primary

key="${HARLIE_E2E_PRIVATE_KEY_PATH:?set HARLIE_E2E_PRIVATE_KEY_PATH}"
tenant="${HARLIE_E2E_TENANT_ID:?set HARLIE_E2E_TENANT_ID}"
build="${HARLIE_E2E_BUILD_ID:?set HARLIE_E2E_BUILD_ID}"
task="${HARLIE_E2E_TASK_ID:?set HARLIE_E2E_TASK_ID}"
repo="${HARLIE_E2E_REPO_ID:?set HARLIE_E2E_REPO_ID}"
full="${HARLIE_E2E_FULL_NAME:?set HARLIE_E2E_FULL_NAME}"
perm="${HARLIE_E2E_PERMISSION:-read}"
ttl="${HARLIE_E2E_TTL_SECS:-3600}"
kid="${HARLIE_E2E_KID:-primary}"
allow="${HARLIE_E2E_ALLOWLIST:-}"

b64url() { base64 | tr -d '\n' | tr '+/' '-_' | tr -d '='; }
now="$(date +%s)"
exp="$((now + ttl))"
allow_json="$(printf '%s' "$allow" | awk -F, '{for(i=1;i<=NF;i++){gsub(/^ +| +$/,"",$i); if($i!="") printf "%s\"%s\"", (n++?",":""), $i}}')"
header="$(printf '{"alg":"RS256","typ":"JWT","kid":"%s"}' "$kid" | b64url)"
payload="$(printf '{"iss":"harlie-engine","sub":"%s","aud":"harlie-git-proxy","tenant_id":"%s","build_id":"%s","task_id":"%s","repo_id":"%s","full_name":"%s","repo_allowlist":[%s],"perm":"%s","iat":%s,"exp":%s,"jti":"%s"}' \
  "$task" "$tenant" "$build" "$task" "$repo" "$full" "$allow_json" "$perm" "$now" "$exp" "$(uuidgen | tr '[:upper:]' '[:lower:]')" | b64url)"
unsigned="$header.$payload"
signature="$(printf '%s' "$unsigned" | openssl dgst -sha256 -sign "$key" | b64url)"
printf '%s.%s\n' "$unsigned" "$signature"
