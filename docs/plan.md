# Zecraft: plan (draft, from Zeek @ e58587e)

Zecraft is an open-source kit for provably fair private games on shielded Zcash, extracted from Zeek. Its first example game is a testnet raffle. Zeek is never edited, rebuilt or redeployed: Zecraft copies code into a new repo.

## 1. Inventory

| Part | What it does | Kind |
|---|---|---|
| `zeek-vault` (4.2k lines) | 2-of-3 FROST RedPallas vault: view-only wallet, PCZT spend, lightwalletd pool with failover | Generic |
| `zeek-core` (5.2k) | Pure wasm-safe rules: params, rounds, drand beacon, timing rule, draws, HPKE seal, memo, ZIP 321, classify, settle, proof bundle, board | Mixed. Beacon, timing, random, seal, rounds and payment are generic. Memo, digs, settle and board are Zeek-only (25 tiles, rake, Deep Cache, Campfire). |
| `zeek-engine` (11.6k) | Chain follower, IVK dig detection, round processor, payout coordinator, SQLite, HTTP API, relay | Mixed. Chain, decrypt, store, db, coordinator, protocol, identity, relay and edge are generic. Processor and API are wired to Zeek rules. Campfire, share, mempool landing and audit are Zeek-only. |
| `zeek-keyholder` (2.3k) | DKG and signing node that recomputes each settlement and checks the PCZT before signing | Generic once `zeek_core::settle` sits behind a trait |
| `zeek-verify` (3.1k) | I/O-free five checks (beacon, digs, timing, settlement, payout), prove-a-win, offline replay. CLI plus `relay` subcommand | Mixed. Checks and win proof are generic; replay is Zeek-only. |
| `zeek-dig` (1.3k) | Test player CLI: init, address `--fresh`, sync, send, `pay-uri`, dig | Generic except `dig` |
| `zeek-pocket` + `packages/zeek-pocket` (3.5k Rust, 1.6k TS) | Browser wallet (wasm, worker, IndexedDB): scan, prove, send, dig, verify | Mixed. The wallet is generic; dig, shout and request are Zeek-only. Needs a nightly wasm build. |
| `apps/web` (~15k TS) | The Zeek game UI, art, sound, Pages Functions | Zeek-only |
| `apps/harness` | Pocket test page | Generic (dev only) |
| `deploy/railway`, `scripts/railway-*` | Pinned image, entrypoint, volume import, backups | Generic in pattern; paths and names are Zeek's |
| `vendor/zcash_protocol`, `.cargo/config.toml` | NU7 testnet patch and cfg | Generic, required |

## 2. Kit modules (crate `zecraft-*`)

1. **vault**, from `zeek-vault` (copied almost verbatim). Generalise only names and env vars (`ZEEK_*` becomes `ZECRAFT_*`). API: `Vault::open(cfg)`, `sync()`, `balance()`, `propose(outputs) -> Pczt`, `frost::sign(pczt, shares)`, `Pool::new(urls)`.
2. **draw**, from core `beacon`, `timing`, `random` and `rounds`. Generalise: split `GameParams` into `ChainParams` (genesis, blocks per round, spacing, MTP window, skew, drand chain, reveal delay) and game params. Domains become caller-supplied. API: `beacon_round_for(&ChainParams, t_start)`, `verify_beacon`, `check_window_close`, `uniform_below(domain, round, &rand, n)`.
3. **entry**, from core `memo`, `seal` and `payment`. Generalise the memo to `0xFF | magic[4] | version | round u32 | payload[N] | sealed receiver(92)`, with the text form `<prefix>1:` plus base64url. The AAD is the header, so the seal binds to each game's payload. Drop tiles and Campfire. API: `EntryMemo<P: Payload>::{encode, encode_text, decode}`, `seal/open`, `entry_request_uri(addr, amount, memo)`.
4. **rules seam** (new, about 150 lines). This is the only new abstraction:
   ```rust
   trait Rules { type Params; type Entry; type Outcome;
     fn classify(&self, p, window, notes: &[ReceivedNote]) -> Classified<Self::Entry>;   // valid / refund / forfeit
     fn settle(&self, p, &Classified<_>, carry_in: u64, draw: Option<&Randomness>) -> Settlement<Self::Outcome>;
     fn board(&self, p, &Classified<_>) -> serde_json::Value; }
   ```
   The generic `Settlement` keeps what the coordinator and keyholders read: `payouts: Vec<Payout{sealed, aad, gross, fee, net}>`, `payout_inputs`, fees, `carry_in` and `carry_out`. Conservation (`in + carry_in == paid + fees + carry_out`) is checked in generic code.
