//! HTTP API (axum): public game state for players, UIs and verifiers.
//!
//! Every response is built from the database through the processor; sealed
//! receivers are served as sealed bytes, nothing here can open them.

use std::sync::Arc;
use std::time::Instant;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::{HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tower_http::cors::{AllowOrigin, CorsLayer};
use zecraft_core::draw::{RoundContext, RoundStatus, RoundWindow};
use zecraft_core::entry::SealingPublicKey;
use zecraft_core::params::ChainParams;
use zecraft_core::rules::{Payout, PayoutKind, Rules};

use crate::config::{GameConfig, SETTLE_CONFIRMATIONS};
use crate::coordinator::LedgerView;
use crate::db::{Db, PayoutRecord, PlanStatus};
use crate::error::EngineError;
use crate::ledger::Ledger;
use crate::processor::{BoardState, Event, PendingRound, Processor};
use crate::proof::ProofBundle;
use crate::store::{Reader, Store};

/// Shared state of the HTTP handlers.
pub struct AppState<R: Rules> {
    pub processor: Processor<R>,
    /// Shared with the engine loop; handlers read through its read pool.
    pub store: Arc<Store>,
    pub events: broadcast::Sender<Event>,
    pub started: Instant,
    /// Present when this engine also coordinates payouts.
    /// The vault ledger (payout coordinator engines only).
    pub ledger: Option<LedgerView>,
    /// The engine's `lightwalletd` servers (for the status page).
    pub chain_servers: Option<Arc<zecraft_vault::pool::Pool>>,
    /// Whether entries are open (false: the vault is withheld).
    pub entries_open: bool,
    /// Open `/v1/stream` connections allowed at once.
    pub stream_slots: Arc<tokio::sync::Semaphore>,
    /// Present when this engine relays chain data to browser wallets.
    pub relay: Option<Arc<crate::relay::RelayState>>,
    /// Where `/relay/*` goes: another relay (proxied, GET only), or this
    /// engine's own relay when `None`.
    pub relay_upstream: Option<String>,
    /// The built web page, served at `/` (with `index.html` for unknown paths).
    pub web_dir: Option<std::path::PathBuf>,
    /// Configured keyholder identifiers and the signing threshold (empty
    /// when this engine does not coordinate payouts).
    pub keyholder_ids: Vec<u16>,
    pub threshold: u16,
    /// Rate limit of win proof submissions (verification costs about a
    /// second of CPU each).
    pub proof_limiter: crate::relay::RateLimiter,
}

/// `GET /v1/board`: the board.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BoardResponse {
    #[serde(flatten)]
    pub board: BoardState,
}

/// Win proof submissions per minute per client address.
pub const PROOF_SUBMISSIONS_PER_MIN: u32 = 10;
/// Largest accepted submission body (a proof is about 7.5 kB, 10 kB as base64url).
pub const MAX_PROOF_BODY: usize = 32 * 1024;

/// `POST /v1/proofs` body.
#[derive(Clone, Debug, Deserialize)]
pub struct SubmitProof {
    /// The win proof, base64url (no padding).
    pub proof: String,
}

/// `POST /v1/proofs` and `GET /v1/proofs/{id}`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredProof {
    pub id: String,
    pub round: u32,
    pub created_at: u64,
    /// The proof, base64url.
    pub proof: String,
    /// The engine's verdict (anyone can recheck with `zecraft-verify win`).
    pub verdict: zecraft_verify::win::WinVerdict,
}

/// The game's memo format, so clients can build entries.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoInfo {
    /// The four magic bytes, as ASCII.
    pub magic: String,
    pub version: u8,
    pub text_prefix: String,
}

/// `GET /v1/config`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConfigResponse<P> {
    pub chain: ChainParams,
    pub game: P,
    pub memo: MemoInfo,
    /// Whether this engine accepts entries. When false the vault's address,
    /// sealing key and viewing key are withheld, so nobody can send funds to
    /// a vault that is not open.
    pub entries_open: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vault_address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sealing_public_key: Option<SealingPublicKey>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uivk: Option<String>,
    /// Confirmations the engine waits for after the timing-rule blocks.
    pub settle_confirmations: u32,
    /// Value carried into round 0 from a previous vault.
    pub initial_carry_zat: u64,
}

/// `GET /v1/rounds/pending`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingResponse {
    pub rounds: Vec<PendingRound>,
}

