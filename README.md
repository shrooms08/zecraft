# Zecraft

A game engine for fair, private games on shielded Zcash. Build one in a weekend.

## What it is

- An open-source Rust engine, with a WebAssembly verifier that runs in the browser.
- You write a game's rules in three functions: `classify`, `settle`, `board`.
- Zecraft handles the rest: rounds, private entries, drand draws, a 2-of-3 FROST vault, sealed payouts to fresh addresses, and public verification.

## Proof it works

- The first live raffle round settled on Zcash testnet: round 0, payout transaction
  `2c3b07f1c2ea033f7c331a54ec9c49e5de9840a98141137923002154782e4922`.
  All five verification checks pass: [proof bundle](docs/examples/raffle-round-0.proof.json), [verifier output](docs/examples/raffle-round-0.verify.txt).
- A second game, the coin flip, runs through the full engine in the tests. Its entries are settled, a FROST-signed payout is approved by a keyholder and mined, and all five checks pass
  ([`coinflip_e2e.rs`](crates/zecraft-keyholder/tests/coinflip_e2e.rs)).

## Quickstart

```sh
env -u RUSTFLAGS cargo build --release -p zecraft-cli
target/release/zecraft new mygame
env -u RUSTFLAGS cargo test --release -p zecraft-mygame
```

`zecraft new dice` creates:

```
games/dice/README.md
games/dice/game-params.json
games/dice/rules/Cargo.toml
games/dice/rules/src/lib.rs        <- your game: edit the lines marked YOUR GAME
games/dice/rules/tests/dice.rs     <- tests that pass out of the box
games/dice/server/Cargo.toml
games/dice/server/src/bin/zecraft-dice-{engine,keyholder,verify}.rs
```

Then follow [docs/BUILD_A_GAME.md](docs/BUILD_A_GAME.md) to run it on testnet.

`env -u RUSTFLAGS` matters: `.cargo/config.toml` sets the NU7 testnet flag, and an exported `RUSTFLAGS` would override it.

## How a round works

1. **Enter.** Players pay the vault with a shielded memo that names the round, carries the entry, and seals a fresh payout address.
2. **Close.** The round's window of 24 blocks closes.
3. **Draw.** A drand beacon published after the reveal delay (15 minutes on testnet) supplies the randomness. If block times can't prove the round closed first, it is voided and refunded.
4. **Settle.** The game's rules turn the round into payouts. The engine checks that value is conserved.
5. **Sign.** Each keyholder recomputes the round itself, checks the payout transaction, and signs only if it matches. Two of three FROST signatures pay it.
6. **Verify.** Anyone can recheck a round: the five checks run in the browser or with `zecraft-<game>-verify round <n>`.

## Privacy, honestly

Private: who entered, which entries belong to the same person, and who won. Public by design: each entry's amount and ticket count, and every payout amount, via the published testnet viewing key, so anyone can verify the draw.

## Known limits

- **One operator.** In the local and hosted demo, one person runs all three keyholders. Production needs independent operators.
- **One testnet server.** No independent testnet lightwalletd answers right now. The verifier re-checks every proof and signature itself, but its chain data comes from one server.
- **Testnet only.**

## Games built with Zecraft

| Game | Status |
|---|---|
| [Raffle](games/raffle) | Live on testnet |
| [Coin flip](games/coinflip) | Example, and the `zecraft new` template |
| Zeek | The game Zecraft was extracted from: [playzeek.pages.dev](https://playzeek.pages.dev), [github.com/shrooms08/zeek](https://github.com/shrooms08/zeek) |

## Repo layout

| Path | What |
|---|---|
| `crates/zecraft-core` | Pure rules: chain params, drand draw, entry memos, the `Rules` trait, proof bundles |
| `crates/zecraft-engine` | Round server: follows the chain, settles, coordinates payouts, serves the API and page |
| `crates/zecraft-keyholder` | FROST keyholder that re-checks every payout before signing |
| `crates/zecraft-verify` | The five checks (native CLI and wasm) |
| `crates/zecraft-vault`, `crates/zecraft-player` | FROST vault wallet; test player wallet |
| `crates/zecraft-cli` | `zecraft new` |
| `games/` | Raffle (rules, server, wasm, web page) and coin flip |
| `scripts/local/`, `config/`, `docs/` | Local testnet runner, the raffle's public config, guides and examples |

## Roadmap

- A browser wallet module, from Zeek Pocket.
- Independent keyholder operators.
- A second testnet lightwalletd for verification.
- More game templates.
- Mainnet, after review.
- A rename to zblock (links will redirect).

## Funding testnet

Get TAZ from [fauzec.com](https://fauzec.com). The valargroup faucet currently can't pay NU7 addresses.

## Licence

MIT OR Apache-2.0, as Zeek ([LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE)).
Extracted from Zeek: see [PROVENANCE.md](PROVENANCE.md).