5. **round server**, from `zeek-engine` minus campfire, share, mempool and audit. The processor, API and proof bundle become generic over `R: Rules`. API: `Engine::<R>::run(cfg, rules)`, with routes `/v1/config`, `/v1/board`, `/v1/rounds/{n}`, `/v1/rounds/{n}/proof`, `/v1/chain/*` and `/v1/tx/*`.
6. **keyholder**, from `zeek-keyholder`. `verify_payout` takes `R: Rules` and the generic settlement. API: `zecraft-keyholder dkg|serve --config`.
7. **verify**, from `zeek-verify` minus replay. The checks take `R`. API: `verify_round::<R>(bundle, config, chain) -> Report`, CLI `zecraft-verify round`, plus a stable-toolchain wasm build (core, entry and verify only, no prover).

The browser wallet (Pocket) is **not** in the 24-hour kit. It stays a later module.

## 3. Raffle template (`zecraft-raffle`)

- **Ticket purchase.** The memo uses magic `ZRFL`, payload `tickets u16`, and a sealed fresh diversified address (`player address --fresh`). The value must be exactly `tickets × ticket_price`.
- **Classification.** In window, right round, tickets ≥ 1 and exact value make the entry valid. Wrong round, outside the window, wrong value, or a voided round gives a refund minus the output fee. An undecodable memo, or a value too small to refund, is forfeited to `carry`.
- **Draw.** Order entries by (height, txid, action). Entry *i* owns tickets [sᵢ, sᵢ+tᵢ). The winning ticket is `uniform_below("zecraft/raffle/v1", round, beacon, T)`.
- **Settlement.** The winner gets `Σvalid − entry fees + carry_in − output fee` in one output. There is no rake. No tickets means everything carries over. The timing rule, the 900 s reveal delay and settlement at window end + 21 blocks are unchanged.
- **Kit modules used:** vault, draw, entry, rules seam, round server, keyholder, verify.
- **New code, about 1,100 lines:**
  - rules: about 350, plus about 300 of tests
  - engine and keyholder binaries: about 100
  - player CLI `buy` command: about 120, the rest copied from `zeek-dig`
  - wasm bindings: about 80
  - web page: about 250 TS
- **Web page** (one Vite page served by the engine itself). It needs:
  - round number and countdown
  - vault address
  - tickets sold and the pot
  - a buy form (ticket count and fresh payout address) that outputs a ZIP 321 URI and QR code, sealed in the browser by the wasm build
  - the last result: beacon round, winning ticket and payout txid
  - a **Verify** button that runs the five checks through the engine's relay

## 4. Hosting

Create a **new Railway project `zecraft-testnet` with one service**. Its entrypoint starts keyholders 1–3 on `127.0.0.1:7701-7703`, then runs the engine on `7787`; the relay listens on `7790`. These ports are Zecraft's own (amended in Phase B), so a Zecraft and a Zeek stack can run on the same Mac. The engine also serves the web page, so there is one public domain and one volume (`/data/{engine,kh1,kh2,kh3}`). Reuse Zeek's Dockerfile pattern: zigbuild binaries, SHA256SUMS check, wait for `.imported`, then `setpriv`. The DKG runs on the Mac and the volume is imported, as for Zeek.

**Cost:** about $1–2 a month. Zeek measured $1.70 for engine plus 3 keyholders plus relay at about 190 MB. This fits in the Hobby plan's included usage.

**Caveat:** the project shares the workspace's **$10 hard limit** with Zeek. Hitting it stops Zeek too, so check this month's usage before deploying.

