//! Hand-written Azure DevOps REST client (api-version 7.1).

mod auth;
mod client;
mod runtime;
mod urls;

pub use auth::{AuthScheme, StaticTokens, TokenSource, authorization_header};
pub use client::{AdoClient, ClientError};
pub use runtime::AdoRuntime;
pub use urls::{
    PRODUCTION_ADO_BASE, PRODUCTION_VSSPS_BASE, connection_data_urls, identities_search_host,
    profile_lookup_urls, resolve_ado_base, resolve_vssps_base,
};
