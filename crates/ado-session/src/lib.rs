//! PAT (default) and OAuth PKCE sessions. Secrets live in the OS keyring.

mod auth_mode;
mod identity;
mod oauth;
mod organization;
mod persist;
mod provider;
mod shared;

pub use auth_mode::entra_client_id_from_env;
pub use identity::{
    PatIdentity, fetch_pat_identity, identity_from_connection_data, identity_from_profile,
};
pub use oauth::OauthEndpoints;
pub use organization::normalize_organization;
pub use persist::{EncryptedStore, MemoryStore, SessionStore, should_persist_session};
pub use provider::{LoginCreds, PlannerSession, create_session};
pub use shared::SharedSession;
