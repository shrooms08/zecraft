//! The keyholder's payout verification against real PCZTs built offline:
//! an honest payout for a settlement this node computed is approved and
//! signable; every tampering (redirected output, changed amount, extra or
//! missing output, missing or foreign input, needless float note, fee above
//! the collected fees, foreign vault, unknown round, already-signed round,
//! spending a kept note) is refused with a reason naming the exact check and
//! item. The game is the raffle.

use std::convert::Infallible;
use std::sync::OnceLock;

use orchard::circuit::{OrchardCircuitVersion, ProvingKey};
use orchard::keys::{FullViewingKey, Scope};
use orchard::note_encryption::IronwoodDomain;
use orchard::tree::{MerkleHashOrchard, MerklePath};
use orchard::{Anchor, Note};
use pczt::roles::creator::Creator;
use pczt::roles::io_finalizer::IoFinalizer;
use pczt::roles::verifier::Verifier;
use pczt::Pczt;
use rand_09::rngs::StdRng;
use rand_09::SeedableRng;
use rand_core::OsRng;
use shardtree::store::memory::MemoryShardStore;
use shardtree::ShardTree;
use zcash_note_encryption::try_note_decryption;
use zcash_primitives::transaction::builder::{BuildConfig, Builder, BundlePadding, PcztResult};
use zcash_primitives::transaction::fees::zip317;
use zcash_protocol::consensus::TEST_NETWORK;
use zcash_protocol::memo::MemoBytes;
use zcash_protocol::value::Zatoshis;
use zecraft_core::draw::{Randomness, TimingVerdict};
use zecraft_core::entry::seal::generate_sealing_keypair;
use zecraft_core::entry::{EntryMemo, PayoutReceiver, SealedReceiver};
use zecraft_core::params::{ChainParams, Network};
use zecraft_core::rules::{settle_checked, PayoutKind, Settlement};
use zecraft_engine::config::GameConfig;
use zecraft_engine::db::{BlockRow, Db, NoteRow, PlanStatus, Pool, SettlementRow};
use zecraft_engine::processor::Processor;
use zecraft_keyholder::db::NodeDb;
use zecraft_keyholder::verify::{verify_payout, zat, VerifyContext};
use zecraft_keyholder::KeyholderError;
use zecraft_raffle::{Raffle, RaffleParams, Tickets, FORMAT};
use zecraft_vault::frost;
use zecraft_vault::keys;

static PROVING_KEY: OnceLock<ProvingKey> = OnceLock::new();

fn proving_key() -> &'static ProvingKey {
    PROVING_KEY.get_or_init(|| ProvingKey::build(OrchardCircuitVersion::PostNu6_3))
}

/// A post-NU7 testnet height above the stored chain tip (the genesis block),
/// so the PCZT is V6 on the NU7 branch and not expired.
const TARGET: u32 = 4_500_000;
const GENESIS: u32 = 4_499_000;

/// A vault: FROST group key, viewing key, sealing key.
struct Vault {
    fvk: FullViewingKey,
    sealing: zecraft_core::entry::seal::SealingPrivateKey,
    sealing_pk: zecraft_core::entry::seal::SealingPublicKey,
}

fn vault(seed: u64) -> Vault {
    let out = frost::dealer_keygen(2, 3, OsRng).expect("keygen");
    let group = frost::group_key_bytes(&out.public_key_package).expect("gk");
    let fvk = keys::derive_full_viewing_key_from_seed(&group, [seed as u8; 32]).expect("fvk");
    let (sealing, sealing_pk) = generate_sealing_keypair(&mut StdRng::seed_from_u64(seed));
    Vault {
        fvk,
        sealing,
        sealing_pk,
    }
}

/// A note the vault holds, witnessed in a tree checkpointed at GENESIS.
struct VaultNote {
    note: Note,
    path: MerklePath,
    cmx: [u8; 32],
}

