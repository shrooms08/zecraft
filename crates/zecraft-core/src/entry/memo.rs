//! The entry memo: how a player's payment to the vault says what it is for.
//!
//! An entry is a shielded payment to the vault whose 512-byte memo (ZIP 302)
//! is arbitrary data (first byte `0xFF`) laid out as:
//!
//! ```text
//! 0xFF | magic[4] | version u8 | round u32 BE | payload[P::LEN] | sealed payout receiver (92 bytes)
//! ```
//!
//! followed by zero padding to 512 bytes. Magic, version, text prefix and
//! HPKE context are the game's ([`MemoFormat`]); the payload is the game's
//! fixed-length [`Payload`] (a tile, a ticket count, a coin side).
//!
//! The sealed payout receiver is the HPKE encryption (see
//! [`crate::entry::seal`]) of the 44-byte plaintext
//! `orchard_receiver (43 bytes) || network tag (1 byte)` under the vault's
//! sealing key, with the **header** `magic | version | round | payload` as
//! AAD, so a sealed receiver is bound to the game, the round and the payload
//! and cannot be moved to another entry. Only Orchard-protocol receivers are
//! representable: this is the receiver the Ironwood pool pays to, and
//! transparent payouts are not supported.
//!
//! # Text encoding
//!
//! Some wallets can only send UTF-8 text memos. The same bytes may therefore
//! be sent as a ZIP 302 text memo: the game's text prefix (for example
//! `zrfl1:`) followed by the unpadded base64url of everything after the
//! `0xFF` byte, zero-padded to 512 bytes. Both encodings decode to the same
//! [`EntryMemo`].
//!
//! Decoding is strict: any deviation (wrong lead byte, magic or version, a
//! payload the game rejects, a sealed receiver of the wrong size, non-zero
//! padding, non-canonical base64) is an error.

use std::fmt::Debug;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand_core_09::{CryptoRng, RngCore};
use serde::{Deserialize, Serialize};
use zcash_address::unified::{self, Container, Encoding, Receiver};
use zcash_address::{ConversionError, ToAddress, TryFromAddress, ZcashAddress};
use zcash_protocol::consensus::NetworkType;

use crate::entry::seal::{
    self, SealError, SealingPrivateKey, SealingPublicKey, ENCAPPED_KEY_LEN, TAG_LEN,
};
use crate::params::Network;

/// Size of every memo field.
pub const MEMO_LEN: usize = 512;
/// ZIP 302 lead byte for arbitrary data.
pub const ARBITRARY_DATA_LEAD: u8 = 0xFF;
/// Length of a raw Orchard-protocol receiver.
pub const ORCHARD_RECEIVER_LEN: usize = 43;
/// Length of the sealed plaintext: receiver plus network tag.
pub const RECEIVER_PLAINTEXT_LEN: usize = ORCHARD_RECEIVER_LEN + 1;
/// Length of a sealed receiver: encapsulated key, plaintext, tag.
pub const SEALED_RECEIVER_LEN: usize = ENCAPPED_KEY_LEN + RECEIVER_PLAINTEXT_LEN + TAG_LEN;
/// Bytes of the header before the payload: `0xFF | magic | version | round`.
pub const HEADER_PREFIX_LEN: usize = 1 + 4 + 1 + 4;

/// A game's memo format. Every field is the game's own: nothing in the kit
/// names a game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoFormat {
    /// Four magic bytes after the `0xFF` lead byte.
    pub magic: [u8; 4],
    /// Memo layout version byte.
    pub version: u8,
    /// Prefix of the text encoding, for example `zrfl1:` (ASCII).
    pub text_prefix: &'static str,
    /// HPKE `info` the payout receiver is sealed under.
    pub seal_info: &'static [u8],
}

impl MemoFormat {
    /// Length of an unpadded binary entry memo with a `payload_len`-byte payload.
    pub const fn memo_len(&self, payload_len: usize) -> usize {
        HEADER_PREFIX_LEN + payload_len + SEALED_RECEIVER_LEN
    }

    /// Length of the text encoding of such a memo.
    pub const fn text_len(&self, payload_len: usize) -> usize {
        self.text_prefix.len() + (self.memo_len(payload_len) - 1).div_ceil(3) * 4
    }

