use thiserror::Error;

/// Validation errors in configuration
#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("validation failed: {0}")]
    Validate(#[from] validator::ValidationErrors),
}

/// Errors raised during scheduling or provider updates
#[derive(Error, Debug)]
pub enum CoreError {
    #[error("fatal error: {0}")]
    Fatal(#[from] anyhow::Error),

    #[error("retryable error: {0}")]
    Retryable(anyhow::Error),
}
