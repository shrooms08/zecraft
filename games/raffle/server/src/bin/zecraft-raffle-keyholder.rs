//! A raffle keyholder: `zecraft-keyholder` with the raffle's rules
//! (`identity-keygen`, `identity`, `address`, `dkg`, `serve`).

fn main() -> anyhow::Result<()> {
    zecraft_keyholder::cli::main(zecraft_raffle::Raffle, "zecraft-raffle-keyholder")
}
