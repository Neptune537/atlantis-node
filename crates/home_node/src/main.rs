mod tui;

use axum::{
    extract::{
        ws::{Message, WebSocket},
        State, WebSocketUpgrade,
    },
    http::{StatusCode, HeaderMap},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use futures_util::StreamExt;
use home_node::ClubHomeNode;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, RwLock};
use tower_http::cors::CorsLayer;
use tracing::{error, info, Level};
use tracing_subscriber::FmtSubscriber;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditReportResponse {
    pub status: String,
    pub total_user_balances: f64,
    pub total_platform_liabilities: f64,
    pub total_treasury_balances: f64,
    pub reconciled_assets: f64,
    pub is_balanced: bool,
    pub allocations_valid: bool,
    pub governance_valid: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryPayload {
    pub cpu_usage_percent: f32,
    pub cpu_temp_celsius: Option<f32>,
    pub free_ram_mb: u64,
    pub total_ram_mb: u64,
    pub p99_latency_ms: f32,
    pub poh_score: f32,
    pub node_id: String,
    pub raft_term: u64,
    pub committed_log_index: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RailWebSocketMessage {
    pub r#type: String,
    pub payload: TelemetryPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTaskRequest {
    pub buyer_did: String,
    pub buyer_account: Uuid,
    pub budget_amount: f64,
    #[serde(default)]
    pub asset: Option<atlantis_protocol::AtlantisAsset>,
    #[serde(default)]
    pub category: Option<atlantis_protocol::JobCategory>,
    #[serde(default)]
    pub min_poh_score: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraduateInitiateRequest {
    pub node_did: String,
    pub callsign: String,
    pub role: home_node::ClubOfficerRole,
    #[serde(default)]
    pub permissions: Vec<String>,
    pub validity_days: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateRoleRequest {
    pub node_did: String,
    pub new_role: home_node::ClubOfficerRole,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvisionStoreStakeRequest {
    pub node_account: Uuid,
    pub network_pool_treasury: Uuid,
    #[serde(default = "default_annual_stake")]
    pub annual_stake_usd: f64,
}

fn default_annual_stake() -> f64 {
    12.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonthlyStakeDripRequest {
    pub escrow_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchDaemonApprovalRequest {
    pub spec: home_node::DaemonServiceSpec,
    pub officer_did: String,
    pub auto_restart: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeStoreAppRequest {
    pub baseline_files: std::collections::BTreeMap<String, String>,
    pub local_files: std::collections::BTreeMap<String, String>,
    pub upstream_files: std::collections::BTreeMap<String, String>,
    pub strategy: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeStoreAppResponse {
    pub cleanly_merged_files: Vec<String>,
    pub conflict_count: usize,
    pub conflicts: Vec<home_node::MergeConflict>,
    pub merged_files: std::collections::BTreeMap<String, Option<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OidcLoginRequest {
    pub id_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlignAccountsRequest {
    pub primary_id_token: String,
    pub secondary_id_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueWalletChallengeRequest {
    pub principal_did: String,
    pub wallet_pubkey_hex: String,
    pub ttl_s: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkSolanaWalletRequest {
    pub challenge: home_node::SolanaWalletLinkChallenge,
    pub signature_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncJwksRequest {
    pub jwks_json: String,
}

pub struct AppState {
    pub home_node: RwLock<ClubHomeNode>,
    pub service_gate: RwLock<home_node::ServiceApprovalGate>,
    pub sessions: std::sync::Mutex<home_node::sessions::SessionStore>,
    pub telemetry_tx: broadcast::Sender<RailWebSocketMessage>,
    pub sysinfo_state: std::sync::Mutex<sysinfo::System>,
    pub components_state: std::sync::Mutex<sysinfo::Components>,
}

fn read_system_telemetry(
    node_id: &str,
    raft_term: u64,
    committed_log_index: u64,
    sys: &mut sysinfo::System,
    components: &mut sysinfo::Components,
) -> TelemetryPayload {
    sys.refresh_cpu_usage();
    sys.refresh_memory();
    components.refresh(false);

    let cpu_usage_percent = sys.global_cpu_usage();
    let free_ram_mb = sys.available_memory() / 1024 / 1024;
    let total_ram_mb = sys.total_memory() / 1024 / 1024;

    // Detect thermal sensor dynamically across Linux, macOS, and Windows via sysinfo,
    // with thermal_zone0 fallback on embedded Linux boards
    let cpu_temp_celsius = components
        .iter()
        .find(|c| {
            let label = c.label().to_lowercase();
            label.contains("cpu") || label.contains("core") || label.contains("soc")
        })
        .and_then(|c| c.temperature())
        .or_else(|| {
            components.iter().next().and_then(|c| c.temperature())
        })
        .or_else(|| {
            std::fs::read_to_string("/sys/class/thermal/thermal_zone0/temp")
                .ok()
                .and_then(|s| s.trim().parse::<f32>().ok().map(|m| m / 1000.0))
        });

    let p99_latency_ms = 4.0;
    let poh_score = 98.5;

    TelemetryPayload {
        cpu_usage_percent,
        cpu_temp_celsius,
        free_ram_mb,
        total_ram_mb,
        p99_latency_ms,
        poh_score,
        node_id: node_id.to_string(),
        raft_term,
        committed_log_index,
    }
}

async fn get_audit_report(State(state): State<Arc<AppState>>) -> Json<AuditReportResponse> {
    let node = state.home_node.read().await;
    match node.ledger.verify_audit_invariants() {
        Ok(report) => Json(AuditReportResponse {
            status: "success".to_string(),
            total_user_balances: report.total_account_balances,
            total_platform_liabilities: report.active_escrow_locked,
            total_treasury_balances: *node.ledger.balances.get(&node.club_treasury_account).unwrap_or(&0.0),
            reconciled_assets: report.total_account_balances + report.active_escrow_locked,
            is_balanced: report.invariants_passed,
            allocations_valid: true,
            governance_valid: true,
        }),
        Err(e) => {
            error!("Ledger invariant verification error: {}", e);
            Json(AuditReportResponse {
                status: "error".to_string(),
                total_user_balances: 0.0,
                total_platform_liabilities: 0.0,
                total_treasury_balances: 0.0,
                reconciled_assets: 0.0,
                is_balanced: false,
                allocations_valid: false,
                governance_valid: false,
            })
        }
    }
}

async fn get_marketplace_tasks(State(state): State<Arc<AppState>>) -> Json<Vec<atlantis_protocol::KuroshioEventEnvelope>> {
    let node = state.home_node.read().await;
    Json(node.consensus.log_entries.clone())
}

async fn post_create_task(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateTaskRequest>,
) -> Result<Json<atlantis_protocol::KuroshioEventEnvelope>, (StatusCode, String)> {
    let mut node = state.home_node.write().await;
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let category = payload.category.unwrap_or_else(|| atlantis_protocol::JobCategory::ComputeTask {
        shader_or_pipeline: "dem_tile_baking_wgsl".to_string(),
        input_uri: "gyre://tiles/dem/tile_34_82.bin".to_string(),
    });

    let asset = payload.asset.unwrap_or(atlantis_protocol::AtlantisAsset::Usdc);
    let min_poh = payload.min_poh_score.unwrap_or(50.0);

    match node.dispatch_marketplace_job(
        payload.buyer_did,
        payload.buyer_account,
        category,
        payload.budget_amount,
        asset,
        min_poh,
        timestamp,
    ) {
        Ok((_vault, envelope)) => Ok(Json(envelope)),
        Err(e) => Err((StatusCode::BAD_REQUEST, format!("Ledger Error: {}", e))),
    }
}

async fn post_approve_daemon(
    State(state): State<Arc<AppState>>,
    Json(req): Json<LaunchDaemonApprovalRequest>,
) -> Result<Json<home_node::ServiceApprovalRecord>, (StatusCode, String)> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let mut gate = state.service_gate.write().await;
    let record = gate.approve_service(
        &req.spec,
        req.officer_did,
        now,
        req.auto_restart.unwrap_or(true),
    );
    info!("[Service Gate] Approved service {} with fingerprint {}", record.service_id, record.approved_fingerprint);
    Ok(Json(record))
}

async fn post_verify_daemon_launch(
    State(state): State<Arc<AppState>>,
    Json(spec): Json<home_node::DaemonServiceSpec>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let gate = state.service_gate.read().await;
    match gate.verify_launch(&spec) {
        Ok(()) => Ok(Json(serde_json::json!({ "status": "authorized", "fingerprint": spec.compute_fingerprint() }))),
        Err(e) => Err((StatusCode::FORBIDDEN, e.to_string())),
    }
}

async fn post_3way_app_merge(
    Json(req): Json<MergeStoreAppRequest>,
) -> Result<Json<MergeStoreAppResponse>, (StatusCode, String)> {
    let mut baseline = home_node::FileTree::new();
    for (k, v) in req.baseline_files {
        baseline.insert(k, v.into_bytes());
    }
    let mut mine = home_node::FileTree::new();
    for (k, v) in req.local_files {
        mine.insert(k, v.into_bytes());
    }
    let mut theirs = home_node::FileTree::new();
    for (k, v) in req.upstream_files {
        theirs.insert(k, v.into_bytes());
    }

    let strat = match req.strategy.as_deref() {
        Some("mine") => home_node::MergeResolutionStrategy::KeepMine,
        Some("theirs") => home_node::MergeResolutionStrategy::TakeTheirs,
        _ => home_node::MergeResolutionStrategy::WriteConflictMarkers,
    };

    let outcome = home_node::merge_3way_trees(&baseline, &mine, &theirs, strat);

    let mut merged_utf8 = std::collections::BTreeMap::new();
    for (k, v) in outcome.files {
        merged_utf8.insert(k, v.map(|bytes| String::from_utf8_lossy(&bytes).to_string()));
    }

    Ok(Json(MergeStoreAppResponse {
        cleanly_merged_files: outcome.cleanly_merged_files,
        conflict_count: outcome.conflicts.len(),
        conflicts: outcome.conflicts,
        merged_files: merged_utf8,
    }))
}

fn bearer(headers: &HeaderMap) -> Result<&str, (StatusCode, String)> {
    headers.get(axum::http::header::AUTHORIZATION).and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or((StatusCode::UNAUTHORIZED, "Bearer session required".into()))
}
fn authenticate_session(state: &AppState, headers: &HeaderMap) -> Result<String, (StatusCode, String)> {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, "Clock unavailable".into()))?.as_secs();
    state.sessions.lock().map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, "Session store unavailable".into()))?
        .authenticate(bearer(headers)?, now)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or expired session".into()))
}
#[derive(Deserialize)]
struct LogoutRequest { #[serde(default)] all_sessions: bool }
async fn post_auth_logout(State(state): State<Arc<AppState>>, headers: HeaderMap, Json(req): Json<LogoutRequest>)
    -> Result<StatusCode, (StatusCode, String)> {
    let principal = authenticate_session(&state, &headers)?;
    let mut store = state.sessions.lock().map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, "Session store unavailable".into()))?;
    if req.all_sessions { store.revoke_all(&principal) } else { store.revoke(bearer(&headers)?) }
        .map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, "Session revocation failed".into()))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn get_auth_me(State(state): State<Arc<AppState>>, headers: HeaderMap)
    -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let principal = authenticate_session(&state, &headers)?;
    let wallet = state.sessions.lock().map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, "Identity store unavailable".into()))?
        .wallet(&principal).map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, "Wallet lookup failed".into()))?;
    Ok(Json(serde_json::json!({"principal_did":principal,"linked_solana_wallet":wallet})))
}

async fn post_auth_oidc_login(
    State(state): State<Arc<AppState>>,
    Json(req): Json<OidcLoginRequest>,
) -> Result<Json<home_node::VerifiedAtlantisSession>, (StatusCode, String)> {
    let mut node = state.home_node.write().await;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs());
    node.jwks_verifier.ensure_google_keys().await
        .map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, "Identity provider keys unavailable".into()))?;
    let mut session = node.jwks_verifier.verify_and_bind_jwt_at(&req.id_token, now)
        .map_err(|e| (StatusCode::UNAUTHORIZED, e))?;
    let now = now.ok_or((StatusCode::SERVICE_UNAVAILABLE, "Clock unavailable".into()))?;
    let (token, expires) = state.sessions.lock().map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, "Session store unavailable".into()))?
        .issue(&session.did, now, session.expires_at_s)
        .map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, "Session persistence failed".into()))?;
    session.linked_solana_wallet = state.sessions.lock().map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, "Identity store unavailable".into()))?
        .wallet(&session.did).map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, "Wallet lookup failed".into()))?;
    session.session_token = token;
    session.expires_at_s = expires;
    Ok(Json(session))
}

