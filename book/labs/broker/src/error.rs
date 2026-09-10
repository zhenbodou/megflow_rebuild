//! 独立实验只需要通道关闭错误；主工程使用其完整 Error。
#[derive(Debug)]
pub enum Error {
    ChannelClosed,
}
impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("channel closed")
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;
