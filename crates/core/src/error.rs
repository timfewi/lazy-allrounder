use thiserror::Error;

#[derive(Debug, Error)]
pub enum PortError {
    #[error("provider authentication failed")]
    Authentication,
    #[error("provider rate limit reached")]
    RateLimited,
    #[error("provider request timed out")]
    Timeout,
    #[error("provider is unavailable")]
    ProviderUnavailable,
    #[error("provider returned a malformed response")]
    MalformedResponse,
    #[error("{capability} is not supported on this platform")]
    UnsupportedCapability { capability: &'static str },
    #[error("{message}")]
    Other { message: String },
}

impl PortError {
    pub fn unsupported(capability: &'static str) -> Self {
        Self::UnsupportedCapability { capability }
    }
}

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("input text cannot be empty")]
    EmptyInput,
    #[error("input text is too large")]
    InputTooLarge,
    #[error("question cannot be empty")]
    EmptyQuestion,
    #[error(transparent)]
    Port(#[from] PortError),
}
