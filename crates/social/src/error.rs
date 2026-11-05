use platform::crypto::CryptoError;

/// Infrastructure failures of the social module (never shown to clients).
#[derive(Debug, thiserror::Error)]
pub enum SocialError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("crypto error: {0}")]
    Crypto(#[from] CryptoError),
    #[error("system random number generator failed")]
    Randomness,
    /// The identity module (age, account) failed.
    #[error("identity error: {0}")]
    Identity(String),
    #[error("outbox error: {0}")]
    Outbox(#[from] platform::outbox::OutboxError),
    #[error("corrupt row: {0}")]
    Corrupt(String),
}

/// Why an invite token cannot be used. `NotFound` and `Expired` are the only outcomes a client
/// may learn (a reset, expired or used-up token is `Expired`).
#[derive(Debug, thiserror::Error)]
pub enum InviteError {
    #[error("invite not found")]
    NotFound,
    #[error("invite expired")]
    Expired,
    #[error(transparent)]
    Social(#[from] SocialError),
}

impl From<sqlx::Error> for InviteError {
    fn from(err: sqlx::Error) -> Self {
        Self::Social(SocialError::Database(err))
    }
}
