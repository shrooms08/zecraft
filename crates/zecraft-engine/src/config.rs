//! Game configuration (committed, public) and engine settings (environment).

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use orchard::keys::{IncomingViewingKey, PreparedIncomingViewingKey};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use zcash_keys::keys::UnifiedIncomingViewingKey;
use zecraft_core::entry::{PayoutReceiver, SealingPublicKey};
use zecraft_core::params::{ChainParams, Network};
use zecraft_core::rules::Rules;

use crate::error::EngineError;

/// Public description of a game: everything a verifier needs besides the
/// chain. `P` is the game's own parameter type (`Rules::Params`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameConfig<P> {
    /// Network, round clock, fees and beacon (including `genesis_height`).
    pub chain: ChainParams,
    /// The game's parameters.
    pub game: P,
    /// The vault's unified address players pay to.
    pub vault_address: String,
    /// Public key players seal their payout receivers to (hex, 32 bytes).
    pub sealing_public_key: SealingPublicKey,
    /// Unified incoming viewing key, external scope only, bech32m.
    pub uivk: String,
    /// Value carried into round 0 from a previous vault (zatoshis). It is the
    /// first round's `carry_in` and an obligation in the ledger from the start.
    #[serde(default)]
    pub initial_carry_zat: u64,
}

impl<P: DeserializeOwned> GameConfig<P> {
    /// Reads and validates the game config file against `rules`.
    pub fn load<R: Rules<Params = P>>(path: &Path, rules: &R) -> Result<Self, EngineError> {
        let bytes = fs::read(path).map_err(|e| EngineError::io(path, e))?;
        let config: GameConfig<P> =
            serde_json::from_slice(&bytes).map_err(|e| EngineError::Json {
                path: path.to_path_buf(),
                source: e,
            })?;
        config.validate(rules)?;
        Ok(config)
    }
}

impl<P> GameConfig<P> {
    /// Checks the config is internally consistent and decodable.
    pub fn validate<R: Rules<Params = P>>(&self, rules: &R) -> Result<(), EngineError> {
        self.chain
            .validate()
            .map_err(|e| EngineError::Config(e.to_string()))?;
        rules
            .validate(&self.chain, &self.game)
            .map_err(|e| EngineError::Config(format!("game parameters: {e}")))?;
        PayoutReceiver::from_unified_address(self.chain.network, &self.vault_address)
            .map_err(|e| EngineError::Config(format!("vault_address: {e}")))?;
        self.incoming_viewing_key()?;
        Ok(())
    }

    /// The external-scope Orchard incoming viewing key, prepared for trial
    /// decryption of Orchard and Ironwood outputs.
    pub fn incoming_viewing_key(&self) -> Result<PreparedIncomingViewingKey, EngineError> {
        let ivk = self.orchard_ivk()?;
        Ok(PreparedIncomingViewingKey::new(&ivk))
    }

    fn orchard_ivk(&self) -> Result<IncomingViewingKey, EngineError> {
        let uivk = match self.chain.network {
            Network::Main => UnifiedIncomingViewingKey::decode(
                &zcash_protocol::consensus::MAIN_NETWORK,
                &self.uivk,
            ),
            Network::Test => UnifiedIncomingViewingKey::decode(
                &zcash_protocol::consensus::TEST_NETWORK,
                &self.uivk,
            ),
        }
        .map_err(|e| EngineError::Config(format!("uivk: {e}")))?;
        uivk.orchard()
            .clone()
            .ok_or_else(|| EngineError::Config("uivk has no Orchard component".into()))
    }
}

/// Environment variable names.
/// One or more `lightwalletd` servers, comma-separated (default: the
/// verified public servers of the game's network).
pub const LIGHTWALLETD_URL_VAR: &str = "ZECRAFT_LIGHTWALLETD_URL";
pub const GAME_CONFIG_VAR: &str = "ZECRAFT_GAME_CONFIG";
/// `on` or `off`: whether the engine advertises entries as open. Default:
/// on for testnet, off for mainnet.
pub const ENTRIES_VAR: &str = "ZECRAFT_ENTRIES";
/// Address of the public listener (the tunnel's target); unset disables it.
pub const PUBLIC_BIND_VAR: &str = "ZECRAFT_PUBLIC_BIND";
/// File holding the secret the Pages Function sends on every request.
pub const PROXY_SECRET_FILE_VAR: &str = "ZECRAFT_PROXY_SECRET_FILE";
/// Direct mode of the public listener (`crate::edge`): the header the host's
/// edge sets to the client address, for example `X-Forwarded-For` on
/// Railway. Unset: only the proxy may call the public listener.
pub const CLIENT_IP_HEADER_VAR: &str = "ZECRAFT_CLIENT_IP_HEADER";
pub const DB_PATH_VAR: &str = "ZECRAFT_ENGINE_DB";
pub const BIND_VAR: &str = "ZECRAFT_ENGINE_BIND";
pub const CORS_VAR: &str = "ZECRAFT_ENGINE_CORS";
pub const RELAYS_VAR: &str = "ZECRAFT_DRAND_RELAYS";
pub const POLL_SECS_VAR: &str = "ZECRAFT_ENGINE_POLL_SECS";

/// Default drand relays (League of Entropy, from docs.drand.love/developer/http-api):
/// the drand.sh relays speak v2, Cloudflare only v1 (by chain hash).
pub const DEFAULT_RELAYS: &str =
    "https://api.drand.sh,https://api2.drand.sh,https://api3.drand.sh,https://drand.cloudflare.com";

/// Blocks a settlement waits for after the last block the timing rule reads:
/// ZIP 315's untrusted-note depth, the same the vault requires before it
/// spends a received note.
pub const SETTLE_CONFIRMATIONS: u32 = 10;

