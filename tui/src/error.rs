use rocraters::ro_crate::read::CrateReadError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum TuiError {
    #[error("CrateReadError: `{0}`")]
    RoCrate(#[from] CrateReadError),
    #[error("Url parsing error: `{0}`")]
    Url(#[from] url::ParseError),
    #[error("Json parsing error: `{0}`")]
    SerdeJson(#[from] serde_json::Error),
    #[error("No crate loaded yet.")]
    NoState,
    #[error("Id `{0}` not found")]
    IdNotFound(String),
}
