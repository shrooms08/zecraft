#!/usr/bin/env bash
# Stops only GAME's (default raffle) processes recorded in <run>/*.pid.
set -uo pipefail
cd "$(dirname "$0")/../.."
. scripts/local/env.sh
shopt -s nullglob
for pidfile in "$RUN"/*.pid; do
  name=$(basename "$pidfile" .pid); pid=$(cat "$pidfile")
  if kill -0 "$pid" 2>/dev/null; then
    kill "$pid" && echo "stopped $name ($pid)"
    for _ in $(seq 1 20); do kill -0 "$pid" 2>/dev/null || break; sleep 0.5; done
    kill -0 "$pid" 2>/dev/null && echo "$name ($pid) still running; check it by hand"
  else
    echo "$name ($pid) was not running"
  fi
  rm -f "$pidfile"
done