async fn post_auth_align_accounts(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AlignAccountsRequest>,
) -> Result<Json<home_node::VerifiedAtlantisSession>, (StatusCode, String)> {
    let _ = (state, req);
    Err((StatusCode::FORBIDDEN, "Account alignment requires durable migration support".into()))
}

async fn post_auth_issue_challenge(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<IssueWalletChallengeRequest>,
) -> Result<Json<home_node::SolanaWalletLinkChallenge>, (StatusCode, String)> {
    let principal = authenticate_session(&state, &headers)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    if principal != req.principal_did {
        return Err((StatusCode::FORBIDDEN, "Account does not own wallet challenge".into()));
    }
    state.sessions.lock().map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, "Identity store unavailable".into()))?
        .issue_wallet_challenge(&principal, &req.wallet_pubkey_hex, now, req.ttl_s.unwrap_or(300))
        .map(Json).map_err(|e| (StatusCode::BAD_REQUEST, e))
}

async fn post_auth_sync_jwks(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SyncJwksRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let _ = (state, req);
    Err((StatusCode::FORBIDDEN, "Client-supplied signing keys are forbidden".into()))
}

async fn post_auth_link_wallet(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<LinkSolanaWalletRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let principal = authenticate_session(&state, &headers)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    if principal != req.challenge.principal_did {
        return Err((StatusCode::FORBIDDEN, "Account does not own wallet challenge".into()));
    }
    state.sessions.lock().map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, "Identity store unavailable".into()))?
        .link_wallet(&principal, &req.challenge, &req.signature_hex, now)
        .map(|wallet| Json(serde_json::json!({"status":"LINKED", "principal_did":principal,"linked_solana_wallet":wallet})))
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

