# Provenance

Every file below was copied out of Zeek (`~/Projects/zeek`) at commit `e58587e5fd4c5452da25b10072e8b4b516130516`,
with `git archive` of the tracked tree (so no untracked or ignored file, such as
`.secrets/`, `data/`, `.env` or `target/`, could be included). Nothing here
depends on the Zeek repository by path or by git.

Status: **verbatim** is byte-identical to the source; **modified** had only the
Phase A renames (crate names, binaries, `ZEEK_*` to `ZECRAFT_*` env vars, ports,
default data dirs, proxy header names) and the removal of Zeek-only modules.

`docs/plan.md` was moved (not copied) from Zeek's untracked `docs/zecraft-plan.md`.

| Zecraft path | Zeek source path | Status |
|---|---|---|
| `.cargo/config.toml` | `.cargo/config.toml` | verbatim |
| `Cargo.lock` | `Cargo.lock` | modified (renames / removals) |
| `Cargo.toml` | `Cargo.toml` | modified (renames / removals) |
| `LICENSE-APACHE` | `LICENSE-APACHE` | verbatim |
| `LICENSE-MIT` | `LICENSE-MIT` | verbatim |
| `crates/zecraft-core/Cargo.toml` | `crates/zeek-core/Cargo.toml` | modified (renames / removals) |
| `crates/zecraft-core/src/beacon.rs` | `crates/zeek-core/src/beacon.rs` | verbatim |
| `crates/zecraft-core/src/board.rs` | `crates/zeek-core/src/board.rs` | verbatim |
| `crates/zecraft-core/src/digs.rs` | `crates/zeek-core/src/digs.rs` | verbatim |
| `crates/zecraft-core/src/lib.rs` | `crates/zeek-core/src/lib.rs` | modified (renames / removals) |
| `crates/zecraft-core/src/memo.rs` | `crates/zeek-core/src/memo.rs` | verbatim |
| `crates/zecraft-core/src/params.rs` | `crates/zeek-core/src/params.rs` | verbatim |
| `crates/zecraft-core/src/payment.rs` | `crates/zeek-core/src/payment.rs` | verbatim |
| `crates/zecraft-core/src/proof.rs` | `crates/zeek-core/src/proof.rs` | modified (renames / removals) |
| `crates/zecraft-core/src/random.rs` | `crates/zeek-core/src/random.rs` | verbatim |
| `crates/zecraft-core/src/rounds.rs` | `crates/zeek-core/src/rounds.rs` | verbatim |
| `crates/zecraft-core/src/seal.rs` | `crates/zeek-core/src/seal.rs` | verbatim |
| `crates/zecraft-core/src/settle.rs` | `crates/zeek-core/src/settle.rs` | verbatim |
| `crates/zecraft-core/src/timing.rs` | `crates/zeek-core/src/timing.rs` | verbatim |
| `crates/zecraft-core/tests/drand_fixtures.rs` | `crates/zeek-core/tests/drand_fixtures.rs` | modified (renames / removals) |
| `crates/zecraft-core/tests/fixtures/quicknet.json` | `crates/zeek-core/tests/fixtures/quicknet.json` | verbatim |
| `crates/zecraft-core/tests/properties.proptest-regressions` | `crates/zeek-core/tests/properties.proptest-regressions` | verbatim |
| `crates/zecraft-core/tests/properties.rs` | `crates/zeek-core/tests/properties.rs` | modified (renames / removals) |
| `crates/zecraft-player/Cargo.toml` | `crates/zeek-dig/Cargo.toml` | modified (renames / removals) |
| `crates/zecraft-player/src/dig.rs` | `crates/zeek-dig/src/dig.rs` | modified (renames / removals) |
| `crates/zecraft-player/src/main.rs` | `crates/zeek-dig/src/main.rs` | modified (renames / removals) |
| `crates/zecraft-player/src/notes.rs` | `crates/zeek-dig/src/notes.rs` | modified (renames / removals) |
| `crates/zecraft-player/src/wallet.rs` | `crates/zeek-dig/src/wallet.rs` | modified (renames / removals) |
| `crates/zecraft-engine/Cargo.toml` | `crates/zeek-engine/Cargo.toml` | modified (renames / removals) |
| `crates/zecraft-engine/src/api.rs` | `crates/zeek-engine/src/api.rs` | modified (renames / removals) |
| `-` | `crates/zeek-engine/src/audit.rs` | not copied (removed, see below) |
| `crates/zecraft-engine/src/beacon.rs` | `crates/zeek-engine/src/beacon.rs` | modified (renames / removals) |
| `crates/zecraft-engine/src/bin/zecraft-engine.rs` | `crates/zeek-engine/src/bin/zeek-engine.rs` | modified (renames / removals) |
| `-` | `crates/zeek-engine/src/campfire.rs` | not copied (removed, see below) |
| `crates/zecraft-engine/src/chain.rs` | `crates/zeek-engine/src/chain.rs` | modified (renames / removals) |
| `crates/zecraft-engine/src/config.rs` | `crates/zeek-engine/src/config.rs` | modified (renames / removals) |
| `crates/zecraft-engine/src/coordinator.rs` | `crates/zeek-engine/src/coordinator.rs` | modified (renames / removals) |
| `crates/zecraft-engine/src/db.rs` | `crates/zeek-engine/src/db.rs` | modified (renames / removals) |
| `crates/zecraft-engine/src/decrypt.rs` | `crates/zeek-engine/src/decrypt.rs` | modified (renames / removals) |
| `crates/zecraft-engine/src/edge.rs` | `crates/zeek-engine/src/edge.rs` | modified (renames / removals) |
| `crates/zecraft-engine/src/error.rs` | `crates/zeek-engine/src/error.rs` | modified (renames / removals) |
| `crates/zecraft-engine/src/identity.rs` | `crates/zeek-engine/src/identity.rs` | verbatim |
| `crates/zecraft-engine/src/ledger.rs` | `crates/zeek-engine/src/ledger.rs` | verbatim |
| `crates/zecraft-engine/src/lib.rs` | `crates/zeek-engine/src/lib.rs` | modified (renames / removals) |
| `crates/zecraft-engine/src/logging.rs` | `crates/zeek-engine/src/logging.rs` | modified (renames / removals) |
| `-` | `crates/zeek-engine/src/mempool.rs` | not copied (removed, see below) |
| `crates/zecraft-engine/src/processor.rs` | `crates/zeek-engine/src/processor.rs` | modified (renames / removals) |
| `crates/zecraft-engine/src/proof.rs` | `crates/zeek-engine/src/proof.rs` | modified (renames / removals) |
| `crates/zecraft-engine/src/protocol.rs` | `crates/zeek-engine/src/protocol.rs` | modified (renames / removals) |
| `crates/zecraft-engine/src/relay.rs` | `crates/zeek-engine/src/relay.rs` | modified (renames / removals) |
| `-` | `crates/zeek-engine/src/share.rs` | not copied (removed, see below) |
| `crates/zecraft-engine/src/store.rs` | `crates/zeek-engine/src/store.rs` | verbatim |
| `crates/zecraft-engine/src/synthetic.rs` | `crates/zeek-engine/src/synthetic.rs` | modified (renames / removals) |
| `crates/zecraft-engine/tests/cache.rs` | `crates/zeek-engine/tests/cache.rs` | modified (renames / removals) |
| `crates/zecraft-engine/tests/common/mod.rs` | `crates/zeek-engine/tests/common/mod.rs` | modified (renames / removals) |
| `crates/zecraft-engine/tests/engine_e2e.rs` | `crates/zeek-engine/tests/engine_e2e.rs` | modified (renames / removals) |
| `-` | `crates/zeek-engine/tests/landing.rs` | not copied (removed, see below) |
| `crates/zecraft-engine/tests/live_artifacts.rs` | `crates/zeek-engine/tests/live_artifacts.rs` | modified (renames / removals) |
| `crates/zecraft-engine/tests/stall.rs` | `crates/zeek-engine/tests/stall.rs` | modified (renames / removals) |
| `crates/zecraft-keyholder/Cargo.toml` | `crates/zeek-keyholder/Cargo.toml` | modified (renames / removals) |
| `crates/zecraft-keyholder/src/api.rs` | `crates/zeek-keyholder/src/api.rs` | modified (renames / removals) |
| `crates/zecraft-keyholder/src/config.rs` | `crates/zeek-keyholder/src/config.rs` | modified (renames / removals) |
| `crates/zecraft-keyholder/src/db.rs` | `crates/zeek-keyholder/src/db.rs` | verbatim |
| `crates/zecraft-keyholder/src/dkg.rs` | `crates/zeek-keyholder/src/dkg.rs` | modified (renames / removals) |
| `crates/zecraft-keyholder/src/error.rs` | `crates/zeek-keyholder/src/error.rs` | modified (renames / removals) |
| `crates/zecraft-keyholder/src/follower.rs` | `crates/zeek-keyholder/src/follower.rs` | modified (renames / removals) |
| `crates/zecraft-keyholder/src/lib.rs` | `crates/zeek-keyholder/src/lib.rs` | modified (renames / removals) |
| `crates/zecraft-keyholder/src/main.rs` | `crates/zeek-keyholder/src/main.rs` | modified (renames / removals) |
| `crates/zecraft-keyholder/src/signing.rs` | `crates/zeek-keyholder/src/signing.rs` | modified (renames / removals) |
| `crates/zecraft-keyholder/src/verify.rs` | `crates/zeek-keyholder/src/verify.rs` | modified (renames / removals) |
| `crates/zecraft-keyholder/tests/verify_pczt.rs` | `crates/zeek-keyholder/tests/verify_pczt.rs` | modified (renames / removals) |
| `crates/zecraft-vault/Cargo.toml` | `crates/zeek-vault/Cargo.toml` | modified (renames / removals) |
| `crates/zecraft-vault/src/bin/zecraft-vault.rs` | `crates/zeek-vault/src/bin/zeek-vault.rs` | modified (renames / removals) |
| `crates/zecraft-vault/src/config.rs` | `crates/zeek-vault/src/config.rs` | modified (renames / removals) |
| `crates/zecraft-vault/src/error.rs` | `crates/zeek-vault/src/error.rs` | modified (renames / removals) |
| `crates/zecraft-vault/src/frost.rs` | `crates/zeek-vault/src/frost.rs` | verbatim |
| `crates/zecraft-vault/src/keys.rs` | `crates/zeek-vault/src/keys.rs` | verbatim |
| `crates/zecraft-vault/src/lib.rs` | `crates/zeek-vault/src/lib.rs` | modified (renames / removals) |
| `crates/zecraft-vault/src/lightwalletd.rs` | `crates/zeek-vault/src/lightwalletd.rs` | modified (renames / removals) |
| `crates/zecraft-vault/src/pool.rs` | `crates/zeek-vault/src/pool.rs` | modified (renames / removals) |
| `crates/zecraft-vault/src/send.rs` | `crates/zeek-vault/src/send.rs` | verbatim |
| `crates/zecraft-vault/src/store.rs` | `crates/zeek-vault/src/store.rs` | modified (renames / removals) |
| `crates/zecraft-vault/src/wallet.rs` | `crates/zeek-vault/src/wallet.rs` | modified (renames / removals) |
| `crates/zecraft-vault/tests/network.rs` | `crates/zeek-vault/tests/network.rs` | modified (renames / removals) |
| `crates/zecraft-vault/tests/offline_pczt.rs` | `crates/zeek-vault/tests/offline_pczt.rs` | modified (renames / removals) |
| `crates/zecraft-verify/Cargo.toml` | `crates/zeek-verify/Cargo.toml` | modified (renames / removals) |
| `crates/zecraft-verify/src/bin/zecraft-verify.rs` | `crates/zeek-verify/src/bin/zeek-verify.rs` | modified (renames / removals) |
| `crates/zecraft-verify/src/chain.rs` | `crates/zeek-verify/src/chain.rs` | modified (renames / removals) |
| `crates/zecraft-verify/src/checks.rs` | `crates/zeek-verify/src/checks.rs` | modified (renames / removals) |
| `crates/zecraft-verify/src/lib.rs` | `crates/zeek-verify/src/lib.rs` | modified (renames / removals) |
| `crates/zecraft-verify/src/offline.rs` | `crates/zeek-verify/src/offline.rs` | modified (renames / removals) |
| `-` | `crates/zeek-verify/src/replay.rs` | not copied (removed, see below) |
| `crates/zecraft-verify/src/report.rs` | `crates/zeek-verify/src/report.rs` | verbatim |
| `crates/zecraft-verify/src/win.rs` | `crates/zeek-verify/src/win.rs` | modified (renames / removals) |
| `-` | `crates/zeek-verify/tests/offline.rs` | not copied (removed, see below) |
| `deploy/railway/Dockerfile` | `deploy/railway/Dockerfile` | modified (renames / removals) |
| `deploy/railway/entrypoint.sh` | `deploy/railway/entrypoint.sh` | modified (renames / removals) |
| `deploy/railway/import.sh` | `deploy/railway/import.sh` | modified (renames / removals) |
| `deploy/railway/keyholder-1.json` | `deploy/railway/keyholder-1.json` | modified (renames / removals) |
| `deploy/railway/keyholder-2.json` | `deploy/railway/keyholder-2.json` | modified (renames / removals) |
| `deploy/railway/keyholder-3.json` | `deploy/railway/keyholder-3.json` | modified (renames / removals) |
| `deploy/railway/ops.sh` | `deploy/railway/ops.sh` | modified (renames / removals) |
| `vendor/zcash_protocol/CHANGELOG.md` | `vendor/zcash_protocol/CHANGELOG.md` | verbatim |
| `vendor/zcash_protocol/Cargo.toml` | `vendor/zcash_protocol/Cargo.toml` | verbatim |
| `vendor/zcash_protocol/LICENSE-APACHE` | `vendor/zcash_protocol/LICENSE-APACHE` | verbatim |
| `vendor/zcash_protocol/LICENSE-MIT` | `vendor/zcash_protocol/LICENSE-MIT` | verbatim |
| `vendor/zcash_protocol/README.md` | `vendor/zcash_protocol/README.md` | verbatim |
| `vendor/zcash_protocol/ZEEK-PATCH.md` | `vendor/zcash_protocol/ZEEK-PATCH.md` | verbatim |
| `vendor/zcash_protocol/src/consensus.rs` | `vendor/zcash_protocol/src/consensus.rs` | verbatim |
| `vendor/zcash_protocol/src/constants.rs` | `vendor/zcash_protocol/src/constants.rs` | verbatim |
| `vendor/zcash_protocol/src/constants/mainnet.rs` | `vendor/zcash_protocol/src/constants/mainnet.rs` | verbatim |
| `vendor/zcash_protocol/src/constants/regtest.rs` | `vendor/zcash_protocol/src/constants/regtest.rs` | verbatim |
| `vendor/zcash_protocol/src/constants/testnet.rs` | `vendor/zcash_protocol/src/constants/testnet.rs` | verbatim |
| `vendor/zcash_protocol/src/lib.rs` | `vendor/zcash_protocol/src/lib.rs` | verbatim |
| `vendor/zcash_protocol/src/local_consensus.rs` | `vendor/zcash_protocol/src/local_consensus.rs` | verbatim |
| `vendor/zcash_protocol/src/memo.rs` | `vendor/zcash_protocol/src/memo.rs` | verbatim |
| `vendor/zcash_protocol/src/txid.rs` | `vendor/zcash_protocol/src/txid.rs` | verbatim |
| `vendor/zcash_protocol/src/value.rs` | `vendor/zcash_protocol/src/value.rs` | verbatim |
| `vendor/zcash_protocol/src/zip318.rs` | `vendor/zcash_protocol/src/zip318.rs` | verbatim |
| `docs/examples/phase4/old-vault-round-0.proof.json` | `docs/examples/phase4/old-vault-round-0.proof.json` | verbatim |
| `docs/examples/phase4/old-vault-round-0.round.json` | `docs/examples/phase4/old-vault-round-0.round.json` | verbatim |
| `docs/examples/phase4/round-0.proof.json` | `docs/examples/phase4/round-0.proof.json` | verbatim |
| `docs/examples/phase4/round-0.round.json` | `docs/examples/phase4/round-0.round.json` | verbatim |
| `docs/examples/phase4/round-7.proof.json` | `docs/examples/phase4/round-7.proof.json` | verbatim |
| `docs/examples/phase4/round-7.round.json` | `docs/examples/phase4/round-7.round.json` | verbatim |
| `docs/examples/phase4/round-8.proof.json` | `docs/examples/phase4/round-8.proof.json` | verbatim |
| `docs/examples/phase4/round-8.round.json` | `docs/examples/phase4/round-8.round.json` | verbatim |
| `docs/examples/phase4b/round-16.proof.json` | `docs/examples/phase4b/round-16.proof.json` | verbatim |
| `docs/examples/phase4b/round-16.round.json` | `docs/examples/phase4b/round-16.round.json` | verbatim |
| `docs/examples/phase4b/round-17.proof.json` | `docs/examples/phase4b/round-17.proof.json` | verbatim |
| `docs/examples/phase4b/round-17.round.json` | `docs/examples/phase4b/round-17.round.json` | verbatim |
| `docs/examples/phase5/round-58.proof.json` | `docs/examples/phase5/round-58.proof.json` | verbatim |
| `docs/examples/phase5/round-58.round.json` | `docs/examples/phase5/round-58.round.json` | verbatim |
| `docs/examples/phase5/round-74.proof.json` | `docs/examples/phase5/round-74.proof.json` | verbatim |
| `docs/examples/phase5/round-74.round.json` | `docs/examples/phase5/round-74.round.json` | verbatim |
| `docs/examples/phase6/round-74.proof.json` | `docs/examples/phase6/round-74.proof.json` | verbatim |
| `docs/examples/phase6/round-74.round.json` | `docs/examples/phase6/round-74.round.json` | verbatim |
| `docs/examples/phase6/round-83.proof.json` | `docs/examples/phase6/round-83.proof.json` | verbatim |
| `docs/examples/phase6/round-83.round.json` | `docs/examples/phase6/round-83.round.json` | verbatim |
| `docs/examples/phase6/round-84.proof.json` | `docs/examples/phase6/round-84.proof.json` | verbatim |
| `docs/examples/phase6/round-84.round.json` | `docs/examples/phase6/round-84.round.json` | verbatim |
| `docs/examples/round-0.proof.json` | `docs/examples/round-0.proof.json` | verbatim |
| `docs/examples/round-0.round.json` | `docs/examples/round-0.round.json` | verbatim |
| `docs/examples/phase8b/round-245.proof.json` | `docs/examples/phase8b/round-245.proof.json` | verbatim |
| `docs/examples/phase8b/round-251.proof.json` | `docs/examples/phase8b/round-251.proof.json` | verbatim |

