//! A coin flip keyholder: `zecraft-keyholder` with the coin flip's rules.

fn main() -> anyhow::Result<()> {
    zecraft_keyholder::cli::main(zecraft_coinflip::CoinFlip, "zecraft-coinflip-keyholder")
}
