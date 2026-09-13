#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("template type inference failed")]
    TemplateInferFault,
    #[error("no compatible channel type")]
    ChannelTypeMismatch,
    #[error("channel closed")]
    ChannelClosed,
    #[error("message type mismatch on recv")]
    TypeMismatch,
}

pub type Result<T> = std::result::Result<T, Error>;