/// Ironwood notes to the vault in one bundle (`scope`, value, memo each),
/// all witnessed under one anchor.
fn vault_notes(
    fvk: &FullViewingKey,
    specs: &[(Scope, u64, [u8; 512])],
) -> (Vec<VaultNote>, Anchor) {
    let bundle_version = orchard::bundle::BundleVersion::ironwood_v3();
    let mut builder = orchard::builder::Builder::new(
        orchard::builder::BundleType::DEFAULT,
        bundle_version,
        bundle_version.default_flags(),
        Anchor::empty_tree(),
    )
    .expect("builder");
    for (scope, value, memo) in specs {
        builder
            .add_output(
                None,
                fvk.address_at(0u32, *scope),
                orchard::value::NoteValue::from_raw(*value),
                *memo,
            )
            .expect("output");
    }
    let (bundle, meta) = builder.build::<i64>(OsRng).expect("build").expect("bundle");
    let mut tree =
        ShardTree::<_, 32, 16>::new(MemoryShardStore::<MerkleHashOrchard, u32>::empty(), 100);
    let mut decrypted = Vec::with_capacity(specs.len());
    for (i, (scope, _, _)) in specs.iter().enumerate() {
        let action = &bundle.actions()[meta.output_action_index(i).expect("index")];
        let ivk = fvk.to_ivk(*scope).prepare();
        let (note, _, _) = try_note_decryption(&IronwoodDomain::for_action(action), &ivk, action)
            .expect("decrypt");
        let cmx: orchard::note::ExtractedNoteCommitment = note.commitment().into();
        tree.append(
            MerkleHashOrchard::from_cmx(&cmx),
            incrementalmerkletree::Retention::Marked,
        )
        .expect("append");
        decrypted.push((note, cmx));
    }
    tree.checkpoint(GENESIS).expect("checkpoint");
    let mut notes = Vec::with_capacity(specs.len());
    let mut anchor = None;
    for (i, (note, cmx)) in decrypted.into_iter().enumerate() {
        let path = tree
            .witness_at_checkpoint_depth((i as u64).into(), 0)
            .expect("witness")
            .expect("exists");
        let root: Anchor = path.root(MerkleHashOrchard::from_cmx(&cmx)).into();
        anchor = Some(root);
        notes.push(VaultNote {
            note,
            path: path.into(),
            cmx: cmx.to_bytes(),
        });
    }
    (notes, anchor.expect("at least one note"))
}

/// Builds a proven payout PCZT spending `spends` to `outputs` plus change.
fn payout_pczt(
    v: &Vault,
    spends: &[&VaultNote],
    anchor: Anchor,
    outputs: &[(orchard::Address, u64)],
) -> Pczt {
    let mut builder = Builder::new(
        TEST_NETWORK,
        TARGET.into(),
        BuildConfig::Standard {
            sapling_anchor: None,
            orchard_anchor: None,
            ironwood_anchor: Some(anchor),
            orchard_padding: BundlePadding::DEFAULT,
            ironwood_padding: BundlePadding::DEFAULT,
        },
    );
    let mut spent = 0u64;
    for spend in spends {
        builder
            .add_ironwood_spend::<zip317::FeeError>(v.fvk.clone(), spend.note, spend.path.clone())
            .expect("spend");
        spent += spend.note.value().inner();
    }
    let mut paid = 0u64;
    for (address, value) in outputs {
        builder
            .add_ironwood_output::<zip317::FeeError>(
                Some(v.fvk.to_ovk(Scope::External)),
                *address,
                Zatoshis::const_from_u64(*value),
                MemoBytes::from_bytes(b"Zecraft round 0: win").expect("memo"),
            )
            .expect("output");
        paid += value;
    }
    // ZIP 317: max(spends, outputs incl. change) actions, at least two.
    let actions = spends.len().max(outputs.len() + 1).max(2) as u64;
    let fee = 5_000 * actions;
    let change = spent
        .checked_sub(paid + fee)
        .filter(|c| *c > 0)
        .expect("test shapes leave positive change");
    builder
        .add_ironwood_output::<zip317::FeeError>(
            Some(v.fvk.to_ovk(Scope::Internal)),
            v.fvk.address_at(0u32, Scope::Internal),
            Zatoshis::const_from_u64(change),
            MemoBytes::empty(),
        )
        .expect("change");
    let PcztResult { pczt_parts, .. } = builder
        .build_for_pczt(OsRng, &zip317::FeeRule::standard())
        .expect("pczt");
    let pczt = Creator::build_from_parts(pczt_parts).expect("creator");
    let pczt = IoFinalizer::new(pczt).finalize_io().expect("io");
    zecraft_vault::send::prove(pczt, proving_key()).expect("prove")
}

