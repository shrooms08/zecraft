//! Buying raffle tickets.
//!
//! The memo and the ZIP 321 request come from `zecraft-core` and
//! `zecraft-raffle`; the payment goes through the same Ironwood PCZT pipeline
//! as the vault (propose, prove, sign, extract, broadcast), signed with the
//! player's own spend authorizing key instead of FROST.
//!
//! # Payout receivers
//!
//! By default every purchase seals a **fresh diversified address** of the
//! player's wallet, taken from the wallet's own next-address allocator,
//! which persists each index as an exposed address in its database. No
//! receiver is ever reused across purchases, so the keyholders, who open the
//! sealed receivers of the payouts they sign, cannot link one player's
//! entries to each other. Detection of a payout does not depend on the
//! recorded index: the wallet trial-decrypts every output with its incoming
//! viewing key, which covers all diversifiers. `--payout-address <addr>`
//! seals any other unified address with an Orchard receiver instead (for
//! example one from another wallet, with `--uri-only`).

use crate::NETWORK;
use orchard::circuit::{ProvingKey, VerifyingKey};
use orchard::keys::SpendAuthorizingKey;
use orchard::ValuePool;
use pczt::roles::signer::Signer;
use pczt::Pczt;
use rand_09::TryRngCore;
use serde::Deserialize;
use zcash_client_backend::data_api::WalletWrite;
use zcash_keys::keys::{UnifiedAddressRequest, UnifiedSpendingKey};
use zcash_protocol::consensus::BlockHeight;
use zecraft_core::entry::{entry_request_uri, EntryMemo, PayoutReceiver, SealingPublicKey};
use zecraft_core::params::{ChainParams, Network};
use zecraft_raffle::{RaffleParams, Tickets, FORMAT};
use zecraft_vault::lightwalletd::Client;
use zecraft_vault::send::{self, PreparedPczt};
use zecraft_vault::wallet::{self, Db};
use zecraft_vault::VaultError;

use crate::wallet::{Player, PlayerError};

/// Blocks that must remain in the window for a purchase to be sent.
pub const SAFETY_MARGIN_BLOCKS: u32 = 3;

/// The subset of `GET /v1/config` the player needs.
#[derive(Clone, Debug, Deserialize)]
pub struct EngineConfig {
    pub chain: ChainParams,
    pub game: RaffleParams,
    /// Withheld while the engine's entries are closed.
    pub vault_address: Option<String>,
    pub sealing_public_key: Option<SealingPublicKey>,
}

/// The subset of `GET /v1/board` the player needs.
#[derive(Clone, Debug, Deserialize)]
pub struct EngineBoard {
    pub tip_height: u32,
    pub round: u32,
    pub blocks_remaining: u32,
    pub status: String,
}

/// Fetches config and board from the engine.
pub async fn fetch_engine(url: &str) -> Result<(EngineConfig, EngineBoard), PlayerError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| PlayerError::Engine(e.to_string()))?;
    let base = url.trim_end_matches('/');
    let config: EngineConfig = client
        .get(format!("{base}/v1/config"))
        .send()
        .await
        .map_err(|e| PlayerError::Engine(e.to_string()))?
        .error_for_status()
        .map_err(|e| PlayerError::Engine(e.to_string()))?
        .json()
        .await
        .map_err(|e| PlayerError::EngineResponse(e.to_string()))?;
    let board: EngineBoard = client
        .get(format!("{base}/v1/board"))
        .send()
        .await
        .map_err(|e| PlayerError::Engine(e.to_string()))?
        .error_for_status()
        .map_err(|e| PlayerError::Engine(e.to_string()))?
        .json()
        .await
        .map_err(|e| PlayerError::EngineResponse(e.to_string()))?;
    Ok((config, board))
}

/// Which round the memo names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoundChoice {
    /// The board's current round.
    Auto,
    /// An explicit round (may differ from the board's: the entry is refunded).
    Explicit(u32),
}

/// Decides the memo round and enforces the safety margin.
pub fn choose_round(board: &EngineBoard, choice: RoundChoice) -> Result<u32, PlayerError> {
    if board.blocks_remaining < SAFETY_MARGIN_BLOCKS + 1 {
        return Err(PlayerError::ClosingTooSoon {
            round: board.round,
            blocks_remaining: board.blocks_remaining,
            margin: SAFETY_MARGIN_BLOCKS,
        });
    }
    Ok(match choice {
        RoundChoice::Auto => board.round,
        RoundChoice::Explicit(round) => round,
    })
}

/// Where the winnings or the refund of a purchase go.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PayoutAddress {
    /// A fresh diversified address of the player's own wallet.
    Fresh,
    /// Any unified address with an Orchard receiver.
    Given(String),
}

impl std::str::FromStr for PayoutAddress {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(if s == "fresh" {
            PayoutAddress::Fresh
        } else {
            PayoutAddress::Given(s.trim().to_owned())
        })
    }
}

