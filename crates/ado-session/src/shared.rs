use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;

use ado_client::{AuthScheme, TokenSource};
use ado_core::Error;

use crate::provider::PlannerSession;

/// `PlannerSession` behind a lock so GPUI and `AdoClient` can share it.
#[derive(Clone)]
pub struct SharedSession {
    scheme: AuthScheme,
    inner: Arc<Mutex<PlannerSession>>,
}

impl SharedSession {
    pub fn new(session: PlannerSession) -> Self {
        Self {
            scheme: session.auth_scheme(),
            inner: Arc::new(Mutex::new(session)),
        }
    }

    pub fn inner(&self) -> Arc<Mutex<PlannerSession>> {
        self.inner.clone()
    }
}

#[async_trait]
impl TokenSource for SharedSession {
    fn scheme(&self) -> AuthScheme {
        self.scheme
    }

    async fn access_token(&self) -> Result<String, Error> {
        self.inner.lock().await.ensure_access_token().await
    }

    async fn organization(&self) -> Result<Option<String>, Error> {
        TokenSource::organization(&*self.inner.lock().await).await
    }
}
