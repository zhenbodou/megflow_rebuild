#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("config parse error: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("bad port reference {0:?}, expected node:port")]
    BadPortRef(String),
}

pub type Result<T> = std::result::Result<T, Error>;
