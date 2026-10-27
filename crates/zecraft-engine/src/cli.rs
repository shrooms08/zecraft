//! The round server's process: follows the chain, settles rounds with a
//! game's [`Rules`], coordinates payouts and serves the API. A game's engine
//! binary is one call: `crate::cli::main(MyGame, "my-game-engine")`.

use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::api::{router, AppState};
use crate::beacon::RelayFetcher;
use crate::chain::{self, PoolSource};
use crate::config::{EngineSettings, GameConfig};
use crate::coordinator::{Coordinator, CoordinatorSettings};
use crate::decrypt::EntryScanner;
use crate::edge;
use crate::processor::{Event, Processor};
use crate::relay::{LightwalletdBackend, RelayState};
use crate::store::Store;
use anyhow::Context;
use clap::{FromArgMatches, Parser};
use tokio::sync::broadcast;
use zecraft_core::rules::Rules;

#[derive(Parser)]
#[command(version, about = "Zecraft round server (Zcash testnet)")]
struct Cli {}

/// Runs the engine for `rules` until Ctrl-C. `name` is the binary's name,
/// shown by `--help`.
pub fn main<R: Rules + Clone>(rules: R, name: &'static str) -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();
    use clap::CommandFactory;
    let Cli {} = Cli::from_arg_matches_mut(&mut Cli::command().name(name).get_matches())?;
    let _log_guard = crate::logging::init("engine")?;
    let settings = EngineSettings::from_env()?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("starting async runtime")?;
    let config = GameConfig::load(&settings.game_config_path, &rules)
        .with_context(|| format!("loading {}", settings.game_config_path.display()))?;
    runtime.block_on(run(rules, settings, config))
}

/// Worker threads of the chain loop's own runtime.
const CHAIN_WORKERS: usize = 2;

/// How often every lightwalletd server is health-checked.
const PROBE_EVERY: Duration = Duration::from_secs(60);

/// How often the vault wallet is synced between payouts, so the public
/// ledger's vault balance is never far behind the settlements.
const VAULT_SYNC_EVERY: Duration = Duration::from_secs(300);

/// Longest an engine pass may run (a payout proof takes about a minute).
const PASS_TIMEOUT: Duration = Duration::from_secs(600);