/// Post-settlement details of a round.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SettledRound {
    pub voided: bool,
    /// The game's outcome.
    pub outcome: serde_json::Value,
    pub drand_round: u64,
    pub beacon_time: u64,
    /// Everything the round received.
    pub received: u64,
    pub carry_in: u64,
    pub carry_out: u64,
    /// Outputs the keyholders must sign (sealed receivers only).
    pub payouts: Vec<Payout>,
    pub paid: u64,
    pub fees: u64,
    pub plan_status: PlanStatus,
    pub settled_at_height: u32,
    /// Payout transaction, once broadcast.
    pub payout: Option<PayoutRecord>,
}

/// `GET /v1/rounds/{n}`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RoundResponse {
    pub round: u32,
    pub status: RoundStatus,
    pub window: RoundWindow,
    pub context: RoundContext,
    /// Valid entries counted so far (or at settlement).
    pub entries: usize,
    /// Refunded entries.
    pub refunds: usize,
    /// The game's board of the round (`Rules::board`), as-is.
    pub game: serde_json::Value,
    pub settled: Option<SettledRound>,
}

/// `GET /v1/health`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthResponse {
    pub ok: bool,
    pub tip_height: Option<u32>,
    pub settled_rounds: usize,
    pub uptime_secs: u64,
}

/// JSON error body.
#[derive(Debug, Serialize, Deserialize)]
pub struct ErrorBody {
    pub error: String,
}

/// Error wrapper for handlers.
pub struct ApiError(StatusCode, String);

impl From<EngineError> for ApiError {
    /// Internal details (database, paths, configuration) stay in the log.
    fn from(e: EngineError) -> Self {
        if let EngineError::Busy(why) = e {
            tracing::warn!(why, "request refused: busy");
            return ApiError(
                StatusCode::SERVICE_UNAVAILABLE,
                "busy; try again in a moment".into(),
            );
        }
        tracing::error!(error = %e, "request failed");
        internal()
    }
}

/// The body of every internal error (security review L1).
fn internal() -> ApiError {
    ApiError(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal error; it has been logged".into(),
    )
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(ErrorBody { error: self.1 })).into_response()
    }
}

type ApiResult<T> = Result<Json<T>, ApiError>;

/// A read-only connection: never waits for the engine loop's writes.
fn reader<R: Rules>(state: &AppState<R>) -> Result<Reader<'_>, ApiError> {
    Ok(state.store.reader()?)
}

/// Builds the router.
pub fn router<R: Rules>(state: Arc<AppState<R>>, cors_origins: &[String]) -> Router {
    let mut router = Router::new()
        .route("/v1/config", get(config::<R>))
        .route("/v1/board", get(board::<R>))
        .route("/v1/rounds/pending", get(pending::<R>))
        .route("/v1/rounds/{round}", get(round::<R>))
        .route("/v1/rounds/{round}/proof", get(proof::<R>))
        .route("/v1/health", get(health::<R>))
        .route("/v1/status", get(status::<R>))
        .route("/v1/vault/ledger", get(vault_ledger::<R>))
        .route("/v1/stream", get(stream::<R>))
        .route("/v1/rounds", get(rounds_list::<R>))
        .route("/v1/rounds/{round}/digests", get(round_digests::<R>))
        .route("/v1/keyholders/status", get(keyholders_status::<R>))
        .route("/v1/proofs", axum::routing::post(submit_proof::<R>))
        .route("/v1/proofs/{id}", get(get_proof::<R>))
        .layer(axum::extract::DefaultBodyLimit::max(MAX_PROOF_BODY))
        .with_state(state.clone());
    if let Some(relay) = &state.relay {
        router = router.merge(crate::relay::router(relay.clone()));
    }
    // One origin for the browser: the relay also under /relay.
    match (&state.relay_upstream, &state.relay) {
        (Some(upstream), _) => {
            let proxy = Arc::new(RelayProxy {
                upstream: upstream.trim_end_matches('/').to_owned(),
                http: reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(60))
                    .build()
                    .unwrap_or_default(),
            });
            router = router.route("/relay/{*path}", get(relay_proxy).with_state(proxy));
        }
        (None, Some(relay)) => {
            router = router.nest("/relay", crate::relay::router(relay.clone()));
        }
        (None, None) => {}
    }
    if let Some(dir) = &state.web_dir {
        let index = dir.join("index.html");
        router = router.fallback_service(
            tower_http::services::ServeDir::new(dir)
                .fallback(tower_http::services::ServeFile::new(index)),
        );
    }
    if !cors_origins.is_empty() {
        let origins: Vec<HeaderValue> = cors_origins
            .iter()
            .filter_map(|o| HeaderValue::from_str(o).ok())
            .collect();
        router = router.layer(
            CorsLayer::new()
                .allow_origin(AllowOrigin::list(origins))
                .allow_methods([Method::GET, Method::POST])
                .allow_headers([axum::http::header::CONTENT_TYPE]),
        );
    }
    router
}