/// The Ironwood action index whose output pays `address` (the builder
/// shuffles actions, so tests look indices up).
fn output_index_of(pczt: &Pczt, address: &orchard::Address) -> usize {
    let mut found = None;
    Verifier::new(pczt.clone())
        .with_ironwood::<Infallible, _>(|bundle| {
            for (i, action) in bundle.actions().iter().enumerate() {
                if action.output().recipient().as_ref() == Some(address) {
                    found = Some(i);
                }
            }
            Ok(())
        })
        .expect("ironwood bundle");
    found.expect("output present")
}

/// The Ironwood action index whose spend is the note with commitment `cmx`.
fn spend_index_of(pczt: &Pczt, cmx: &[u8; 32]) -> usize {
    let mut found = None;
    Verifier::new(pczt.clone())
        .with_ironwood::<Infallible, _>(|bundle| {
            for (i, action) in bundle.actions().iter().enumerate() {
                let spend = action.spend();
                if let (Some(r), Some(v), Some(rho), Some(rseed)) =
                    (spend.recipient(), spend.value(), spend.rho(), spend.rseed())
                {
                    let note: Option<Note> = Option::from(Note::from_parts(
                        *r,
                        *v,
                        *rho,
                        *rseed,
                        *spend.note_version(),
                    ));
                    if let Some(note) = note {
                        let c: orchard::note::ExtractedNoteCommitment = note.commitment().into();
                        if c.to_bytes() == *cmx {
                            found = Some(i);
                        }
                    }
                }
            }
            Ok(())
        })
        .expect("ironwood bundle");
    found.expect("spend present")
}

/// A player: an address and a sealed raffle memo for `round`/`tickets`.
fn player(v: &Vault, seed: u8, round: u32, tickets: u16) -> (orchard::Address, EntryMemo<Tickets>) {
    let sk = loop {
        let mut bytes = [seed; 32];
        bytes[0] = bytes[0].wrapping_add(1);
        if let Some(sk) = Option::<orchard::keys::SpendingKey>::from(
            orchard::keys::SpendingKey::from_bytes(bytes),
        ) {
            break sk;
        }
    };
    let address = FullViewingKey::from(&sk).address_at(0u32, Scope::External);
    let receiver = PayoutReceiver {
        network: Network::Test,
        orchard: address.to_raw_address_bytes(),
    };
    let memo = EntryMemo::seal(
        &FORMAT,
        round,
        Tickets(tickets),
        &receiver,
        &v.sealing_pk,
        &mut StdRng::seed_from_u64(u64::from(seed)),
    )
    .expect("seal");
    (address, memo)
}

/// A note to store: value, memo, the vault note that carries it.
struct Entry<'a> {
    value: u64,
    memo: [u8; 512],
    note: &'a VaultNote,
    height: u32,
}

/// The randomness every test round is drawn with.
const DRAW: Randomness = Randomness([7; 32]);

