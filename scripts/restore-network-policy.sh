#!/usr/bin/env bash
set -euo pipefail

# Restores the git proxy ingress/egress NetworkPolicy and verifies that a
# build namespace can still reach the proxy.

dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)

kubectl apply -f "$dir/k8s/networkpolicy.yml"
kubectl -n harlie get networkpolicy harlie-git-proxy

ns=$(kubectl get ns -l harlie.dev/purpose=build-execution \
  -o jsonpath='{.items[0].metadata.name}' 2>/dev/null || true)

if [[ -z $ns ]]; then
  echo "no build-execution namespace found; policy applied, skipping connectivity check"
  exit 0
fi

echo "testing connectivity from namespace: $ns"
if ! kubectl -n "$ns" run "harlie-netcheck-$RANDOM" --rm -i --restart=Never \
  --image=busybox:1.36 -- \
  sh -c 'nc -zv harlie-git-proxy.harlie.svc.cluster.local 3010'; then
  echo
  echo "connectivity check failed. The policy is still applied."
  echo "To roll back temporarily:"
  echo "  kubectl -n harlie delete networkpolicy harlie-git-proxy"
  exit 1
fi

echo "network policy applied and connectivity verified"
