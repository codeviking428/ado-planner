use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use ado_client::{AdoClient, AuthScheme, StaticTokens, TokenSource};
use ado_core::{AuthMode, Error, SessionInfo};

use crate::auth_mode::entra_client_id;
use crate::identity::{PatIdentity, fetch_pat_identity, fetch_session_identity};
use crate::oauth::{self, OauthEndpoints};
use crate::organization::normalize_organization;
use crate::persist::{
    EncryptedStore, FileStore, MemoryStore, StoredOauth, StoredPat, delete_keyring_key, unix_now,
};

pub struct LoginCreds {
    pub pat: Option<String>,
    pub organization: Option<String>,
}

pub struct PlannerSession {
    scheme: AuthScheme,
    store: EncryptedStore,
    memory_pat: Option<StoredPat>,
    memory_oauth: Option<StoredOauth>,
    identity: Option<PatIdentity>,
    loaded: bool,
    ado_base: Option<String>,
    vssps_base: Option<String>,
    oauth_endpoints: OauthEndpoints,
    client_id: Option<String>,
    open_browser: bool,
}

impl PlannerSession {
    pub fn pat(store: EncryptedStore) -> Self {
        Self {
            scheme: AuthScheme::Pat,
            store,
            memory_pat: None,
            memory_oauth: None,
            identity: None,
            loaded: false,
            ado_base: None,
            vssps_base: None,
            oauth_endpoints: OauthEndpoints::default(),
            client_id: None,
            open_browser: true,
        }
    }

    pub fn oauth(store: EncryptedStore, client_id: String) -> Self {
        Self {
            scheme: AuthScheme::Bearer,
            store,
            memory_pat: None,
            memory_oauth: None,
            identity: None,
            loaded: false,
            ado_base: None,
            vssps_base: None,
            oauth_endpoints: OauthEndpoints::default(),
            client_id: Some(client_id),
            open_browser: true,
        }
    }

    pub fn with_bases(mut self, ado: impl Into<String>, vssps: impl Into<String>) -> Self {
        self.ado_base = Some(ado.into());
        self.vssps_base = Some(vssps.into());
        self
    }

    pub fn with_oauth_endpoints(mut self, endpoints: OauthEndpoints) -> Self {
        self.oauth_endpoints = endpoints;
        self
    }

    pub fn without_browser(mut self) -> Self {
        self.open_browser = false;
        self
    }

    pub fn auth_scheme(&self) -> AuthScheme {
        self.scheme
    }

    fn load_stored(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        if self.store.persist_to_disk()
            && let Some(plain) = self.store.read_plain()
        {
            self.memory_pat = serde_json::from_str(&plain).ok();
            self.memory_oauth = serde_json::from_str(&plain).ok();
        }
    }

    fn load_pat(&mut self) -> Option<StoredPat> {
        self.load_stored();
        self.memory_pat.clone()
    }

    fn persist_oauth(&self, stored: &StoredOauth) -> Result<(), Error> {
        if self.store.persist_to_disk() {
            self.store
                .write_plain(&serde_json::to_string(stored).unwrap_or_else(|_| "{}".into()))?;
        }
        Ok(())
    }

    async fn attach_oauth_identity(&self, stored: &mut StoredOauth) {
        if let Ok(Some(identity)) = fetch_session_identity(
            &stored.access_token,
            AuthScheme::Bearer,
            None,
            self.ado_base.as_deref(),
            self.vssps_base.as_deref(),
        )
        .await
        {
            stored.display_name = Some(identity.display_name);
            stored.username = Some(identity.username);
        }
    }

    pub async fn login(&mut self, creds: Option<LoginCreds>) -> Result<SessionInfo, Error> {
        match self.scheme {
            AuthScheme::Pat => self.login_pat(creds).await,
            AuthScheme::Bearer => self.login_oauth().await,
        }
    }

    async fn login_pat(&mut self, creds: Option<LoginCreds>) -> Result<SessionInfo, Error> {
        let pat = creds
            .as_ref()
            .and_then(|c| c.pat.as_deref())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or(Error::PatRequired)?
            .to_string();
        let organization = normalize_organization(
            creds
                .as_ref()
                .and_then(|c| c.organization.as_deref())
                .unwrap_or(""),
        )?;
        self.validate_pat(&organization, &pat).await?;
        self.memory_pat = Some(StoredPat {
            pat: pat.clone(),
            organization: organization.clone(),
        });
        self.identity = None;
        self.loaded = true;
        if self.store.persist_to_disk() {
            self.store
                .write_plain(&json!({ "pat": pat, "organization": organization }).to_string())?;
        }
        self.session_info().await
    }

    async fn validate_pat(&self, org: &str, pat: &str) -> Result<(), Error> {
        let tokens = Arc::new(StaticTokens {
            token: pat.to_string(),
            organization: Some(org.to_string()),
            scheme: AuthScheme::Pat,
        });
        let client = match (&self.ado_base, &self.vssps_base) {
            (Some(ado), Some(vssps)) => AdoClient::with_base_urls(tokens, ado, vssps),
            _ => AdoClient::new(tokens),
        };
        client
            .list_projects(org)
            .await
            .map_err(|err| Error::message(err.to_string()))?;
        Ok(())
    }