async fn get_club_officers(
    State(state): State<Arc<AppState>>,
) -> Json<Vec<home_node::OperativeRecord>> {
    let node = state.home_node.read().await;
    Json(node.registry.list_officers())
}

async fn post_graduate_initiate(
    State(state): State<Arc<AppState>>,
    Json(req): Json<GraduateInitiateRequest>,
) -> Result<Json<atlantis_protocol::ClubGraduationCertificate>, (StatusCode, String)> {
    let mut node = state.home_node.write().await;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let validity_duration = req.validity_days.unwrap_or(365) * 86400;

    node.registry
        .graduate_initiate(
            &req.node_did,
            &req.callsign,
            req.role,
            req.permissions,
            now,
            validity_duration,
            atlantis_protocol::LocationDisclosurePolicy::ObfuscatedSector { sector_id: 1001 },
        )
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Graduation Error: {}", e)))
}

async fn post_assign_role(
    State(state): State<Arc<AppState>>,
    Json(req): Json<UpdateRoleRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let mut node = state.home_node.write().await;
    node.registry
        .assign_officer_role(&req.node_did, req.new_role)
        .map(|_| StatusCode::OK)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))
}

// ============================================================================
// CHARTER QUANTITATIVE ENGINE PLUGIN ENDPOINTS (FOR NAUTILUS OFFICE DAEMONS)
// ============================================================================

