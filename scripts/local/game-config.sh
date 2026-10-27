#!/usr/bin/env bash
# Writes config/raffle.testnet.json for genesis height G from the vault's
# public values (.zecraft/vault.env). Usage: scripts/local/game-config.sh G
set -euo pipefail
cd "$(dirname "$0")/../.."
G=${1:?usage: game-config.sh GENESIS_HEIGHT}
[ -f .zecraft/vault.env ] || { echo "run keys.sh first" >&2; exit 1; }
mkdir -p config
python3 -I - "$G" <<'PY'
import json, sys
env = dict(l.strip().split("=", 1) for l in open(".zecraft/vault.env") if "=" in l)
g = int(sys.argv[1])
config = {
    # ChainParams::testnet(G): 24 blocks of 25 s, ZIP 317 fee, drand quicknet,
    # reveal delay 900 s, MTP over 11 blocks, 300 s honest clock skew.
    "chain": {
        "network": "test",
        "genesis_height": g,
        "blocks_per_round": 24,
        "target_spacing_secs": 25,
        "marginal_fee_zat": 5000,
        "drand": {
            "beacon_id": "quicknet",
            "chain_hash": "52db9ba70e0cc0f6eaf7803dd07447a1f5477735fd3f661792ba94600c84e971",
            "public_key": "83cf0f2896adee7eb8b5f01fcad3912212c437e0073e911fb90022d3e760183c8c4b450b6a0a6c3ac6a5776a2d1064510d1fec758c921cc22b0e17e63aaf4bcb5ed66304de9cf809bd274ca73bab4af5a6e9c76a4bc09e76eae8991ef5ece45a",
            "period_secs": 3,
            "genesis_time": 1692803367,
            "scheme": "bls-unchained-g1-rfc9380",
        },
        "reveal_delay_secs": 900,
        "mtp_window": 11,
        "honest_clock_skew_secs": 300,
    },
    "game": {"ticket_price_zat": 100000, "entry_fee_zat": 5000, "output_fee_zat": 5000},
    "vault_address": env["VAULT_UA"],
    "sealing_public_key": env["SEALING_PUBLIC_KEY"],
    "uivk": env["VAULT_UIVK"],
    "initial_carry_zat": 0,
}
with open("config/raffle.testnet.json", "w") as f:
    json.dump(config, f, indent=2)
    f.write("\n")
print(f"config/raffle.testnet.json: genesis {g}, vault {env['VAULT_UA'][:16]}...")
PY