    /// The AAD (header without the lead byte) binding a sealed receiver:
    /// `magic | version | round BE | payload`.
    pub fn aad(&self, round: u32, payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(4 + 1 + 4 + payload.len());
        out.extend_from_slice(&self.magic);
        out.push(self.version);
        out.extend_from_slice(&round.to_be_bytes());
        out.extend_from_slice(payload);
        out
    }
}

/// Why a payload's bytes are not acceptable to its game.
#[derive(Clone, Debug, thiserror::Error, PartialEq, Eq)]
#[error("{0}")]
pub struct PayloadError(pub String);

/// A game's fixed-length entry payload.
pub trait Payload: Sized + Clone + Debug + PartialEq {
    /// Encoded length in bytes.
    const LEN: usize;
    /// The `LEN`-byte encoding.
    fn to_bytes(&self) -> Vec<u8>;
    /// Decodes exactly `LEN` bytes (the caller checks the length).
    fn from_bytes(bytes: &[u8]) -> Result<Self, PayloadError>;
}

/// Errors from encoding, decoding, sealing and opening entry memos.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum MemoError {
    #[error("memo is {0} bytes, longer than the 512-byte memo field")]
    TooLong(usize),
    #[error("memo is {0} bytes, shorter than an entry memo")]
    TooShort(usize),
    #[error("memo lead byte is {0:#04x}, not the ZIP 302 arbitrary-data byte 0xFF")]
    NotArbitraryData(u8),
    #[error("memo does not carry this game's magic")]
    BadMagic,
    #[error("unsupported entry memo version {0}")]
    UnsupportedVersion(u8),
    #[error("payload rejected: {0}")]
    BadPayload(#[from] PayloadError),
    #[error("memo bytes after the entry memo are not all zero")]
    NonZeroPadding,
    #[error("text memo is not a well-formed entry memo: {0}")]
    BadText(&'static str),
    #[error("sealed receiver must be {SEALED_RECEIVER_LEN} bytes, got {0}")]
    BadSealedLength(usize),
    #[error("opened receiver has {0} bytes, expected {RECEIVER_PLAINTEXT_LEN}")]
    BadPlaintextLength(usize),
    #[error("opened receiver carries unknown network tag {0:#04x}")]
    UnknownNetworkTag(u8),
    #[error("opened receiver is for the wrong network")]
    WrongNetwork,
    #[error("address is not a valid Zcash address: {0}")]
    InvalidAddress(String),
    #[error("address is for the wrong network")]
    AddressWrongNetwork,
    #[error(
        "address has no Orchard receiver; only shielded Orchard-protocol payouts are supported"
    )]
    NoOrchardReceiver,
    #[error("cannot encode the receiver as a unified address: {0}")]
    AddressEncoding(String),
    #[error(transparent)]
    Seal(#[from] SealError),
}

/// Where an entry's winnings or refund are paid: an Orchard-protocol receiver.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PayoutReceiver {
    /// Network the receiver belongs to.
    pub network: Network,
    /// Raw 43-byte Orchard receiver (ZIP 316 typecode 3).
    #[serde(with = "hex_43")]
    pub orchard: [u8; ORCHARD_RECEIVER_LEN],
}

mod hex_43 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8; 43], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hex::encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 43], D::Error> {
        let text = String::deserialize(d)?;
        let bytes = hex::decode(text).map_err(serde::de::Error::custom)?;
        <[u8; 43]>::try_from(bytes.as_slice())
            .map_err(|_| serde::de::Error::custom("orchard receiver must be 43 bytes"))
    }
}

impl core::fmt::Debug for PayoutReceiver {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "PayoutReceiver({:?}, {})",
            self.network,
            hex::encode(self.orchard)
        )
    }
}

/// Extracts the Orchard receiver of a unified address; every other address
/// kind is rejected.
struct OrchardOnly([u8; ORCHARD_RECEIVER_LEN]);

impl TryFromAddress for OrchardOnly {
    type Error = MemoError;

    fn try_from_unified(
        _net: NetworkType,
        data: unified::Address,
    ) -> Result<Self, ConversionError<Self::Error>> {
        data.items()
            .into_iter()
            .find_map(|item| match item {
                Receiver::Orchard(raw) => Some(OrchardOnly(raw)),
                _ => None,
            })
            .ok_or(ConversionError::User(MemoError::NoOrchardReceiver))
    }
}