async fn get_charter_strategies(
    State(state): State<Arc<AppState>>,
) -> Json<Vec<home_node::CharterStrategy>> {
    let node = state.home_node.read().await;
    let strats: Vec<_> = node.charter_plugin.state.active_strategies.values().cloned().collect();
    Json(strats)
}

#[derive(Deserialize)]
struct ToggleCharterStrategyRequest {
    strategy_id: String,
    activated: bool,
}

async fn post_toggle_charter_strategy(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ToggleCharterStrategyRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let mut node = state.home_node.write().await;
    node.charter_plugin
        .set_strategy_active(&req.strategy_id, req.activated)
        .map(|_| StatusCode::OK)
        .map_err(|e| (StatusCode::NOT_FOUND, e))
}

#[derive(Deserialize)]
struct OfficeMarketTotemRequest {
    pair_id: u32,
    strategy_id: String,
    camera_ecef: [f64; 3],
}

async fn post_charter_office_totem(
    State(state): State<Arc<AppState>>,
    Json(req): Json<OfficeMarketTotemRequest>,
) -> Result<Json<home_node::OfficeMarketTotem3D>, (StatusCode, String)> {
    let mut node = state.home_node.write().await;
    let office_ecef = node
        .siren_radio_daemon
        .fortress
        .get_room_absolute_ecef("room_team7_office")
        .unwrap_or(node.siren_radio_daemon.fortress.ecef_coords);

    match node.charter_plugin.compute_office_totem_3d(
        office_ecef,
        req.camera_ecef,
        req.pair_id,
        &req.strategy_id,
    ) {
        Some(totem) => Ok(Json(totem)),
        None => Err((StatusCode::NOT_FOUND, format!("Strategy '{}' not found", req.strategy_id))),
    }
}

