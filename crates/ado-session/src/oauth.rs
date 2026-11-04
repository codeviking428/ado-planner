use std::net::SocketAddr;
use std::time::Duration;

use oauth2::basic::BasicClient;
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, CsrfToken, EndpointNotSet, EndpointSet,
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, RefreshToken, Scope, TokenResponse, TokenUrl,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use url::Url;

use ado_core::{ADO_RESOURCE_SCOPE, ENTRA_AUTHORITY, Error};

use crate::persist::{StoredOauth, unix_now};

type ConfiguredClient =
    BasicClient<EndpointSet, EndpointNotSet, EndpointNotSet, EndpointNotSet, EndpointSet>;

#[derive(Debug, Clone)]
pub struct OauthEndpoints {
    pub auth_url: String,
    pub token_url: String,
}

impl Default for OauthEndpoints {
    fn default() -> Self {
        Self {
            auth_url: format!("{ENTRA_AUTHORITY}/oauth2/v2.0/authorize"),
            token_url: format!("{ENTRA_AUTHORITY}/oauth2/v2.0/token"),
        }
    }
}

fn client(
    client_id: &str,
    endpoints: &OauthEndpoints,
    redirect: &str,
) -> Result<ConfiguredClient, Error> {
    Ok(BasicClient::new(ClientId::new(client_id.to_string()))
        .set_auth_uri(
            AuthUrl::new(endpoints.auth_url.clone())
                .map_err(|err| Error::message(err.to_string()))?,
        )
        .set_token_uri(
            TokenUrl::new(endpoints.token_url.clone())
                .map_err(|err| Error::message(err.to_string()))?,
        )
        .set_redirect_uri(
            RedirectUrl::new(redirect.to_string())
                .map_err(|err| Error::message(err.to_string()))?,
        ))
}

pub struct OauthStart {
    pub authorize_url: Url,
    pub csrf: CsrfToken,
    pub pkce_verifier: PkceCodeVerifier,
    pub redirect: String,
    pub listener: TcpListener,
}

pub async fn start_login(client_id: &str, endpoints: &OauthEndpoints) -> Result<OauthStart, Error> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|err| Error::message(err.to_string()))?;
    let addr: SocketAddr = listener
        .local_addr()
        .map_err(|err| Error::message(err.to_string()))?;
    let redirect = format!("http://127.0.0.1:{}", addr.port());
    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
    let oauth = client(client_id, endpoints, &redirect)?;
    let (authorize_url, csrf) = oauth
        .authorize_url(CsrfToken::new_random)
        .add_scope(Scope::new(format!("{ADO_RESOURCE_SCOPE} offline_access")))
        .set_pkce_challenge(pkce_challenge)
        .url();
    Ok(OauthStart {
        authorize_url,
        csrf,
        pkce_verifier,
        redirect,
        listener,
    })
}

fn http_client() -> Result<reqwest::Client, Error> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|err| Error::message(err.to_string()))
}

pub async fn wait_for_redirect(listener: TcpListener) -> Result<Url, Error> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(Error::message("OAuth redirect timed out"));
        }
        let (mut stream, _) = tokio::time::timeout(remaining, listener.accept())
            .await
            .map_err(|_| Error::message("OAuth redirect timed out"))?
            .map_err(|err| Error::message(err.to_string()))?;
        let mut buf = vec![0u8; 4096];
        let n = stream
            .read(&mut buf)
            .await
            .map_err(|err| Error::message(err.to_string()))?;
        let request = String::from_utf8_lossy(&buf[..n]);
        let path = request
            .lines()
            .next()
            .and_then(|line| line.split(' ').nth(1))
            .unwrap_or("/");
        let body = b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n<html><body>You can close this window.</body></html>";
        let _ = stream.write_all(body).await;
        let url = Url::parse(&format!("http://127.0.0.1{path}"))
            .map_err(|err| Error::message(err.to_string()))?;
        let mut code = false;
        let mut state = false;
        let mut error: Option<String> = None;
        let mut error_description: Option<String> = None;
        for (key, value) in url.query_pairs() {
            match key.as_ref() {
                "code" => code = true,
                "state" => state = true,
                "error" => error = Some(value.into_owned()),
                "error_description" => error_description = Some(value.into_owned()),
                _ => {}
            }
        }
        if let Some(error) = error {
            return Err(Error::message(error_description.unwrap_or(error)));
        }
        if code || state {
            return Ok(url);
        }
    }
}

pub async fn exchange_code(
    client_id: &str,
    endpoints: &OauthEndpoints,
    redirect: &str,
    code: &str,
    pkce_verifier: PkceCodeVerifier,
) -> Result<StoredOauth, Error> {
    let oauth = client(client_id, endpoints, redirect)?;
    let http = http_client()?;
    let token = oauth
        .exchange_code(AuthorizationCode::new(code.to_string()))
        .set_pkce_verifier(pkce_verifier)
        .request_async(&http)
        .await
        .map_err(|err| Error::message(err.to_string()))?;
    Ok(stored_from_token(&token, redirect))
}

pub async fn refresh(
    client_id: &str,
    endpoints: &OauthEndpoints,
    redirect: &str,
    refresh_token: &str,
) -> Result<StoredOauth, Error> {
    let oauth = client(client_id, endpoints, redirect)?;
    let http = http_client()?;
    let token = oauth
        .exchange_refresh_token(&RefreshToken::new(refresh_token.to_string()))
        .request_async(&http)
        .await
        .map_err(|err| Error::message(err.to_string()))?;
    Ok(stored_from_token(&token, redirect))
}

const DEFAULT_ACCESS_TOKEN_LIFETIME_SECS: i64 = 3600;

fn stored_from_token(
    token: &oauth2::StandardTokenResponse<
        oauth2::EmptyExtraTokenFields,
        oauth2::basic::BasicTokenType,
    >,
    redirect: &str,
) -> StoredOauth {
    let lifetime = token
        .expires_in()
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(DEFAULT_ACCESS_TOKEN_LIFETIME_SECS);
    StoredOauth {
        access_token: token.access_token().secret().to_string(),
        refresh_token: token.refresh_token().map(|t| t.secret().to_string()),
        expires_at: Some(unix_now().saturating_add(lifetime)),
        redirect: Some(redirect.to_string()),
        display_name: None,
        username: None,
    }
}

pub fn parse_code(redirect: &Url, expected_csrf: &CsrfToken) -> Result<String, Error> {
    let mut code = None;
    let mut state = None;
    let mut error = None;
    let mut error_description = None;
    for (key, value) in redirect.query_pairs() {
        match key.as_ref() {
            "code" => code = Some(value.to_string()),
            "state" => state = Some(value.to_string()),
            "error" => error = Some(value.to_string()),
            "error_description" => error_description = Some(value.to_string()),
            _ => {}
        }
    }
    if let Some(error) = error {
        return Err(Error::message(error_description.unwrap_or(error)));
    }
    if state.as_deref() != Some(expected_csrf.secret()) {
        return Err(Error::message("OAuth state mismatch"));
    }
    code.ok_or_else(|| Error::message("OAuth code missing"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_code_surfaces_error_description() {
        let url = Url::parse(
            "http://127.0.0.1:9/?error=access_denied&error_description=user%20cancelled&state=abc",
        )
        .unwrap();
        let csrf = CsrfToken::new("abc".into());
        let err = parse_code(&url, &csrf).unwrap_err();
        assert!(err.to_string().contains("user cancelled"));
    }
}
