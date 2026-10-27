//! Entries: how a player's shielded payment to the vault names its round and
//! its game payload, and where its winnings or refund go.
//!
//! * [`memo`]: [`EntryMemo`], laid out by the game's [`MemoFormat`] around
//!   its fixed-length [`Payload`], with binary and text encodings.
//! * [`seal`]: HPKE sealing of the payout receiver to the vault.
//! * [`payment`]: the ZIP 321 request a wallet pays ([`entry_request_uri`]).

pub mod memo;
pub mod payment;
pub mod seal;

pub use memo::{
    open_receiver, EntryMemo, MemoError, MemoFormat, Payload, PayloadError, PayoutReceiver,
    SealedReceiver, MEMO_LEN, ORCHARD_RECEIVER_LEN, SEALED_RECEIVER_LEN,
};
pub use payment::{amount_param, entry_request_uri, PaymentError};
pub use seal::{
    generate_sealing_keypair, open, seal, SealError, SealingPrivateKey, SealingPublicKey,
};
