use ado_session::{
    EncryptedStore, LoginCreds, MemoryStore, OauthEndpoints, PlannerSession,
    entra_client_id_from_env,
};
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

#[tokio::test]
async fn oauth_exchange_against_fake_token_endpoint() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = Router::new().route("/token", post(token));
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let token_url = format!("http://{addr}/token");
    let store = EncryptedStore::new(Box::new(MemoryStore::new(true)), Some([9u8; 32]));
    let mut session = PlannerSession::oauth(store, "11111111-2222-3333-4444-555555555555".into())
        .with_oauth_endpoints(OauthEndpoints {
            auth_url: format!("http://{addr}/authorize"),
            token_url,
        })
        .without_browser();

    let (challenge, verifier) = oauth2::PkceCodeChallenge::new_random_sha256();
    let csrf = oauth2::CsrfToken::new("csrf-token".into());
    let _ = challenge;
    let redirect = url::Url::parse("http://127.0.0.1:9/?code=test-code&state=csrf-token").unwrap();
    let info = session
        .complete_oauth_for_test(
            "test-code",
            verifier,
            "http://127.0.0.1:9/",
            &csrf,
            &redirect,
        )
        .await
        .expect("oauth");
    assert!(info.signed_in);
    assert_eq!(info.auth_mode, ado_core::AuthMode::Entra);
}

async fn token() -> Json<serde_json::Value> {
    Json(json!({
        "token_type": "Bearer",
        "access_token": "fake-access",
        "refresh_token": "fake-refresh",
        "expires_in": 3600
    }))
}

#[test]
fn entra_client_id_from_process_env_map() {
    let mut env = std::collections::HashMap::new();
    env.insert("ENTRA_CLIENT_ID".into(), String::new());
    assert_eq!(entra_client_id_from_env(&env), None);
}