/// Runtime settings of the engine process.
#[derive(Clone, Debug)]
pub struct EngineSettings {
    /// Configured `lightwalletd` servers; empty means the defaults for the
    /// game's network (see [`EngineSettings::servers`]).
    pub lightwalletd_servers: Vec<String>,
    pub game_config_path: PathBuf,
    pub db_path: PathBuf,
    pub bind: String,
    /// `ZECRAFT_ENTRIES`, when set.
    pub entries_open: Option<bool>,
    /// Public listener and the file holding its proxy secret.
    pub public: Option<(String, PathBuf)>,
    /// `ZECRAFT_CLIENT_IP_HEADER`, when set (direct mode).
    pub client_ip_header: Option<String>,
    /// Allowed CORS origins; empty means CORS headers are not sent.
    pub cors_origins: Vec<String>,
    pub relays: Vec<String>,
    pub poll_secs: u64,
}

impl EngineSettings {
    /// Reads the settings from the process environment.
    pub fn from_env() -> Result<Self, EngineError> {
        let var = |name: &str| -> Result<Option<String>, EngineError> {
            match env::var(name) {
                Ok(v) if v.trim().is_empty() => Ok(None),
                Ok(v) => Ok(Some(v)),
                Err(env::VarError::NotPresent) => Ok(None),
                Err(env::VarError::NotUnicode(_)) => Err(EngineError::Config(format!(
                    "{name} contains invalid UTF-8"
                ))),
            }
        };
        let lightwalletd_servers = var(LIGHTWALLETD_URL_VAR)?
            .map(|v| zecraft_vault::pool::parse_servers(&v))
            .unwrap_or_default();
        let poll_secs = match var(POLL_SECS_VAR)? {
            Some(v) => v.parse::<u64>().ok().filter(|s| *s > 0).ok_or_else(|| {
                EngineError::Config(format!("{POLL_SECS_VAR} must be a positive integer"))
            })?,
            None => 10,
        };
        let list = |value: Option<String>, default: &str| -> Vec<String> {
            value
                .unwrap_or_else(|| default.to_owned())
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect()
        };
        Ok(EngineSettings {
            lightwalletd_servers,
            game_config_path: var(GAME_CONFIG_VAR)?
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("config/zecraft-game.testnet.json")),
            db_path: var(DB_PATH_VAR)?
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(".zecraft/data/zecraft-engine.sqlite")),
            bind: var(BIND_VAR)?.unwrap_or_else(|| "127.0.0.1:7787".to_owned()),
            entries_open: match var(ENTRIES_VAR)?.as_deref() {
                None => None,
                Some("on") => Some(true),
                Some("off") => Some(false),
                Some(other) => {
                    return Err(EngineError::Config(format!(
                        "{ENTRIES_VAR} must be `on` or `off`, got `{other}`"
                    )))
                }
            },
            public: match var(PUBLIC_BIND_VAR)? {
                Some(bind) => Some((
                    bind,
                    var(PROXY_SECRET_FILE_VAR)?
                        .map(PathBuf::from)
                        .unwrap_or_else(|| PathBuf::from(".zecraft/secrets/proxy-secret")),
                )),
                None => None,
            },
            client_ip_header: var(CLIENT_IP_HEADER_VAR)?.filter(|h| !h.trim().is_empty()),
            cors_origins: list(var(CORS_VAR)?, ""),
            relays: list(var(RELAYS_VAR)?, DEFAULT_RELAYS),
            poll_secs,
        })
    }
}

impl EngineSettings {
    /// Whether entries are open on `network` (see [`ENTRIES_VAR`]).
    pub fn entries_open(&self, network: zecraft_core::params::Network) -> bool {
        self.entries_open
            .unwrap_or(network == zecraft_core::params::Network::Test)
    }

    /// The `lightwalletd` servers to use on `network`.
    pub fn servers(&self, network: zecraft_core::params::Network) -> Vec<String> {
        if self.lightwalletd_servers.is_empty() {
            zecraft_vault::pool::default_servers(network)
        } else {
            self.lightwalletd_servers.clone()
        }
    }

    /// A pool over [`EngineSettings::servers`] (the engine prefers the first).
    pub fn pool(
        &self,
        network: zecraft_core::params::Network,
    ) -> Result<zecraft_vault::pool::Pool, EngineError> {
        Ok(zecraft_vault::pool::Pool::new(
            network,
            self.servers(network),
            0,
        )?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synthetic::TestVault;

    #[test]
    fn config_validates_and_round_trips() {
        let vault = TestVault::new(1);
        let config = vault.config(1_000, zecraft_raffle::RaffleParams::default());
        let rules = zecraft_raffle::Raffle;
        config.validate(&rules).expect("valid");
        let json = serde_json::to_string_pretty(&config).expect("json");
        let back: GameConfig<zecraft_raffle::RaffleParams> =
            serde_json::from_str(&json).expect("parse");
        assert_eq!(back, config);
        back.incoming_viewing_key().expect("ivk");

        let mut bad = config.clone();
        bad.chain.blocks_per_round = 0;
        assert!(matches!(bad.validate(&rules), Err(EngineError::Config(_))));
        let mut bad = config.clone();
        bad.uivk = "uivktest1nonsense".into();
        assert!(matches!(bad.validate(&rules), Err(EngineError::Config(_))));
        let mut bad = config;
        bad.vault_address = "tmEZhbWHTpdKMw5it8YDspUXSMGQyFwovpU".into();
        assert!(matches!(bad.validate(&rules), Err(EngineError::Config(_))));
    }
}
