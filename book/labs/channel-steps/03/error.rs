#[derive(Debug)]
pub enum Error {
    ChannelClosed,
    TypeMismatch,
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::ChannelClosed => formatter.write_str("channel closed"),
            Error::TypeMismatch => formatter.write_str("message type mismatch on recv"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
