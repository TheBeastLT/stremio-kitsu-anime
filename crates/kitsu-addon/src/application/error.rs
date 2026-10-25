#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    NotFound(String),
    #[error(transparent)]
    Failed(#[from] anyhow::Error),
}

impl Error {
    pub fn failed(error: impl Into<anyhow::Error>) -> Self {
        Self::Failed(error.into())
    }
}