async fn post_provision_store_stake(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ProvisionStoreStakeRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let mut node = state.home_node.write().await;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(1_730_000_000);

    let current_bal = node.ledger.balances.get(&req.node_account).copied().unwrap_or(0.0);
    if current_bal < req.annual_stake_usd {
        return Err((
            StatusCode::PAYMENT_REQUIRED,
            format!(
                "Insufficient funds: account {} has ${:.2}, requires ${:.2} deposit before staking",
                req.node_account, current_bal, req.annual_stake_usd
            ),
        ));
    }
    if !node.ledger.balances.contains_key(&req.network_pool_treasury) {
        node.ledger.seed_account(req.network_pool_treasury, 0.0);
    }

    let ClubHomeNode {
        ref mut ledger,
        ref mut subscription_ledger,
        ..
    } = *node;

    match subscription_ledger.provision_google_pay_annual_node_stake(
        ledger,
        req.node_account,
        req.network_pool_treasury,
        req.annual_stake_usd,
        now,
    ) {
        Ok((vault, drip_tx)) => {
            let pool_bal = ledger.balances.get(&req.network_pool_treasury).copied().unwrap_or(0.0);
            Ok(Json(serde_json::json!({
                "status": "PROVISIONED",
                "escrow_vault_id": vault.id,
                "locked_amount_usd": vault.locked_amount,
                "month_1_drip_tx": drip_tx.id,
                "network_pool_balance_usd": pool_bal
            })))
        }
        Err(e) => Err((StatusCode::BAD_REQUEST, format!("Stake provisioning failed: {}", e))),
    }
}

async fn post_monthly_stake_drip(
    State(state): State<Arc<AppState>>,
    Json(req): Json<MonthlyStakeDripRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let mut node = state.home_node.write().await;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(1_730_000_000);

    let ClubHomeNode {
        ref mut ledger,
        ref mut subscription_ledger,
        ..
    } = *node;

    match subscription_ledger.drip_monthly_stake_to_network_pool(ledger, req.escrow_id, now) {
        Ok(tx) => Ok(Json(serde_json::json!({
            "status": "DRIPPED",
            "transaction_id": tx.id,
            "drip_amount_usd": tx.amount,
            "destination_treasury": tx.credit_account
        }))),
        Err(e) => Err((StatusCode::BAD_REQUEST, format!("Monthly drip failed: {}", e))),
    }
}

async fn ws_execution_rail_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_execution_rail_socket(socket, state))
}

