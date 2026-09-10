#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("channel closed")]
    ChannelClosed,
    #[error("message type mismatch on recv")]
    TypeMismatch,
}

pub type Result<T> = std::result::Result<T, Error>;