pub(crate) fn network_type(network: Network) -> NetworkType {
    match network {
        Network::Main => NetworkType::Main,
        Network::Test => NetworkType::Test,
    }
}

impl PayoutReceiver {
    /// Parses a unified address of `network` and keeps its Orchard receiver.
    pub fn from_unified_address(network: Network, address: &str) -> Result<Self, MemoError> {
        let parsed = ZcashAddress::try_from_encoded(address.trim())
            .map_err(|e| MemoError::InvalidAddress(e.to_string()))?;
        match parsed.convert_if_network::<OrchardOnly>(network_type(network)) {
            Ok(OrchardOnly(orchard)) => Ok(PayoutReceiver { network, orchard }),
            Err(ConversionError::IncorrectNetwork { .. }) => Err(MemoError::AddressWrongNetwork),
            Err(ConversionError::Unsupported(_)) => Err(MemoError::NoOrchardReceiver),
            Err(ConversionError::User(e)) => Err(e),
        }
    }

    /// Encodes the receiver as a unified address with only an Orchard item.
    pub fn to_unified_address(&self) -> Result<String, MemoError> {
        let ua = unified::Address::try_from_items(vec![Receiver::Orchard(self.orchard)])
            .map_err(|e| MemoError::AddressEncoding(e.to_string()))?;
        Ok(ZcashAddress::from_unified(network_type(self.network), ua).encode())
    }

    /// The 44-byte sealed plaintext.
    pub fn to_plaintext(&self) -> [u8; RECEIVER_PLAINTEXT_LEN] {
        let mut out = [0u8; RECEIVER_PLAINTEXT_LEN];
        out[..ORCHARD_RECEIVER_LEN].copy_from_slice(&self.orchard);
        out[ORCHARD_RECEIVER_LEN] = self.network.receiver_tag();
        out
    }

    /// Parses a sealed plaintext, checking it is for `expected` network.
    pub fn from_plaintext(expected: Network, bytes: &[u8]) -> Result<Self, MemoError> {
        if bytes.len() != RECEIVER_PLAINTEXT_LEN {
            return Err(MemoError::BadPlaintextLength(bytes.len()));
        }
        let tag = bytes[ORCHARD_RECEIVER_LEN];
        let network = Network::from_receiver_tag(tag).ok_or(MemoError::UnknownNetworkTag(tag))?;
        if network != expected {
            return Err(MemoError::WrongNetwork);
        }
        let mut orchard = [0u8; ORCHARD_RECEIVER_LEN];
        orchard.copy_from_slice(&bytes[..ORCHARD_RECEIVER_LEN]);
        Ok(PayoutReceiver { network, orchard })
    }
}

/// An HPKE-sealed payout receiver (92 bytes, hex in serde).
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SealedReceiver([u8; SEALED_RECEIVER_LEN]);

impl SealedReceiver {
    /// Wraps sealed bytes, checking the length.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, MemoError> {
        <[u8; SEALED_RECEIVER_LEN]>::try_from(bytes)
            .map(SealedReceiver)
            .map_err(|_| MemoError::BadSealedLength(bytes.len()))
    }

    /// The sealed bytes.
    pub fn as_bytes(&self) -> &[u8; SEALED_RECEIVER_LEN] {
        &self.0
    }
}

impl core::fmt::Debug for SealedReceiver {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "SealedReceiver({})", hex::encode(self.0))
    }
}

impl Serialize for SealedReceiver {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&hex::encode(self.0))
    }
}

impl<'de> Deserialize<'de> for SealedReceiver {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        let bytes = hex::decode(text).map_err(serde::de::Error::custom)?;
        SealedReceiver::from_bytes(&bytes).map_err(serde::de::Error::custom)
    }
}

/// Opens a sealed receiver given its AAD (as a settlement's payout carries
/// them) under the game's format.
pub fn open_receiver(
    format: &MemoFormat,
    network: Network,
    key: &SealingPrivateKey,
    sealed: &SealedReceiver,
    aad: &[u8],
) -> Result<PayoutReceiver, MemoError> {
    let plaintext = seal::open(format.seal_info, key, sealed.as_bytes(), aad)?;
    PayoutReceiver::from_plaintext(network, &plaintext)
}

