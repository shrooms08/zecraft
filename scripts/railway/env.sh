# shellcheck shell=bash disable=SC2034
# Sourced by scripts/railway/*.sh. Every command that changes something on
# Railway runs from the repository root, which is linked to the project
# zecraft-testnet, and only after `guard` has confirmed that link: these
# scripts never touch any other Railway project.
PROJECT=zecraft-testnet
SERVICE=zecraft
VOLUME=zecraft-data
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
SSH_KEY=${ZECRAFT_RAILWAY_KEY:-$HOME/.ssh/zeek_railway_ed25519}

say() { echo "== $*"; }
fail() {
  echo "$(basename "$0"): $*" >&2
  exit 1
}

# Stops unless `railway status` (from the repository root) shows zecraft-testnet.
guard() {
  local name
  name=$(cd "$ROOT" && railway status --json 2>/dev/null |
    python3 -I -c 'import json,sys; print(json.load(sys.stdin).get("name",""))' 2>/dev/null) || true
  [ "$name" = "$PROJECT" ] || fail "railway status shows project '${name:-none}', not $PROJECT: stopping"
}

rw() { (cd "$ROOT" && railway "$@"); }
