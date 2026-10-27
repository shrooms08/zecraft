#!/usr/bin/env bash
# Moves the Mac's raffle state into the Railway volume (the cut-over).
#
#   scripts/railway/import.sh pack OUT_DIR    the local stack must be stopped
#   scripts/railway/import.sh push OUT_DIR
#
# pack: OUT_DIR/zecraft.tar (+ .sha256) holding .zecraft/secrets as secrets/,
#   .zecraft/data as data/, the game config and the three keyholder configs
#   as config/ with every local path rewritten to /data, .zecraft/vault.env
#   as vault.env, and MANIFEST.sha256 over every file. OUT_DIR must be
#   outside the repository (a scratch folder): the tar holds the key shares.
# push: uploads the tar and its sha256 to /import/ on the volume (Railway's
#   API, over TLS), then runs /app/unpack.sh in the container (railway ssh),
#   which checks the tar and every file, unpacks them, and writes
#   /data/.imported: the waiting stack starts. The local tar is then deleted.
# Prints hashes and counts only, never file contents.
set -euo pipefail
umask 077
# shellcheck source=scripts/railway/env.sh
. "$(dirname "$0")/env.sh"

cmd=${1:-}
out=${2:-}
[ -n "$out" ] || fail "usage: import.sh pack|push OUT_DIR"
mkdir -p "$out"
out=$(cd "$out" && pwd)
case "$out/" in "$ROOT/"*) fail "OUT_DIR must be outside the repository" ;; esac
tar=$out/zecraft.tar

case "$cmd" in
  pack)
    Z=$ROOT/.zecraft
    [ -d "$Z/secrets" ] && [ -d "$Z/data" ] && [ -f "$Z/vault.env" ] || fail "no local raffle state in $Z"
    for port in 7701 7702 7703 7787 7790; do
      if lsof -iTCP:"$port" -sTCP:LISTEN -P >/dev/null 2>&1; then
        fail "port $port is in use: stop the local stack first (scripts/local/down.sh)"
      fi
    done
    if pgrep -f 'zecraft-raffle-(engine|keyholder|verify)' >/dev/null; then
      fail "a zecraft process is still running"
    fi
    work=$(mktemp -d "$out/pack.XXXXXX")
    trap 'rm -rf "$work"' EXIT
    cp -R "$Z/secrets" "$work/secrets"
    cp -R "$Z/data" "$work/data"
    cp "$Z/vault.env" "$work/vault.env"
    mkdir "$work/config"
    cp "$ROOT/config/raffle.testnet.json" "$work/config/"
    for n in 1 2 3; do
      python3 -I - "$ROOT/config/keyholder-$n.json" "$work/config/keyholder-$n.json" "$n" <<'EOF'
import json, sys
src, dst, n = sys.argv[1], sys.argv[2], sys.argv[3]
c = json.load(open(src))
c["data_dir"] = f"/data/data/kh{n}"
c["secrets_dir"] = f"/data/secrets/keyholder-{n}"
c["game_config"] = "/data/config/raffle.testnet.json"
c["bind"] = f"127.0.0.1:770{n}"
left = [k for k, v in c.items() if isinstance(v, str) and (".zecraft" in v or "/Users/" in v or v.startswith("config/"))]
if left:
    sys.exit(f"keyholder-{n}: a local path is left in {left}")
json.dump(c, open(dst, "w"), indent=2)
EOF
    done
    (cd "$work" && find secrets data config vault.env -type f | LC_ALL=C sort | xargs shasum -a 256 >MANIFEST.sha256)
    rm -f "$tar" "$tar.sha256"
    COPYFILE_DISABLE=1 tar --no-xattrs --no-mac-metadata --uid 0 --gid 0 -cf "$tar" \
      -C "$work" MANIFEST.sha256 secrets data config vault.env
    shasum -a 256 "$tar" | cut -d' ' -f1 >"$tar.sha256"
    say "$(wc -l <"$work/MANIFEST.sha256" | tr -d ' ') files packed ($(du -h "$tar" | cut -f1)), sha256 $(cat "$tar.sha256")"
    ;;
  push)
    [ -f "$tar" ] && [ -f "$tar.sha256" ] || fail "no export in $out (import.sh pack first)"
    [ "$(shasum -a 256 "$tar" | cut -d' ' -f1)" = "$(cat "$tar.sha256")" ] || fail "zecraft.tar does not match its sha256"
    [ -f "$SSH_KEY" ] || fail "missing the Railway SSH key $SSH_KEY"
    guard
    say "Uploading the export to $VOLUME (sha256 $(cat "$tar.sha256"))"
    rw volume --service "$SERVICE" files --volume "$VOLUME" upload "$tar" /import/zecraft.tar >/dev/null
    rw volume --service "$SERVICE" files --volume "$VOLUME" upload "$tar.sha256" /import/zecraft.tar.sha256 >/dev/null
    guard
    say "Unpacking in the container"
    rw ssh --service "$SERVICE" --identity-file "$SSH_KEY" -- /app/unpack.sh
    rm -f "$tar" "$tar.sha256"
    say "Imported; the local copy of the export was deleted"
    ;;
  *) fail "usage: import.sh pack|push OUT_DIR" ;;
esac
