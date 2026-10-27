#!/usr/bin/env bash
# Builds the raffle for Linux on this Mac and deploys it to the Railway
# service zecraft in the project zecraft-testnet (one container, the whole
# stack; deploy/railway/entrypoint.sh).
#
#   scripts/railway/deploy.sh                    build, stage, upload
#   scripts/railway/deploy.sh --no-healthcheck   the same, without Railway's
#                                                healthcheck (first deploy, while
#                                                the container waits for the import)
#
# The stage is a fresh temporary folder: deploy/railway's files, the three
# Linux binaries, the built web page, and SHA256SUMS over all of them, which
# the image build checks. Railway's builder only copies files, so what runs
# is exactly what this script hashed. Nothing secret is staged.
set -euo pipefail
umask 077
# shellcheck source=scripts/railway/env.sh
. "$(dirname "$0")/env.sh"

TARGET=x86_64-unknown-linux-gnu
GLIBC=2.36 # Debian bookworm
HEALTH=1
case "${1:-}" in
  "") ;;
  --no-healthcheck) HEALTH=0 ;;
  *) fail "usage: deploy.sh [--no-healthcheck]" ;;
esac
command -v railway >/dev/null || fail "the Railway CLI is not installed"
command -v cargo-zigbuild >/dev/null || fail "cargo-zigbuild is missing: brew install zig && cargo install cargo-zigbuild --locked"

say "Building the Linux binaries (no-op when up to date)"
(cd "$ROOT" && env -u RUSTFLAGS CARGO_INCREMENTAL=0 cargo zigbuild --release -q \
  --target "$TARGET.$GLIBC" -p zecraft-raffle-server)
say "Building the web page"
(cd "$ROOT/games/raffle/web" && npm run build >/dev/null)

STAGE=$(mktemp -d "${TMPDIR:-/tmp}/zecraft-railway.XXXXXX")
trap 'rm -rf "$STAGE"' EXIT
cp "$ROOT"/deploy/railway/{Dockerfile,entrypoint.sh,unpack.sh,ops.sh} "$STAGE/"
if [ "$HEALTH" = 1 ]; then
  cp "$ROOT/deploy/railway/railway.json" "$STAGE/"
else
  python3 -I -c '
import json, sys
c = json.load(open(sys.argv[1]))
for k in ("healthcheckPath", "healthcheckTimeout"):
    c["deploy"].pop(k, None)
json.dump(c, open(sys.argv[2], "w"), indent=2)
' "$ROOT/deploy/railway/railway.json" "$STAGE/railway.json"
fi
for b in engine keyholder verify; do
  cp "$ROOT/target/$TARGET/release/zecraft-raffle-$b" "$STAGE/"
done
cp -R "$ROOT/games/raffle/web/dist" "$STAGE/web"
(cd "$STAGE" && find . -type f ! -name Dockerfile ! -name railway.json | LC_ALL=C sort |
  sed 's|^\./||' | xargs shasum -a 256) >"$STAGE.sums"
mv "$STAGE.sums" "$STAGE/SHA256SUMS"
chmod -R go+rX "$STAGE"
grep ' zecraft-raffle-' "$STAGE/SHA256SUMS" | while read -r h f; do say "$f: $h"; done
say "$(wc -l <"$STAGE/SHA256SUMS" | tr -d ' ') files staged, healthcheck $([ "$HEALTH" = 1 ] && echo on || echo off)"

guard
say "Uploading to $PROJECT / $SERVICE"
rw up "$STAGE" --path-as-root --no-gitignore --service "$SERVICE" --detach
say "Uploaded. Follow: railway logs --build (then railway logs)"
