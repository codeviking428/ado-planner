//! Loopback Azure DevOps REST mock. Fixture JSON is shared with the TypeScript mock.

mod fixtures;
mod server;

pub use server::{AdoMock, start};