    async fn login_oauth(&mut self) -> Result<SessionInfo, Error> {
        let client_id = self
            .client_id
            .clone()
            .ok_or_else(|| Error::message("ENTRA_CLIENT_ID is not set"))?;
        let start = oauth::start_login(&client_id, &self.oauth_endpoints).await?;
        if self.open_browser {
            let _ = open::that(start.authorize_url.as_str());
        }
        let redirect = oauth::wait_for_redirect(start.listener).await?;
        let code = oauth::parse_code(&redirect, &start.csrf)?;
        let mut stored = oauth::exchange_code(
            &client_id,
            &self.oauth_endpoints,
            &start.redirect,
            &code,
            start.pkce_verifier,
        )
        .await?;
        self.attach_oauth_identity(&mut stored).await;
        self.memory_oauth = Some(stored.clone());
        self.loaded = true;
        self.persist_oauth(&stored)?;
        self.session_info().await
    }

    pub async fn complete_oauth_for_test(
        &mut self,
        code: &str,
        pkce_verifier: oauth2::PkceCodeVerifier,
        redirect: &str,
        csrf: &oauth2::CsrfToken,
        returned: &url::Url,
    ) -> Result<SessionInfo, Error> {
        let client_id = self
            .client_id
            .clone()
            .ok_or_else(|| Error::message("ENTRA_CLIENT_ID is not set"))?;
        let parsed = oauth::parse_code(returned, csrf)?;
        if parsed != code {
            return Err(Error::message("code mismatch"));
        }
        let mut stored = oauth::exchange_code(
            &client_id,
            &self.oauth_endpoints,
            redirect,
            code,
            pkce_verifier,
        )
        .await?;
        self.attach_oauth_identity(&mut stored).await;
        self.memory_oauth = Some(stored.clone());
        self.loaded = true;
        self.persist_oauth(&stored)?;
        self.session_info().await
    }

    pub async fn ensure_access_token(&mut self) -> Result<String, Error> {
        self.load_stored();
        match self.scheme {
            AuthScheme::Pat => self.access_token().await,
            AuthScheme::Bearer => {
                let Some(oauth) = self.memory_oauth.clone() else {
                    return Err(Error::message("No OAuth Session"));
                };
                if !oauth.needs_refresh(unix_now()) {
                    return Ok(oauth.access_token);
                }
                let Some(refresh_token) = oauth.refresh_token.clone() else {
                    return Ok(oauth.access_token);
                };
                let client_id = self
                    .client_id
                    .clone()
                    .ok_or_else(|| Error::message("ENTRA_CLIENT_ID is not set"))?;
                let redirect = oauth
                    .redirect
                    .clone()
                    .unwrap_or_else(|| "http://127.0.0.1".into());
                match crate::oauth::refresh(
                    &client_id,
                    &self.oauth_endpoints,
                    &redirect,
                    &refresh_token,
                )
                .await
                {
                    Ok(mut next) => {
                        if next.display_name.is_none() {
                            next.display_name = oauth.display_name;
                            next.username = oauth.username;
                        }
                        if next.refresh_token.is_none() {
                            next.refresh_token = oauth.refresh_token;
                        }
                        self.memory_oauth = Some(next.clone());
                        self.persist_oauth(&next)?;
                        Ok(next.access_token)
                    }
                    Err(_) => Ok(oauth.access_token),
                }
            }
        }
    }

    pub async fn logout(&mut self) -> Result<(), Error> {
        self.memory_pat = None;
        self.memory_oauth = None;
        self.identity = None;
        self.loaded = true;
        self.store.unlink();
        delete_keyring_key();
        Ok(())
    }

    pub async fn session_info(&mut self) -> Result<SessionInfo, Error> {
        match self.scheme {
            AuthScheme::Pat => {
                let Some(pat) = self.load_pat() else {
                    return Ok(SessionInfo {
                        signed_in: false,
                        display_name: None,
                        username: None,
                        auth_mode: AuthMode::Pat,
                    });
                };
                if self.identity.is_none() {
                    self.identity = fetch_pat_identity(
                        &pat.organization,
                        &pat.pat,
                        self.ado_base.as_deref(),
                        self.vssps_base.as_deref(),
                    )
                    .await
                    .ok()
                    .flatten();
                }
                Ok(SessionInfo {
                    signed_in: true,
                    display_name: Some(
                        self.identity
                            .as_ref()
                            .map(|id| id.display_name.clone())
                            .unwrap_or_else(|| "Signed in".into()),
                    ),
                    username: self.identity.as_ref().map(|id| id.username.clone()),
                    auth_mode: AuthMode::Pat,
                })
            }
            AuthScheme::Bearer => {
                self.load_stored();
                match &self.memory_oauth {
                    Some(oauth) => Ok(SessionInfo {
                        signed_in: true,
                        display_name: oauth.display_name.clone(),
                        username: oauth.username.clone(),
                        auth_mode: AuthMode::Entra,
                    }),
                    None => Ok(SessionInfo {
                        signed_in: false,
                        display_name: None,
                        username: None,
                        auth_mode: AuthMode::Entra,
                    }),
                }
            }
        }
    }
}