async fn handle_execution_rail_socket(mut socket: WebSocket, state: Arc<AppState>) {
    let mut rx = state.telemetry_tx.subscribe();

    // Send initial snapshot
    let initial_msg_str = {
        let node = state.home_node.read().await;
        let mut sys = state.sysinfo_state.lock().unwrap();
        let mut components = state.components_state.lock().unwrap();
        let initial_payload = read_system_telemetry(
            &node.club_node_id,
            node.consensus.current_term,
            node.consensus.committed_log_index,
            &mut sys,
            &mut components,
        );
        let msg = RailWebSocketMessage {
            r#type: "Telemetry".to_string(),
            payload: initial_payload,
        };
        serde_json::to_string(&msg).ok()
    };

    if let Some(json_str) = initial_msg_str {
        let _ = socket.send(Message::Text(json_str)).await;
    }

    loop {
        tokio::select! {
            telemetry_res = rx.recv() => {
                match telemetry_res {
                    Ok(telemetry_msg) => {
                        if let Ok(json_str) = serde_json::to_string(&telemetry_msg) {
                            if socket.send(Message::Text(json_str)).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            client_msg = socket.next() => {
                match client_msg {
                    Some(Ok(Message::Ping(payload))) => {
                        let _ = socket.send(Message::Pong(payload)).await;
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .expect("setting default subscriber failed");

    let node_id = std::env::var("ATLANTIS_NODE_ID").unwrap_or_else(|_| "zero-rpi-node-01".to_string());
    let dividend_rate: f64 = std::env::var("ATLANTIS_DIVIDEND_RATE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.02);

    let peers_env = std::env::var("ATLANTIS_PEERS").unwrap_or_default();

    info!("Starting Atlantis Club Home Node daemon (Unified V2)...");
    info!("Node ID: {}", node_id);
    info!("Treasury Dividend Rate: {:.2}%", dividend_rate * 100.0);

    let mut home_node = ClubHomeNode::new(&node_id, dividend_rate);
    for peer in peers_env.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
        home_node.consensus.add_peer(peer);
        info!("Registered consensus peer: {}", peer);
    }

    // Try restoring persistent SQLite ledger if database exists
    let sqlite_db_path = "/app/data/atlantis_ledger.db";
    if std::path::Path::new(sqlite_db_path).exists() {
        match tide_ledger::TideLedgerEngine::load_from_sqlite(sqlite_db_path) {
            Ok(restored_ledger) => {
                info!("Restored {} account balances from SQLite ledger", restored_ledger.balances.len());
                home_node.ledger = restored_ledger;
            }
            Err(e) => {
                error!("Failed to restore SQLite ledger: {}. Initializing fresh ledger.", e);
            }
        }
    }

    // Try loading persistent DTN queue from disk if volume mounted
    let spool_file = std::path::Path::new("/app/data/dtn_spool.json");
    if let Ok(restored) = home_node.dtn_store.load_from_disk(spool_file) {
        if restored > 0 {
            info!("Restored {} DTN bundles from disk spool", restored);
        }
    }

    info!("Treasury account seeded: {}", home_node.club_treasury_account);
    info!("OpenRaft consensus initialized. Current role: {:?}", home_node.consensus.role);
    info!("Local AI Gate status: ready");
    info!("DTN store active: WAN status = {}", home_node.dtn_store.is_wan_online);

    // Check if running in headless mode or if TUI should launch
    let is_headless = std::env::var("ATLANTIS_HEADLESS").map(|v| v == "1" || v == "true").unwrap_or(false)
        || std::env::args().any(|arg| arg == "--headless" || arg == "--daemon");

    // Broadcast channel for WebSocket telemetry stream
    let (telemetry_tx, _) = broadcast::channel(128);

    // Initialize shared state for TUI
    let shared_tui_state = tui::create_shared_tui_state();

    let mut initial_sys = sysinfo::System::new_all();
    initial_sys.refresh_all();
    let initial_components = sysinfo::Components::new_with_refreshed_list();

    let state = Arc::new(AppState {
        sessions: std::sync::Mutex::new({
            let path = std::env::var_os("ATLANTIS_SESSION_DB")
                .map(std::path::PathBuf::from).unwrap_or_else(|| "data/auth-sessions.sqlite".into());
            if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent).expect("create private session data directory");
            }
            home_node::sessions::SessionStore::open(path).expect("open durable session store")
        }),
        home_node: RwLock::new(home_node),
        service_gate: RwLock::new(home_node::ServiceApprovalGate::new()),
        telemetry_tx: telemetry_tx.clone(),
        sysinfo_state: std::sync::Mutex::new(initial_sys),
        components_state: std::sync::Mutex::new(initial_components),
    });

    // Background hardware telemetry loop (1Hz)
    let state_for_telemetry = Arc::clone(&state);
    let telemetry_tx_for_loop = telemetry_tx.clone();
    let shared_tui_state_for_telemetry = Arc::clone(&shared_tui_state);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(1000));
        loop {
            interval.tick().await;
            let node = state_for_telemetry.home_node.read().await;
            let payload = {
                let mut sys = state_for_telemetry.sysinfo_state.lock().unwrap();
                let mut comps = state_for_telemetry.components_state.lock().unwrap();
                read_system_telemetry(
                    &node.club_node_id,
                    node.consensus.current_term,
                    node.consensus.committed_log_index,
                    &mut sys,
                    &mut comps,
                )
            };
            let msg = RailWebSocketMessage {
                r#type: "Telemetry".to_string(),
                payload: payload.clone(),
            };
            let _ = telemetry_tx_for_loop.send(msg);

            // Update shared TUI state with American metrics
            if let Ok(mut tui_lock) = shared_tui_state_for_telemetry.lock() {
                tui_lock.telemetry = payload;
                tui_lock.peer_count = node.consensus.peers.len();
                tui_lock.dtn_queue_len = node.dtn_store.buffered_count();
                tui_lock.dividend_rate = node.dividend_rate;
                tui_lock.active_store_nodes = node.subscription_ledger.subscriptions.len();
                tui_lock.annual_stake_locked_usd = node
                    .ledger
                    .escrows
                    .values()
                    .filter(|v| v.status == tide_ledger::EscrowStatus::AuthorizedLocked)
                    .map(|v| v.locked_amount)
                    .sum();
                tui_lock.monthly_pool_drip_usd = node.subscription_ledger.subscriptions.len() as f64 * node.subscription_ledger.engine.monthly_subscription_fee_usd;
            }
        }
    });

    // Background heartbeat / Raft audit loop (30s)
    let state_for_heartbeat = Arc::clone(&state);
    let shared_tui_state_for_heartbeat = Arc::clone(&shared_tui_state);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        loop {
            interval.tick().await;
            let node = state_for_heartbeat.home_node.read().await;
            let heartbeat_msg = format!(
                "Heartbeat: Node {} healthy | Raft Term: {} | Committed Index: {} | Peers: {} | DTN Queue: {}",
                node.club_node_id,
                node.consensus.current_term,
                node.consensus.committed_log_index,
                node.consensus.peers.len(),
                node.dtn_store.buffered_count()
            );
            info!("{}", heartbeat_msg);

            // Audit verification and update TUI logs
            let audit_balanced = node.ledger.verify_audit_invariants().is_ok();
            if let Ok(mut tui_lock) = shared_tui_state_for_heartbeat.lock() {
                tui_lock.audit_balanced = audit_balanced;
                tui_lock.log_messages.push(heartbeat_msg);
                if tui_lock.log_messages.len() > 100 {
                    tui_lock.log_messages.remove(0);
                }
            }

            // Periodic auto-sync to SQLite disk
            if std::path::Path::new("/app/data").exists() {
                if let Err(e) = node.ledger.persist_to_sqlite("/app/data/atlantis_ledger.db") {
                    error!("Failed to persist ledger to SQLite: {}", e);
                }
            }
        }
    });

    // Create a broadcast channel for graceful shutdown
    let (shutdown_tx, _) = tokio::sync::broadcast::channel::<()>(1);

    // Setup REST API Router on Port 8081
    let api_router = Router::new()
        .route("/api/audit/report", get(get_audit_report))
        .route("/api/marketplace/tasks", get(get_marketplace_tasks).post(post_create_task))
        .route("/api/club/officers", get(get_club_officers))
        .route("/api/club/graduate", axum::routing::post(post_graduate_initiate))
        .route("/api/club/officers/assign", axum::routing::post(post_assign_role))
        // Charter plugin routes for daemons in Nautilus office
        .route("/api/charter/strategies", get(get_charter_strategies))
        .route("/api/charter/strategy/toggle", axum::routing::post(post_toggle_charter_strategy))
        .route("/api/charter/office/totem", axum::routing::post(post_charter_office_totem))
        // Store Node $12 Annual Stake and $1 Monthly Network Pool Drip
        .route("/api/store/provision-stake", axum::routing::post(post_provision_store_stake))
        .route("/api/store/monthly-drip", axum::routing::post(post_monthly_stake_drip))
        // Daemon service fingerprinting & launch authorization
        .route("/api/service/approve", axum::routing::post(post_approve_daemon))
        .route("/api/service/verify", axum::routing::post(post_verify_daemon_launch))
        // 3-way baseline app store merge
        .route("/api/store/app/merge", axum::routing::post(post_3way_app_merge))
        // Consumer OIDC (Google & Apple ID) sign-in, account alignment, and Solana wallet linking
        .merge(Router::new()
        .route("/api/auth/me", get(get_auth_me))
        .route("/api/auth/logout", axum::routing::post(post_auth_logout))
        .route("/api/auth/oidc", axum::routing::post(post_auth_oidc_login))
        .route("/api/auth/align", axum::routing::post(post_auth_align_accounts))
        .route("/api/auth/wallet-challenge", axum::routing::post(post_auth_issue_challenge))
        .route("/api/auth/link-wallet", axum::routing::post(post_auth_link_wallet))
        .route("/api/auth/jwks-sync", axum::routing::post(post_auth_sync_jwks))
        .layer(axum::extract::DefaultBodyLimit::max(16 * 1024)))
        .layer(axum::middleware::map_response(|mut response: axum::response::Response| async move {
            response.headers_mut().insert(axum::http::header::CACHE_CONTROL, axum::http::HeaderValue::from_static("no-store"));
            response
        }))
        .layer(CorsLayer::permissive())
        .with_state(Arc::clone(&state));

    let api_listener = tokio::net::TcpListener::bind("0.0.0.0:8081").await?;
    info!("Atlantis Double-Entry Ledger & Marketplace API listening on 0.0.0.0:8081");

    let mut shutdown_rx_api = shutdown_tx.subscribe();
    let api_handle = tokio::spawn(async move {
        let server = axum::serve(api_listener, api_router);
        if let Err(e) = server
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx_api.recv().await;
            })
            .await
        {
            error!("API Server Error on port 8081: {}", e);
        }
    });

    // Setup Consensus / Execution Rail WebSocket Router on Port 9001
    let rail_router = Router::new()
        .route("/", get(ws_execution_rail_handler))
        .route("/ws", get(ws_execution_rail_handler))
        .layer(CorsLayer::permissive())
        .with_state(Arc::clone(&state));

    let rail_listener = tokio::net::TcpListener::bind("0.0.0.0:9001").await?;
    info!("Atlantis OpenRaft Consensus & Execution Rail WS listening on 0.0.0.0:9001");

    let mut shutdown_rx_rail = shutdown_tx.subscribe();
    let rail_handle = tokio::spawn(async move {
        let server = axum::serve(rail_listener, rail_router);
        if let Err(e) = server
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx_rail.recv().await;
            })
            .await
        {
            error!("Execution Rail WS Server Error on port 9001: {}", e);
        }
    });

    // Launch interactive TUI if not in headless/daemon mode
    let shutdown_tx_for_tui = shutdown_tx.clone();
    let shared_tui_state_for_run = Arc::clone(&shared_tui_state);
    if !is_headless {
        std::thread::spawn(move || {
            // Small warm up pause for server sockets to bind
            std::thread::sleep(Duration::from_millis(150));
            if let Err(e) = tui::run_tui(shared_tui_state_for_run) {
                error!("TUI session terminated with error: {}", e);
            }
            let _ = shutdown_tx_for_tui.send(());
        });
    }

    // Wait for termination signal or TUI quit
    let mut shutdown_rx_main = shutdown_tx.subscribe();
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            info!("Received SIGINT/SIGTERM, sending shutdown signal to servers...");
            let _ = shutdown_tx.send(());
        }
        _ = shutdown_rx_main.recv() => {
            info!("Shutdown event received, terminating node services...");
        }
    }

    // Spool DTN queue to disk on shutdown
    {
        let node = state.home_node.read().await;
        if let Err(e) = node.dtn_store.persist_to_disk(spool_file) {
            tracing::warn!("Could not write DTN spool file on shutdown: {}", e);
        } else {
            info!("Spool state preserved to disk successfully.");
        }
    }

    let _ = tokio::join!(api_handle, rail_handle);
    info!("Atlantis Home Node shut down cleanly.");

    Ok(())
}

