# Coin flip

The smallest complete Zecraft game, and the template `zecraft new` copies.

| | |
|---|---|
| Entry | stake exactly `stake_zat` (default 0.001 TAZ) on heads (0) or tails (1) |
| Memo | `0xFF \| "ZCFL" \| 1 \| round \| side u8 \| sealed payout address`, or text `zcfl1:` + base64url |
| Draw | `uniform_below("zecraft/coinflip/v1", round, beacon, 2)` |
| Payout | everyone on the drawn side splits `Σ stakes − entry fees + carry`, one output each; dust and a no-winner pot carry over |
| Refund | wrong round, outside the window, wrong stake, voided round: `value − entry_fee − output_fee` |

- `rules/`: the `Rules` implementation (`src/lib.rs`) and its tests.
- `server/`: `zecraft-coinflip-engine`, `-keyholder`, `-verify`.
- `game-params.json`: the defaults `scripts/local/game-config.sh` writes into `config/coinflip.testnet.json`.

Run it locally: `GAME=coinflip scripts/local/keys.sh`, fund the vault, then
`GAME=coinflip scripts/local/game-config.sh <genesis>` and `GAME=coinflip scripts/local/up.sh`.
