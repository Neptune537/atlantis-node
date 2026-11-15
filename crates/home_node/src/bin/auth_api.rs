//! Isolated public auth API: no ledger, marketplace or daemon-control routes.
use axum::{extract::{State, Query, DefaultBodyLimit}, http::{StatusCode, HeaderMap, HeaderValue, Method}, Json, Router, routing::{get,post}};
use serde::{Serialize, Deserialize};
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;
struct AppState {
    verifier: RwLock<home_node::GoogleJwksVerifier>,
    sessions: Mutex<home_node::sessions::SessionStore>,
}
// Enough for provider JWTs and a 500-character UTF-8 chat message. Bound
// extraction before allocation/JSON parsing, including chunked request bodies.
const AUTH_JSON_LIMIT: usize = 16 * 1024;
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
fn current_time() -> Result<u64, (StatusCode, String)> {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, "Clock unavailable".into()))
}
#[derive(Deserialize)]
struct SceneQuery { scene: String, #[serde(default)] after: i64 }
#[derive(Deserialize)]
struct PresenceRequest { scene: String }
#[derive(Deserialize)]
struct ClubMessageRequest { scene: String, text: String }
async fn get_club_presence(State(state): State<Arc<AppState>>, headers: HeaderMap, Query(query): Query<SceneQuery>)
    -> Result<Json<Vec<home_node::sessions::ClubPresence>>, (StatusCode, String)> {
    authenticate_session(&state, &headers)?;
    state.sessions.lock().map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, "Club store unavailable".into()))?
        .club_presence(&query.scene, current_time()?).map(Json).map_err(|error| (StatusCode::BAD_REQUEST, error))
}
async fn post_club_presence(State(state): State<Arc<AppState>>, headers: HeaderMap, Json(request): Json<PresenceRequest>)
    -> Result<StatusCode, (StatusCode, String)> {
    let principal = authenticate_session(&state, &headers)?;
    state.sessions.lock().map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, "Club store unavailable".into()))?
        .set_club_presence(&principal, &request.scene, current_time()?)
        .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    Ok(StatusCode::NO_CONTENT)
}
async fn post_club_leave(State(state): State<Arc<AppState>>, headers: HeaderMap)
    -> Result<StatusCode, (StatusCode, String)> {
    let principal = authenticate_session(&state, &headers)?;
    state.sessions.lock().map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, "Club store unavailable".into()))?
        .clear_club_presence(&principal).map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, "Club store unavailable".into()))?;
    Ok(StatusCode::NO_CONTENT)
}
async fn get_club_messages(State(state): State<Arc<AppState>>, headers: HeaderMap, Query(query): Query<SceneQuery>)
    -> Result<Json<Vec<home_node::sessions::ClubMessage>>, (StatusCode, String)> {
    authenticate_session(&state, &headers)?;
    state.sessions.lock().map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, "Club store unavailable".into()))?
        .club_messages(&query.scene, query.after).map(Json).map_err(|error| (StatusCode::BAD_REQUEST, error))
}
async fn post_club_message(State(state): State<Arc<AppState>>, headers: HeaderMap, Json(request): Json<ClubMessageRequest>)
    -> Result<Json<home_node::sessions::ClubMessage>, (StatusCode, String)> {
    let principal = authenticate_session(&state, &headers)?;
    state.sessions.lock().map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, "Club store unavailable".into()))?
        .post_club_message(&principal, &request.scene, &request.text, current_time()?)
        .map(Json).map_err(|error| (StatusCode::BAD_REQUEST, error))
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

// A capability probe only. The browser worker has no public session endpoint
// until owner binding, isolation, and private streaming are deployed together.
async fn get_node_browser_capability(State(state): State<Arc<AppState>>, headers: HeaderMap)
    -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let principal = authenticate_session(&state, &headers)?;
    let owner = std::env::var("ATLANTIS_NODE_BROWSER_OWNER_DID").ok();
    let authorized = owner.as_deref() == Some(principal.as_str());
    Ok(Json(serde_json::json!({
        "authorized": authorized,
        "available": false,
        "reason": if authorized { "Browser worker and private stream are not deployed" }
                  else { "No Home Node browser is bound to this account" }
    })))
}

async fn post_auth_oidc_login(
    State(state): State<Arc<AppState>>,
    Json(req): Json<OidcLoginRequest>,
) -> Result<Json<home_node::VerifiedAtlantisSession>, (StatusCode, String)> {
    let mut node = state.verifier.write().await;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs());
    node.ensure_google_keys().await
        .map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, "Identity provider keys unavailable".into()))?;
    let mut session = node.verify_and_bind_jwt_at(&req.id_token, now)
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


#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client_id = std::env::var("GOOGLE_OAUTH_CLIENT_ID")?;
    if !client_id.ends_with(".apps.googleusercontent.com") { return Err("A registered Google client ID is required".into()); }
    let path = std::env::var("ATLANTIS_SESSION_DB")?;
    let sessions = home_node::sessions::SessionStore::open(path)?;
    let mut verifier = home_node::GoogleJwksVerifier::new(client_id);
    verifier.ensure_google_keys().await?;
    let state = Arc::new(AppState {verifier:RwLock::new(verifier), sessions:Mutex::new(sessions)});
    let origins: Vec<HeaderValue> = std::env::var("ATLANTIS_AUTH_ORIGINS")?
        .split(',').map(|v| v.trim().parse()).collect::<Result<_,_>>()?;
    if origins.is_empty() || origins.iter().any(|v| v == "*") { return Err("Explicit browser origins required".into()); }
    let router = auth_router(state, origins);
    let bind = std::env::var("ATLANTIS_AUTH_BIND").unwrap_or_else(|_| "127.0.0.1:8087".into());
    let listener = tokio::net::TcpListener::bind(bind).await?;
    axum::serve(listener,router).await?;
    Ok(())
}

