use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("{0}")]
    Message(String),
    #[error(transparent)]
    Client(#[from] ado_client::ClientError),
    #[error(transparent)]
    Core(#[from] ado_core::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl AppError {
    pub fn message(text: impl Into<String>) -> Self {
        Self::Message(text.into())
    }
}
