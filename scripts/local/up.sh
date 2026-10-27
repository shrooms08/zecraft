#!/usr/bin/env bash
# Starts keyholders 1-3, the raffle engine (payout coordinator) and an
# independent verify relay with nohup. Logs: .zecraft/logs/<name>.log,
# pids: .zecraft/run/<name>.pid. Then health-checks each one.
set -euo pipefail
cd "$(dirname "$0")/../.."

B=./target/release
S=.zecraft/secrets
D=.zecraft/data
GAME_CONFIG=config/raffle.testnet.json
ENGINE_LWD=${ZECRAFT_ENGINE_LWD:-https://testnet.zec.rocks:443}
# zaino first (independent of the engine); testnet.zec.rocks keeps the relay up
# while zaino is down (it was, HTTP 503, during Phase C).
RELAY_LWD=${ZECRAFT_RELAY_LWD:-https://zaino.testnet.unsafe.zec.rocks:443,https://testnet.zec.rocks:443}

die() { echo "up.sh: $*" >&2; exit 1; }
[ -f .zecraft/vault.env ] || die "run scripts/local/keys.sh first"
[ -f "$GAME_CONFIG" ] || die "$GAME_CONFIG is missing"
# shellcheck disable=SC1091
. .zecraft/vault.env
mkdir -p .zecraft/logs .zecraft/run "$D"

start() { # name port command...
  local name=$1 port=$2; shift 2
  local pidfile=.zecraft/run/$name.pid
  if [ -f "$pidfile" ] && kill -0 "$(cat "$pidfile")" 2>/dev/null; then
    echo "$name already running (pid $(cat "$pidfile"))"; return
  fi
  if lsof -iTCP:"$port" -sTCP:LISTEN -P >/dev/null 2>&1; then die "port $port is in use by another process"; fi
  nohup "$@" >> ".zecraft/logs/$name.log" 2>&1 &
  echo $! > "$pidfile"
  echo "started $name (pid $!) on $port"
}

for n in 1 2 3; do
  start "kh$n" "770$n" "$B/zecraft-raffle-keyholder" serve --config "config/keyholder-$n.json"
done

start engine 7787 env \
  ZECRAFT_GAME_CONFIG="$GAME_CONFIG" \
  ZECRAFT_ENGINE_DB="$D/engine.sqlite" \
  ZECRAFT_ENGINE_BIND=127.0.0.1:7787 \
  ZECRAFT_LIGHTWALLETD_URL="$ENGINE_LWD" \
  ZECRAFT_KEYHOLDERS="$KEYHOLDERS" \
  ZECRAFT_COORDINATOR_IDENTITY="$S/coordinator/identity.json" \
  ZECRAFT_VAULT_SECRETS_DIR="$S/vault" \
  ZECRAFT_VAULT_DATA_DIR="$D/wallet" \
  "$B/zecraft-raffle-engine"

start relay 7790 "$B/zecraft-raffle-verify" relay --lightwalletd "$RELAY_LWD" --bind 127.0.0.1:7790

echo "== health"
wait_port() { # name port
  for _ in $(seq 1 60); do
    lsof -iTCP:"$2" -sTCP:LISTEN -P >/dev/null 2>&1 && { echo "$1 listening on $2"; return 0; }
    kill -0 "$(cat ".zecraft/run/$1.pid")" 2>/dev/null || { tail -20 ".zecraft/logs/$1.log" >&2; die "$1 exited"; }
    sleep 1
  done
  tail -20 ".zecraft/logs/$1.log" >&2; die "$1 is not listening on $2 after 60 s"
}
for n in 1 2 3; do wait_port "kh$n" "770$n"; done
wait_port engine 7787
wait_port relay 7790
for n in 1 2 3; do
  curl -fsS "http://127.0.0.1:770$n/v1/health" >/dev/null || die "keyholder $n /v1/health failed"
done
curl -fsS http://127.0.0.1:7787/v1/config | python3 -I -c '
import json,sys
c=json.load(sys.stdin)
assert c["entries_open"], "entries are closed"
print("engine /v1/config: network %s, genesis %d, ticket %d zat, memo %s" % (c["chain"]["network"], c["chain"]["genesis_height"], c["game"]["ticket_price_zat"], c["memo"]["magic"]))
' || die "engine /v1/config failed"
echo "all up"
