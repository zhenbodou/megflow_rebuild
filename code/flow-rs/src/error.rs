//! flow-rs · error —— 引擎的类型化错误（重写版，用 thiserror）。
//!
//! Ch1.1 的决策：引擎是库，错误要被调用方 `match`，故用 `thiserror` 定义
//! 类型化枚举，而非原版的 `anyhow` 黑盒。枚举**按需生长**——每个变体都在被
//! 真正构造时才加入（避免 dead_code）。Ch1.4 的通道两种 + Ch3.1 的配置两种 +
//! Ch3.2 建图校验的一批（跨引用不成立、参数缺失/类型错、暂不支持的接线形态）+
//! Ch3.3 调度的一种（节点任务 panic/取消）。
//! Engine error type. Grows variant-by-variant as they're actually used.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("template type inference failed")]
    TemplateInferFault,
    #[error("node type is not match")]
    ChannelTypeMismatch,
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
    /// `main` 指向的入口图不存在（Ch3.2 build）。/ entry graph named by `main` is absent.
    #[error("main graph {0:?} not found")]
    MainGraphNotFound(String),
    /// TOML 里的节点类型名在注册表里查不到（Ch3.2 build）。/ node type not in registry.
    #[error("unknown node type {0:?}")]
    UnknownNodeType(String),
    /// 端口引用指向一个 `nodes` 里没有的节点（Ch3.2 build）。/ port ref names an undefined node.
    #[error("port reference names an undefined node {0:?}")]
    UnknownNode(String),
    /// 配置给某节点接了一个它类型上没有的端口（Ch3.2 build）。/ node has no such port.
    #[error("node {node:?} has no port {port:?}")]
    UnknownPort {
        /// 节点实例名。
        node: String,
        /// 配置里写的端口名。
        port: String,
    },
    /// 节点类型声明了某端口，但配置从未给它接线（Ch3.2 build）。/ declared port left unwired.
    #[error("node {node:?} port {port:?} is not connected")]
    PortNotConnected {
        /// 节点实例名。
        node: String,
        /// 未接线的端口名。
        port: String,
    },
    /// 一条内部连接的形态不合法（Ch4.1 build）：mpsc 单消费者要求一条连接恰有 1 个接收端
    /// （输入端口）、≥1 个发送端（输出端口）；扇出到多个消费者需要 bcast 节点。
    /// a `connections` edge had an invalid shape (needs exactly one consumer + ≥1 producer).
    #[error("bad connection: {0}")]
    BadConnection(String),
    /// 同一个端口被接了不止一次（Ch4.1 build）：一个输入端口只能有一个来源 channel、
    /// 一个输出端口只能发往一个 channel（一对多的扇出需要 bcast）。
    /// a port was wired more than once (an input has one source, an output one sink).
    #[error("node {node:?} port {port:?} is already connected")]
    PortAlreadyConnected {
        /// 节点实例名。
        node: String,
        /// 被重复接线的端口名。
        port: String,
    },
    /// 节点自有参数缺失或类型不符（Ch3.2 `BuildFromPorts::build` 反序列化 `args`）。
    /// node arg missing or of the wrong type.
    #[error("bad node arg {key:?}: {msg}")]
    Arg {
        /// 出问题的参数键。
        key: String,
        /// 具体原因（缺失 / 反序列化错误）。
        msg: String,
    },
    /// 该接线形态本教学子集尚未支持（如图输入扇出，留到 Ch4.1 广播）。
    /// wiring shape not yet supported in this teaching subset.
    #[error("unsupported: {0}")]
    Unsupported(String),
    /// 节点任务异常收尾——panic 或被取消，即 tokio `JoinError` 的抬升（Ch3.3 调度）。
    /// 正常返回 `Err` 的节点不走这里（那是节点自己的错误原样抬出），只有任务本身
    /// 崩了才落到这个变体。/ a node task panicked or was cancelled (JoinError).
    #[error("node task join error: {0}")]
    TaskJoin(String),
    /// TOML 的 `resources` 里写了个注册表里查不到的资源类型名（Ch4.3 build）。
    /// resource type name not found in the resource registry.
    #[error("unknown resource type {0:?}")]
    UnknownResourceType(String),
    /// 子图内联展开时发现环（Ch4.4 flatten）：某张图沿引用链**直接或间接引用了自己**，
    /// 展开会无限递归。携带那张造成环的图名。注意兄弟式复用（`b1`/`b2` 都引用 `Branch`）
    /// **不是**环——它们前缀不同、是两份独立实例；只有「引用路径上重复出现同一张图」才是环。
    /// a subgraph reference cycle was detected while flattening (a graph reaches itself).
    #[error("subgraph reference cycle through graph {0:?}")]
    SubgraphCycle(String),
}

/// 引擎统一的 `Result` 别名。/ the engine's `Result` alias.
pub type Result<T> = std::result::Result<T, Error>;
