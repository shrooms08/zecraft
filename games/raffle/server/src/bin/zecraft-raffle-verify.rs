//! The raffle's independent verifier: `zecraft-verify` with the raffle's
//! rules (`round`, `win`, `relay`).

fn main() -> anyhow::Result<()> {
    zecraft_verify::cli::main(zecraft_raffle::Raffle, "zecraft-raffle-verify")
}
