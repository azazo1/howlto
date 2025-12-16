use tokio::io;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    IoError(#[from] io::Error),
    #[error(transparent)]
    TomlSerError(#[from] toml::ser::Error),
    #[error(transparent)]
    TomlDeError(#[from] toml::de::Error),
    #[error("Profile {profile} not found.")]
    ProfileNotFound { profile: &'static str },
    #[error(transparent)]
    RigError(#[from] rig::http_client::Error),
    #[error(transparent)]
    ReqwestError(#[from] reqwest::Error),
    #[error("Streaming, {0}")]
    StreamingError(String),
    #[error("{0}")]
    InvalidInput(String),
    #[error("{0}")]
    ClipboardError(String),
    #[error(transparent)]
    TokioJoinError(#[from] tokio::task::JoinError),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn profile_not_found(profile: &'static str) -> Self {
        Self::ProfileNotFound { profile }
    }
}
