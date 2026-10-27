//! Funding a player from a transparent-only source, such as the testnet faucet.
//!
//! Some wallets (the testnet faucet among them) cannot pay an Orchard-only
//! unified address. They can pay the player's transparent address, which
//! `zecraft-player address <name> --transparent` prints: the BIP 44 external
//! address at index 0 of the player's own seed. [`shield`] then moves every
//! spendable UTXO of that address into the player's Ironwood balance with
//! one shielding transaction (transparent inputs, an Ironwood change output
//! to the player's internal address), through the same PCZT pipeline as a
//! dig: propose, prove, sign, finalize, extract, broadcast.

use orchard::circuit::{ProvingKey, VerifyingKey};
use orchard::keys::SpendAuthorizingKey;
use orchard::ValuePool;
use pczt::roles::signer::Signer;
use pczt::roles::spend_finalizer::SpendFinalizer;
use pczt::Pczt;
use transparent::address::TransparentAddress;
use transparent::keys::{IncomingViewingKey, NonHardenedChildIndex};
use zcash_client_backend::data_api::wallet::input_selection::{
    GreedyInputSelector, GreedyInputSelectorError,
};
use zcash_client_backend::data_api::wallet::{
    create_pczt_from_proposal, propose_shielding, ConfirmationsPolicy,
};
use zcash_client_backend::data_api::WalletCommitmentTrees;
use zcash_client_backend::fees::zip317::SingleOutputChangeStrategy;
use zcash_client_backend::fees::DustOutputPolicy;
use zcash_client_backend::wallet::OvkPolicy;
use zcash_keys::address::Address;
use zcash_keys::keys::UnifiedSpendingKey;
use zcash_primitives::transaction::builder::BundlePadding;
use zcash_primitives::transaction::fees::zip317;
use zcash_protocol::consensus::BlockHeight;
use zcash_protocol::value::Zatoshis;
use zcash_protocol::ShieldedPool;
use zecraft_vault::lightwalletd::Client;
use zecraft_vault::send;
use zecraft_vault::wallet::{self, Db};
use zecraft_vault::VaultError;

use crate::wallet::{Player, PlayerError};
use crate::NETWORK;

/// Shields nothing below this (the ZIP 317 fee of a one-input shielding
/// transaction is 0.0001 ZEC, so smaller UTXOs would be eaten by the fee).
const SHIELDING_THRESHOLD_ZAT: u64 = 100_000;

/// The player's transparent address: BIP 44 external index 0 of its seed.
pub fn transparent_address(usk: &UnifiedSpendingKey) -> Result<TransparentAddress, PlayerError> {
    let ivk = usk
        .transparent()
        .to_account_pubkey()
        .derive_external_ivk()
        .map_err(|e| PlayerError::Keys(format!("transparent viewing key: {e:?}")))?;
    ivk.derive_address(NonHardenedChildIndex::ZERO)
        .map_err(|e| PlayerError::Keys(format!("transparent address: {e:?}")))
}

/// The encoded transparent address (`tm...` on testnet).
pub fn encode_transparent(address: TransparentAddress) -> String {
    Address::from(address).encode(&NETWORK)
}

/// What [`shield`] did.
pub struct ShieldOutcome {
    pub txid: String,
    pub inputs: usize,
    pub shielded_zat: u64,
    pub fee_zat: u64,
}