## Written new in Zecraft (not from Zeek)

`.gitignore`, `PROVENANCE.md`, `games/raffle/rules/README.md`, `games/raffle/web/README.md`.
## Removed in Phase A

| Zeek file | Why |
|---|---|
| `crates/zeek-engine/src/audit.rs` | Zeek-only (plan section 1). The `audit` and `payout-recover` engine subcommands went with it. |
| `crates/zeek-engine/src/campfire.rs` | Zeek-only (Campfire chat). `/v1/campfire` and its stream events removed. |
| `crates/zeek-engine/src/mempool.rs` | Zeek-only (mempool "landing" digs). The watcher, `/v1/board`'s `landing` field and the `Landing` event removed. |
| `crates/zeek-engine/src/share.rs` | Zeek-only (share cards). `/share/*` routes, `ZECRAFT_APP_URL`, `ZECRAFT_PUBLIC_URL` and the `resvg` dependency removed. |
| `crates/zeek-engine/tests/landing.rs` | Tests of the removed mempool module. |
| `crates/zeek-verify/src/replay.rs` | Zeek-only (web app Replay). The `replay` CLI subcommand removed. |
| `crates/zeek-verify/tests/offline.rs` | Needs `docs/examples/phase8b/testnet-config.json`, which was withheld (below). |

## Deliberately not copied

- `docs/examples/phase8b/testnet-config.json`: Zeek's public game config, but it carries the
  Zeek testnet vault's UIVK (a viewing key), so it was withheld under the no-key-material rule.
- `deploy/railway/keyholder-{1,2,3}.json` were copied, but the Zeek keyholder and coordinator
  ed25519 identities in them were replaced with `REPLACE_AFTER_DKG_*` placeholders.
- Everything untracked or ignored in Zeek: `.secrets/`, `data/`, `.env`, `web/`, `target/`,
  `node_modules/`. Also not copied: `config/` (vault and keyholder configs), `scripts/`,
  `apps/`, `packages/`, `crates/zeek-pocket`, the rest of `docs/`.
