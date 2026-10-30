use async_trait::async_trait;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use ado_core::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthScheme {
    Bearer,
    Pat,
}

pub fn authorization_header(token: &str, scheme: AuthScheme) -> String {
    match scheme {
        AuthScheme::Pat => {
            let encoded = STANDARD.encode(format!(":{token}"));
            format!("Basic {encoded}")
        }
        AuthScheme::Bearer => format!("Bearer {token}"),
    }
}

#[async_trait]
pub trait TokenSource: Send + Sync {
    fn scheme(&self) -> AuthScheme;
    async fn access_token(&self) -> Result<String, Error>;
    async fn organization(&self) -> Result<Option<String>, Error>;
}

#[derive(Debug, Clone)]
pub struct StaticTokens {
    pub token: String,
    pub organization: Option<String>,
    pub scheme: AuthScheme,
}

#[async_trait]
impl TokenSource for StaticTokens {
    fn scheme(&self) -> AuthScheme {
        self.scheme
    }

    async fn access_token(&self) -> Result<String, Error> {
        Ok(self.token.clone())
    }

    async fn organization(&self) -> Result<Option<String>, Error> {
        Ok(self.organization.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pat_uses_http_basic_with_an_empty_username() {
        assert_eq!(
            authorization_header("pat-secret-value", AuthScheme::Pat),
            "Basic OnBhdC1zZWNyZXQtdmFsdWU="
        );
    }

    #[test]
    fn entra_session_uses_bearer() {
        assert_eq!(
            authorization_header("eyJhbGciOiJIUzI1NiJ9", AuthScheme::Bearer),
            "Bearer eyJhbGciOiJIUzI1NiJ9"
        );
    }
}
