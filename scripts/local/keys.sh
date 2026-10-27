#!/usr/bin/env bash
# Zecraft local testnet keys: identities for keyholders 1-3 and the
# coordinator, config/keyholder-{1,2,3}.json, a FROST DKG on testnet, and the
# sealing and vault files the engine needs. Refuses to run twice (it would
# make a new vault). Secrets stay under .zecraft/secrets (gitignored).
set -euo pipefail
cd "$(dirname "$0")/../.."

B=./target/release
KH=$B/zecraft-raffle-keyholder
S=.zecraft/secrets
D=.zecraft/data
LWD_PRIMARY=${ZECRAFT_LWD_PRIMARY:-https://testnet.zec.rocks:443}
LWD_SECOND=${ZECRAFT_LWD_SECOND:-https://zaino.testnet.unsafe.zec.rocks:443}
GAME_CONFIG=config/raffle.testnet.json

die() { echo "keys.sh: $*" >&2; exit 1; }

[ -x "$KH" ] || die "build first: env -u RUSTFLAGS cargo build --release"
[ ! -e "$S/vault/vault.json" ] || die "$S/vault/vault.json exists: the vault was already created (delete .zecraft only if you mean to make a new vault)"
for port in 7701 7702 7703; do
  if lsof -iTCP:$port -sTCP:LISTEN -P >/dev/null 2>&1; then die "port $port is in use"; fi
done

umask 077
mkdir -p "$S" "$D" config .zecraft/logs

echo "== identities"
declare -a ID
for n in 1 2 3; do
  ID[$n]=$("$KH" identity-keygen --secrets-dir "$S/keyholder-$n" 2>/dev/null | tail -1)
  echo "keyholder $n: ${ID[$n]}"
done
COORD=$("$KH" identity-keygen --secrets-dir "$S/coordinator" 2>/dev/null | tail -1)
echo "coordinator: $COORD"

echo "== config/keyholder-{1,2,3}.json"
for n in 1 2 3; do
  peers=""
  for m in 1 2 3; do
    [ "$m" = "$n" ] && continue
    [ -n "$peers" ] && peers="$peers,"
    peers="$peers
    { \"identifier\": $m, \"url\": \"http://127.0.0.1:770$m\", \"identity\": \"${ID[$m]}\" }"
  done
  # Keyholder n prefers server n-1 (mod 2), so they read from different upstreams.
  cat > "config/keyholder-$n.json" <<JSON
{
  "identifier": $n,
  "threshold": 2,
  "signers": 3,
  "bind": "127.0.0.1:770$n",
  "data_dir": "$D/kh$n",
  "secrets_dir": "$S/keyholder-$n",
  "game_config": "$GAME_CONFIG",
  "lightwalletd": ["$LWD_PRIMARY", "$LWD_SECOND"],
  "peers": [$peers
  ],
  "coordinator_identity": "$COORD",
  "poll_secs": 10,
  "dkg": false
}
JSON
done

echo "== DKG (testnet)"
SESSION=$(openssl rand -hex 32)
pids=()
for n in 1 2 3; do
  "$KH" dkg --config "config/keyholder-$n.json" --session "$SESSION" --network test \
    > ".zecraft/logs/dkg-$n.log" 2>&1 &
  pids+=($!)
done
ok=1
for i in 0 1 2; do
  # A ceremony that hangs is killed after 120 s.
  ( sleep 120; kill "${pids[$i]}" 2>/dev/null ) & watchdog=$!
  wait "${pids[$i]}" || ok=0
  kill "$watchdog" 2>/dev/null || true
done
[ "$ok" = 1 ] || { tail -20 .zecraft/logs/dkg-*.log >&2; die "DKG failed (logs above)"; }
grep -h "DKG complete\|sealing public key" .zecraft/logs/dkg-*.log || true

echo "== sealing and vault files"
[ -s "$S/keyholder-1/payout-sealing.json" ] || die "the lead keyholder wrote no payout-sealing.json"
for n in 2 3; do install -m 600 "$S/keyholder-1/payout-sealing.json" "$S/keyholder-$n/"; done
mkdir -p -m 700 "$S/vault"
install -m 600 "$S/keyholder-1/vault.json" "$S/keyholder-1/payout-sealing.json" "$S/vault/"
for n in 2 3; do
  cmp -s "$S/keyholder-1/vault.json" "$S/keyholder-$n/vault.json" \
    || die "keyholder $n's vault.json differs from keyholder 1's"
done

echo "== every keyholder derives the same vault"
first=""
for n in 1 2 3; do
  out=$("$KH" address --config "config/keyholder-$n.json" 2>/dev/null)
  [ -n "$out" ] || die "keyholder $n prints no address"
  if [ -z "$first" ]; then first=$out
  elif [ "$out" != "$first" ]; then
    echo "keyholder 1: $first" >&2; echo "keyholder $n: $out" >&2
    die "keyholder $n derived a DIFFERENT vault address or UIVK"
  fi
done
UA=$(echo "$first" | sed -n 1p)
UIVK=$(echo "$first" | sed -n 2p)
SEAL_PK=$(python3 -I -c 'import json,sys; print(json.load(open(sys.argv[1]))["public_key"])' "$S/vault/payout-sealing.json")
cat > .zecraft/vault.env <<ENV
VAULT_UA=$UA
VAULT_UIVK=$UIVK
SEALING_PUBLIC_KEY=$SEAL_PK
COORDINATOR_IDENTITY=$COORD
KEYHOLDERS=1=http://127.0.0.1:7701=${ID[1]},2=http://127.0.0.1:7702=${ID[2]},3=http://127.0.0.1:7703=${ID[3]}
ENV
echo "all three keyholders agree"
echo "vault UA:   $UA"
echo "vault UIVK: $UIVK"
echo "sealing public key: $SEAL_PK"
echo "public values saved to .zecraft/vault.env"
