//! The coin flip's round engine: `zecraft-engine` with the coin flip's rules.

fn main() -> anyhow::Result<()> {
    zecraft_engine::cli::main(zecraft_coinflip::CoinFlip, "zecraft-coinflip-engine")
}