/// `/relay/*` forwarded to another relay.
pub struct RelayProxy {
    upstream: String,
    http: reqwest::Client,
}

/// Largest relay response passed through (500 compact blocks fit easily).
const MAX_PROXIED_BYTES: usize = 32 * 1024 * 1024;

async fn relay_proxy(
    State(proxy): State<Arc<RelayProxy>>,
    Path(path): Path<String>,
    axum::extract::RawQuery(query): axum::extract::RawQuery,
) -> Response {
    // Only the relay's own read routes are forwarded.
    if !(path.starts_with("v1/chain/") || path.starts_with("v1/tx/")) || path.contains("..") {
        return bad(StatusCode::NOT_FOUND, "not a relay route").into_response();
    }
    let url = match query {
        Some(q) => format!("{}/{path}?{q}", proxy.upstream),
        None => format!("{}/{path}", proxy.upstream),
    };
    let upstream = match proxy.http.get(&url).send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "relay upstream unreachable");
            return bad(StatusCode::BAD_GATEWAY, "the relay upstream is unreachable")
                .into_response();
        }
    };
    let status =
        StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let content_type = upstream
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_owned();
    match upstream.bytes().await {
        Ok(body) if body.len() <= MAX_PROXIED_BYTES => (
            status,
            [(axum::http::header::CONTENT_TYPE, content_type)],
            body,
        )
            .into_response(),
        Ok(_) => bad(StatusCode::BAD_GATEWAY, "relay response too large").into_response(),
        Err(_) => bad(StatusCode::BAD_GATEWAY, "relay response cut off").into_response(),
    }
}

/// The public config response (`entries_open`: whether entries are open).
pub fn config_response<R: Rules>(
    config: &GameConfig<R::Params>,
    entries_open: bool,
) -> ConfigResponse<R::Params> {
    let open = |v| if entries_open { Some(v) } else { None };
    ConfigResponse {
        chain: config.chain.clone(),
        game: config.game.clone(),
        memo: MemoInfo {
            magic: String::from_utf8_lossy(&R::FORMAT.magic).into_owned(),
            version: R::FORMAT.version,
            text_prefix: R::FORMAT.text_prefix.to_owned(),
        },
        entries_open,
        vault_address: open(config.vault_address.clone()),
        sealing_public_key: entries_open.then(|| config.sealing_public_key.clone()),
        uivk: open(config.uivk.clone()),
        settle_confirmations: SETTLE_CONFIRMATIONS,
        initial_carry_zat: config.initial_carry_zat,
    }
}

async fn config<R: Rules>(
    State(state): State<Arc<AppState<R>>>,
) -> ApiResult<ConfigResponse<R::Params>> {
    Ok(Json(config_response::<R>(
        state.processor.config(),
        state.entries_open,
    )))
}

async fn board<R: Rules>(State(state): State<Arc<AppState<R>>>) -> ApiResult<BoardResponse> {
    let db = reader(&state)?;
    let board = state.processor.board(&db)?;
    Ok(Json(BoardResponse { board }))
}

async fn pending<R: Rules>(State(state): State<Arc<AppState<R>>>) -> ApiResult<PendingResponse> {
    let db = reader(&state)?;
    Ok(Json(PendingResponse {
        rounds: state.processor.pending_rounds(&db)?,
    }))
}

