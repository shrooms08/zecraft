//! ZIP 321 payment request URIs for entries.
//!
//! Grammar used (ZIP 321): `zcash:<address>?amount=<decimal ZEC>&memo=<base64url>`.
//! The amount is decimal ZEC with at most eight fractional digits and no
//! trailing dot; the memo is the base64url encoding (RFC 4648 § 5, no `=`
//! padding) of the memo bytes, which the receiving wallet pads with zeros to
//! 512 bytes. A memo may only be attached to an address that can receive one,
//! so transparent-only addresses are rejected.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use zcash_address::unified;
use zcash_address::{ConversionError, TryFromAddress, ZcashAddress};
use zcash_protocol::consensus::NetworkType;

use crate::entry::memo::{network_type, MemoError, MEMO_LEN};
use crate::params::Network;
use crate::COIN;

/// Errors from building a payment URI.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PaymentError {
    #[error("amount must be positive")]
    ZeroAmount,
    #[error("amount {0} zatoshis exceeds the 21,000,000 ZEC supply")]
    AmountTooLarge(u64),
    #[error("memo is {0} bytes, longer than the 512-byte memo field")]
    MemoTooLong(usize),
    #[error("address cannot receive a memo (it has no shielded receiver)")]
    NoMemoReceiver,
    #[error(transparent)]
    Memo(#[from] MemoError),
}

/// Accepts every address kind; used only to run `zcash_address`'s network check.
struct AnyKind;

impl TryFromAddress for AnyKind {
    type Error = core::convert::Infallible;

    fn try_from_sprout(_: NetworkType, _: [u8; 64]) -> Result<Self, ConversionError<Self::Error>> {
        Ok(AnyKind)
    }

    fn try_from_sapling(_: NetworkType, _: [u8; 43]) -> Result<Self, ConversionError<Self::Error>> {
        Ok(AnyKind)
    }

    fn try_from_unified(
        _: NetworkType,
        _: unified::Address,
    ) -> Result<Self, ConversionError<Self::Error>> {
        Ok(AnyKind)
    }

    fn try_from_transparent_p2pkh(
        _: NetworkType,
        _: [u8; 20],
    ) -> Result<Self, ConversionError<Self::Error>> {
        Ok(AnyKind)
    }

    fn try_from_transparent_p2sh(
        _: NetworkType,
        _: [u8; 20],
    ) -> Result<Self, ConversionError<Self::Error>> {
        Ok(AnyKind)
    }

    fn try_from_tex(_: NetworkType, _: [u8; 20]) -> Result<Self, ConversionError<Self::Error>> {
        Ok(AnyKind)
    }
}

/// Largest amount ZIP 321 admits: 21,000,000 ZEC.
pub const MAX_AMOUNT_ZAT: u64 = 21_000_000 * COIN;

/// Formats zatoshis as a ZIP 321 decimal amount (`0.001`, `1`, `2.5`).
pub fn amount_param(zatoshis: u64) -> String {
    let whole = zatoshis / COIN;
    let frac = zatoshis % COIN;
    if frac == 0 {
        return whole.to_string();
    }
    let digits = format!("{frac:08}");
    format!("{whole}.{}", digits.trim_end_matches('0'))
}