/// Syncs, then shields every spendable UTXO of the player's transparent
/// address into its Ironwood balance.
pub async fn shield(
    player: &Player,
    db: &mut Db,
    client: &mut Client,
) -> Result<ShieldOutcome, PlayerError> {
    let usk = player.spending_key()?;
    let account = player
        .find_account(db)?
        .ok_or_else(|| PlayerError::PlayerMissing(player.name().to_owned()))?;
    // `sync` refreshes the UTXOs of every transparent receiver of the account.
    wallet::sync(db, client).await?;
    let report = wallet::balance(db, account)?;
    let from = transparent_address(&usk)?;

    let expected_target = BlockHeight::from_u32(report.chain_tip) + 1;
    let circuit = send::circuit_version_for(&NETWORK, expected_target)?;
    tracing::info!("building Orchard proving key for {circuit:?}");
    let proving_key = ProvingKey::build(circuit);
    let verifying_key = VerifyingKey::build(circuit);

    // Change from a shielding transaction is its shielded output: Ironwood.
    let change = SingleOutputChangeStrategy::<_, Db>::new(
        zip317::FeeRule::standard(),
        None,
        ShieldedPool::Ironwood,
        DustOutputPolicy::default(),
    );
    let proposal = propose_shielding::<_, _, _, _, <Db as WalletCommitmentTrees>::Error>(
        db,
        &NETWORK,
        &GreedyInputSelector::<Db>::new(),
        &change,
        Zatoshis::const_from_u64(SHIELDING_THRESHOLD_ZAT),
        &[from],
        account,
        ConfirmationsPolicy::default(),
        Default::default(),
        None,
    )
    .map_err(|e| {
        PlayerError::Game(format!(
            "nothing to shield at {}: {e}",
            encode_transparent(from)
        ))
    })?;
    let step = proposal.steps().first();
    let inputs = step.transparent_inputs().len();
    let input_total: u64 = step
        .transparent_inputs()
        .iter()
        .map(|i| i.txout().value().into_u64())
        .sum();
    let fee_zat = step.balance().fee_required().into_u64();
    let target_height = BlockHeight::from(proposal.min_target_height());
    if send::circuit_version_for(&NETWORK, target_height)? != circuit {
        return Err(VaultError::CircuitMismatch {
            built: format!("{circuit:?}"),
            required: format!("{:?}", send::circuit_version_for(&NETWORK, target_height)?),
        }
        .into());
    }

    let pczt = create_pczt_from_proposal::<_, _, GreedyInputSelectorError, _, zip317::FeeError, _>(
        db,
        &NETWORK,
        account,
        OvkPolicy::Sender,
        &proposal,
        None,
        BundlePadding::DEFAULT,
    )
    .map_err(|e| VaultError::Build(e.to_string()))?;
    let pczt = send::prove(pczt, &proving_key)?;
    let pczt = sign(pczt, &usk)?;
    let pczt = SpendFinalizer::new(pczt)
        .finalize_spends()
        .map_err(|e| VaultError::Pczt(format!("finalize transparent spends: {e:?}")))?;
    let transaction =
        send::extract_broadcast_store(db, client, pczt, &verifying_key, target_height).await?;
    Ok(ShieldOutcome {
        txid: transaction.txid().to_string(),
        inputs,
        shielded_zat: input_total.saturating_sub(fee_zat),
        fee_zat,
    })
}

/// Signs every transparent input with the index-0 external key (the only
/// address [`shield`] spends from) and any Orchard-protocol spend the
/// builder added with the player's spend authorizing key.
fn sign(pczt: Pczt, usk: &UnifiedSpendingKey) -> Result<Pczt, PlayerError> {
    let transparent_inputs = pczt.transparent().inputs().len();
    let pending = match send::pending_spends(&pczt) {
        Ok(pending) => pending,
        Err(VaultError::NothingToSign) => Vec::new(),
        Err(e) => return Err(e.into()),
    };
    let sk = usk
        .transparent()
        .derive_external_secret_key(NonHardenedChildIndex::ZERO)
        .map_err(|e| PlayerError::Keys(format!("transparent key: {e:?}")))?;
    let ask = SpendAuthorizingKey::from(usk.orchard());
    let mut signer =
        Signer::new(pczt).map_err(|e| VaultError::Pczt(format!("signer init: {e:?}")))?;
    for index in 0..transparent_inputs {
        signer
            .sign_transparent(index, &sk)
            .map_err(|e| VaultError::Pczt(format!("sign transparent input {index}: {e:?}")))?;
    }
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

    #[test]
    fn transparent_address_is_deterministic_and_testnet() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let player = Player::new(tmp.path(), "faucet").expect("name");
        player.init().expect("init");
        let usk = player.spending_key().expect("usk");
        let a = encode_transparent(transparent_address(&usk).expect("address"));
        let b = encode_transparent(transparent_address(&usk).expect("again"));
        assert_eq!(a, b);
        assert!(a.starts_with("tm"), "{a}");
    }
}