/// A decoded entry memo.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryMemo<P> {
    /// Round the player is entering.
    pub round: u32,
    /// The game's payload.
    pub payload: P,
    /// The player's sealed payout receiver.
    pub sealed_receiver: SealedReceiver,
}

impl<P: Payload> EntryMemo<P> {
    /// Seals `receiver` to the vault's key and builds the memo of an entry.
    pub fn seal<R: CryptoRng + RngCore>(
        format: &MemoFormat,
        round: u32,
        payload: P,
        receiver: &PayoutReceiver,
        sealing_key: &SealingPublicKey,
        rng: &mut R,
    ) -> Result<Self, MemoError> {
        let bytes = payload.to_bytes();
        // Refuse a payload the game itself would not decode.
        P::from_bytes(&bytes)?;
        let sealed = seal::seal(
            format.seal_info,
            sealing_key,
            &receiver.to_plaintext(),
            &format.aad(round, &bytes),
            rng,
        )?;
        Ok(EntryMemo {
            round,
            payload,
            sealed_receiver: SealedReceiver::from_bytes(&sealed)?,
        })
    }

    /// The AAD that binds this memo's sealed receiver.
    pub fn aad(&self, format: &MemoFormat) -> Vec<u8> {
        format.aad(self.round, &self.payload.to_bytes())
    }

    /// Opens the sealed receiver with the vault's private sealing key.
    pub fn open(
        &self,
        format: &MemoFormat,
        network: Network,
        sealing_key: &SealingPrivateKey,
    ) -> Result<PayoutReceiver, MemoError> {
        open_receiver(
            format,
            network,
            sealing_key,
            &self.sealed_receiver,
            &self.aad(format),
        )
    }

    /// The memo bytes without padding.
    pub fn encode_unpadded(&self, format: &MemoFormat) -> Vec<u8> {
        let mut out = Vec::with_capacity(format.memo_len(P::LEN));
        out.push(ARBITRARY_DATA_LEAD);
        out.extend_from_slice(&format.aad(self.round, &self.payload.to_bytes()));
        out.extend_from_slice(self.sealed_receiver.as_bytes());
        out
    }

    /// The full 512-byte memo field.
    pub fn encode(&self, format: &MemoFormat) -> [u8; MEMO_LEN] {
        let binary = self.encode_unpadded(format);
        let mut out = [0u8; MEMO_LEN];
        let len = binary.len().min(MEMO_LEN);
        out[..len].copy_from_slice(&binary[..len]);
        out
    }

    /// The text encoding: the game's prefix plus base64url (no padding) of
    /// the binary memo without its `0xFF` lead byte.
    pub fn encode_text(&self, format: &MemoFormat) -> String {
        let binary = self.encode_unpadded(format);
        let mut out = String::with_capacity(format.text_len(P::LEN));
        out.push_str(format.text_prefix);
        URL_SAFE_NO_PAD.encode_string(&binary[1..], &mut out);
        out
    }

    /// Strictly parses a binary memo field (padded to 512 bytes or unpadded).
    pub fn decode(format: &MemoFormat, memo: &[u8]) -> Result<Self, MemoError> {
        let len = format.memo_len(P::LEN);
        if memo.len() > MEMO_LEN {
            return Err(MemoError::TooLong(memo.len()));
        }
        if memo.len() < len {
            return Err(MemoError::TooShort(memo.len()));
        }
        if memo[0] != ARBITRARY_DATA_LEAD {
            return Err(MemoError::NotArbitraryData(memo[0]));
        }
        if memo[1..5] != format.magic {
            return Err(MemoError::BadMagic);
        }
        if memo[5] != format.version {
            return Err(MemoError::UnsupportedVersion(memo[5]));
        }
        let round = u32::from_be_bytes([memo[6], memo[7], memo[8], memo[9]]);
        let payload_end = HEADER_PREFIX_LEN + P::LEN;
        let payload = P::from_bytes(&memo[HEADER_PREFIX_LEN..payload_end])?;
        if memo[len..].iter().any(|b| *b != 0) {
            return Err(MemoError::NonZeroPadding);
        }
        Ok(EntryMemo {
            round,
            payload,
            sealed_receiver: SealedReceiver::from_bytes(&memo[payload_end..len])?,
        })
    }