async fn run<R: Rules + Clone>(
    rules: R,
    settings: EngineSettings,
    config: GameConfig<R::Params>,
) -> anyhow::Result<()> {
    let scanner = EntryScanner::new(config.incoming_viewing_key()?);
    let store = Arc::new(Store::open(&settings.db_path)?);
    // One pool for the chain loop, the relay and the coordinator: one health
    // view, one failover order.
    let pool = Arc::new(settings.pool(config.chain.network)?);
    pool.probe().await;
    tracing::info!(servers = ?settings.servers(config.chain.network), "lightwalletd pool");
    let source = PoolSource::new(pool.clone());
    let fetcher = RelayFetcher::new(settings.relays.clone())?;
    let (events, _) = broadcast::channel::<Event>(256);
    tracing::info!(
        genesis = config.chain.genesis_height,
        blocks_per_round = config.chain.blocks_per_round,
        db = %settings.db_path.display(),
        "engine starting"
    );

    let entries_open = settings.entries_open(config.chain.network);
    if !entries_open {
        tracing::warn!(network = ?config.chain.network, "READINESS MODE: entries are off; the vault address is withheld and nothing is paid or served publicly");
    }
    let coordinator_settings = CoordinatorSettings::from_env()?;
    anyhow::ensure!(
        entries_open || coordinator_settings.is_none(),
        "entries are off: the payout coordinator must not run (unset ZECRAFT_KEYHOLDERS)"
    );
    anyhow::ensure!(
        entries_open || settings.public.is_none(),
        "entries are off: the engine must not be exposed (unset ZECRAFT_PUBLIC_BIND)"
    );
    let coordinator = match coordinator_settings {
        Some(settings) => {
            let coordinator = Coordinator::new(settings, pool.clone())?;
            tracing::info!(
                vault = coordinator.vault_address(),
                "payout coordinator enabled"
            );
            Some(coordinator)
        }
        None => None,
    };
    let (keyholder_ids, threshold) = match &coordinator {
        Some(c) => c.keyholder_ids(),
        None => (Vec::new(), 0),
    };
    let state = Arc::new(AppState {
        keyholder_ids,
        threshold,
        processor: Processor::new(rules.clone(), config.clone()),
        store: store.clone(),
        events: events.clone(),
        started: Instant::now(),
        ledger: coordinator.as_ref().map(Coordinator::ledger_view),
        chain_servers: Some(pool.clone()),
        entries_open,
        stream_slots: Arc::new(tokio::sync::Semaphore::new(crate::api::MAX_STREAMS)),
        relay: Some(Arc::new(RelayState::new(
            Box::new(LightwalletdBackend::new(pool.clone())),
            RelayState::rate_from_env(),
        ))),
        proof_limiter: crate::relay::RateLimiter::new(crate::api::PROOF_SUBMISSIONS_PER_MIN),
    });

    let listener = tokio::net::TcpListener::bind(&settings.bind)
        .await
        .with_context(|| format!("binding {}", settings.bind))?;
    tracing::info!(bind = %settings.bind, "local API listening");
    let app = router(state.clone(), &settings.cors_origins);
    let local = edge::local(app.clone());
    let mut servers = tokio::task::JoinSet::new();
    servers.spawn(async move {
        axum::serve(
            listener,
            local.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
    });
    if let Some((bind, secret_file)) = &settings.public {
        let secret = std::fs::read_to_string(secret_file)
            .with_context(|| format!("reading the proxy secret {}", secret_file.display()))?;
        let secret = edge::ProxySecret::new(&secret)?;
        let access = match &settings.client_ip_header {
            Some(header) => edge::PublicAccess::direct(secret, header)?,
            None => edge::PublicAccess::proxy_only(secret),
        };
        let direct = access.is_direct();
        let public = edge::public(app, access);
        let listener = tokio::net::TcpListener::bind(bind)
            .await
            .with_context(|| format!("binding {bind}"))?;
        if direct {
            tracing::info!(bind = %bind, "public API listening (direct, and the proxy)");
        } else {
            tracing::info!(bind = %bind, "public API listening (proxy secret required)");
        }
        servers.spawn(async move {
            axum::serve(
                listener,
                public.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .with_graceful_shutdown(async {
                let _ = tokio::signal::ctrl_c().await;
            })
            .await
        });
    }
    let server = async move {
        while let Some(result) = servers.join_next().await {
            match result {
                Ok(Ok(())) => {}
                Ok(Err(e)) => return Err(anyhow::Error::from(e)),
                Err(e) => return Err(anyhow::Error::from(e)),
            }
        }
        Ok(())
    };

    // The chain loop runs on its own runtime and OS threads, so its
    // synchronous CPU work (trial decryption, wallet scanning, transaction
    // building) never occupies a worker the API needs: on a shared runtime a
    // busy worker strands requests queued behind it (seen live: a 1.9 s
    // board read while a payout was being built). Inside, it is still a
    // spawned task, so the store's guards (not `Send`) cannot be held across
    // an `.await` (network, beacon, proving, signing).
    let (done_tx, done_rx) = tokio::sync::oneshot::channel::<Result<(), String>>();
    let loop_state = state.clone();
    let poll_secs = settings.poll_secs;
    std::thread::Builder::new()
        .name("zecraft-chain".into())
        .spawn(move || {
            let outcome = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(CHAIN_WORKERS)
                .thread_name("zecraft-chain")
                .enable_all()
                .build()
                .map_err(|e| e.to_string())
                .and_then(|rt| {
                    rt.block_on(async {
                        tokio::spawn(chain_loop(
                            rules,
                            loop_state,
                            source,
                            fetcher,
                            coordinator,
                            scanner,
                            config,
                            poll_secs,
                        ))
                        .await
                        .map_err(|e| e.to_string())
                    })
                });
            let _ = done_tx.send(outcome);
        })
        .context("starting the chain loop thread")?;
    tokio::select! {
        result = server => {
            if let Err(e) = result {
                tracing::error!(error = %e, "API server stopped");
            }
        }
        result = done_rx => {
            match result {
                Ok(Ok(())) => {}
                Ok(Err(e)) => tracing::error!(error = %e, "chain loop stopped"),
                Err(_) => tracing::error!("chain loop thread ended without a result"),
            }
        }
    }
    tracing::info!("engine stopping");
    Ok(())
}

/// Follows the chain, settles rounds and pays them, until Ctrl-C.
#[allow(clippy::too_many_arguments)]
async fn chain_loop<R: Rules + Clone>(
    rules: R,
    state: Arc<AppState<R>>,
    mut source: PoolSource,
    mut fetcher: RelayFetcher,
    mut coordinator: Option<Coordinator>,
    scanner: EntryScanner,
    config: GameConfig<R::Params>,
    poll_secs: u64,
) {
    let store = state.store.clone();
    let mut processor = Processor::new(rules, config.clone());
    let mut last_board = None;
    let mut last_probe = Instant::now();
    // The first pass syncs the vault wallet at once.
    let mut last_vault_sync: Option<Instant> = None;
    loop {
        // Health check of every server (tips, latency) for the status page;
        // failures found by ordinary calls are recorded as they happen.
        if last_probe.elapsed() >= PROBE_EVERY {
            source.pool().probe().await;
            last_probe = Instant::now();
        }
        let pass = async {
            let report = chain::ingest(&mut source, &store, &scanner, &config.chain).await?;
            if report.new_blocks > 0 || report.new_notes > 0 || report.reorg_from.is_some() {
                tracing::info!(
                    tip = ?report.stored_tip,
                    chain_tip = report.chain_tip,
                    new_blocks = report.new_blocks,
                    new_notes = report.new_notes,
                    reorg_from = ?report.reorg_from,
                    "ingested"
                );
            }
            let before = store.read(|db| processor.pending_rounds(db))?;
            let mut events_out = processor.process(&store, &mut fetcher, unix_now()).await?;
            let after = store.read(|db| processor.pending_rounds(db))?;
            for closed in after
                .iter()
                .filter(|p| !before.iter().any(|b| b.round == p.round))
            {
                events_out.push(Event::RoundClosed {
                    round: closed.round,
                    window: closed.window,
                    drand_round: closed.drand_round,
                    beacon_time: closed.beacon_time,
                });
            }
            if let Some(coordinator) = coordinator.as_mut() {
                match coordinator.pay_next(&store, &processor).await {
                    Ok(Some(report)) => events_out.push(Event::Paid {
                        round: report.round,
                        txid: report.txid,
                        signers: report.signers,
                    }),
                    Ok(None) => {}
                    Err(e) => tracing::error!(error = %e, "payout attempt failed"),
                }
                if let Err(e) = coordinator.probe_keyholders(&store).await {
                    tracing::warn!(error = %e, "keyholder health probe failed");
                }
                if last_vault_sync.is_none_or(|t| t.elapsed() >= VAULT_SYNC_EVERY) {
                    match coordinator.sync_vault_wallet().await {
                        Ok(tip) => {
                            last_vault_sync = Some(Instant::now());
                            tracing::debug!(tip, "vault wallet synced");
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "vault wallet sync failed; retrying next pass")
                        }
                    }
                }
                if let Err(e) = coordinator
                    .record_openings(&store, &mut source, &processor)
                    .await
                {
                    tracing::warn!(error = %e, "recording payout openings failed; retrying next pass");
                }
            }
            let board = store.read(|db| processor.board(db))?;
            if last_board.as_ref() != Some(&board) {
                events_out.push(Event::Board(board.clone()));
                last_board = Some(board);
            }
            Ok::<_, anyhow::Error>(events_out)
        };
        // A backstop against a call that never returns. No database
        // connection is held across a network call, so the API keeps
        // answering either way; every step resumes from the database.
        let pass = tokio::time::timeout(PASS_TIMEOUT, pass)
            .await
            .unwrap_or_else(|_| {
                Err(anyhow::anyhow!(
                    "engine pass timed out after {PASS_TIMEOUT:?}"
                ))
            });
        match pass {
            Ok(events_out) => {
                for event in events_out {
                    let _ = state.events.send(event);
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "engine pass failed; retrying after the poll interval")
            }
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(poll_secs)) => {}
            _ = tokio::signal::ctrl_c() => break,
        }
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