/// Stores `entries` as the node's notes and settles round 0 from them, as
/// the node's own processor would.
fn settle_round(
    db: &Db,
    processor: &Processor<Raffle>,
    entries: &[Entry<'_>],
    carry_in: u64,
) -> Settlement {
    db.insert_block(&BlockRow {
        height: GENESIS,
        hash: vec![1; 32],
        prev_hash: vec![0; 32],
        time: 1,
    })
    .expect("block");
    for (i, entry) in entries.iter().enumerate() {
        let row = NoteRow {
            txid: [0xaa; 32],
            action_index: i as u16,
            pool: Pool::Ironwood,
            height: entry.height,
            block_time: 1,
            value: entry.value,
            memo: entry.memo.to_vec(),
            cmx: Some(entry.note.cmx),
        };
        assert!(db.insert_note(&row).expect("note"));
    }
    let classified = processor.classified(db, 0, false).expect("classify");
    let settlement = settle_checked(
        processor.rules(),
        &processor.config().game,
        &classified,
        carry_in,
        Some(&DRAW),
    )
    .expect("settle")
    .erase()
    .expect("erase");
    db.insert_settlement(&SettlementRow {
        round: 0,
        settled_at_height: GENESIS,
        start_block_time: 1,
        post_close_block_times: vec![2; 11],
        drand_round: 1,
        beacon_time: 3,
        timing: TimingVerdict::Safe {
            median_time_past: 2,
            beacon_time: 3,
            margin_secs: 1,
        },
        voided: false,
        settlement: settlement.clone(),
        status: PlanStatus::AwaitingSignatures,
    })
    .expect("settlement");
    settlement
}

fn processor(v: &Vault) -> Processor<Raffle> {
    let receiver = PayoutReceiver {
        network: Network::Test,
        orchard: v
            .fvk
            .address_at(0u32, Scope::External)
            .to_raw_address_bytes(),
    };
    let ufvk = keys::unified_full_viewing_key(&v.fvk).expect("ufvk");
    Processor::new(
        Raffle,
        GameConfig {
            chain: ChainParams::testnet(GENESIS),
            game: RaffleParams::default(),
            vault_address: receiver.to_unified_address().expect("address"),
            sealing_public_key: v.sealing_pk.clone(),
            uivk: keys::encode_uivk(
                &keys::unified_incoming_viewing_key(&ufvk),
                &zecraft_vault::Params::TestNetwork,
            ),
            initial_carry_zat: 0,
        },
    )
}

fn refusal(result: Result<zecraft_keyholder::verify::Approved, KeyholderError>) -> String {
    match result {
        Err(KeyholderError::Refused(reason)) => reason,
        Err(other) => panic!("expected a refusal, got {other}"),
        Ok(_) => panic!("expected a refusal, got approval"),
    }
}

#[test]
fn honest_payout_is_approved_and_tampered_ones_are_refused() {
    let v = vault(1);
    let processor = processor(&v);
    let (alice, alice_memo) = player(&v, 10, 0, 3);
    let (bob, bob_memo) = player(&v, 20, 0, 2);
    let (dave, dave_memo) = player(&v, 50, 9, 1); // names round 9: refunded
    let (mallory, _) = player(&v, 30, 0, 1);
    let (_carol, carol_memo) = player(&v, 40, 1, 1);
    // Vault notes: alice's, bob's and dave's entries, carol's round-1 entry,
    // two float notes.
    let enc = |m: &EntryMemo<Tickets>| m.encode(&FORMAT);
    let (notes, anchor) = vault_notes(
        &v.fvk,
        &[
            (Scope::External, 300_000, enc(&alice_memo)),
            (Scope::External, 200_000, enc(&bob_memo)),
            (Scope::External, 100_000, enc(&dave_memo)),
            (Scope::External, 100_000, enc(&carol_memo)),
            (Scope::Internal, 50_000, [0xF6; 512]),
            (Scope::Internal, 1_200_000, [0xF6; 512]),
        ],
    );
    let (alice_note, bob_note, dave_note, carol_note, small_float, big_float) = (
        &notes[0], &notes[1], &notes[2], &notes[3], &notes[4], &notes[5],
    );
    let chain = Db::open_in_memory().expect("db");
    let node = NodeDb::open_in_memory().expect("node db");
    let entry = |value, memo, note, height| Entry {
        value,
        memo,
        note,
        height,
    };
    let settlement = settle_round(
        &chain,
        &processor,
        &[
            entry(300_000, enc(&alice_memo), alice_note, GENESIS + 1),
            entry(200_000, enc(&bob_memo), bob_note, GENESIS + 2),
            entry(100_000, enc(&dave_memo), dave_note, GENESIS + 3),
            entry(100_000, enc(&carol_memo), carol_note, GENESIS + 30), // round 1
        ],
        0,
    );
    assert_eq!(settlement.payouts.len(), 2, "the winner and dave's refund");
    assert_eq!(settlement.payouts[0].kind, PayoutKind::Win);
    let winner = if settlement.payouts[0].sealed == alice_memo.sealed_receiver {
        alice
    } else {
        assert_eq!(settlement.payouts[0].sealed, bob_memo.sealed_receiver);
        bob
    };
    let (win_net, refund_net) = (settlement.payouts[0].net, settlement.payouts[1].net);
    assert_eq!((win_net, refund_net), (490_000 - 5_000, 90_000));
    assert_eq!(settlement.fees, 25_000, "3 entry fees + 2 output fees");
    let ctx = VerifyContext {
        now: 2,
        processor: &processor,
        fvk: &v.fvk,
        sealing: &v.sealing,
        chain: &chain,
        node: &node,
    };
    let spent = [alice_note, bob_note, dave_note];
    let honest_outputs = [(winner, win_net), (dave, refund_net)];

    // Honest: every entry spent, the winner and the refund paid, change back.
    let honest = payout_pczt(&v, &spent, anchor, &honest_outputs);
    let honest_bytes = honest.serialize().expect("serialize");
    let approved = verify_payout(&ctx, &honest_bytes, &[0]).expect("approved");
    assert_eq!(approved.payouts, 2);
    assert_eq!(approved.distinct_receivers, 2);
    assert_eq!(approved.charged_inputs, 3);
    assert_eq!(approved.float_inputs, 0);
    assert_eq!(approved.rounds, vec![0]);
    assert_eq!(
        approved.pending.len(),
        3,
        "three real Ironwood spends to sign"
    );
    assert_eq!(
        approved.fee, 15_000,
        "3 actions x 5000 zat, within the 25,000 collected"
    );

    // A node whose chain view stalled refuses (review L3), and so does one
    // that knows the chain moved on without it.
    let stale = VerifyContext {
        now: 1 + 25 * zecraft_keyholder::verify::MAX_TIP_AGE_SPACINGS + 1,
        ..ctx
    };
    assert!(
        refusal(verify_payout(&stale, &honest_bytes, &[0])).starts_with("this node's newest block")
    );
    chain.set_chain_tip(GENESIS + 10).expect("chain tip");
    assert_eq!(
        refusal(verify_payout(&ctx, &honest_bytes, &[0])),
        format!(
            "this node is 10 blocks behind the chain tip {}",
            GENESIS + 10
        )
    );
    chain.set_chain_tip(GENESIS).expect("chain tip");

    // Redirected output: the amount still matches a payout.
    let redirected = payout_pczt(
        &v,
        &spent,
        anchor,
        &[(mallory, win_net), (dave, refund_net)],
    );
    let i = output_index_of(&redirected, &mallory);
    assert_eq!(
        refusal(verify_payout(&ctx, &redirected.serialize().expect("ser"), &[0])),
        format!("output {i}: recipient does not match the sealed payout receiver for round 0 (amount matches)")
    );

    // Changed amount to the right receiver.
    let short = payout_pczt(
        &v,
        &spent,
        anchor,
        &[(winner, win_net - 1), (dave, refund_net)],
    );
    let i = output_index_of(&short, &winner);
    assert_eq!(
        refusal(verify_payout(&ctx, &short.serialize().expect("ser"), &[0])),
        format!(
            "output {i}: amount {} expected {}",
            zat(win_net - 1),
            zat(win_net)
        )
    );

    // Extra output (paid from a float note, so the outputs still balance).
    let extra = payout_pczt(
        &v,
        &[alice_note, bob_note, dave_note, big_float],
        anchor,
        &[(winner, win_net), (dave, refund_net), (mallory, 10_000)],
    );
    let i = output_index_of(&extra, &mallory);
    assert_eq!(
        refusal(verify_payout(&ctx, &extra.serialize().expect("ser"), &[0])),
        format!("output {i}: unexpected output of 10,000 zatoshis (not a payout of rounds [0] nor vault change)")
    );

    // Missing output: dave's refund is not paid.
    let missing = payout_pczt(&v, &spent, anchor, &[(winner, win_net)]);
    assert_eq!(
        refusal(verify_payout(
            &ctx,
            &missing.serialize().expect("ser"),
            &[0]
        )),
        "round 0 payout 1 (net 90,000) is not among the outputs"
    );

    // Missing input: bob's entry note is not spent (a float note pays instead).
    let no_bob = payout_pczt(
        &v,
        &[alice_note, dave_note, big_float],
        anchor,
        &honest_outputs,
    );
    assert_eq!(
        refusal(verify_payout(&ctx, &no_bob.serialize().expect("ser"), &[0])),
        format!(
            "entry note {}:1 of round 0 is not spent by the transaction",
            "aa".repeat(32)
        )
    );

    // Another round's entry note spent as if it were float.
    let foreign_entry = payout_pczt(
        &v,
        &[alice_note, bob_note, dave_note, carol_note],
        anchor,
        &honest_outputs,
    );
    let i = spend_index_of(&foreign_entry, &carol_note.cmx);
    assert_eq!(
        refusal(verify_payout(
            &ctx,
            &foreign_entry.serialize().expect("ser"),
            &[0]
        )),
        format!("input {i}: note is not an entry of rounds [0]")
    );

    // A float note that is not needed.
    let needless = payout_pczt(
        &v,
        &[alice_note, bob_note, dave_note, small_float],
        anchor,
        &honest_outputs,
    );
    let i = spend_index_of(&needless, &small_float.cmx);
    assert_eq!(
        refusal(verify_payout(&ctx, &needless.serialize().expect("ser"), &[0])),
        format!("input {i}: float note not needed: the entry notes of rounds [0] cover the outputs and the fee")
    );

    // Unknown and duplicated rounds.
    assert_eq!(
        refusal(verify_payout(&ctx, &honest_bytes, &[1])),
        "round 1 is not settled on this node"
    );
    assert_eq!(
        refusal(verify_payout(&ctx, &honest_bytes, &[0, 0])),
        "round 0 is listed twice"
    );
    assert_eq!(
        refusal(verify_payout(&ctx, &honest_bytes, &[])),
        "no rounds claimed"
    );

    // Another vault's notes cannot be spent here.
    let other = vault(2);
    let (other_notes, other_anchor) =
        vault_notes(&other.fvk, &[(Scope::External, 900_000, enc(&alice_memo))]);
    let foreign = payout_pczt(&other, &[&other_notes[0]], other_anchor, &honest_outputs);
    let reason = refusal(verify_payout(
        &ctx,
        &foreign.serialize().expect("ser"),
        &[0],
    ));
    assert!(
        reason.starts_with("input ") && reason.contains(": spend is not a note of this vault ("),
        "{reason}"
    );

    // Wrong sealing key: no receiver opens, so this node expects no payout
    // outputs at all and refuses to sign any.
    let wrong_key = VerifyContext {
        now: 2,
        sealing: &other.sealing,
        ..ctx
    };
    let reason = refusal(verify_payout(&wrong_key, &honest_bytes, &[0]));
    assert!(
        reason.starts_with("output ") && reason.contains("unexpected output"),
        "{reason}"
    );

    // Already signed.
    node.mark_signed(&[0], "earlier", 1).expect("mark");
    assert_eq!(
        refusal(verify_payout(&ctx, &honest_bytes, &[0])),
        "round 0 was already signed in session earlier"
    );

    // Garbage is refused, not a crash.
    assert!(refusal(verify_payout(&ctx, b"not a pczt", &[0])).starts_with("PCZT does not parse: "));
}

/// A winning entry whose sealed receiver is junk (security review H1) must
/// not block the round: its payout is unclaimable, the refund is paid, and
/// the junk payout's value stays in the vault's change.
#[test]
fn a_junk_receiver_is_left_in_the_vault_and_the_round_still_pays() {
    let v = vault(5);
    let processor = processor(&v);
    let (alice, alice_memo) = player(&v, 12, 4, 1); // names round 4: refunded
    let (mallory, _) = player(&v, 31, 0, 1);
    let junk_memo = EntryMemo {
        round: 0,
        payload: Tickets(2),
        sealed_receiver: SealedReceiver::from_bytes(&[0x42; 92]).expect("92 bytes"),
    };
    let enc = |m: &EntryMemo<Tickets>| m.encode(&FORMAT);
    let (notes, anchor) = vault_notes(
        &v.fvk,
        &[
            (Scope::External, 100_000, enc(&alice_memo)),
            (Scope::External, 200_000, enc(&junk_memo)),
        ],
    );
    let chain = Db::open_in_memory().expect("db");
    let node = NodeDb::open_in_memory().expect("node db");
    let settlement = settle_round(
        &chain,
        &processor,
        &[
            Entry {
                value: 100_000,
                memo: enc(&alice_memo),
                note: &notes[0],
                height: GENESIS + 1,
            },
            Entry {
                value: 200_000,
                memo: enc(&junk_memo),
                note: &notes[1],
                height: GENESIS + 2,
            },
        ],
        0,
    );
    assert_eq!(
        settlement.payouts.len(),
        2,
        "the junk entry wins; alice is refunded"
    );
    let (junk_net, alice_net) = (settlement.payouts[0].net, settlement.payouts[1].net);
    assert!(zecraft_engine::coordinator::payable_receiver(
        Network::Test,
        &FORMAT,
        &v.sealing,
        &settlement.payouts[0]
    )
    .is_err());
    let ctx = VerifyContext {
        now: 2,
        processor: &processor,
        fvk: &v.fvk,
        sealing: &v.sealing,
        chain: &chain,
        node: &node,
    };
    let spent = [&notes[0], &notes[1]];

    // Alice is refunded; the junk payout's value stays in the change output.
    let honest = payout_pczt(&v, &spent, anchor, &[(alice, alice_net)]);
    let approved = verify_payout(&ctx, &honest.serialize().expect("ser"), &[0]).expect("approved");
    assert_eq!(approved.payouts, 1);

    // Nobody may collect the unclaimable amount.
    let stolen = payout_pczt(
        &v,
        &spent,
        anchor,
        &[(alice, alice_net), (mallory, junk_net)],
    );
    let reason = refusal(verify_payout(&ctx, &stolen.serialize().expect("ser"), &[0]));
    assert!(reason.contains("unexpected output"), "{reason}");
}

#[test]
fn a_carry_payout_may_use_float_notes_but_only_within_the_collected_fees() {
    let v = vault(3);
    let processor = processor(&v);
    let (alice, alice_memo) = player(&v, 11, 0, 1);
    let (notes, anchor) = vault_notes(
        &v.fvk,
        &[
            (Scope::External, 100_000, alice_memo.encode(&FORMAT)),
            (Scope::Internal, 1_200_000, [0xF6; 512]),
            (Scope::Internal, 600_000, [0xF6; 512]),
            (Scope::Internal, 600_000, [0xF6; 512]),
        ],
    );
    let (ticket, big, half_a, half_b) = (&notes[0], &notes[1], &notes[2], &notes[3]);
    let chain = Db::open_in_memory().expect("db");
    let node = NodeDb::open_in_memory().expect("node db");
    // 1,000,000 carried in; the sole ticket wins it.
    let settlement = settle_round(
        &chain,
        &processor,
        &[Entry {
            value: 100_000,
            memo: alice_memo.encode(&FORMAT),
            note: ticket,
            height: GENESIS + 1,
        }],
        1_000_000,
    );
    assert_eq!(settlement.payouts.len(), 1);
    assert_eq!(settlement.payouts[0].net, 1_090_000);
    assert_eq!(settlement.fees, 10_000);
    let ctx = VerifyContext {
        now: 2,
        processor: &processor,
        fvk: &v.fvk,
        sealing: &v.sealing,
        chain: &chain,
        node: &node,
    };

    // One float note: 2 spends, 2 outputs, fee 10,000 = collected. Approved.
    let one_float = payout_pczt(&v, &[ticket, big], anchor, &[(alice, 1_090_000)]);
    let approved =
        verify_payout(&ctx, &one_float.serialize().expect("ser"), &[0]).expect("approved");
    assert_eq!(approved.float_inputs, 1);
    assert_eq!(approved.fee, 10_000);

    // Two float notes: 3 spends, fee 15,000 > 10,000 collected. Refused.
    let two_floats = payout_pczt(&v, &[ticket, half_a, half_b], anchor, &[(alice, 1_090_000)]);
    assert_eq!(
        refusal(verify_payout(
            &ctx,
            &two_floats.serialize().expect("ser"),
            &[0]
        )),
        "fee 15,000 exceeds the fees collected by the rules for rounds [0] (10,000)"
    );
}

/// A note the game keeps (an undecodable memo) is never a payout input and
/// never float: the vault holds it for the carry.
#[test]
fn kept_notes_are_never_spent_by_a_payout() {
    let v = vault(4);
    let processor = processor(&v);
    let (alice, alice_memo) = player(&v, 21, 0, 1);
    let (notes, anchor) = vault_notes(
        &v.fvk,
        &[
            (Scope::External, 100_000, alice_memo.encode(&FORMAT)),
            (Scope::External, 60_000, [0xF6; 512]),
            (Scope::Internal, 50_000, [0xF6; 512]),
        ],
    );
    // One ticket exactly pays its payout and the fee, so the coordinator
    // adds a float note for the change output.
    let (ticket, kept, float) = (&notes[0], &notes[1], &notes[2]);
    let chain = Db::open_in_memory().expect("db");
    let node = NodeDb::open_in_memory().expect("node db");
    let settlement = settle_round(
        &chain,
        &processor,
        &[
            Entry {
                value: 100_000,
                memo: alice_memo.encode(&FORMAT),
                note: ticket,
                height: GENESIS + 1,
            },
            Entry {
                value: 60_000,
                memo: [0xF6; 512],
                note: kept,
                height: GENESIS + 2,
            },
        ],
        0,
    );
    assert_eq!(settlement.carry_out, 60_000);
    assert_eq!(settlement.payouts.len(), 1);
    let net = settlement.payouts[0].net;
    let ctx = VerifyContext {
        now: 2,
        processor: &processor,
        fvk: &v.fvk,
        sealing: &v.sealing,
        chain: &chain,
        node: &node,
    };
    let honest = payout_pczt(&v, &[ticket, float], anchor, &[(alice, net)]);
    verify_payout(&ctx, &honest.serialize().expect("ser"), &[0]).expect("approved");
    let with_kept = payout_pczt(&v, &[ticket, kept], anchor, &[(alice, net)]);
    let i = spend_index_of(&with_kept, &kept.cmx);
    assert_eq!(
        refusal(verify_payout(
            &ctx,
            &with_kept.serialize().expect("ser"),
            &[0]
        )),
        format!("input {i}: note is not an entry of rounds [0]")
    );
}
