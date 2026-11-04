#!/usr/bin/env bash
set -euo pipefail

# Staging E2E for the git credential broker.
#
# Required environment:
#   HARLIE_E2E_PROXY_HOST      e.g. harlie-git-proxy.harlie.svc.cluster.local:3010
#   HARLIE_E2E_PRIVATE_TOKEN   task-scoped proxy JWT for a private repo
#   HARLIE_E2E_PRIVATE_REPO_ID platform repo id for the private repo
#   HARLIE_E2E_PUBLIC_TOKEN    task-scoped proxy JWT for a public repo
#   HARLIE_E2E_PUBLIC_REPO_ID  platform repo id for the public repo
#   HARLIE_E2E_OTHER_REPO_ID   a repo id the private token must NOT access
# Optional:
#   HARLIE_E2E_WRITE_TOKEN     write-scoped proxy JWT
#   HARLIE_E2E_WRITE_REPO_ID   repo id for the push test
#   HARLIE_E2E_CA_FILE         internal CA bundle for TLS verification
#   HARLIE_E2E_QUEUE_SECONDS   sleep before cloning to mimic queue delay
#
# The token is supplied through a temporary askpass file, never argv.

need() { [[ -n "${!1:-}" ]] || { echo "missing $1" >&2; exit 2; }; }
need HARLIE_E2E_PROXY_HOST
need HARLIE_E2E_PRIVATE_TOKEN
need HARLIE_E2E_PRIVATE_REPO_ID
need HARLIE_E2E_PUBLIC_TOKEN
need HARLIE_E2E_PUBLIC_REPO_ID
need HARLIE_E2E_OTHER_REPO_ID

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
askpass="$work/askpass"
cat >"$askpass" <<'ASK'
#!/bin/sh
case "$1" in
  *Username*) echo x-access-token ;;
  *Password*) printf '%s' "${HARLIE_E2E_TOKEN:-}" ;;
esac
ASK
chmod +x "$askpass"
export GIT_ASKPASS="$askpass"
export GIT_TERMINAL_PROMPT=0
if [[ -n "${HARLIE_E2E_CA_FILE:-}" ]]; then
  export GIT_SSL_CAINFO="$HARLIE_E2E_CA_FILE"
fi

clone() {
  local token="$1" repo_id="$2" dest="$3"
  HARLIE_E2E_TOKEN="$token" git clone --no-checkout \
    "https://${HARLIE_E2E_PROXY_HOST}/git/${repo_id}.git" "$dest"
}

echo "== public repo clone =="
clone "$HARLIE_E2E_PUBLIC_TOKEN" "$HARLIE_E2E_PUBLIC_REPO_ID" "$work/public"

echo "== private repo clone =="
clone "$HARLIE_E2E_PRIVATE_TOKEN" "$HARLIE_E2E_PRIVATE_REPO_ID" "$work/private"

echo "== queue-delay clone (${HARLIE_E2E_QUEUE_SECONDS:-0}s) =="
sleep "${HARLIE_E2E_QUEUE_SECONDS:-0}"
clone "$HARLIE_E2E_PRIVATE_TOKEN" "$HARLIE_E2E_PRIVATE_REPO_ID" "$work/delayed"

echo "== private submodule clone =="
if [[ -f "$work/private/.gitmodules" ]]; then
  ( cd "$work/private" && git submodule update --init --recursive )
fi

echo "== private LFS pull =="
if [[ -f "$work/private/.gitattributes" ]] && grep -q 'filter=lfs' "$work/private/.gitattributes"; then
  ( cd "$work/private" && git lfs pull )
fi

echo "== tenant isolation =="
if clone "$HARLIE_E2E_PRIVATE_TOKEN" "$HARLIE_E2E_OTHER_REPO_ID" "$work/forbidden" 2>/dev/null; then
  echo "ERROR: private token cloned an unauthorized repo" >&2
  exit 1
fi

if [[ -n "${HARLIE_E2E_WRITE_TOKEN:-}" && -n "${HARLIE_E2E_WRITE_REPO_ID:-}" ]]; then
  echo "== write-permitted push =="
  clone "$HARLIE_E2E_WRITE_TOKEN" "$HARLIE_E2E_WRITE_REPO_ID" "$work/write"
  ( cd "$work/write" && git checkout -q -B e2e-proxy-test && \
    git -c user.email=e2e@harlie.invalid -c user.name=harlie-e2e \
      commit --allow-empty -q -m "e2e proxy check" && git push -q origin e2e-proxy-test )
fi

echo "git proxy E2E passed"
