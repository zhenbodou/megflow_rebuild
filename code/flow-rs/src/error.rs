//! flow-rs · error —— 引擎的类型化错误（重写版，用 thiserror）。
//!
//! Ch1.1 的决策：引擎是库，错误要被调用方 `match`，故用 `thiserror` 定义
//! 类型化枚举，而非原版的 `anyhow` 黑盒。枚举**按需生长**——每个变体都在被
//! 真正构造时才加入（避免 dead_code）。当前只有通道相关的两种（Ch1.4）；
//! `UnknownNode` / `Config` 等到 Part 2/3 用到时再补。
//! Engine error type. Grows variant-by-variant as they're actually used.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    /// 通道已关闭（对端全部 drop）。/ channel closed (all peers dropped).
    #[error("channel closed")]
    ChannelClosed,
    /// `recv::<T>` 时封箱内的真实类型与请求的 `T` 不符。
    /// payload type inside the sealed envelope didn't match the requested `T`.
    #[error("message type mismatch on recv")]
    TypeMismatch,
}

/// 引擎统一的 `Result` 别名。/ the engine's `Result` alias.
pub type Result<T> = std::result::Result<T, Error>;
