#!/bin/sh
# Proves `zecraft new` makes a game that builds and passes its tests: scaffolds
# "dice" into a temporary copy of the workspace, checks the refusals, builds
# the game's crates in release and runs its tests, then removes everything it
# made. The repository itself is never modified.
set -eu
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
env -u RUSTFLAGS cargo build --release -p zecraft-cli
zecraft="$root/target/release/zecraft"
tmp="$(mktemp -d "${TMPDIR:-/tmp}/zecraft-scaffold.XXXXXX")"
cleanup() {
  # The copy's crates were built into the shared target directory: remove them.
  if [ -f "$tmp/games/dice/rules/Cargo.toml" ]; then
    (cd "$tmp" && CARGO_TARGET_DIR="$root/target" env -u RUSTFLAGS \
      cargo clean --release -p zecraft-dice -p zecraft-dice-server >/dev/null 2>&1) || true
  fi
  rm -f "$root"/target/release/zecraft-dice-*
  rm -rf "$tmp"
}
trap cleanup EXIT INT TERM
rsync -a --exclude /target --exclude /.git --exclude /.zecraft --exclude /.secrets \
  --exclude node_modules --exclude dist --exclude /games/raffle/web/src/wasm ./ "$tmp/"
cd "$tmp"

expect_refusal() { # description, args...
  what=$1; shift
  if "$zecraft" "$@" >/dev/null 2>&1; then echo "FAIL: zecraft $* was accepted ($what)" >&2; exit 1; fi
  echo "refused as expected: zecraft $* ($what)"
}
expect_refusal "not a valid name" new Dice
expect_refusal "an existing game" new raffle
expect_refusal "outside the workspace" new dice --path ../dice

started=$(date +%s)
"$zecraft" new dice
expect_refusal "already exists" new dice
expect_refusal "magic already used" new dice2 --magic ZRFL
CARGO_TARGET_DIR="$root/target" env -u RUSTFLAGS cargo test --release -p zecraft-dice
CARGO_TARGET_DIR="$root/target" env -u RUSTFLAGS cargo build --release -p zecraft-dice-server
for bin in engine keyholder verify; do
  "$root/target/release/zecraft-dice-$bin" --help >/dev/null
done
finished=$(date +%s)
echo "test-scaffold: PASS (zecraft new dice to green tests and built binaries in $((finished - started)) s)"
