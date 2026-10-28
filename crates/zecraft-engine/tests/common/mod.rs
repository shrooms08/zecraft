//! Shared fixtures of the engine's integration tests: a synthetic chain
//! with one raffle round, and the recorded drand beacon it settles with.

use std::collections::HashMap;

use zecraft_core::draw::Beacon;
use zecraft_core::params::ChainParams;
use zecraft_engine::beacon::FixedBeacons;
use zecraft_engine::config::{GameConfig, SETTLE_CONFIRMATIONS};
use zecraft_engine::db::Pool;
use zecraft_engine::synthetic::{SyntheticChain, TestVault};
use zecraft_raffle::{RaffleParams, Tickets, FORMAT};

/// A real quicknet beacon (tests/fixtures/quicknet.json in zecraft-core, from
/// https://api.drand.sh/v2/beacons/quicknet/rounds/10000000).
pub const FIXTURE_ROUND: u64 = 10_000_000;
const FIXTURE_SIGNATURE: &str =
    "a187c9f5800521e986b5658924696682071ce7ae965486e8fd08bc2c4758bdbb9ed9ecb7beee392a38214e321ecd434f";

pub const GENESIS: u32 = 5_000;

/// Start time of round 0 such that its bound drand round is the fixture round.
pub fn start_time(params: &ChainParams) -> u64 {
    let beacon_time = params.drand.genesis_time + (FIXTURE_ROUND - 1) * params.drand.period_secs;
    beacon_time - params.round_duration_secs().expect("duration") - params.reveal_delay_secs
}

pub fn fixture_beacons() -> FixedBeacons {
    let mut beacons = HashMap::new();
    beacons.insert(
        FIXTURE_ROUND,
        Beacon {
            round: FIXTURE_ROUND,
            signature: FIXTURE_SIGNATURE.to_owned(),
        },
    );
    FixedBeacons { beacons }
}

pub fn config(vault: &TestVault) -> GameConfig<RaffleParams> {
    vault.config(GENESIS, RaffleParams::default())
}

pub fn binary(vault: &TestVault, round: u32, tickets: u16, player: u8) -> [u8; 512] {
    vault
        .entry_memo(&FORMAT, round, Tickets(tickets), player)
        .encode(&FORMAT)
}

pub fn text(vault: &TestVault, round: u32, tickets: u16, player: u8) -> [u8; 512] {
    let text = vault
        .entry_memo(&FORMAT, round, Tickets(tickets), player)
        .encode_text(&FORMAT);
    let mut field = [0u8; 512];
    field[..text.len()].copy_from_slice(text.as_bytes());
    field
}

pub struct Scenario {
    pub vault: TestVault,
    pub chain: SyntheticChain,
}

/// Round 0 of the raffle (0.001 per ticket, 5,000 zat entry and output fees):
///
/// * player 1: 3 tickets (0.003); player 2: 2 tickets (0.002); player 3:
///   1 ticket as a text memo: valid, 6 tickets in all;
/// * player 4: names round 7: refund; player 5: 2 tickets for 0.0015: refund;
/// * player 6: 1 ticket for 8,000 zat: cannot pay its refund fees, kept;
/// * a 0.0015 note with a non-raffle memo: kept;
/// * the vault's own change (invisible to the engine).
///
/// Then the blocks the timing rule and the confirmation policy need.
pub fn scenario(delay_close_secs: u64) -> Scenario {
    scenario_with(delay_close_secs, |_, _| {})
}

/// The same round with `extra` transactions mined inside its window (after
/// player 2's entry).
pub fn scenario_with(
    delay_close_secs: u64,
    extra: impl FnOnce(&mut SyntheticChain, &TestVault),
) -> Scenario {
    let vault = TestVault::new(42);
    let params = config(&vault).chain;
    let mut chain = SyntheticChain::new(&vault, GENESIS - 3, start_time(&params) - 75);
    chain.advance(3); // GENESIS-3 .. GENESIS-1 (operator float era)
    assert_eq!(chain.next_height(), GENESIS);
    chain.push_block(vec![]); // GENESIS: start block, time == start_time
    chain.advance(4);
    chain.push_block(vec![
        chain.payment(300_000, Pool::Ironwood, binary(&vault, 0, 3, 1)),
        chain.change(1_000),
    ]);
    chain.push_block(vec![chain.payment(
        200_000,
        Pool::Ironwood,
        binary(&vault, 0, 2, 2),
    )]);
    extra(&mut chain, &vault);
    chain.advance(2);
    chain.push_block(vec![
        chain.payment(100_000, Pool::Ironwood, text(&vault, 0, 1, 3)),
        chain.payment(100_000, Pool::Ironwood, binary(&vault, 7, 1, 4)),
        chain.payment(150_000, Pool::Ironwood, binary(&vault, 0, 2, 5)),
        chain.payment(8_000, Pool::Ironwood, binary(&vault, 0, 1, 6)),
        chain.payment(150_000, Pool::Ironwood, [0xF6; 512]),
    ]);
    while chain.next_height() < GENESIS + params.blocks_per_round {
        chain.advance(1);
    }
    chain.delay(delay_close_secs);
    // Post-close blocks for the timing rule plus the confirmation depth.
    chain.advance(params.mtp_window + SETTLE_CONFIRMATIONS);
    Scenario { vault, chain }
}
