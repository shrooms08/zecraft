# Build a game with Zecraft

Zecraft is a game engine for fair, private games on shielded Zcash. You write
the rules; the engine runs everything else.

## What the engine does for you

- **Rounds.** It follows the chain and cuts it into rounds of 24 blocks (about 10 minutes on testnet).
- **Entries.** Players pay the vault with a memo naming the round and your game's payload. The engine finds them with the vault's public viewing key and decodes them for you.
- **A fair draw.** Each round is bound to a drand beacon published 15 minutes after it closes. If block timestamps can't prove the round closed first, the round is voided and refunded.
- **Sealed payouts.** Each player seals a fresh payout address into the memo. Only the vault's keyholders can open it, and only for the entries your rules pay.
- **A threshold vault.** Two of three FROST keyholders must sign every payout. Each one recomputes the round with your rules first and refuses anything else.
- **Verification.** Anyone can recheck a round: five checks, in the browser or with `zecraft-<game>-verify`.

## The three functions you write

Your game is one type that implements `Rules` (`crates/zecraft-core/src/rules.rs`).
The coin flip (`games/coinflip/rules/src/lib.rs`) is the worked example.

| Function | Receives | Returns | Coin flip |
|---|---|---|---|
| `classify` | your params, the round (number, window, voided?), every note the vault received in it | `Classified`: valid entries, refunds, kept notes | Valid if the memo names this round and the value is exactly `stake_zat`; otherwise refund it, or keep it if it can't pay its fees |
| `settle` | params, the classified round, the carry from earlier rounds, the beacon's randomness (`None` if voided) | `Settlement`: payouts, the notes to spend, fees, the carry out, your outcome | Draw a side with `uniform_below(DRAW_DOMAIN, round, randomness, 2)`; that side splits the pot plus carry; dust carries |
| `board` | params, the classified round so far | any JSON | Heads, tails, entries, pot |

You also choose:
- your memo's `FORMAT`: 4-byte magic, text prefix and sealing context;
- your payload, a fixed-length entry such as `Side(u8)`;
- your params and your outcome.

`screen()` does the common part of `classify` for you: decoding, voided rounds, window and round number.

## Steps

1. **Create the game.** `cargo run --release -p zecraft-cli -- new mygame` (or `target/release/zecraft new mygame`). You get `games/mygame/` and it joins the workspace.
2. **Edit the rules.** Change the lines marked `YOUR GAME:` in `games/mygame/rules/src/lib.rs`.
3. **Test.** `env -u RUSTFLAGS cargo test --release -p zecraft-mygame`. Classification, draw known answers and a conservation property test are already there. When you change the draw, update the `KAT` line.
4. **Build.** `env -u RUSTFLAGS cargo build --release -p zecraft-mygame-server`.
5. **Get testnet ZEC.** Use https://fauzec.com. The valargroup faucet currently can't pay NU7 addresses; to use it, pay `zecraft-player address <name> --transparent`, then run `zecraft-player shield <name>`.
6. **Make the vault.** `GAME=mygame scripts/local/keys.sh` runs the 2-of-3 DKG and prints the vault address.
7. **Fund it.** Send a small operator float (for example 0.005 TAZ) to the vault before genesis. Pick genesis as the float's height plus about 12 blocks, then run `GAME=mygame scripts/local/game-config.sh <genesis>`.
8. **Run it.** `GAME=mygame scripts/local/up.sh`, then `GAME=mygame scripts/local/status.sh`.
9. **Enter.** Pay the vault with your game's memo. The raffle's `zecraft-player buy` is one example of building that memo.
10. **Verify.** `target/release/zecraft-mygame-verify round <n> --engine http://127.0.0.1:7787 --lightwalletd https://testnet.zec.rocks:443`.

## Rules every game keeps

The engine enforces most of these for you. Don't fight them:

- **Conservation.** `received + carry_in = paid + fees + carry_out`, exactly. The engine, every keyholder and every verifier reject a settlement that breaks it.
- **The draw comes after the close.** Draw only from the `randomness` you are given. It comes from a beacon published at least the reveal delay (15 min on testnet) after the round, and only if the timing rule proves the round closed first.
- **Refunds pay both fees.** A refund returns `value − entry_fee − output_fee`. A note that can't pay both is kept and carried.
- **Pay only your entries.** Payouts may only go to receivers sealed by this round's entries, at most once per entry.
- **Amounts are public.** The viewing key is published so anyone can verify, which makes every entry's amount and payload and every payout amount public. Who entered and who won stay private.

## Known limits

- **One operator.** Locally and on the hosted demo, one person runs all three keyholders. Real security needs independent keyholder operators.
- **One testnet server.** There is no independent testnet lightwalletd right now (zaino returns HTTP 503). The verifier re-checks every proof and signature, but its chain data comes from the same server the engine uses.
- **One game at a time.** All games share the local ports 7701–7703, 7787 and 7790, so only one runs locally at a time.
