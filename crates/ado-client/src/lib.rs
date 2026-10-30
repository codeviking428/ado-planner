//! Hand-written Azure DevOps REST client (api-version 7.1).

mod auth;
mod client;
mod runtime;

pub use auth::{AuthScheme, StaticTokens, TokenSource, authorization_header};
pub use client::{AdoClient, ClientError};
pub use runtime::AdoRuntime;
