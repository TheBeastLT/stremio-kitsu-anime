use std::sync::Arc;

use super::TypeError;

#[derive(Debug, Clone, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Failed(Arc<anyhow::Error>),
    #[error("{0}")]
    Busy(String),
}

impl Error {
    pub fn failed(error: impl Into<anyhow::Error>) -> Self {
        Self::Failed(Arc::new(error.into()))
    }

    #[must_use]
    pub fn into_failed(self) -> Self {
        match self {
            Self::NotFound(reason) => Self::failed(anyhow::anyhow!(reason)),
            other => other,
        }
    }
}

impl From<anyhow::Error> for Error {
    fn from(error: anyhow::Error) -> Self {
        Self::failed(error)
    }
}

impl From<TypeError> for Error {
    fn from(error: TypeError) -> Self {
        Self::failed(anyhow::anyhow!(error.0))
    }
}