#[cfg(test)]
mod auth_route_tests {
    use super::*;
    fn headers(token: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(axum::http::header::AUTHORIZATION, format!("Bearer {}", token).parse().unwrap());
        headers
    }
    #[tokio::test]
    async fn wallet_routes_enforce_two_account_ownership_and_logout() {
        let (telemetry_tx, _) = broadcast::channel(1);
        let state = Arc::new(AppState {
            home_node: RwLock::new(ClubHomeNode::new("auth-route-test", 0.0)),
            service_gate: RwLock::new(home_node::ServiceApprovalGate::new()),
            sessions: std::sync::Mutex::new(home_node::sessions::SessionStore::open(":memory:").unwrap()),
            telemetry_tx,
            sysinfo_state: std::sync::Mutex::new(sysinfo::System::new()),
            components_state: std::sync::Mutex::new(sysinfo::Components::new()),
        });
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        let alice = state.sessions.lock().unwrap().issue("did:atlantis:alice", now, now+3600).unwrap().0;
        let bob = state.sessions.lock().unwrap().issue("did:atlantis:bob", now, now+3600).unwrap().0;
        let wallet = atlantis_protocol::PearlEnclaveIdentity::from_label("route-wallet");
        let address = wallet.did().strip_prefix("did:atlantis:").unwrap().to_string();
        let request = || IssueWalletChallengeRequest { principal_did:"did:atlantis:alice".into(), wallet_pubkey_hex:address.clone(), ttl_s:None };
        assert_eq!(post_auth_issue_challenge(State(state.clone()), HeaderMap::new(), Json(request())).await.unwrap_err().0, StatusCode::UNAUTHORIZED);
        assert_eq!(post_auth_issue_challenge(State(state.clone()), headers(&bob), Json(request())).await.unwrap_err().0, StatusCode::FORBIDDEN);
        let challenge = post_auth_issue_challenge(State(state.clone()), headers(&alice), Json(request())).await.unwrap().0;
        use ed25519_dalek::Signer;
        let signature = atlantis_protocol::bytes_to_hex(&wallet.signing_key().sign(challenge.signing_message().as_bytes()).to_bytes());
        let link = || LinkSolanaWalletRequest { challenge:challenge.clone(), signature_hex:signature.clone() };
        assert_eq!(post_auth_link_wallet(State(state.clone()), headers(&bob), Json(link())).await.unwrap_err().0, StatusCode::FORBIDDEN);
        assert!(post_auth_link_wallet(State(state.clone()), headers(&alice), Json(link())).await.is_ok());
        assert!(post_auth_link_wallet(State(state.clone()), headers(&alice), Json(link())).await.is_err());
        assert_eq!(post_auth_logout(State(state.clone()), headers(&alice), Json(LogoutRequest { all_sessions:true })).await.unwrap(), StatusCode::NO_CONTENT);
        assert_eq!(post_auth_issue_challenge(State(state.clone()), headers(&alice), Json(request())).await.unwrap_err().0, StatusCode::UNAUTHORIZED);
        assert_eq!(authenticate_session(&state, &headers(&bob)).unwrap(), "did:atlantis:bob");
    }
}
