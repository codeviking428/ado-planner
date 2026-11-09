use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum Error {
    #[error("{0}")]
    Message(String),
    #[error("Enter an Azure DevOps organization URL such as https://dev.azure.com/contoso")]
    InvalidOrganizationUrl,
    #[error("required field is empty: {0}")]
    RequiredField(String),
    #[error("PAT is required")]
    PatRequired,
    #[error("No PAT Session")]
    NoPatSession,
    #[error(
        "Azure DevOps rejected the personal access token. It may be invalid, expired, or missing access to this organization."
    )]
    PatRejected,
    #[error("Your Microsoft sign-in has expired. Sign in again to keep planning.")]
    SignInRequired,
    #[error("No Azure DevOps organization is configured")]
    NoOrganization,
}

impl Error {
    pub fn message(text: impl Into<String>) -> Self {
        Self::Message(text.into())
    }
}
