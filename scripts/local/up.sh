#!/usr/bin/env bash
# Starts GAME's (default raffle) keyholders 1-3, engine (payout coordinator)
# and verify relay with nohup. Logs: <logs>/<name>.log, pids: <run>/<name>.pid
# (scripts/local/env.sh says where). Then health-checks each one.
set -euo pipefail
cd "$(dirname "$0")/../.."

. scripts/local/env.sh
ENGINE_LWD=${ZECRAFT_ENGINE_LWD:-https://testnet.zec.rocks:443}
# zaino first (independent of the engine); testnet.zec.rocks keeps the relay up
# while zaino is down (it was, HTTP 503, during Phase C).
RELAY_LWD=${ZECRAFT_RELAY_LWD:-https://zaino.testnet.unsafe.zec.rocks:443,https://testnet.zec.rocks:443}

die() { echo "up.sh: $*" >&2; exit 1; }
[ -f "$VAULT_ENV" ] || die "run GAME=$GAME scripts/local/keys.sh first"
[ -f "$GAME_CONFIG" ] || die "$GAME_CONFIG is missing (GAME=$GAME scripts/local/game-config.sh <genesis>)"
for bin in "$ENGINE_BIN" "$KH_BIN" "$VERIFY_BIN"; do
  [ -x "$bin" ] || die "$bin is missing: env -u RUSTFLAGS cargo build --release -p zecraft-$GAME-server"
done
# shellcheck disable=SC1090
. "$VAULT_ENV"
mkdir -p "$LOGS" "$RUN" "$D"

start() { # name port command...
  local name=$1 port=$2; shift 2
  local pidfile=$RUN/$name.pid
  if [ -f "$pidfile" ] && kill -0 "$(cat "$pidfile")" 2>/dev/null; then
    echo "$name already running (pid $(cat "$pidfile"))"; return
  fi
  if lsof -iTCP:"$port" -sTCP:LISTEN -P >/dev/null 2>&1; then die "port $port is in use by another process"; fi
  nohup "$@" >> "$LOGS/$name.log" 2>&1 &
  echo $! > "$pidfile"
  echo "started $name (pid $!) on $port"
}

for n in 1 2 3; do
  start "kh$n" "770$n" "$KH_BIN" serve --config "$KH_CONFIG_DIR/keyholder-$n.json"
done

start relay 7790 "$VERIFY_BIN" relay --lightwalletd "$RELAY_LWD" --bind 127.0.0.1:7790

start engine 7787 env \
  ZECRAFT_GAME_CONFIG="$GAME_CONFIG" \
  ZECRAFT_ENGINE_DB="$D/engine.sqlite" \
  ZECRAFT_ENGINE_BIND=127.0.0.1:7787 \
  ZECRAFT_LIGHTWALLETD_URL="$ENGINE_LWD" \
  ZECRAFT_KEYHOLDERS="$KEYHOLDERS" \
  ZECRAFT_COORDINATOR_IDENTITY="$S/coordinator/identity.json" \
  ZECRAFT_VAULT_SECRETS_DIR="$S/vault" \
  ZECRAFT_VAULT_DATA_DIR="$D/wallet" \
  ZECRAFT_WEB_DIR="$WEB_DIR" \
  ZECRAFT_RELAY_UPSTREAM=http://127.0.0.1:7790 \
  "$ENGINE_BIN"


echo "== health"
wait_port() { # name port
  for _ in $(seq 1 60); do
    lsof -iTCP:"$2" -sTCP:LISTEN -P >/dev/null 2>&1 && { echo "$1 listening on $2"; return 0; }
    kill -0 "$(cat "$RUN/$1.pid")" 2>/dev/null || { tail -20 "$LOGS/$1.log" >&2; die "$1 exited"; }
    sleep 1
  done
  tail -20 "$LOGS/$1.log" >&2; die "$1 is not listening on $2 after 60 s"
}
for n in 1 2 3; do wait_port "kh$n" "770$n"; done
wait_port relay 7790
wait_port engine 7787
for n in 1 2 3; do
  curl -fsS "http://127.0.0.1:770$n/v1/health" >/dev/null || die "keyholder $n /v1/health failed"
done
curl -fsS http://127.0.0.1:7787/v1/config | python3 -I -c '
import json,sys
c=json.load(sys.stdin)
assert c["entries_open"], "entries are closed"
print("engine /v1/config: network %s, genesis %d, memo %s, game %s" % (c["chain"]["network"], c["chain"]["genesis_height"], c["memo"]["magic"], json.dumps(c["game"], separators=(",", ":"))))
' || die "engine /v1/config failed"
if [ -f "$WEB_DIR/index.html" ]; then
  curl -fsS -o /dev/null http://127.0.0.1:7787/ && echo "web page served at http://127.0.0.1:7787/"
  curl -fsS -o /dev/null "http://127.0.0.1:7787/relay/v1/chain/tip" && echo "relay proxied at /relay"
else
  echo "no web build in $WEB_DIR (the API runs without the page)"
fi
echo "all up ($GAME)"
