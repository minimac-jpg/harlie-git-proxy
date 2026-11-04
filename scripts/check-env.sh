#!/usr/bin/env sh
# check-env.sh — CI / pre-deploy gate for service environment wiring.
#
# Guarantees every HARLIE_* env var the service READS is either set in
# k8s/deployment.yml or explicitly allowlisted in k8s/env-defaults.txt as
# relying on a code default. Catches the "set in the manifest but never read"
# and "read by code but never set" wiring bugs (e.g. a URL/token that is
# silently absent so a whole integration is disabled).
#
# Usage:
#   scripts/check-env.sh [service-dir]              # default: repo root
#   scripts/check-env.sh --update-defaults [dir]    # regenerate the allowlist
#   CHECK_CLUSTER=1 scripts/check-env.sh            # also verify secret refs
#                                                   # (needs kubectl + access)
#
# Exit status: 0 = wired, 1 = a read var is neither set nor allowlisted.
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
default_dir=$(CDPATH= cd -- "$script_dir/.." && pwd)

update=0
if [ "${1:-}" = "--update-defaults" ]; then
  update=1
  shift
fi
dir=${1:-$default_dir}

manifest="$dir/k8s/deployment.yml"
src="$dir/src"
allow="$dir/k8s/env-defaults.txt"

if [ ! -f "$manifest" ]; then
  echo "check-env: no $manifest — nothing to check"
  exit 0
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# Env names the code reads (Rust reader helpers, std::env::var, process.env).
grep -rhoE 'env_(str|u64|u32|bool|var)\("HARLIE_[A-Z0-9_]+"\)|std::env::var\("HARLIE_[A-Z0-9_]+"\)|process\.env\.HARLIE_[A-Z0-9_]+' \
  "$src" 2>/dev/null \
  | sed -E 's/.*"([A-Z0-9_]+)".*/\1/; s/.*process\.env\.(HARLIE_[A-Z0-9_]+).*/\1/' \
  | sort -u > "$tmp/read" || true

# Env names the manifest sets.
grep -oE 'name: HARLIE_[A-Z0-9_]+' "$manifest" 2>/dev/null \
  | sed 's/name: //' | sort -u > "$tmp/set" || true

comm -23 "$tmp/read" "$tmp/set" > "$tmp/missing" || true

if [ "$update" = "1" ]; then
  {
    echo "# Env vars this service reads but intentionally relies on a code"
    echo "# default for (or sets via a shared crate). Regenerate with:"
    echo "#   scripts/check-env.sh --update-defaults"
    grep -vE '^[[:space:]]*$' "$tmp/missing" || true
  } > "$allow"
  echo "check-env: wrote $allow ($(grep -cvE '^#|^[[:space:]]*$' "$allow" || true) entries)"
  exit 0
fi

: > "$tmp/allow"
if [ -f "$allow" ]; then
  grep -vE '^[[:space:]]*#|^[[:space:]]*$' "$allow" | sort -u > "$tmp/allow" || true
fi

comm -23 "$tmp/missing" "$tmp/allow" > "$tmp/unexpected" || true

status=0
if [ -s "$tmp/unexpected" ]; then
  echo "check-env: FAIL — env vars read by code but not set in k8s/deployment.yml:"
  while IFS= read -r var; do echo "    $var"; done < "$tmp/unexpected"
  echo "  Fix: add them to $manifest, or to $allow if a default is intended."
  status=1
fi

# Dead wiring is a warning (shared crates can legitimately read some vars).
comm -13 "$tmp/read" "$tmp/set" > "$tmp/dead" || true
if [ -s "$tmp/dead" ]; then
  echo "check-env: warning — set in the manifest but not read by src (dead wiring?):"
  while IFS= read -r var; do echo "    $var"; done < "$tmp/dead"
fi

# Optional cluster checks: optional:true secret refs that are empty (the
# silent-miss class), and whether running pods have the value loaded.
if [ "${CHECK_CLUSTER:-0}" = "1" ] && command -v kubectl >/dev/null 2>&1; then
  ns=${NS:-harlie}
  secret=${SECRET:-harlie-secrets}
  echo "check-env: cluster checks (namespace=$ns secret=$secret)"
  # Pair each secretKeyRef key with its optional flag.
  awk '
    /key:[[:space:]]/ { key=$2 }
    /optional:[[:space:]]*true/ { print key }
  ' "$manifest" | sort -u > "$tmp/optional" || true
  while IFS= read -r key; do
    [ -n "$key" ] || continue
    value=$(kubectl -n "$ns" get secret "$secret" -o jsonpath="{.data.$key}" 2>/dev/null | base64 -d 2>/dev/null || true)
    if [ -n "$value" ]; then
      echo "  OK   optional secret '$key' is set"
    else
      echo "  WARN optional secret '$key' is EMPTY — silent misconfiguration"
    fi
  done < "$tmp/optional"
fi

if [ "$status" = "0" ]; then
  echo "check-env: OK"
fi
exit "$status"
