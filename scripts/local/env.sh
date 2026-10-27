# Sourced by the local scripts: which game, and where its files live.
#   GAME=<name> (default raffle): binaries zecraft-<game>-{engine,keyholder,verify},
#   config config/<game>.testnet.json, default params games/<game>/game-params.json.
# The raffle keeps the paths of the first live vault (Phase C); every other
# game gets its own .zecraft/games/<game>/ and config/<game>/. All games use
# the same ports (7701-7703, 7787, 7790), so one runs at a time.
GAME=${GAME:-raffle}
case "$GAME" in
  "" | *[!a-z0-9]*) echo "GAME must be a game name (lowercase letters and digits), got '$GAME'" >&2; exit 1 ;;
esac
[ -d "games/$GAME" ] || { echo "no game games/$GAME (zecraft new $GAME creates one)" >&2; exit 1; }
B=./target/release
ENGINE_BIN=$B/zecraft-$GAME-engine
KH_BIN=$B/zecraft-$GAME-keyholder
VERIFY_BIN=$B/zecraft-$GAME-verify
GAME_CONFIG=config/$GAME.testnet.json
GAME_PARAMS=games/$GAME/game-params.json
WEB_DIR=games/$GAME/web/dist
if [ "$GAME" = raffle ]; then
  Z=.zecraft
  KH_CONFIG_DIR=config
else
  Z=.zecraft/games/$GAME
  KH_CONFIG_DIR=config/$GAME
fi
S=$Z/secrets
D=$Z/data
LOGS=$Z/logs
RUN=$Z/run
VAULT_ENV=$Z/vault.env