/// Builds the round response (shared with tests).
pub fn round_response<R: Rules>(
    processor: &Processor<R>,
    db: &Db,
    round: u32,
) -> Result<RoundResponse, EngineError> {
    let (window, status, context) = processor.status(db, round)?;
    let settlement = db.settlement(round)?;
    let payout = db.payout(round)?;
    let classified = processor.classified(db, round, status == RoundStatus::Voided)?;
    let settled = settlement.map(|row| {
        let s = row.settlement;
        SettledRound {
            voided: row.voided,
            outcome: s.outcome,
            drand_round: row.drand_round,
            beacon_time: row.beacon_time,
            received: s.received,
            carry_in: s.carry_in,
            carry_out: s.carry_out,
            payouts: s.payouts,
            paid: s.paid,
            fees: s.fees,
            plan_status: row.status,
            settled_at_height: row.settled_at_height,
            payout: payout.clone(),
        }
    });
    Ok(RoundResponse {
        round,
        status,
        window,
        context,
        entries: classified.valid.len(),
        refunds: classified.refunds.len(),
        game: processor
            .rules()
            .board(&processor.config().game, &classified),
        settled,
    })
}

async fn round<R: Rules>(
    State(state): State<Arc<AppState<R>>>,
    Path(round): Path<u32>,
) -> ApiResult<RoundResponse> {
    let db = reader(&state)?;
    Ok(Json(round_response(&state.processor, &db, round)?))
}

/// Builds the proof bundle of a settled round (shared with tests).
pub fn proof_response<R: Rules>(
    processor: &Processor<R>,
    db: &Db,
    round: u32,
) -> Result<Option<ProofBundle<R::Params>>, EngineError> {
    match db.settlement(round)? {
        Some(row) => Ok(Some(processor.proof_bundle(db, &row)?)),
        None => Ok(None),
    }
}

/// Cache policy of data that can never change again.
pub const IMMUTABLE: &str = "public, max-age=31536000, immutable";

/// A proof bundle is final once its payout is mined (its openings are in)
/// or when it pays nothing; until then it may still gain the openings.
pub fn bundle_is_final<P>(bundle: &ProofBundle<P>) -> bool {
    bundle.payout_openings.is_some() || bundle.settlement.payouts.is_empty()
}

async fn proof<R: Rules>(
    State(state): State<Arc<AppState<R>>>,
    Path(round): Path<u32>,
) -> Result<Response, ApiError> {
    let db = reader(&state)?;
    match proof_response(&state.processor, &db, round)? {
        Some(bundle) => {
            let cache = if bundle_is_final(&bundle) {
                IMMUTABLE
            } else {
                "no-store"
            };
            Ok(([(axum::http::header::CACHE_CONTROL, cache)], Json(bundle)).into_response())
        }
        None => Err(ApiError(
            StatusCode::NOT_FOUND,
            format!("round {round} is not settled yet"),
        )),
    }
}

async fn vault_ledger<R: Rules>(State(state): State<Arc<AppState<R>>>) -> ApiResult<Ledger> {
    let Some(view) = &state.ledger else {
        return Err(ApiError(
            StatusCode::NOT_FOUND,
            "this engine is not configured as the payout coordinator".into(),
        ));
    };
    Ok(Json(view.ledger(&state.store, &state.processor)?))
}

async fn health<R: Rules>(State(state): State<Arc<AppState<R>>>) -> ApiResult<HealthResponse> {
    let db = reader(&state)?;
    Ok(Json(HealthResponse {
        ok: true,
        tip_height: db.tip()?.map(|b| b.height),
        settled_rounds: db.settled_rounds()?.len(),
        uptime_secs: state.started.elapsed().as_secs(),
    }))
}

/// The engine's own state, for the status page.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineStatus {
    pub uptime_secs: u64,
    /// Highest block stored.
    pub stored_tip: Option<u32>,
    /// Chain tip the servers last reported.
    pub chain_tip: Option<u32>,
    /// `nTime` of the highest stored block.
    pub last_block_time: Option<u64>,
    pub last_settled_round: Option<u32>,
}

/// `GET /v1/status`: everything the public status page shows. Each part
/// reports its own last success, so a stale or failing part is visible.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusResponse {
    /// Unix seconds when this was produced.
    pub now: u64,
    pub network: zecraft_core::params::Network,
    pub entries_open: bool,
    pub engine: EngineStatus,
    pub threshold: u16,
    pub keyholders: Vec<KeyholderStatus>,
    pub chain_servers: Vec<zecraft_vault::pool::ServerStatus>,
}

