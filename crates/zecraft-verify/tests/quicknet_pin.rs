//! The beacon must come from drand quicknet: an operator that publishes its
//! own BLS key in the config (and the same key in the bundle) can sign the
//! beacon itself and so pick the winner. Before the pin, such a bundle
//! passed check 1, because the signature does verify under the key the
//! bundle names.
//!
//! The forged bundle here is the live round 0 (docs/examples, identical to
//! the engine's `/v1/rounds/0/proof`) with its beacon re-signed by another
//! key.

use std::collections::BTreeMap;

use bls12_381::hash_to_curve::{ExpandMsgXmd, HashToCurve};
use bls12_381::{G1Affine, G1Projective, G2Affine, G2Projective, Scalar};
use drand_verify::{G2PubkeyRfc, Pubkey};
use sha2_09::{Digest, Sha256};
use zcash_client_backend::proto::compact_formats::CompactBlock;
use zecraft_core::draw::{verify_beacon, BeaconError, Randomness};
use zecraft_core::params::{ParamsError, QUICKNET_CHAIN_HASH, QUICKNET_PUBLIC_KEY};
use zecraft_core::proof::ProofBundle;
use zecraft_raffle::{Raffle, RaffleParams};
use zecraft_verify::checks::{verify_round, PublicConfig, RoundInput};
use zecraft_verify::{CheckId, CheckResult, CheckStatus};

const ROUND_0: &str = include_str!("../../../docs/examples/raffle-round-0.proof.json");
const CONFIG: &str = include_str!("../../../config/raffle.testnet.json");
/// The RFC 9380 domain drand quicknet signs under (drand-verify `verify.rs`).
const DST_G1: &[u8] = b"BLS_SIG_BLS12381G1_XMD:SHA-256_SSWU_RO_NUL_";

/// An operator's own key, in drand quicknet's scheme: (public key, signer).
fn operator_key() -> ([u8; 96], impl Fn(u64) -> [u8; 48]) {
    let sk = Scalar::from(0x00c0_ffee_u64) * Scalar::from(0x0bad_5eed_u64);
    let pk = G2Affine::from(G2Projective::generator() * sk).to_compressed();
    let sign = move |round: u64| {
        let message = Sha256::digest(&round.to_be_bytes());
        let point =
            <G1Projective as HashToCurve<ExpandMsgXmd<Sha256>>>::hash_to_curve(message, DST_G1);
        G1Affine::from(point * sk).to_compressed()
    };
    (pk, sign)
}

/// Check 1 of `bundle` against `config`, with the chain data check 1 reads
/// (the round's first block and its time).
fn check_1(bundle: &ProofBundle<RaffleParams>, config: &PublicConfig<RaffleParams>) -> CheckResult {
    let blocks = zecraft_verify::chain::Blocks::new([CompactBlock {
        height: u64::from(bundle.window.start),
        hash: vec![0; 32],
        prev_hash: vec![0; 32],
        time: u32::try_from(bundle.start_block_time).expect("time fits"),
        ..CompactBlock::default()
    }]);
    let report = verify_round(&RoundInput {
        rules: &Raffle,
        config,
        bundle,
        blocks: &blocks,
        txs: &BTreeMap::new(),
        ivk: &config.ivk().expect("ivk"),
    });
    report
        .checks
        .into_iter()
        .find(|c| c.id == CheckId::Beacon)
        .expect("check 1 runs")
}

fn round_0() -> (ProofBundle<RaffleParams>, PublicConfig<RaffleParams>) {
    (
        serde_json::from_str(ROUND_0).expect("bundle"),
        serde_json::from_str(CONFIG).expect("config"),
    )
}

#[test]
fn the_real_round_0_passes_check_1() {
    let (bundle, config) = round_0();
    assert_eq!(bundle.chain.drand.chain_hash, QUICKNET_CHAIN_HASH);
    assert_eq!(bundle.chain.drand.public_key, QUICKNET_PUBLIC_KEY);
    let check = check_1(&bundle, &config);
    assert_eq!(check.status, CheckStatus::Pass, "{check:?}");
}

#[test]
fn a_beacon_signed_under_another_key_fails_check_1() {
    let (mut bundle, mut config) = round_0();
    let (pk, sign) = operator_key();
    let round = bundle.drand_round;
    let signature = sign(round);

    // The forgery is cryptographically sound: the signature verifies under
    // the operator's key exactly as quicknet's verify under quicknet's. This
    // is what check 1 used to accept.
    let key = G2PubkeyRfc::from_fixed(pk).expect("valid G2 point");
    assert!(key.verify(round, &[], &signature).expect("verify"));

    // The operator publishes its key in both the config and the bundle
    // (they must agree), and settles with the beacon it signed.
    let mut forged_chain = bundle.chain.drand.clone();
    forged_chain.public_key = hex::encode(pk);
    forged_chain.chain_hash = hex::encode(Sha256::digest(&pk));
    bundle.chain.drand = forged_chain.clone();
    config.chain.drand = forged_chain.clone();
    let beacon = bundle.beacon.as_mut().expect("round 0 was drawn");
    beacon.signature = hex::encode(signature);
    bundle.randomness = Some(Randomness(drand_verify::derive_randomness(&signature)));

    let check = check_1(&bundle, &config);
    assert_eq!(check.status, CheckStatus::Fail, "{check:?}");
    assert!(
        check.summary.contains("beacon key is not drand quicknet"),
        "{}",
        check.summary
    );

    // The same pin in zecraft-core: the beacon does not verify, and a game
    // config naming that key does not load (engine and keyholders refuse to
    // start).
    let beacon = bundle.beacon.as_ref().expect("beacon");
    let err = verify_beacon(&forged_chain, beacon).expect_err("not quicknet");
    assert_eq!(
        err,
        BeaconError::NotQuicknet {
            field: "chain hash"
        }
    );
    assert!(err.to_string().contains("beacon key is not drand quicknet"));
    assert_eq!(
        config.chain.validate(),
        Err(ParamsError::NotQuicknet {
            field: "chain hash"
        })
    );

    // Keeping quicknet's chain hash but swapping the key is caught too.
    let mut same_hash = forged_chain;
    same_hash.chain_hash = QUICKNET_CHAIN_HASH.to_owned();
    bundle.chain.drand = same_hash.clone();
    config.chain.drand = same_hash.clone();
    let check = check_1(&bundle, &config);
    assert_eq!(check.status, CheckStatus::Fail);
    assert!(check.summary.contains("beacon key is not drand quicknet"));
    assert_eq!(
        verify_beacon(&same_hash, bundle.beacon.as_ref().expect("beacon")),
        Err(BeaconError::NotQuicknet {
            field: "public key"
        })
    );
}

#[test]
fn moving_quicknets_genesis_or_period_fails_check_1() {
    // Same key, but a shifted clock: the round would bind to a drand round
    // that was already published.
    let (mut bundle, mut config) = round_0();
    bundle.chain.drand.genesis_time -= 3_600;
    config.chain.drand.genesis_time -= 3_600;
    let check = check_1(&bundle, &config);
    assert_eq!(check.status, CheckStatus::Fail);
    assert!(check.summary.contains("beacon key is not drand quicknet"));
    assert!(check.detail.iter().any(|d| d.contains("genesis time")));
}