**$0 fallback:** run the same binaries on the Mac (new ports and data dirs) behind a cloudflared quick tunnel for the demo window only.

All five processes in one container belong to one operator. That is the same trust caveat as Zeek's hosted setup, and the README must say so.

## 5. Build order (one session)

0. **Preflight.**
   - Run `df -h`; clear `target/debug` if under 10 GB free.
   - Record that `git -C ../zeek status --porcelain` is empty, and check it again at the end.
   - Request faucet TAZ now (once per 24 h): about 0.02 for 3 players plus about 0.005 float.
   - Confirm that `testnet.zec.rocks` answers.
1. **Repo setup.** Create `~/Projects/zecraft` with its own git, and copy, never symlink:
   - `Cargo.toml`, **`Cargo.lock`**, `.cargo/config.toml`, `vendor/zcash_protocol`
   - the licences
   - `deploy/railway` with renames

   Layout: `crates/{zecraft-vault,-core,-engine,-keyholder,-verify,-player}`, `games/raffle/{rules,web}`, `deploy/`. Nothing depends on the Zeek repo by path or git: Zeek is a source to copy from. A header in `PROVENANCE.md` records which Zeek commit each file came from.
2. **Copy and rename.** Copy the vault, keyholder, verify and player crates, and the engine minus the Zeek-only modules. Get `cargo build --release` green with Zeek's rules still inside. This checkpoint should take about 2 hours.
3. **Split the core.** Split `zeek-core` into draw and entry, and add the `Rules` trait. Move Zeek's tile rules into `tests/` as a second `Rules` implementation, so the trait is proven against a game it did not come from.
4. **Make the server generic.** Make the processor, coordinator, keyholder `verify_payout` and the verify checks generic over `R`. Port Zeek's synthetic-chain e2e and `verify_pczt` refusal tests to the raffle.
5. **Write the raffle rules.** Tests:
   - classification table
   - ticket-boundary mapping
   - conservation property test (proptest < 1.7)
   - draw known-answer test
   - voided round refunds everything
6. **Local live round.**
   - Run the DKG for 3 keyholders into `zecraft/.secrets`, and fund the float.
   - 2–3 players run `pay-uri` with tickets.
   - Watch the round close, the beacon, the settlement, the FROST payout, and the winner seeing the note.
   - `zecraft-verify round N` must pass all five checks. Save the bundle under `docs/examples/`.
7. **Web page.** Build it, including the wasm build, then run Verify in the browser.
8. **Hosting.** Create the new Railway project from the zecraft directory (its own `data/railway` link), deploy, import the volume, and play round 2 hosted.
9. **Finish.** Write the README (threat model and one-operator caveat), and pass fmt, clippy `-D warnings` and `cargo test --release`.

## 6. Risks and cuts

**Risks**
- **Making the engine generic.** It is 11.6k lines, and this is the biggest time risk. The trait seam is kept to classify, settle and board, and the Zeek tiles test keeps it honest.
- **Testnet TAZ.** The faucet allows one request a day, and the test wallets were near empty on 2026-10-23. One failed request costs a day, so ask first. Ticket price: 0.001 TAZ.
- **One testnet lightwalletd.** `testnet.zec.rocks` is the only one, and Zeek shares it. An outage stalls both.
- **Slow rounds.** Each round takes about 45 minutes from open to payout, so plan at most 2 live rounds.
- **The NU7 patch.** It must be copied exactly, and an exported `RUSTFLAGS` overrides it.
- **Disk.** A second Zcash workspace adds 3 GB in release and up to 30 GB in debug. Build release only.
- **Shared Railway spend limit with Zeek** (see section 4).

**Cut, in order, if time runs short**
1. The hosted deploy: use the Mac and a quick tunnel instead.
2. The web page's buy form: the CLI is enough.
3. In-browser verify: use the CLI.
4. Prove-a-win.
5. Text memos: binary only.

**Out of scope for 24 hours**
- Moving Zeek onto Zecraft
- A generic browser wallet (Pocket)
- Keyholders run by separate operators
- Mainnet
- Publishing to crates.io
- Checking that the "Zecraft" name is free
