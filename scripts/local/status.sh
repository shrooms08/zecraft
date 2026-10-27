#!/usr/bin/env bash
# GAME's (default raffle) pids, the engine's board, the vault ledger and the tip.
set -uo pipefail
cd "$(dirname "$0")/../.."
. scripts/local/env.sh
shopt -s nullglob
bad=0
echo "== processes ($GAME)"
for name in kh1 kh2 kh3 engine relay; do
  pidfile=$RUN/$name.pid
  if [ -f "$pidfile" ] && kill -0 "$(cat "$pidfile")" 2>/dev/null; then
    echo "  ok    $name (pid $(cat "$pidfile"))"
  else
    echo "  DOWN  $name"; bad=1
  fi
done
echo "== keyholders"
for n in 1 2 3; do
  curl -fsS -m 5 "http://127.0.0.1:770$n/v1/health" | python3 -I -c '
import json,sys; h=json.load(sys.stdin)
print("  kh%d: key share %s, tip %s, settled rounds %d" % (h["identifier"], h["has_key_share"], h["tip_height"], h["settled_rounds"]))' || { echo "  kh$n: no health"; bad=1; }
done
echo "== engine"
curl -fsS -m 5 http://127.0.0.1:7787/v1/board | python3 -I -c '
import json,sys; b=json.load(sys.stdin); g=b["game"]
print("  tip %d (chain %d), round %d %s, window %d..%d, %d blocks left" % (b["tip_height"], b["chain_tip"], b["round"], b["status"], b["window"]["start"], b["window"]["end"]-1, b["blocks_remaining"]))
print("  %s: %s, carry %d zat" % (g.get("game", "game"), ", ".join("%s %s" % (k, v) for k, v in g.items() if k != "game"), b["carry"]))' || { echo "  engine board unavailable"; bad=1; }
curl -fsS -m 10 http://127.0.0.1:7787/v1/vault/ledger | python3 -I -c '
import json,sys; l=json.load(sys.stdin)
print("  vault %d zat: carry %d, owed %d, reserved fees %d, operator float %d, shortfall %d (wallet tip %d)" % (l["vault_balance"], l["carry"], l["unpaid_obligations"], l["reserved_fees"], l["operator_float"], l["shortfall"], l["last_reconciliation_height"]))' || { echo "  vault ledger unavailable"; bad=1; }
curl -fsS -m 5 http://127.0.0.1:7787/v1/rounds/pending | python3 -I -c '
import json,sys; r=json.load(sys.stdin)["rounds"]
print("  pending rounds: %s" % ", ".join("%d (%s, beacon %d at %d)" % (p["round"], p["status"], p["drand_round"], p["beacon_time"]) for p in r) if r else "  no pending rounds")' || true
echo "== relay"
curl -fsS -m 5 http://127.0.0.1:7790/v1/chain/tip 2>/dev/null | head -c 200 && echo || echo "  relay tip unavailable"
[ "$bad" = 0 ] && echo "STATUS: all green" || { echo "STATUS: NOT green"; exit 1; }
