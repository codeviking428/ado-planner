//! Loopback Azure DevOps REST mock serving the JSON in `crates/ado-mock/fixtures`.

mod fixtures;
mod server;

pub use server::{AdoMock, start};
