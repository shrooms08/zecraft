//! The coin flip's independent verifier: `zecraft-verify` with the coin flip's rules.

fn main() -> anyhow::Result<()> {
    zecraft_verify::cli::main(zecraft_coinflip::CoinFlip, "zecraft-coinflip-verify")
}