async fn status<R: Rules>(State(state): State<Arc<AppState<R>>>) -> ApiResult<StatusResponse> {
    let db = reader(&state)?;
    let tip = db.tip()?;
    Ok(Json(StatusResponse {
        now: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        network: state.processor.config().chain.network,
        entries_open: state.entries_open,
        engine: EngineStatus {
            uptime_secs: state.started.elapsed().as_secs(),
            stored_tip: tip.as_ref().map(|b| b.height),
            chain_tip: db.chain_tip()?,
            last_block_time: tip.map(|b| b.time),
            last_settled_round: db.last_settled_round()?,
        },
        threshold: state.threshold,
        keyholders: keyholder_statuses(&state, &db)?,
        chain_servers: state
            .chain_servers
            .as_ref()
            .map(|p| p.status())
            .unwrap_or_default(),
    }))
}

/// `GET /v1/rounds` query.
#[derive(Clone, Debug, Deserialize)]
pub struct RoundsQuery {
    /// Only rounds below this one (pagination).
    pub before: Option<u32>,
    pub limit: Option<u32>,
    /// `1` to include rounds nobody entered.
    pub include_empty: Option<u8>,
}

/// One past round, amounts only.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RoundSummary {
    pub round: u32,
    /// `paid` (payouts broadcast), `paying` (payout being signed or not yet
    /// broadcast), `no_win` (no winning output: the value carried over),
    /// `voided` (closed too late to be safe: refunded).
    pub status: String,
    /// The game's outcome.
    pub outcome: serde_json::Value,
    /// The game's board of the round (`Rules::board`), as-is.
    pub game: serde_json::Value,
    /// The drand round that decided it.
    pub drand_round: u64,
    pub received: u64,
    pub paid: u64,
    pub carry_out: u64,
    pub wins: usize,
    pub refunds: usize,
    /// Unix time of the window's last block, if stored.
    pub closed_time: Option<u64>,
    /// Unix time the beacon was published.
    pub revealed_time: u64,
    pub payout_txid: Option<String>,
}

/// `GET /v1/rounds`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RoundsResponse {
    pub rounds: Vec<RoundSummary>,
    pub next_before: Option<u32>,
}

/// Summarises a settled round.
pub fn round_summary<R: Rules>(
    db: &Db,
    processor: &Processor<R>,
    row: &crate::db::SettlementRow,
) -> Result<RoundSummary, EngineError> {
    let s = &row.settlement;
    let payout = db.payout(row.round)?;
    let wins = s
        .payouts
        .iter()
        .filter(|p| p.kind == PayoutKind::Win)
        .count();
    let paid_out = matches!(&payout, Some(p) if p.state == crate::db::PayoutState::Broadcast);
    let status = if row.voided {
        "voided"
    } else if wins == 0 {
        "no_win"
    } else if paid_out {
        "paid"
    } else {
        "paying"
    };
    let window = zecraft_core::draw::round_window(&processor.config().chain, row.round)?;
    let classified = processor.classified(db, row.round, row.voided)?;
    Ok(RoundSummary {
        round: row.round,
        status: status.to_owned(),
        outcome: s.outcome.clone(),
        game: processor
            .rules()
            .board(&processor.config().game, &classified),
        drand_round: row.drand_round,
        received: s.received,
        paid: s.paid,
        carry_out: s.carry_out,
        wins,
        refunds: s.payouts.len() - wins,
        closed_time: db.block(window.last())?.map(|b| b.time),
        revealed_time: row.beacon_time,
        payout_txid: payout.and_then(|p| p.txid),
    })
}

async fn rounds_list<R: Rules>(
    State(state): State<Arc<AppState<R>>>,
    axum::extract::Query(q): axum::extract::Query<RoundsQuery>,
) -> ApiResult<RoundsResponse> {
    let limit = q.limit.unwrap_or(20).clamp(1, 100) as usize;
    let include_empty = q.include_empty == Some(1);
    let db = reader(&state)?;
    let Some(last) = db.last_settled_round()? else {
        return Ok(Json(RoundsResponse {
            rounds: Vec::new(),
            next_before: None,
        }));
    };
    let mut round = q
        .before
        .map_or(Some(last), |b| b.checked_sub(1).map(|r| r.min(last)));
    let mut rounds = Vec::new();
    while let Some(r) = round {
        if rounds.len() == limit {
            break;
        }
        if let Some(row) = db.settlement(r)? {
            let s = &row.settlement;
            let empty = s.received == 0 && s.payouts.is_empty();
            if include_empty || !empty {
                rounds.push(round_summary(&db, &state.processor, &row)?);
            }
        }
        round = r.checked_sub(1);
    }
    let next_before = if rounds.len() == limit {
        rounds.last().map(|r| r.round)
    } else {
        None
    };
    Ok(Json(RoundsResponse {
        rounds,
        next_before,
    }))
}

