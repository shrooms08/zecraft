//! The keyholder's command line, for any game: a game's keyholder binary is
//! one call, `crate::cli::main(MyGame, "my-game-keyholder")`.
//! `identity-keygen`, `identity`, `address` and `dkg` need no game; `serve`
//! follows the chain and verifies payouts with the game's rules.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::api::{dkg_router, router, AppState, DkgNode};
use crate::config::KeyholderConfig;
use crate::db::NodeDb;
use crate::dkg::{self, DkgState, KeyMaterial};
use crate::follower::Follower;
use crate::identity::Identity;
use crate::signing::SignerState;
use anyhow::Context;
use clap::{Args, Parser, Subcommand};
use tokio::sync::{Mutex, RwLock};
use zecraft_core::rules::Rules;
use zecraft_engine::config::{GameConfig, DEFAULT_RELAYS};
use zecraft_engine::processor::Processor;
use zecraft_engine::store::Store;

#[derive(Parser)]
#[command(
    version,
    about = "Zecraft keyholder node (FROST DKG participant and payout verifier)"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Generate an ed25519 identity key under a secrets directory and print
    /// its public key (used for keyholders and for the coordinator).
    IdentityKeygen(KeygenArgs),
    /// Print this node's identity public key.
    Identity(ConfigArgs),
    /// Print the vault address and UIVK this node derived in the DKG ceremony.
    Address(ConfigArgs),
    /// Serve the API, follow the chain, and (with --dkg) run the ceremony first.
    Serve(ServeArgs),
    /// Run a DKG ceremony only (serving just the DKG endpoints), for a vault
    /// whose game config cannot exist before its address does (a new
    /// network). Exits when the ceremony completes.
    Dkg(DkgArgs),
}

#[derive(Args)]
struct DkgArgs {
    /// Node config file.
    #[arg(long)]
    config: PathBuf,
    /// 32-byte hex session id (same on all nodes).
    #[arg(long)]
    session: String,
    /// Network the vault is for: `main` or `test`.
    #[arg(long)]
    network: String,
}

#[derive(Args)]
struct ConfigArgs {
    /// Node config file (public, committed).
    #[arg(long, env = "ZECRAFT_KEYHOLDER_CONFIG")]
    config: PathBuf,
}

#[derive(Args)]
struct KeygenArgs {
    /// Secrets directory to write identity.json into (created 0700).
    #[arg(long)]
    secrets_dir: PathBuf,
    /// Replace an existing identity file.
    #[arg(long)]
    force: bool,
}

#[derive(Args)]
struct ServeArgs {
    /// Node config file (public, committed).
    #[arg(long, env = "ZECRAFT_KEYHOLDER_CONFIG")]
    config: PathBuf,
    /// Run the DKG ceremony with this 32-byte hex session id (same on all nodes).
    #[arg(long)]
    dkg: Option<String>,
}

/// Runs the keyholder command line for `rules`.
pub fn main<R: Rules + Clone>(rules: R, name: &'static str) -> anyhow::Result<()> {
    use clap::{CommandFactory, FromArgMatches};
    let _ = dotenvy::dotenv();
    // Each keyholder service has its own ZECRAFT_LOG_DIR.
    let _log_guard = zecraft_engine::logging::init("keyholder")?;
    let cli = Cli::from_arg_matches_mut(&mut Cli::command().name(name).get_matches())?;
    let load = |path: &PathBuf| {
        KeyholderConfig::load(path).with_context(|| format!("loading {}", path.display()))
    };
    match cli.command {
        Command::IdentityKeygen(args) => {
            let identity = Identity::generate();
            identity.save(&args.secrets_dir.join("identity.json"), args.force)?;
            println!("{}", identity.public_key_hex());
            Ok(())
        }
        Command::Identity(args) => {
            let config = load(&args.config)?;
            println!(
                "{}",
                Identity::load(&config.identity_path())?.public_key_hex()
            );
            Ok(())
        }
        Command::Address(args) => {
            let config = load(&args.config)?;
            let keys = KeyMaterial::load(&config)?;
            println!("{}", keys.vault.address);
            println!("{}", keys.vault.uivk()?);
            Ok(())
        }
        Command::Dkg(args) => {
            let config = load(&args.config)?;
            let network = zecraft_vault::parse_network(&args.network)?;
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .context("starting async runtime")?
                .block_on(ceremony_only(config, network, &args.session))
        }
        Command::Serve(args) => {
            let config = load(&args.config)?;
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .context("starting async runtime")?
                .block_on(serve(rules, config, args))
        }
    }
}

fn parse_session(hex_id: &str) -> anyhow::Result<[u8; 32]> {
    hex::decode(hex_id)
        .ok()
        .and_then(|b| <[u8; 32]>::try_from(b.as_slice()).ok())
        .context("the session id must be 32 hex bytes")
}

/// Prints the ceremony's public result.
fn report(output: &dkg::DkgOutput, config: &KeyholderConfig) {
    println!("DKG complete. Vault address: {}", output.address);
    if let Some(pk) = &output.sealing_public_key {
        println!("Payout sealing public key: {}", hex::encode(pk.to_bytes()));
        println!(
            "Sealing key written to {}; copy it to the other keyholders and the engine (local-file distribution).",
            config.secrets_dir.join("payout-sealing.json").display()
        );
    } else {
        println!(
            "Waiting for the lead keyholder's payout-sealing.json to be copied into {}",
            config.secrets_dir.display()
        );
    }
}