/// What the player asked for.
#[derive(Clone, Debug)]
pub struct BuyRequest {
    pub choice: RoundChoice,
    pub tickets: u16,
    pub payout: PayoutAddress,
    /// Carry the memo as ZIP 302 text (`zrfl1:...`) instead of binary.
    pub text_memo: bool,
}

/// A ticket purchase, ready to pay.
pub struct Purchase {
    /// The ZIP 321 request any wallet can pay.
    pub uri: String,
    pub round: u32,
    pub tickets: u16,
    pub amount_zat: u64,
    /// Diversifier index of the fresh payout receiver (player-local
    /// bookkeeping; the receiver itself is never shown), when one was used.
    pub diversifier_index: Option<u128>,
}

/// Builds the sealed `ZRFL` memo and its ZIP 321 request. A fresh payout
/// address needs the player's wallet (`db`, `player`).
pub fn prepare_purchase(
    config: &EngineConfig,
    board: &EngineBoard,
    request: &BuyRequest,
    wallet: Option<(&Player, &mut Db)>,
) -> Result<Purchase, PlayerError> {
    if config.chain.network != Network::Test {
        return Err(PlayerError::Game("this tool plays on testnet only".into()));
    }
    let (Some(vault), Some(sealing_key)) = (&config.vault_address, &config.sealing_public_key)
    else {
        return Err(PlayerError::Game(
            "the engine's entries are closed (it withholds the vault address)".into(),
        ));
    };
    if request.tickets == 0 {
        return Err(PlayerError::Game("buy at least one ticket".into()));
    }
    let round = choose_round(board, request.choice)?;
    let amount_zat = config
        .game
        .price_of(Tickets(request.tickets))
        .ok_or_else(|| PlayerError::Game("ticket price overflows".into()))?;
    let (receiver, diversifier_index) = match &request.payout {
        PayoutAddress::Given(address) => (
            PayoutReceiver::from_unified_address(Network::Test, address)
                .map_err(|e| PlayerError::Game(format!("payout address: {e}")))?,
            None,
        ),
        PayoutAddress::Fresh => {
            let (player, db) = wallet.ok_or_else(|| {
                PlayerError::Game("a fresh payout address needs the player's wallet".into())
            })?;
            let account = player
                .find_account(db)?
                .ok_or_else(|| PlayerError::PlayerMissing(player.name().to_owned()))?;
            let (address, index) = db
                .get_next_available_address(account, UnifiedAddressRequest::ORCHARD)
                .map_err(|e| PlayerError::Keys(format!("next payout address: {e}")))?
                .ok_or_else(|| {
                    PlayerError::Keys("the wallet has no account for this player".into())
                })?;
            let mut index_bytes = [0u8; 16];
            index_bytes[..11].copy_from_slice(index.as_bytes());
            (
                PayoutReceiver::from_unified_address(Network::Test, &address.encode(&NETWORK))
                    .map_err(|e| PlayerError::Game(e.to_string()))?,
                Some(u128::from_le_bytes(index_bytes)),
            )
        }
    };
    let mut rng = rand_09::rngs::OsRng.unwrap_err();
    let memo = EntryMemo::seal(
        &FORMAT,
        round,
        Tickets(request.tickets),
        &receiver,
        sealing_key,
        &mut rng,
    )
    .map_err(|e| PlayerError::Game(e.to_string()))?;
    let bytes = if request.text_memo {
        memo.encode_text(&FORMAT).into_bytes()
    } else {
        memo.encode_unpadded(&FORMAT)
    };
    let uri = entry_request_uri(Network::Test, vault, amount_zat, &bytes)
        .map_err(|e| PlayerError::Game(e.to_string()))?;
    Ok(Purchase {
        uri,
        round,
        tickets: request.tickets,
        amount_zat,
        diversifier_index,
    })
}

/// What a plain payment produced.
pub struct PaymentOutcome {
    pub txid: String,
    pub payments: usize,
    pub total_zat: u64,
    pub fee_zat: u64,
}