#[async_trait]
impl TokenSource for PlannerSession {
    fn scheme(&self) -> AuthScheme {
        self.scheme
    }

    async fn access_token(&self) -> Result<String, Error> {
        match self.scheme {
            AuthScheme::Pat => self
                .memory_pat
                .as_ref()
                .map(|row| row.pat.clone())
                .ok_or(Error::NoPatSession),
            AuthScheme::Bearer => {
                if let Some(oauth) = &self.memory_oauth {
                    return Ok(oauth.access_token.clone());
                }
                Err(Error::message("No OAuth Session"))
            }
        }
    }

    async fn organization(&self) -> Result<Option<String>, Error> {
        Ok(self.memory_pat.as_ref().map(|row| row.organization.clone()))
    }
}

pub fn create_session(data_dir: &std::path::Path) -> PlannerSession {
    let key = crate::persist::load_or_create_keyring_key();
    let persist = crate::persist::should_persist_session(key.is_some());
    let file = FileStore::new(crate::persist::session_dir(data_dir).join("session.bin"));
    let store = if persist {
        EncryptedStore::new(Box::new(file), key)
    } else {
        EncryptedStore::new(Box::new(MemoryStore::new(false)), None)
    };
    if let Some(client_id) = entra_client_id() {
        PlannerSession::oauth(store, client_id)
    } else {
        PlannerSession::pat(store)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persist::EncryptedStore;

    fn session(persist: bool, initial: Option<String>) -> PlannerSession {
        let key = if persist { Some([7u8; 32]) } else { None };
        let inner = if let Some(plain) = initial {
            let store = MemoryStore::new(true);
            let encrypted = EncryptedStore::new(Box::new(store), key);
            encrypted.write_plain(&plain).unwrap();
            encrypted
        } else {
            EncryptedStore::new(Box::new(MemoryStore::new(persist)), key)
        };
        PlannerSession::pat(inner)
    }

    #[tokio::test]
    async fn login_without_a_pat_is_rejected() {
        let mut tokens = session(true, None);
        let err = tokens.login(None).await.unwrap_err();
        assert!(err.to_string().contains("PAT"));
    }

    #[tokio::test]
    async fn blank_pat_is_rejected() {
        let mut tokens = session(true, None);
        let err = tokens
            .login(Some(LoginCreds {
                pat: Some("   ".into()),
                organization: Some("contoso".into()),
            }))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("PAT"));
    }

    #[tokio::test]
    async fn missing_organization_is_rejected() {
        let mut tokens = session(false, None);
        let err = tokens
            .login(Some(LoginCreds {
                pat: Some("pat-secret-value".into()),
                organization: None,
            }))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("organization URL"));
    }

    #[tokio::test]
    async fn stored_credentials_restore_session() {
        let mut tokens = session(
            true,
            Some(json!({ "pat": "pat-secret-value", "organization": "contoso" }).to_string()),
        );
        let info = tokens.session_info().await.unwrap();
        assert!(info.signed_in);
        assert_eq!(tokens.access_token().await.unwrap(), "pat-secret-value");
        assert_eq!(
            tokens.organization().await.unwrap().as_deref(),
            Some("contoso")
        );
    }

    #[tokio::test]
    async fn logout_wipes_the_stored_pat() {
        let mut tokens = session(
            true,
            Some(json!({ "pat": "pat-secret-value", "organization": "contoso" }).to_string()),
        );
        tokens.logout().await.unwrap();
        assert!(!tokens.session_info().await.unwrap().signed_in);
    }

    #[tokio::test]
    async fn memory_only_store_never_writes_the_pat() {
        let mut tokens = session(false, None);
        tokens.memory_pat = Some(StoredPat {
            pat: "pat-secret-value".into(),
            organization: "contoso".into(),
        });
        tokens.loaded = true;
        assert_eq!(tokens.access_token().await.unwrap(), "pat-secret-value");
        assert!(!tokens.store.persist_to_disk());
    }

    #[tokio::test]
    async fn organization_urls_with_extra_path_are_shortened() {
        assert_eq!(
            normalize_organization("https://dev.azure.com/contoso/Shop/_workitems/edit/123")
                .unwrap(),
            "contoso"
        );
    }

    #[tokio::test]
    async fn legacy_pat_only_storage_does_not_create_an_incomplete_session() {
        let mut tokens = session(true, Some("pat-secret-value".into()));
        assert!(!tokens.session_info().await.unwrap().signed_in);
    }
}