/// One note the vault received in a round, as anyone with the public UIVK
/// sees it, with its memo reduced to a digest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteDigest {
    pub height: u32,
    pub value: u64,
    /// SHA-256 of the 512-byte memo field (hex): lets a wallet that knows
    /// its memo confirm the payment landed without the engine learning
    /// anything new.
    pub memo_sha256: String,
}

/// `GET /v1/rounds/{n}/digests`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DigestsResponse {
    pub round: u32,
    pub notes: Vec<NoteDigest>,
}

async fn round_digests<R: Rules>(
    State(state): State<Arc<AppState<R>>>,
    Path(round): Path<u32>,
) -> ApiResult<DigestsResponse> {
    use sha2::{Digest, Sha256};
    let window = zecraft_core::draw::round_window(&state.processor.config().chain, round)
        .map_err(|e| bad(StatusCode::BAD_REQUEST, e.to_string()))?;
    let db = reader(&state)?;
    // Late payments land after the window; include the next round's window
    // too so a wallet can see where its payment went.
    let notes = db
        .notes_in_range(
            window.start,
            window.end.saturating_add(window.end - window.start),
        )?
        .into_iter()
        .map(|n| NoteDigest {
            height: n.height,
            value: n.value,
            memo_sha256: hex::encode(Sha256::digest(&n.memo)),
        })
        .collect();
    Ok(Json(DigestsResponse { round, notes }))
}

/// One keyholder's public status.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyholderStatus {
    pub identifier: u16,
    /// "Keyholder A", "B", …
    pub label: String,
    pub operator: String,
    pub reachable: Option<bool>,
    /// Unix seconds of the last successful health check.
    pub last_seen: Option<u64>,
    /// Unix seconds of the last health check.
    pub last_check: Option<u64>,
    /// Signing requests it approved / refused (recorded since Phase 7a).
    pub approved: u64,
    pub refused: u64,
}

/// A refused payout attempt (one signing session).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefusedAttempt {
    pub time: u64,
    pub rounds: Vec<u32>,
    /// What the keyholders said.
    pub reason: String,
    /// How many keyholders refused it.
    pub refusals: usize,
}

/// `GET /v1/keyholders/status`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyholdersResponse {
    pub threshold: u16,
    pub keyholders: Vec<KeyholderStatus>,
    pub refused_attempts: Vec<RefusedAttempt>,
}

/// Every configured keyholder's public status.
fn keyholder_statuses<R: Rules>(
    state: &AppState<R>,
    db: &Db,
) -> Result<Vec<KeyholderStatus>, EngineError> {
    let health = db.keyholder_health()?;
    let counts = db.keyholder_counts()?;
    Ok(state
        .keyholder_ids
        .iter()
        .map(|id| {
            let h = health.iter().find(|h| h.keyholder == *id);
            let (approved, refused) = counts
                .iter()
                .find(|c| c.0 == *id)
                .map_or((0, 0), |c| (c.1, c.2));
            let letter =
                char::from(b'A'.saturating_add(u8::try_from(id.saturating_sub(1)).unwrap_or(25)));
            KeyholderStatus {
                identifier: *id,
                label: format!("Keyholder {letter}"),
                operator: "game operator".to_owned(),
                reachable: h.map(|h| h.reachable),
                last_seen: h.and_then(|h| h.last_seen),
                last_check: h.map(|h| h.last_check),
                approved,
                refused,
            }
        })
        .collect())
}

async fn keyholders_status<R: Rules>(
    State(state): State<Arc<AppState<R>>>,
) -> ApiResult<KeyholdersResponse> {
    let db = reader(&state)?;
    let keyholders = keyholder_statuses(&state, &db)?;
    let mut refused_attempts: Vec<RefusedAttempt> = Vec::new();
    let mut sessions: Vec<String> = Vec::new();
    for event in db.keyholder_events(500)? {
        if event.outcome != crate::db::KeyholderOutcome::Refused {
            continue;
        }
        match sessions.iter().position(|s| *s == event.session) {
            Some(i) => refused_attempts[i].refusals += 1,
            None => {
                sessions.push(event.session.clone());
                refused_attempts.push(RefusedAttempt {
                    time: event.time,
                    rounds: event.rounds.clone(),
                    reason: event.reason.clone(),
                    refusals: 1,
                });
            }
        }
    }
    refused_attempts.truncate(50);
    Ok(Json(KeyholdersResponse {
        threshold: state.threshold,
        keyholders,
        refused_attempts,
    }))
}

