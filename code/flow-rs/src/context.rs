//! flow-rs · context —— 节点的运行时上下文（Ch4.3）。
//!
//! Ch3.4 定死的节点生命周期是：`initialize` → `while !closed { exec }` → `close` → `finalize`
//! （`#[derive(Actor)]` 生成的三段式循环）。到此为止，节点从**外界**能拿到的只有「构造时从
//! TOML 参数填进来的字段」和「接线好的端口」——没有任何「运行期由引擎注入的环境」。
//!
//! `Context` 就是补上这一环：引擎在启动每个节点时，交给它一份**运行时上下文**——目前装两样
//! 东西，节点名 `name`（日志、诊断用）和[资源集合](crate::resource::ResourceCollection)。节点
//! 在 `initialize` 时从 `ctx` 里**按名把共享资源借出来**（`Arc<T>`），存进自己的字段备用。
//!
//! ## 为什么只穿过 `initialize`，不动 `exec`
//!
//! 一个刻意的克制：`Context` 只作为 [`Actor::start`](crate::node::Actor::start) 和
//! `initialize` 的参数传入，**不**塞进 `exec`。Ch3.4 已经发布的 `exec(&mut self)` 签名维持
//! 原样——几十个节点的 `exec` 一个不改。节点要用资源，就在 `initialize(&mut self, ctx)` 里
//! 把 `Arc<T>` 从 `ctx` 取出、存进一个 `#[state]` 字段（见 `#[derive(BuildFromPorts)]` 对
//! `#[state]` 的处理），`exec` 里直接读那个字段即可。**「一次性拿、长期持有」**——资源是
//! 构造期就定好的，没必要每轮 `exec` 都重新查表。
//!
//! ## `Context` 必须是 `Send`
//!
//! 它要作为参数被 move 进 `tokio::spawn` 的 future（`Actor::start` 里），故整体必须 `Send`。
//! `String` 是 `Send`，`ResourceCollection`（内部 `Arc<HashMap<String, Arc<dyn Any+Send+Sync>>>`）
//! 也是 `Send + Sync`——于是 `Context` 自动 `Send`，spawn 出的节点 future 保持 `Send`。
//!
//! The per-node runtime context handed to `initialize`. Holds the node name and the
//! shared resource collection. Threaded through `Actor::start`/`initialize` only —
//! `exec(&mut self)` is untouched. `Send`, so it can move into the spawned task.

use crate::resource::ResourceCollection;
use std::any::Any;
use std::sync::Arc;

/// 节点的运行时上下文：节点名 + 共享资源集合。启动时由引擎交给每个节点。
/// Per-node runtime context: the node's name and the shared resource collection.
pub struct Context {
    /// 节点在图内的实例名（诊断 / 日志用）。/ the node's instance name.
    pub name: String,
    /// 本图的共享资源集合（`Arc` 共享，`clone` 廉价）。/ shared resources for this graph.
    pub resources: ResourceCollection,
}

impl Context {
    /// 造一个带名字与资源集合的上下文（Graph Builder 为每个节点各造一个，`resources` 共享克隆）。
    /// A context with a name and (shared-cloned) resource collection.
    pub fn new(name: impl Into<String>, resources: ResourceCollection) -> Self {
        Context {
            name: name.into(),
            resources,
        }
    }

    /// 一个匿名、无资源的空上下文——给不经 Graph Builder 直接 `start` 的场景兜底（单节点
    /// 测试、沙箱、以及 Ch2.x 那些手写 `.start()` 的旧测试）。
    /// An anonymous, resource-less context for direct `start` (tests, sandbox).
    pub fn anonymous() -> Self {
        Context {
            name: String::new(),
            resources: ResourceCollection::default(),
        }
    }

    /// 按名从上下文借出一份共享资源、还原成 `Arc<T>`；查无此名或类型不符 → `None`。
    /// 节点在 `initialize` 里用它把资源取出、存进 `#[state]` 字段。
    /// Borrow a shared resource by name as `Arc<T>`; `None` if absent or wrong type.
    pub fn resource<T: Any + Send + Sync>(&self, name: &str) -> Option<Arc<T>> {
        self.resources.get(name)
    }
}

// ── 测试：匿名上下文查不到资源；带资源的上下文能按名借出（红→绿）──
#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::AnyResource;
    use std::collections::HashMap;

    #[test]
    fn anonymous_has_no_resources() {
        let ctx = Context::anonymous();
        assert_eq!(ctx.name, "");
        assert!(ctx.resource::<i64>("anything").is_none());
    }

    #[test]
    fn context_borrows_resource_by_name() {
        let mut m: HashMap<String, AnyResource> = HashMap::new();
        m.insert("k".to_owned(), Arc::new(9i64));
        let ctx = Context::new("node1", ResourceCollection::from_map(m));
        assert_eq!(ctx.name, "node1");
        assert_eq!(ctx.resource::<i64>("k").map(|a| *a), Some(9));
        assert!(ctx.resource::<String>("k").is_none()); // 类型不符
        assert!(ctx.resource::<i64>("nope").is_none()); // 查无此名
    }
}