    /// Strictly parses the text encoding from a memo field (the text
    /// followed by zero padding): exactly the prefix and the canonical
    /// unpadded base64url of a well-formed memo, nothing before or after.
    pub fn decode_text(format: &MemoFormat, memo: &[u8]) -> Result<Self, MemoError> {
        if memo.len() > MEMO_LEN {
            return Err(MemoError::TooLong(memo.len()));
        }
        let end = memo.iter().rposition(|b| *b != 0).map_or(0, |i| i + 1);
        let text = &memo[..end];
        let encoded = text
            .strip_prefix(format.text_prefix.as_bytes())
            .ok_or(MemoError::BadText("missing the game's text prefix"))?;
        if text.len() != format.text_len(P::LEN) {
            return Err(MemoError::BadText("wrong length"));
        }
        let payload = URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| MemoError::BadText("payload is not canonical base64url"))?;
        if payload.len() + 1 != format.memo_len(P::LEN) {
            return Err(MemoError::BadText("wrong payload length"));
        }
        let mut binary = Vec::with_capacity(payload.len() + 1);
        binary.push(ARBITRARY_DATA_LEAD);
        binary.extend_from_slice(&payload);
        Self::decode(format, &binary)
    }

    /// Decodes either encoding: binary, or text when the memo starts with
    /// the game's text prefix.
    pub fn decode_any(format: &MemoFormat, memo: &[u8]) -> Result<Self, MemoError> {
        match Self::decode(format, memo) {
            Ok(decoded) => Ok(decoded),
            Err(binary_error) => {
                if memo.starts_with(format.text_prefix.as_bytes()) {
                    Self::decode_text(format, memo)
                } else {
                    Err(binary_error)
                }
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::entry::seal::generate_sealing_keypair;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    /// A one-byte test payload that only accepts values below 10.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
    pub(crate) struct Digit(pub u8);

    impl Payload for Digit {
        const LEN: usize = 1;
        fn to_bytes(&self) -> Vec<u8> {
            vec![self.0]
        }
        fn from_bytes(bytes: &[u8]) -> Result<Self, PayloadError> {
            match bytes {
                [d] if *d < 10 => Ok(Digit(*d)),
                _ => Err(PayloadError("digit out of range".into())),
            }
        }
    }

    pub(crate) const FORMAT: MemoFormat = MemoFormat {
        magic: *b"TEST",
        version: 1,
        text_prefix: "test1:",
        seal_info: b"test/payout-seal/v1",
    };

    pub(crate) fn receiver(seed: u8) -> PayoutReceiver {
        PayoutReceiver {
            network: Network::Test,
            orchard: [seed; ORCHARD_RECEIVER_LEN],
        }
    }

    #[test]
    fn memo_round_trips_and_fits_the_field() {
        let mut rng = StdRng::seed_from_u64(1);
        let (sk, pk) = generate_sealing_keypair(&mut rng);
        let memo =
            EntryMemo::seal(&FORMAT, 77, Digit(4), &receiver(9), &pk, &mut rng).expect("seal");
        let bytes = memo.encode(&FORMAT);
        assert_eq!(bytes.len(), MEMO_LEN);
        assert_eq!(memo.encode_unpadded(&FORMAT).len(), FORMAT.memo_len(1));
        assert_eq!(
            EntryMemo::<Digit>::decode(&FORMAT, &bytes).expect("decode"),
            memo
        );
        assert_eq!(
            EntryMemo::<Digit>::decode(&FORMAT, &memo.encode_unpadded(&FORMAT)).expect("decode"),
            memo
        );
        assert_eq!(
            memo.open(&FORMAT, Network::Test, &sk).expect("open"),
            receiver(9)
        );
        assert_eq!(
            memo.open(&FORMAT, Network::Main, &sk),
            Err(MemoError::WrongNetwork)
        );
    }

    #[test]
    fn text_encoding_decodes_to_the_same_memo() {
        let mut rng = StdRng::seed_from_u64(2);
        let (_, pk) = generate_sealing_keypair(&mut rng);
        let memo =
            EntryMemo::seal(&FORMAT, 5, Digit(7), &receiver(3), &pk, &mut rng).expect("seal");
        let text = memo.encode_text(&FORMAT);
        assert!(text.starts_with("test1:"));
        assert_eq!(text.len(), FORMAT.text_len(1));
        let mut field = [0u8; MEMO_LEN];
        field[..text.len()].copy_from_slice(text.as_bytes());
        assert_eq!(
            EntryMemo::<Digit>::decode_text(&FORMAT, &field).expect("decode"),
            memo
        );
        assert_eq!(
            EntryMemo::<Digit>::decode_any(&FORMAT, &field).expect("decode"),
            memo
        );
        // Trailing junk, a wrong prefix, or non-canonical base64 are refused.
        let mut junk = field;
        junk[text.len()] = b'A';
        assert!(EntryMemo::<Digit>::decode_text(&FORMAT, &junk).is_err());
        let mut other = field;
        other[0] = b'x';
        assert!(EntryMemo::<Digit>::decode_any(&FORMAT, &other).is_err());
    }

    #[test]
    fn decode_is_strict() {
        let mut rng = StdRng::seed_from_u64(3);
        let (_, pk) = generate_sealing_keypair(&mut rng);
        let memo =
            EntryMemo::seal(&FORMAT, 1, Digit(1), &receiver(1), &pk, &mut rng).expect("seal");
        let good = memo.encode(&FORMAT);
        let decode = |b: &[u8]| EntryMemo::<Digit>::decode(&FORMAT, b);

        let mut b = good;
        b[0] = 0xF5;
        assert_eq!(decode(&b), Err(MemoError::NotArbitraryData(0xF5)));
        let mut b = good;
        b[1] = b'X';
        assert_eq!(decode(&b), Err(MemoError::BadMagic));
        let mut b = good;
        b[5] = 2;
        assert_eq!(decode(&b), Err(MemoError::UnsupportedVersion(2)));
        let mut b = good;
        b[10] = 10;
        assert!(matches!(decode(&b), Err(MemoError::BadPayload(_))));
        let mut b = good;
        b[300] = 1;
        assert_eq!(decode(&b), Err(MemoError::NonZeroPadding));
        assert!(matches!(decode(&good[..50]), Err(MemoError::TooShort(50))));
        // Another game's format rejects this memo.
        let other = MemoFormat {
            magic: *b"ELSE",
            ..FORMAT
        };
        assert_eq!(
            EntryMemo::<Digit>::decode(&other, &good),
            Err(MemoError::BadMagic)
        );
        // Sealing refuses a payload its own decoder rejects.
        assert!(matches!(
            EntryMemo::seal(&FORMAT, 1, Digit(11), &receiver(1), &pk, &mut rng),
            Err(MemoError::BadPayload(_))
        ));
    }

    #[test]
    fn sealed_receiver_is_bound_to_round_payload_and_game() {
        let mut rng = StdRng::seed_from_u64(4);
        let (sk, pk) = generate_sealing_keypair(&mut rng);
        let memo =
            EntryMemo::seal(&FORMAT, 10, Digit(2), &receiver(5), &pk, &mut rng).expect("seal");
        for moved in [
            EntryMemo {
                round: 11,
                ..memo.clone()
            },
            EntryMemo {
                payload: Digit(3),
                ..memo.clone()
            },
        ] {
            assert!(matches!(
                moved.open(&FORMAT, Network::Test, &sk),
                Err(MemoError::Seal(SealError::Open))
            ));
        }
        let other_game = MemoFormat {
            seal_info: b"else/payout-seal/v1",
            ..FORMAT
        };
        assert!(matches!(
            memo.open(&other_game, Network::Test, &sk),
            Err(MemoError::Seal(SealError::Open))
        ));
    }

    #[test]
    fn unified_address_round_trip_and_rejections() {
        let r = receiver(0x42);
        let ua = r.to_unified_address().expect("ua");
        assert!(ua.starts_with("utest1"));
        assert_eq!(
            PayoutReceiver::from_unified_address(Network::Test, &ua).expect("parse"),
            r
        );
        assert_eq!(
            PayoutReceiver::from_unified_address(Network::Main, &ua),
            Err(MemoError::AddressWrongNetwork)
        );
        assert!(matches!(
            PayoutReceiver::from_unified_address(Network::Test, "not an address"),
            Err(MemoError::InvalidAddress(_))
        ));
        assert_eq!(
            PayoutReceiver::from_unified_address(
                Network::Test,
                "tmEZhbWHTpdKMw5it8YDspUXSMGQyFwovpU"
            ),
            Err(MemoError::NoOrchardReceiver)
        );
    }
}
