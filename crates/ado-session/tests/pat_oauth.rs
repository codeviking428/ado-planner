use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use ado_session::{
    EncryptedStore, LoginCreds, MemoryStore, OauthEndpoints, PlannerSession,
    entra_client_id_from_env,
};
use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use serde_json::json;
use tokio::net::TcpListener;

#[tokio::test]
async fn pat_login_validates_by_listing_projects() {
    let mock = ado_mock::start().await;
    let store = EncryptedStore::new(Box::new(MemoryStore::new(true)), Some([3u8; 32]));
    let mut session = PlannerSession::pat(store).with_bases(&mock.url, &mock.url);
    let info = session
        .login(Some(LoginCreds {
            pat: Some("pat-secret-value".into()),
            organization: Some("https://dev.azure.com/contoso/".into()),
        }))
        .await
        .expect("login");
    assert!(info.signed_in);
    assert_eq!(info.auth_mode, ado_core::AuthMode::Pat);
    assert_eq!(info.display_name.as_deref(), Some("Ada Lovelace"));
    assert_eq!(info.username.as_deref(), Some("ada@contoso.com"));
    assert_eq!(
        TokenSourceExt::org(&session).await.as_deref(),
        Some("contoso")
    );
}

trait TokenSourceExt {
    async fn org(&self) -> Option<String>;
}

impl TokenSourceExt for PlannerSession {
    async fn org(&self) -> Option<String> {
        ado_client::TokenSource::organization(self)
            .await
            .ok()
            .flatten()
    }
}

struct TokenState {
    hits: AtomicU32,
    fail: AtomicBool,
    expires_in: AtomicU32,
}

async fn complete_oauth(
    session: &mut PlannerSession,
    code: &str,
    redirect_uri: &str,
) -> ado_core::SessionInfo {
    let (challenge, verifier) = oauth2::PkceCodeChallenge::new_random_sha256();
    let csrf = oauth2::CsrfToken::new("csrf-token".into());
    let _ = challenge;
    let redirect =
        url::Url::parse(&format!("{redirect_uri}?code={code}&state=csrf-token")).unwrap();
    session
        .complete_oauth_for_test(code, verifier, redirect_uri, &csrf, &redirect)
        .await
        .expect("oauth")
}

async fn start_token_server() -> (String, Arc<TokenState>) {
    let state = Arc::new(TokenState {
        hits: AtomicU32::new(0),
        fail: AtomicBool::new(false),
        expires_in: AtomicU32::new(3600),
    });
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = Router::new()
        .route("/token", post(token))
        .with_state(state.clone());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{addr}"), state)
}

fn oauth_session(token_origin: &str, ado: Option<&str>) -> PlannerSession {
    let store = EncryptedStore::new(Box::new(MemoryStore::new(true)), Some([9u8; 32]));
    let mut session = PlannerSession::oauth(store, "11111111-2222-3333-4444-555555555555".into())
        .with_oauth_endpoints(OauthEndpoints {
            auth_url: format!("{token_origin}/authorize"),
            token_url: format!("{token_origin}/token"),
        })
        .without_browser();
    if let Some(ado) = ado {
        session = session.with_bases(ado, ado);
    }
    session
}

#[tokio::test]
async fn oauth_exchange_against_fake_token_endpoint() {
    let mock = ado_mock::start().await;
    let (origin, hits) = start_token_server().await;
    let mut session = oauth_session(&origin, Some(&mock.url));
    let info = complete_oauth(&mut session, "test-code", "http://127.0.0.1:9/").await;
    assert!(info.signed_in);
    assert_eq!(info.auth_mode, ado_core::AuthMode::Entra);
    assert_eq!(info.display_name.as_deref(), Some("Ada Lovelace"));
    assert_eq!(info.username.as_deref(), Some("ada@contoso.com"));
    assert_eq!(hits.hits.load(Ordering::SeqCst), 1);

    session.ensure_access_token().await.expect("fresh token");
    session.ensure_access_token().await.expect("still fresh");
    assert_eq!(
        hits.hits.load(Ordering::SeqCst),
        1,
        "unexpired access token must not refresh on every request"
    );
}

#[tokio::test]
async fn oauth_refreshes_when_the_access_token_is_expired() {
    let (origin, state) = start_token_server().await;
    state.expires_in.store(0, Ordering::SeqCst);
    let mut session = oauth_session(&origin, Some(&origin));
    complete_oauth(&mut session, "test-code", "http://127.0.0.1:9/").await;
    assert_eq!(state.hits.load(Ordering::SeqCst), 1);
    session.ensure_access_token().await.expect("refresh");
    assert_eq!(state.hits.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn oauth_refresh_failure_keeps_the_access_token() {
    let (origin, state) = start_token_server().await;
    state.expires_in.store(0, Ordering::SeqCst);
    let mut session = oauth_session(&origin, Some(&origin));
    let info = complete_oauth(&mut session, "test-code", "http://127.0.0.1:9/").await;
    assert!(info.signed_in);
    state.fail.store(true, Ordering::SeqCst);
    let token = session
        .ensure_access_token()
        .await
        .expect("keep access token");
    assert_eq!(token, "fake-access");
    assert!(session.session_info().await.unwrap().signed_in);
}

async fn token(State(state): State<Arc<TokenState>>) -> axum::response::Response {
    state.hits.fetch_add(1, Ordering::SeqCst);
    if state.fail.load(Ordering::SeqCst) {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({ "error": "invalid_grant" })),
        )
            .into_response();
    }
    Json(json!({
        "token_type": "Bearer",
        "access_token": "fake-access",
        "refresh_token": "fake-refresh",
        "expires_in": state.expires_in.load(Ordering::SeqCst)
    }))
    .into_response()
}

#[test]
fn entra_client_id_from_process_env_map() {
    let mut env = std::collections::HashMap::new();
    env.insert("ENTRA_CLIENT_ID".into(), String::new());
    assert_eq!(entra_client_id_from_env(&env), None);
}
