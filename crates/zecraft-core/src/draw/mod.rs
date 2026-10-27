//! The draw: binding a round to one drand beacon, verifying the beacon,
//! proving the round's window closed before the beacon existed, and drawing
//! unbiased numbers from it.
//!
//! * [`rounds`]: height ↔ round arithmetic and [`RoundStatus`].
//! * [`beacon`]: drand quicknet verification and [`beacon_round_for`].
//! * [`timing`]: the median-time-past rule ([`check_window_close`]) that
//!   voids a round whose close cannot be shown to precede its beacon.
//! * [`random`]: [`uniform_below`], with a caller-supplied domain.

pub mod beacon;
pub mod random;
pub mod rounds;
pub mod timing;

pub use beacon::{beacon_round_for, beacon_time, verify_beacon, Beacon, BeaconError, Randomness};
pub use random::{uniform_below, RandomError};
pub use rounds::{
    round_for_height, round_status, round_window, RoundContext, RoundError, RoundStatus,
    RoundWindow,
};
pub use timing::{check_window_close, median_time_past, TimingError, TimingVerdict};
