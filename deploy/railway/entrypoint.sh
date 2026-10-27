#!/bin/bash
# Starts the whole Zecraft raffle stack in one Railway container, as an
# unprivileged user:
#
#   keyholder 1-3  127.0.0.1:7701-7703
#   verify relay   127.0.0.1:7790   (reached through the engine's /relay)
#   engine         127.0.0.1:7787   (local API, for operators inside the container)
#                  0.0.0.0:$PORT    (public: the page, the API and /relay)
#
# Railway mounts the volume at /data as root. Nothing starts until the
# operator's import has written /data/.imported (scripts/railway/import.sh),
# so the stack never runs with an empty state: no fresh database, no
# missing key share. Then /data is given to uid 10001 and every process runs
# as that user with no capabilities. Each log line is prefixed with its
# process. If any process exits, the others are stopped and the container
# exits, so Railway restarts it.
#
# The service variable ZECRAFT_HOLD=1 keeps the stack from starting: the
# container waits, so `railway ssh` can reach it and the volume can be
# copied consistently (ops.sh pack).
set -euo pipefail
umask 077
DATA=/data
APP=/app

log() { echo "[entrypoint] $*"; }

if [ "${RAILWAY_VOLUME_MOUNT_PATH:-}" != "$DATA" ]; then
  log "the volume must be mounted at $DATA"
  exit 1
fi
if [ "${ZECRAFT_HOLD:-}" = 1 ]; then
  log "on hold (ZECRAFT_HOLD=1): the stack does not start"
  while :; do sleep 3600; done
fi
if [ ! -e "$DATA/.imported" ]; then
  log "waiting for the import (scripts/railway/import.sh)"
  while [ ! -e "$DATA/.imported" ]; do sleep 5; done
  log "import found"
fi

# The engine's public listener needs a proxy secret file even in direct
# mode (where browsers call it straight). Made once, kept in the volume.
if [ ! -s "$DATA/secrets/proxy-secret" ]; then
  head -c 32 /dev/urandom | od -An -tx1 | tr -d ' \n' >"$DATA/secrets/proxy-secret"
  log "made the proxy secret"
fi
chown -R 10001:10001 "$DATA"
chmod 700 "$DATA"

# KEYHOLDERS: the keyholders' local URLs and public identities.
# shellcheck disable=SC1091
. "$DATA/vault.env"

# Runs a process as uid 10001, every output line prefixed with its name.
start() { # name command...
  local name=$1
  shift
  setpriv --reuid=10001 --regid=10001 --clear-groups --no-new-privs \
    --inh-caps=-all --bounding-set=-all -- "$@" 2>&1 | sed -u "s/^/[$name] /" &
  log "started $name"
}

wait_port() { # name port
  for _ in $(seq 1 120); do
    if (exec 3<>"/dev/tcp/127.0.0.1/$2") 2>/dev/null; then
      log "$1 listening on $2"
      return 0
    fi
    sleep 1
  done
  log "$1 is not listening on $2 after 120 s"
  return 1
}

stop_all() {
  trap - INT TERM
  log "stopping"
  # shellcheck disable=SC2046 # one word per pid
  kill -INT $(jobs -p) 2>/dev/null || true
  wait || true
}
trap 'stop_all; exit 0' INT TERM

for n in 1 2 3; do
  start "kh$n" "$APP/zecraft-raffle-keyholder" serve --config "$DATA/config/keyholder-$n.json"
done
# zaino first (independent of the engine); testnet.zec.rocks keeps the
# relay up while zaino is down.
start relay "$APP/zecraft-raffle-verify" relay \
  --lightwalletd "https://zaino.testnet.unsafe.zec.rocks:443,https://testnet.zec.rocks:443" \
  --bind 127.0.0.1:7790
for n in 1 2 3; do wait_port "kh$n" "770$n"; done
wait_port relay 7790

start engine env \
  RUST_LOG="${RUST_LOG:-info}" \
  ZECRAFT_GAME_CONFIG="$DATA/config/raffle.testnet.json" \
  ZECRAFT_ENGINE_DB="$DATA/data/engine.sqlite" \
  ZECRAFT_ENGINE_BIND=127.0.0.1:7787 \
  ZECRAFT_PUBLIC_BIND="0.0.0.0:${PORT:?PORT is not set}" \
  ZECRAFT_PROXY_SECRET_FILE="$DATA/secrets/proxy-secret" \
  ZECRAFT_CLIENT_IP_HEADER=X-Real-IP \
  ZECRAFT_LIGHTWALLETD_URL=https://testnet.zec.rocks:443 \
  ZECRAFT_KEYHOLDERS="$KEYHOLDERS" \
  ZECRAFT_COORDINATOR_IDENTITY="$DATA/secrets/coordinator/identity.json" \
  ZECRAFT_VAULT_SECRETS_DIR="$DATA/secrets/vault" \
  ZECRAFT_VAULT_DATA_DIR="$DATA/data/wallet" \
  ZECRAFT_WEB_DIR="$APP/web" \
  ZECRAFT_RELAY_UPSTREAM=http://127.0.0.1:7790 \
  "$APP/zecraft-raffle-engine"

# The first process to exit takes the container down with it.
set +e
wait -n
code=$?
log "a process exited (status $code); stopping the others"
stop_all
exit 1