/// Pays a ZIP 321 request exactly as an external wallet would: every payment
/// with its amount and its memo bytes as given (text memos stay text).
pub async fn pay_request(
    player: &Player,
    db: &mut Db,
    client: &mut Client,
    request: zip321::TransactionRequest,
) -> Result<PaymentOutcome, PlayerError> {
    let payments = request.payments().len();
    let total_zat = request
        .total()
        .map_err(|e| PlayerError::Game(format!("request total: {e:?}")))?
        .ok_or_else(|| PlayerError::Game("every payment in the request needs an amount".into()))?
        .into_u64();
    let usk = player.spending_key()?;
    let account = player
        .find_account(db)?
        .ok_or_else(|| PlayerError::PlayerMissing(player.name().to_owned()))?;
    wallet::sync(db, client).await?;
    let report = wallet::balance(db, account)?;
    let expected_target = BlockHeight::from_u32(report.chain_tip) + 1;
    let circuit = send::circuit_version_for(&NETWORK, expected_target)?;
    tracing::info!("building Orchard proving key for {circuit:?}");
    let proving_key = ProvingKey::build(circuit);
    let verifying_key = VerifyingKey::build(circuit);
    let PreparedPczt {
        pczt,
        target_height,
        fee,
    } = send::prepare_pczt(db, account, request, &report)?;
    if send::circuit_version_for(&NETWORK, target_height)? != circuit {
        return Err(VaultError::CircuitMismatch {
            built: format!("{circuit:?}"),
            required: format!("{:?}", send::circuit_version_for(&NETWORK, target_height)?),
        }
        .into());
    }
    let pczt = send::prove(pczt, &proving_key)?;
    let pczt = sign_with_key(pczt, &usk)?;
    let transaction =
        send::extract_broadcast_store(db, client, pczt, &verifying_key, target_height).await?;
    Ok(PaymentOutcome {
        txid: transaction.txid().to_string(),
        payments,
        total_zat,
        fee_zat: fee.into_u64(),
    })
}

/// Signs every pending spend with the player's Orchard spend authorizing key.
fn sign_with_key(pczt: Pczt, usk: &UnifiedSpendingKey) -> Result<Pczt, PlayerError> {
    let ask = SpendAuthorizingKey::from(usk.orchard());
    let pending = send::pending_spends(&pczt)?;
    let mut signer =
        Signer::new(pczt).map_err(|e| VaultError::Pczt(format!("signer init: {e:?}")))?;
    for spend in &pending {
        match spend.pool {
            ValuePool::Ironwood => signer.sign_ironwood(spend.index, &ask),
            ValuePool::Orchard => signer.sign_orchard(spend.index, &ask),
        }
        .map_err(|e| VaultError::Pczt(format!("sign action {}: {e:?}", spend.index)))?;
    }
    Ok(signer.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use zecraft_core::entry::generate_sealing_keypair;

    fn board(blocks_remaining: u32) -> EngineBoard {
        EngineBoard {
            tip_height: 100,
            round: 4,
            blocks_remaining,
            status: "open".into(),
        }
    }

    #[test]
    fn safety_margin_is_enforced() {
        assert_eq!(choose_round(&board(10), RoundChoice::Auto).expect("ok"), 4);
        assert_eq!(
            choose_round(&board(4), RoundChoice::Explicit(9)).expect("ok"),
            9
        );
        assert!(matches!(
            choose_round(&board(3), RoundChoice::Auto),
            Err(PlayerError::ClosingTooSoon {
                round: 4,
                blocks_remaining: 3,
                margin: 3
            })
        ));
        assert!(matches!(
            choose_round(&board(0), RoundChoice::Auto),
            Err(PlayerError::ClosingTooSoon { .. })
        ));
    }

    #[test]
    fn a_purchase_is_a_zip321_request_carrying_a_sealed_zrfl_memo() {
        use rand_09::SeedableRng;
        let (sk, pk) = generate_sealing_keypair(&mut rand_09::rngs::StdRng::seed_from_u64(3));
        let vault = PayoutReceiver {
            network: Network::Test,
            orchard: [5; 43],
        }
        .to_unified_address()
        .expect("vault");
        let payout = PayoutReceiver {
            network: Network::Test,
            orchard: [6; 43],
        };
        let config = EngineConfig {
            chain: ChainParams::testnet(1),
            game: RaffleParams::default(),
            vault_address: Some(vault.clone()),
            sealing_public_key: Some(pk),
        };
        for text_memo in [false, true] {
            let request = BuyRequest {
                choice: RoundChoice::Auto,
                tickets: 3,
                payout: PayoutAddress::Given(payout.to_unified_address().expect("ua")),
                text_memo,
            };
            let purchase = prepare_purchase(&config, &board(10), &request, None).expect("purchase");
            assert_eq!(
                (purchase.round, purchase.tickets, purchase.amount_zat),
                (4, 3, 300_000)
            );
            let parsed = zip321::TransactionRequest::from_uri(&purchase.uri).expect("zip321");
            let p = parsed.payments().get(&0).expect("payment");
            assert_eq!(p.amount().map(u64::from), Some(300_000));
            assert_eq!(p.recipient_address().encode(), vault);
            let memo =
                EntryMemo::<Tickets>::decode_any(&FORMAT, p.memo().expect("memo").as_slice())
                    .expect("a ZRFL memo");
            assert_eq!((memo.round, memo.payload), (4, Tickets(3)));
            assert_eq!(
                memo.open(&FORMAT, Network::Test, &sk).expect("open"),
                payout
            );
        }
        let closed = EngineConfig {
            vault_address: None,
            ..config
        };
        let request = BuyRequest {
            choice: RoundChoice::Auto,
            tickets: 1,
            payout: PayoutAddress::Fresh,
            text_memo: false,
        };
        assert!(prepare_purchase(&closed, &board(10), &request, None).is_err());
    }
}
