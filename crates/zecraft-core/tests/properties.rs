//! Property tests of the generic primitives: entry memos, sealing, draws.

use proptest::prelude::*;
use rand::rngs::StdRng;
use rand::SeedableRng;
use zecraft_core::draw::{uniform_below, Randomness};
use zecraft_core::entry::{
    generate_sealing_keypair, EntryMemo, MemoError, MemoFormat, Payload, PayloadError,
    PayoutReceiver, SealError, ORCHARD_RECEIVER_LEN,
};
use zecraft_core::params::Network;

const FORMAT: MemoFormat = MemoFormat {
    magic: *b"PROP",
    version: 3,
    text_prefix: "prop1:",
    seal_info: b"prop/payout-seal/v1",
};

/// A four-byte payload accepting any value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Word(u32);

impl Payload for Word {
    const LEN: usize = 4;
    fn to_bytes(&self) -> Vec<u8> {
        self.0.to_be_bytes().to_vec()
    }
    fn from_bytes(bytes: &[u8]) -> Result<Self, PayloadError> {
        <[u8; 4]>::try_from(bytes)
            .map(|b| Word(u32::from_be_bytes(b)))
            .map_err(|_| PayloadError("four bytes".into()))
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn memos_round_trip_in_both_encodings(
        round in any::<u32>(),
        word in any::<u32>(),
        seed in any::<u64>(),
        orchard in any::<[u8; ORCHARD_RECEIVER_LEN]>(),
    ) {
        let mut rng = StdRng::seed_from_u64(seed);
        let (sk, pk) = generate_sealing_keypair(&mut rng);
        let receiver = PayoutReceiver { network: Network::Test, orchard };
        let memo = EntryMemo::seal(&FORMAT, round, Word(word), &receiver, &pk, &mut rng).expect("seal");
        let binary = memo.encode(&FORMAT);
        prop_assert_eq!(&EntryMemo::<Word>::decode(&FORMAT, &binary).expect("binary"), &memo);
        let text = memo.encode_text(&FORMAT);
        prop_assert!(text.len() <= 512);
        prop_assert_eq!(&EntryMemo::<Word>::decode_any(&FORMAT, text.as_bytes()).expect("text"), &memo);
        prop_assert_eq!(memo.open(&FORMAT, Network::Test, &sk).expect("open"), receiver);
    }

    #[test]
    fn a_sealed_receiver_only_opens_with_its_key_and_header(
        round in 0u32..u32::MAX,
        word in any::<u32>(),
        seed in any::<u64>(),
    ) {
        let mut rng = StdRng::seed_from_u64(seed);
        let (sk, pk) = generate_sealing_keypair(&mut rng);
        let (other, _) = generate_sealing_keypair(&mut rng);
        let receiver = PayoutReceiver { network: Network::Test, orchard: [7; ORCHARD_RECEIVER_LEN] };
        let memo = EntryMemo::seal(&FORMAT, round, Word(word), &receiver, &pk, &mut rng).expect("seal");
        prop_assert_eq!(memo.open(&FORMAT, Network::Test, &other), Err(MemoError::Seal(SealError::Open)));
        let moved = EntryMemo { round: round + 1, ..memo.clone() };
        prop_assert_eq!(moved.open(&FORMAT, Network::Test, &sk), Err(MemoError::Seal(SealError::Open)));
        let changed = EntryMemo { payload: Word(word ^ 1), ..memo };
        prop_assert_eq!(changed.open(&FORMAT, Network::Test, &sk), Err(MemoError::Seal(SealError::Open)));
    }

    #[test]
    fn draws_are_always_in_range(round in any::<u32>(), seed in any::<[u8; 32]>(), n in 1u32..u32::MAX) {
        let v = uniform_below(b"prop/draw/v1", round, &Randomness(seed), n).expect("draw");
        prop_assert!(v < n);
    }
}

#[test]
fn draws_are_unbiased_over_many_samples() {
    // 25,000 draws over 25 values: each count is within 5 sigma of 1,000.
    let mut counts = [0u32; 25];
    for i in 0..25_000u32 {
        let mut seed = [0u8; 32];
        seed[..4].copy_from_slice(&i.to_be_bytes());
        let v = uniform_below(b"prop/bias/v1", 7, &Randomness(seed), 25).expect("draw");
        counts[v as usize] += 1;
    }
    for c in counts {
        assert!((845..=1155).contains(&c), "{counts:?}");
    }
}
