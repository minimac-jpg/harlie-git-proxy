#!/usr/bin/env bash
set -euo pipefail

# Provisions a dedicated SCM caller token for harlie-git-proxy.
#
# Adds `git_proxy_token` to harlie-secrets, appends it to `scm_callers`
# (the inbound allowlist read by harlie-scm) and to `service_tokens`
# (the fleet identity list used by token rotation).
#
# Usage:
#   ./install-scm-token.sh [--restart]
#
# --restart rolls harlie-scm and harlie-git-proxy so they read the new env.
# The git proxy deployment must already point at `git_proxy_token`.

NS=harlie
SECRET=harlie-secrets
TOKEN_KEY=git_proxy_token
RESTART=0
[[ ${1:-} == "--restart" ]] && RESTART=1

backup="harlie-secrets.backup.$(date +%Y%m%d%H%M%S).yaml"
kubectl -n "$NS" get secret "$SECRET" -o yaml > "$backup"
chmod 600 "$backup"
echo "backup: $backup"

b64() { printf '%s' "$1" | base64 | tr -d '\n'; }

existing=$(kubectl -n "$NS" get secret "$SECRET" \
  -o jsonpath="{.data.$TOKEN_KEY}" 2>/dev/null | base64 -d 2>/dev/null || true)
if [[ -z $existing ]]; then
  existing=$(openssl rand -hex 32)
  echo "generated a new $TOKEN_KEY"
else
  echo "reusing the existing $TOKEN_KEY"
fi

append_unique() {
  local key=$1
  local current
  current=$(kubectl -n "$NS" get secret "$SECRET" \
    -o jsonpath="{.data.$key}" 2>/dev/null | base64 -d 2>/dev/null || true)
  case ",$current," in
    *",$existing,"*) printf '%s' "$current" ;;
    *) if [[ -n $current ]]; then printf '%s,%s' "$current" "$existing"; else printf '%s' "$existing"; fi ;;
  esac
}

scm_callers=$(append_unique scm_callers)
service_tokens=$(append_unique service_tokens)

kubectl -n "$NS" patch secret "$SECRET" --type merge -p "{
  \"data\": {
    \"$TOKEN_KEY\": \"$(b64 "$existing")\",
    \"scm_callers\": \"$(b64 "$scm_callers")\",
    \"service_tokens\": \"$(b64 "$service_tokens")\"
  }
}"

echo "patched $SECRET: $TOKEN_KEY, scm_callers, service_tokens"

if ((RESTART)); then
  kubectl -n "$NS" rollout restart deployment/harlie-scm
  kubectl -n "$NS" rollout restart deployment/harlie-git-proxy
  kubectl -n "$NS" rollout status deployment/harlie-scm --timeout=240s
  kubectl -n "$NS" rollout status deployment/harlie-git-proxy --timeout=240s
else
  echo "restart harlie-scm and harlie-git-proxy to load the new token"
fi