/// Builds the ZIP 321 request a player's wallet should pay to enter a
/// round: `amount_zat` to the vault's `address` on `network`, carrying `memo`
/// (the binary or the text encoding of an [`crate::entry::EntryMemo`],
/// unpadded).
pub fn entry_request_uri(
    network: Network,
    address: &str,
    amount_zat: u64,
    memo: &[u8],
) -> Result<String, PaymentError> {
    if amount_zat == 0 {
        return Err(PaymentError::ZeroAmount);
    }
    if amount_zat > MAX_AMOUNT_ZAT {
        return Err(PaymentError::AmountTooLarge(amount_zat));
    }
    if memo.len() > MEMO_LEN {
        return Err(PaymentError::MemoTooLong(memo.len()));
    }
    let parsed = ZcashAddress::try_from_encoded(address.trim())
        .map_err(|e| MemoError::InvalidAddress(e.to_string()))?;
    parsed
        .clone()
        .convert_if_network::<AnyKind>(network_type(network))
        .map_err(|_| MemoError::AddressWrongNetwork)?;
    if !parsed.can_receive_memo() {
        return Err(PaymentError::NoMemoReceiver);
    }
    Ok(format!(
        "zcash:{}?amount={}&memo={}",
        parsed.encode(),
        amount_param(amount_zat),
        URL_SAFE_NO_PAD.encode(memo)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::memo::tests::{receiver, Digit, FORMAT};
    use crate::entry::memo::EntryMemo;
    use crate::entry::seal::generate_sealing_keypair;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    #[test]
    fn amounts_follow_zip321_formatting() {
        assert_eq!(amount_param(COIN), "1");
        assert_eq!(amount_param(100_000), "0.001");
        assert_eq!(amount_param(250_000_000), "2.5");
        assert_eq!(amount_param(1), "0.00000001");
        assert_eq!(amount_param(123_456_789), "1.23456789");
    }

    #[test]
    fn uri_is_accepted_by_the_reference_zip321_parser_in_both_encodings() {
        let mut rng = StdRng::seed_from_u64(6);
        let (_, pk) = generate_sealing_keypair(&mut rng);
        let memo =
            EntryMemo::seal(&FORMAT, 12, Digit(3), &receiver(7), &pk, &mut rng).expect("seal");
        let vault = receiver(8).to_unified_address().expect("ua");
        for (bytes, text) in [
            (memo.encode_unpadded(&FORMAT), false),
            (memo.encode_text(&FORMAT).into_bytes(), true),
        ] {
            let uri = entry_request_uri(Network::Test, &vault, 100_000, &bytes).expect("uri");
            assert!(
                uri.starts_with(&format!("zcash:{vault}?amount=0.001&memo=")),
                "{uri}"
            );
            let request = zip321::TransactionRequest::from_uri(&uri).expect("zip321 parses it");
            let payment = request.payments().get(&0).expect("one payment");
            assert_eq!(payment.amount().map(u64::from), Some(100_000));
            assert_eq!(payment.recipient_address().encode(), vault);
            let field = payment.memo().expect("memo").as_slice().to_vec();
            let decoded = if text {
                assert!(field[0] <= 0xF4, "a ZIP 302 text memo");
                EntryMemo::<Digit>::decode_text(&FORMAT, &field)
            } else {
                EntryMemo::<Digit>::decode(&FORMAT, &field)
            }
            .expect("decode");
            assert_eq!(decoded, memo);
            assert_eq!(request.to_uri(), uri, "canonical form is stable");
        }
    }

    #[test]
    fn rejects_bad_amounts_and_addresses() {
        let vault = receiver(1).to_unified_address().expect("ua");
        let memo = [0xFFu8; 10];
        assert_eq!(
            entry_request_uri(Network::Test, &vault, 0, &memo),
            Err(PaymentError::ZeroAmount)
        );
        assert_eq!(
            entry_request_uri(Network::Test, &vault, MAX_AMOUNT_ZAT + 1, &memo),
            Err(PaymentError::AmountTooLarge(MAX_AMOUNT_ZAT + 1))
        );
        assert_eq!(
            entry_request_uri(Network::Main, &vault, 1, &memo),
            Err(MemoError::AddressWrongNetwork.into())
        );
        assert_eq!(
            entry_request_uri(
                Network::Test,
                "tmEZhbWHTpdKMw5it8YDspUXSMGQyFwovpU",
                1,
                &memo
            ),
            Err(PaymentError::NoMemoReceiver)
        );
        assert_eq!(
            entry_request_uri(Network::Test, &vault, 1, &[0u8; 513]),
            Err(PaymentError::MemoTooLong(513))
        );
    }
}
