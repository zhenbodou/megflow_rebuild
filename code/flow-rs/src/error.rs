//! flow-rs · error —— 引擎的类型化错误（重写版，用 thiserror）。
//!
//! Ch1.1 的决策：引擎是库，错误要被调用方 `match`，故用 `thiserror` 定义
//! 类型化枚举，而非原版的 `anyhow` 黑盒。枚举**按需生长**——每个变体都在被
//! 真正构造时才加入（避免 dead_code）。Ch1.4 的通道两种 + Ch3.1 的配置两种；
//! `UnknownNode` 等到 Ch3.2 build() 用到时再补。
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
    /// 图配置 TOML 解析失败（Ch3.1）。`#[from]` 让 `toml::from_str` 的错误
    /// 能被 `?` 直接抬升成本类型。/ TOML graph-config parse failure.
    #[error("config parse error: {0}")]
    Toml(#[from] toml::de::Error),
    /// 端口引用不是 `"节点名:端口名"` 形式（Ch3.1）。
    /// port reference wasn't of the form "node:port".
    #[error("bad port reference {0:?}, expected \"node:port\"")]
    BadPortRef(String),
}

/// 引擎统一的 `Result` 别名。/ the engine's `Result` alias.
pub type Result<T> = std::result::Result<T, Error>;