fn bad(status: StatusCode, message: impl Into<String>) -> ApiError {
    ApiError(status, message.into())
}

/// Validates a win proof completely (against this engine's bundle of the
/// round and the payout transaction fetched from its chain source) before
/// storing it; proofs are public data.
async fn submit_proof<R: Rules>(
    State(state): State<Arc<AppState<R>>>,
    client: Option<axum::Extension<crate::edge::ClientIp>>,
    Json(body): Json<SubmitProof>,
) -> Result<(StatusCode, Json<StoredProof>), ApiError> {
    use base64::Engine;
    let address = client.map_or(std::net::IpAddr::from([0, 0, 0, 0]), |c| c.0 .0);
    if !state.proof_limiter.allow(address, Instant::now()).await {
        return Err(bad(
            StatusCode::TOO_MANY_REQUESTS,
            "too many proof submissions; retry in a minute",
        ));
    }
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(body.proof.trim())
        .map_err(|_| bad(StatusCode::BAD_REQUEST, "the proof is not base64url"))?;
    let proof = zecraft_verify::win::WinProof::decode(&bytes)
        .map_err(|e| bad(StatusCode::BAD_REQUEST, e.to_string()))?;
    let id = zecraft_verify::win::WinProof::id(&bytes);
    // A proof already verified is answered from the store (review N6).
    if let Some(stored) = stored_proof(&state, &id)? {
        return Ok((StatusCode::OK, Json(stored)));
    }
    // At most two verifications at once: they share the blocking pool with
    // payout proving (review N6).
    let Ok(_slot) = PROOF_VERIFICATIONS.try_acquire() else {
        return Err(bad(
            StatusCode::SERVICE_UNAVAILABLE,
            "busy verifying other proofs; try again in a few seconds",
        ));
    };
    let bundle = {
        let db = reader(&state)?;
        let row = db.settlement(proof.round)?.ok_or_else(|| {
            bad(
                StatusCode::NOT_FOUND,
                format!("round {} is not settled", proof.round),
            )
        })?;
        state.processor.proof_bundle(&db, &row)?
    };
    let relay = state.relay.as_ref().ok_or_else(|| {
        bad(
            StatusCode::SERVICE_UNAVAILABLE,
            "this engine has no chain source for verification",
        )
    })?;
    let txid = zecraft_verify::chain::txid_bytes(&proof.payout_txid)
        .ok_or_else(|| bad(StatusCode::BAD_REQUEST, "malformed payout txid"))?;
    let raw = relay
        .backend()
        .transaction(txid)
        .await
        .map_err(|e| bad(StatusCode::BAD_GATEWAY, e.to_string()))?;
    let height = u32::try_from(raw.height).unwrap_or(0);
    let blocks = relay
        .backend()
        .blocks(height, height)
        .await
        .map_err(|e| bad(StatusCode::BAD_GATEWAY, e.to_string()))?;
    let included =
        height > 0 && zecraft_verify::chain::Blocks::new(blocks).contains_tx(height, &txid);
    let mined = zecraft_verify::chain::MinedTx {
        raw: raw.data,
        height,
    };
    let verdict = tokio::task::spawn_blocking(move || {
        zecraft_verify::win::verify(&proof, &bundle, &mined, included)
    })
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "proof verification task failed");
        internal()
    })?;
    let round = verdict_round(&bytes);
    let created_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let stored = StoredProof {
        id: id.clone(),
        round,
        created_at,
        proof: base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&bytes),
        verdict: verdict.clone(),
    };
    if !verdict.valid {
        return Ok((StatusCode::UNPROCESSABLE_ENTITY, Json(stored)));
    }
    state
        .store
        .write(|db| db.insert_win_proof(&id, round, created_at, &bytes))?;
    Ok((StatusCode::CREATED, Json(stored)))
}

fn verdict_round(bytes: &[u8]) -> u32 {
    zecraft_verify::win::WinProof::decode(bytes).map_or(0, |p| p.round)
}