fn auth_router(state: Arc<AppState>, origins: Vec<HeaderValue>) -> Router {
    let trusted_origins = origins.clone();
    Router::new()
        .route("/api/auth/oidc", post(post_auth_oidc_login))
        .route("/api/auth/me", get(get_auth_me))
        .route("/api/auth/node-browser/capability", get(get_node_browser_capability))
        .route("/api/auth/logout", post(post_auth_logout))
        .route("/api/auth/wallet-challenge", post(post_auth_issue_challenge))
        .route("/api/auth/link-wallet", post(post_auth_link_wallet))
        .route("/api/auth/align", post(post_auth_align_accounts))
        .route("/api/auth/jwks-sync", post(post_auth_sync_jwks))
        .route("/api/auth/club/presence", get(get_club_presence).post(post_club_presence))
        .route("/api/auth/club/leave", post(post_club_leave))
        .route("/api/auth/club/messages", get(get_club_messages).post(post_club_message))
        .layer(DefaultBodyLimit::max(AUTH_JSON_LIMIT))
        .layer(axum::middleware::from_fn(move |request: axum::extract::Request, next: axum::middleware::Next| {
            let trusted = trusted_origins.clone();
            async move {
                if request.headers().get(axum::http::header::ORIGIN).is_some_and(|v| !trusted.contains(v)) {
                    return axum::response::IntoResponse::into_response(StatusCode::FORBIDDEN);
                }
                let mut response = next.run(request).await;
                response.headers_mut().insert(axum::http::header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
                response
            }
        }))
        .layer(CorsLayer::new().allow_origin(origins)
            .allow_methods([Method::GET,Method::POST,Method::OPTIONS])
            .allow_headers([axum::http::header::CONTENT_TYPE,axum::http::header::AUTHORIZATION]))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::{Body, to_bytes}, http::Request};
    use tower::ServiceExt;

    fn fixture() -> (Router, Arc<AppState>, String, String) {
        let mut sessions = home_node::sessions::SessionStore::open(":memory:").unwrap();
        let now = current_time().unwrap();
        let (alice, _) = sessions.issue("did:atlantis:alice", now, now + 3600).unwrap();
        let (bob, _) = sessions.issue("did:atlantis:bob", now, now + 3600).unwrap();
        let state = Arc::new(AppState {
            verifier: RwLock::new(home_node::GoogleJwksVerifier::new("fixture.apps.googleusercontent.com")),
            sessions: Mutex::new(sessions),
        });
        (auth_router(state.clone(), vec![HeaderValue::from_static("https://play.example")]), state, alice, bob)
    }

    fn post(path: &str, token: &str, body: String) -> Request<Body> {
        Request::builder().method("POST").uri(path)
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/json")
            .body(Body::from(body)).unwrap()
    }

    #[tokio::test]
    async fn actual_router_bounds_json_with_and_without_content_length() {
        let (router, _, alice, _) = fixture();
        for declared in [false, true] {
            let body = serde_json::json!({"scene":"Atlantis:surface","text":"x".repeat(AUTH_JSON_LIMIT)}).to_string();
            let len = body.len();
            let mut request = post("/api/auth/club/messages", &alice, body);
            if declared { request.headers_mut().insert("content-length", HeaderValue::from_str(&len.to_string()).unwrap()); }
            let response = router.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        }
        let response = router.oneshot(post("/api/auth/club/presence", &alice,
            r#"{"scene":"Atlantis:surface"}"#.into())).await.unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn router_uses_bearer_owner_and_rejects_wrong_scene_and_untrusted_origin() {
        let (router, state, alice, bob) = fixture();
        let response = router.clone().oneshot(post("/api/auth/wallet-challenge", &bob,
            serde_json::json!({"principal_did":"did:atlantis:alice","wallet_pubkey_hex":"11".repeat(32)}).to_string())).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let response = router.clone().oneshot(post("/api/auth/club/presence", &alice,
            r#"{"scene":"Atlantis:surface","principal_did":"did:atlantis:bob"}"#.into())).await.unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        let peers = state.sessions.lock().unwrap().club_presence("Atlantis:surface", current_time().unwrap()).unwrap();
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].principal_did, "did:atlantis:alice");
        let response = router.clone().oneshot(post("/api/auth/club/messages", &alice,
            r#"{"scene":"Atlantis:dungeon:1","text":"wrong room"}"#.into())).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let mut request = post("/api/auth/club/presence", &bob, r#"{"scene":"Atlantis:surface"}"#.into());
        request.headers_mut().insert("origin", HeaderValue::from_static("https://untrusted.example"));
        assert_eq!(router.clone().oneshot(request).await.unwrap().status(), StatusCode::FORBIDDEN);
        let response = router.oneshot(post("/api/auth/club/messages", &alice,
            r#"{"scene":"Atlantis:surface","text":"hello","principal_did":"did:atlantis:bob"}"#.into())).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), AUTH_JSON_LIMIT).await.unwrap();
        let message: home_node::sessions::ClubMessage = serde_json::from_slice(&body).unwrap();
        assert_eq!(message.principal_did, "did:atlantis:alice");
    }
}