async fn ceremony_only(
    config: KeyholderConfig,
    network: zecraft_core::params::Network,
    session_hex: &str,
) -> anyhow::Result<()> {
    let identity = Identity::load(&config.identity_path())
        .context("loading the identity key (run `identity-keygen` first)")?;
    let session = parse_session(session_hex)?;
    let node = Arc::new(DkgNode {
        config: config.clone(),
        state: DkgState::default(),
    });
    let listener = tokio::net::TcpListener::bind(&config.bind)
        .await
        .with_context(|| format!("binding {}", config.bind))?;
    let app = dkg_router(node.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;
    let output =
        dkg::run_ceremony(&config, network, &identity, &node.state, session, &client).await;
    // Every peer's confirmation has arrived and ours was delivered before
    // the ceremony returned, so the inbox can close.
    server.abort();
    let output = output?;
    report(&output, &config);
    Ok(())
}

/// Loads this node's key material and checks its sealing key is the game's:
/// with another key no receiver would open, every payout would look
/// unclaimable, and the node would approve leaving winners unpaid (review N7).
fn load_keys<P>(
    config: &KeyholderConfig,
    game: &GameConfig<P>,
) -> Result<KeyMaterial, crate::KeyholderError> {
    let keys = KeyMaterial::load(config)?;
    if keys.sealing.public_key().to_bytes() != game.sealing_public_key.to_bytes() {
        return Err(crate::KeyholderError::Config(
            "this node's payout sealing key does not match the game config's sealing_public_key"
                .into(),
        ));
    }
    Ok(keys)
}

/// Longest a chain follower pass may run.
const FOLLOWER_PASS_TIMEOUT: Duration = Duration::from_secs(300);

async fn serve<R: Rules + Clone>(
    rules: R,
    config: KeyholderConfig,
    args: ServeArgs,
) -> anyhow::Result<()> {
    let identity = Identity::load(&config.identity_path())
        .context("loading the identity key (run `identity-keygen` first)")?;
    let game = GameConfig::load(&config.game_config, &rules)?;
    let chain = Arc::new(Store::open(&config.engine_db_path())?);
    let node_db = NodeDb::open(&config.node_db_path())?;
    let keys = match load_keys(&config, &game) {
        Ok(k) => Some(Arc::new(k)),
        Err(crate::KeyholderError::NoKeyShare) => None,
        Err(e) => return Err(e.into()),
    };
    tracing::info!(
        identifier = config.identifier,
        has_key_share = keys.is_some(),
        bind = %config.bind,
        "keyholder starting"
    );
    let relays: Vec<String> = DEFAULT_RELAYS.split(',').map(str::to_owned).collect();
    // Keyholder n starts at server n-1, so with several healthy servers the
    // three nodes read the chain from different upstreams.
    let prefer = usize::from(config.identifier.saturating_sub(1));
    let mut follower = Follower::new(
        rules.clone(),
        game.clone(),
        config.lightwalletd.clone(),
        prefer,
        relays,
    )?;
    let state = Arc::new(AppState {
        config: config.clone(),
        game: game.clone(),
        processor: Processor::new(rules, game.clone()),
        identity,
        dkg: Arc::new(DkgNode {
            config: config.clone(),
            state: DkgState::default(),
        }),
        signer: SignerState::default(),
        keys: RwLock::new(keys),
        chain: chain.clone(),
        node_db: Mutex::new(node_db),
        chain_servers: follower.pool(),
    });

    let listener = tokio::net::TcpListener::bind(&config.bind)
        .await
        .with_context(|| format!("binding {}", config.bind))?;
    let app = router(state.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = tokio::signal::ctrl_c().await;
            })
            .await
    });

    if let Some(session_hex) = args.dkg {
        if state.keys.read().await.is_some() {
            tracing::info!("key share already present; skipping the DKG ceremony");
        } else {
            let session = parse_session(&session_hex)?;
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()?;
            let output = dkg::run_ceremony(
                &config,
                game.chain.network,
                &state.identity,
                &state.dkg.state,
                session,
                &client,
            )
            .await?;
            report(&output, &config);
            match load_keys(&config, &game) {
                Ok(k) => *state.keys.write().await = Some(Arc::new(k)),
                Err(e) => {
                    tracing::warn!(error = %e, "key material not loadable yet (sealing key missing?); restart after copying it")
                }
            }
        }
    }

    loop {
        if server.is_finished() {
            break;
        }
        // A backstop against a call that never returns; health and signing
        // read through their own connections meanwhile.
        let pass = match tokio::time::timeout(FOLLOWER_PASS_TIMEOUT, follower.pass(&chain)).await {
            Ok(result) => result.map_err(|e| e.to_string()),
            Err(_) => Err(format!(
                "follower pass timed out after {FOLLOWER_PASS_TIMEOUT:?}"
            )),
        };
        if let Err(e) = pass {
            tracing::error!(error = %e, "follower pass failed; retrying after the poll interval");
        }
        if state.keys.read().await.is_none() {
            match load_keys(&config, &game) {
                Ok(k) => {
                    tracing::info!("key material loaded");
                    *state.keys.write().await = Some(Arc::new(k));
                }
                Err(crate::KeyholderError::NoKeyShare) => {}
                Err(e) => tracing::error!(error = %e, "key material refused"),
            }
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(config.poll_secs)) => {}
            _ = tokio::signal::ctrl_c() => break,
        }
    }
    tracing::info!("keyholder stopping");
    Ok(())
}
