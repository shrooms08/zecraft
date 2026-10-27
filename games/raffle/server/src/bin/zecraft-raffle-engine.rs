//! The raffle's round engine: `zecraft-engine` with the raffle's rules.
//! Configured by environment (`ZECRAFT_GAME_CONFIG`, `ZECRAFT_ENGINE_BIND`,
//! `ZECRAFT_KEYHOLDERS`, ...; see `zecraft_engine::config`).

fn main() -> anyhow::Result<()> {
    zecraft_engine::cli::main(zecraft_raffle::Raffle, "zecraft-raffle-engine")
}