/// A stored (verified) win proof, restated.
fn stored_proof<R: Rules>(state: &AppState<R>, id: &str) -> Result<Option<StoredProof>, ApiError> {
    use base64::Engine;
    let Some((round, created_at, bytes)) = state.store.read(|db| db.win_proof(id))? else {
        return Ok(None);
    };
    // Stored proofs were fully verified on submission; the stored verdict
    // is restated here (the claim depends only on public data).
    let proof = zecraft_verify::win::WinProof::decode(&bytes).map_err(|e| {
        tracing::error!(error = %e, "a stored proof does not decode");
        internal()
    })?;
    Ok(Some(StoredProof {
        id: id.to_owned(),
        round,
        created_at,
        proof: base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&bytes),
        verdict: zecraft_verify::win::WinVerdict {
            valid: true,
            claim: format!(
                "Verified on submission: the holder controls the address that received round {}'s winning payout {} (transaction {}).",
                proof.round, proof.payout_index, proof.payout_txid
            ),
            reason: None,
            steps: Vec::new(),
        },
    }))
}

async fn get_proof<R: Rules>(
    State(state): State<Arc<AppState<R>>>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    if id.len() != 32 || !id.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(bad(
            StatusCode::BAD_REQUEST,
            "a proof id is 32 hex characters",
        ));
    }
    // A stored proof was verified once and never changes.
    stored_proof(&state, &id)?
        .map(|p| ([(axum::http::header::CACHE_CONTROL, IMMUTABLE)], Json(p)).into_response())
        .ok_or_else(|| bad(StatusCode::NOT_FOUND, "no such proof"))
}

/// Proof verifications allowed at once.
static PROOF_VERIFICATIONS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);

/// Most simultaneous `/v1/stream` connections.
pub const MAX_STREAMS: usize = 2_000;
/// Most simultaneous `/v1/stream` connections of one client (review N10).
pub const MAX_STREAMS_PER_CLIENT: usize = 8;

/// Open streams per client.
static STREAMS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<std::net::IpAddr, usize>>,
> = std::sync::LazyLock::new(Default::default);

/// One client's open stream; released on drop.
struct StreamSlot(std::net::IpAddr);

impl StreamSlot {
    fn take(client: std::net::IpAddr) -> Option<StreamSlot> {
        let mut open = STREAMS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let count = open.entry(client).or_insert(0);
        if *count >= MAX_STREAMS_PER_CLIENT {
            return None;
        }
        *count += 1;
        Some(StreamSlot(client))
    }
}

impl Drop for StreamSlot {
    fn drop(&mut self) {
        let mut open = STREAMS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(count) = open.get_mut(&self.0) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                open.remove(&self.0);
            }
        }
    }
}

/// Largest client message accepted on `/v1/stream` (clients only listen).
const MAX_CLIENT_MESSAGE: usize = 1_024;

async fn stream<R: Rules>(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState<R>>>,
    client: Option<axum::Extension<crate::edge::ClientIp>>,
) -> Response {
    let address = client.map_or(std::net::IpAddr::from([0, 0, 0, 0]), |c| c.0 .0);
    let Some(own) = StreamSlot::take(address) else {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(ErrorBody {
                error: "too many live connections from this address".into(),
            }),
        )
            .into_response();
    };
    let Ok(slot) = state.stream_slots.clone().try_acquire_owned() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorBody {
                error: "too many live connections; the app falls back to polling".into(),
            }),
        )
            .into_response();
    };
    ws.max_message_size(MAX_CLIENT_MESSAGE)
        .max_frame_size(MAX_CLIENT_MESSAGE)
        .on_upgrade(move |socket| async move {
            handle_socket(socket, state).await;
            drop(slot);
            drop(own);
        })
}

async fn handle_socket<R: Rules>(mut socket: WebSocket, state: Arc<AppState<R>>) {
    let initial = state
        .store
        .read(|db| state.processor.board(db))
        .map(Event::Board);
    let mut rx = state.events.subscribe();
    if let Ok(event) = initial {
        if send_event(&mut socket, &event).await.is_err() {
            return;
        }
    }
    loop {
        tokio::select! {
            incoming = socket.recv() => {
                match incoming {
                    // Pings are answered by the library; the stream is
                    // one-way, so any data from the client ends it.
                    Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
                    _ => return,
                }
            }
            event = rx.recv() => {
                match event {
                    Ok(event) => {
                        if send_event(&mut socket, &event).await.is_err() {
                            return;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
        }
    }
}

async fn send_event(socket: &mut WebSocket, event: &Event) -> Result<(), ()> {
    let text = serde_json::to_string(event).map_err(|_| ())?;
    socket
        .send(Message::Text(text.into()))
        .await
        .map_err(|_| ())
}
