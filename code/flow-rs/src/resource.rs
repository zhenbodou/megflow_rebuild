//! flow-rs · resource —— 跨节点共享的资源（Ch4.3）。
//!
//! 到 Ch4.2 为止，每个节点都是**自给自足**的：自己的字段、自己的参数、自己的端口。可真实
//! 算法仓里，一份**检测模型**动辄几百 MB，检测、跟踪、告警三条支路要是各加载一份，显存瞬间
//! 三倍——它们其实该**共享同一份**。「内存池」同理：一块预分配的大缓冲，多个节点轮流借用，
//! 而非各开各的。这就是**资源（Resource）**：**构造一次、被多个节点共享**的重对象。
//!
//! 共享要跨越两道类型鸿沟：
//! 1. **一张表里放不同类型**——图里可能同时有 `Counter`、`Model`、`MemPool`，它们类型不同，
//!    却要塞进同一张 `name → resource` 表。解法与 Ch1.3 的消息层一样：**类型擦除**。
//! 2. **多个持有者**——一份资源被 N 个节点同时持有，且这些节点各自跑在独立 tokio 任务里。
//!    解法是 [`Arc`]（原子引用计数）：`clone` 只加计数、不拷贝底层对象，最后一个持有者 drop
//!    时才真正释放。
//!
//! 合起来就是 [`AnyResource`] = `Arc<dyn Any + Send + Sync>`：`Arc` 管共享，`dyn Any` 抹类型，
//! `Send + Sync` 保证能安全跨任务共享。
//!
//! ## 与 Ch1.3 的关键对照：这次**不需要**自定义 trait
//!
//! Ch1.3 做消息类型擦除时，我们**没**直接用 `dyn Any`，而是自定义了 `AnyEnvelope` trait
//! （带 `clone_box`）——因为消息需要一个 `std::any::Any` 给不了的**自定义 vtable 行为**：
//! 类型擦除后仍能克隆。而资源**没有**任何这类自定义行为的需求——节点只想「把它按原类型借出来
//! 用」，不需要 clone、不需要跨类型的统一操作。所以这里**直接用标准库的 `Any`** 就够了，配上
//! `Arc<dyn Any>::downcast::<T>()`（std 自 1.29 提供）把 `Arc` 整个还原回 `Arc<T>`。
//! **「需要自定义行为才造自定义 trait，否则用 std 的 `Any`」——这个取舍本身就是本章的一课。**
//!
//! Shared, construct-once resources. `AnyResource = Arc<dyn Any + Send + Sync>`:
//! `Arc` for sharing, `dyn Any` for type erasure, `Send + Sync` for cross-task safety.
//! Unlike Ch1.3's messages (which needed a custom `clone_box` vtable), resources need
//! no custom behavior, so std `Any` + `Arc::downcast` suffices.

use crate::config::Args;
use crate::error::Result;
use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

/// 类型擦除的共享资源句柄：`Arc<dyn Any + Send + Sync>`。
///
/// - `Arc`——多个节点共享同一份（`clone` 加引用计数，不拷贝底层）。
/// - `dyn Any`——抹掉具体类型，好让不同类型的资源塞进同一张表。
/// - `Send + Sync`——能安全地在（跑在不同 tokio 任务里的）节点间共享。
///
/// A type-erased, shared resource handle.
// ANCHOR: any_resource
pub type AnyResource = Arc<dyn Any + Send + Sync>;
// ANCHOR_END: any_resource

/// 把类型擦除的 [`AnyResource`] 还原成具体的 `Arc<T>`；类型不符则返回 `None`。
///
/// 靠的是标准库为 `Arc<dyn Any + Send + Sync>` 提供的 `downcast::<T>()`——它把**整个 `Arc`**
/// （连同共享计数）还原回 `Arc<T>`，成功得 `Ok(Arc<T>)`、失败把原 `Arc` 原样还回 `Err`。这与
/// Ch1.3 的 `downcast_ref`（只借出 `&T`）不同：资源要**持有**（塞进节点字段长期保存），故要
/// 还原出带所有权的 `Arc<T>`，而非一个借用。`.ok()` 把「失败时还回原 `Arc`」这个我们不关心的
/// 分支丢掉，只留 `Option`。
///
/// Downcast a type-erased resource back to `Arc<T>`, or `None` on type mismatch.
// ANCHOR: downcast_arc
pub fn downcast_arc<T: Any + Send + Sync>(r: AnyResource) -> Option<Arc<T>> {
    r.downcast::<T>().ok()
}
// ANCHOR_END: downcast_arc

/// 「可被引擎构造的资源」trait——由 `resource_register!` 注册的资源类型实现它。
///
/// 与节点的 [`crate::registry::BuildFromPorts`] 对偶，但简单得多：资源没有端口、没有接线，
/// 只从**配置参数** `args`（TOML 里资源那一项 `ty`/`name` 之外的键）构造出自己。`Self: Sized`
/// 因为 `build` 按值返回 `Self`——我们只以 `<ConcreteType as BuildResource>::build` 取函数
/// 指针，从不需要 `dyn BuildResource`。
///
/// A resource the engine can construct from config args (dual to `BuildFromPorts`).
// ANCHOR: build_resource
pub trait BuildResource: Any + Send + Sync {
    /// 从配置参数构造资源实例；参数不对 → `Err`。
    /// Construct from config args; bad args → `Err`.
    fn build(args: &Args) -> Result<Self>
    where
        Self: Sized;
}
// ANCHOR_END: build_resource

/// 把 `<T as BuildResource>::build` 包成一个**类型擦除**的构造器——`resource_register!` 生成的
/// 注册条目 `ctor` 就指向它（对偶于 `NodeRegistration::ctor` 指向 `BuildFromPorts::build`）。
///
/// 这里有个 Rust 新手常踩的**类型推断坑**，值得单独点出：不能写成
/// `Ok(Arc::new(T::build(args)?))`。因为 `Ok(..)` 的目标类型是 `Result<AnyResource>`，编译器
/// 会把 `Arc::new(..)` 的类型**先**推成 `Arc<T>`，再期望它等于 `AnyResource = Arc<dyn Any..>`
/// ——而「`Arc<T>` → `Arc<dyn Any>`」这个 **unsize 强转**只在**显式类型标注**的赋值点才会发生，
/// 埋在 `Ok(..)` 里编译器不给做。所以必须先 `let any: AnyResource = r;` 给一个**明确的目标
/// 类型**，让强转在这一行落地，再 `Ok(any)`。
///
/// Wrap `T::build` into a type-erased constructor. The explicit `let any: AnyResource`
/// is the coercion site: `Ok(Arc::new(..))` won't unsize `Arc<T>` → `Arc<dyn Any>`.
// ANCHOR: build_arc
pub fn build_arc<T: BuildResource>(args: &Args) -> Result<AnyResource> {
    let r: Arc<T> = Arc::new(T::build(args)?);
    let any: AnyResource = r; // 显式强转点：Arc<T> → Arc<dyn Any + Send + Sync>
    Ok(any)
}
// ANCHOR_END: build_arc

/// 一张**共享的**「资源名 → 资源」表，随 [`crate::context::Context`] 一起交给每个节点。
///
/// 内部是 `Arc<HashMap<..>>`——整张表本身也 `Arc` 共享：`Context` 分发给 N 个节点时，
/// `clone` 的只是外层 `Arc`（加计数），底层那张表与表里每份资源都**只有一份**。这正是「共享
/// 模型 / 内存池」在数据结构上的落点。表在装配期（Graph Builder）一次建好后**只读**，故用
/// 朴素 `HashMap` 而非并发容器——没有写竞争，读多个 `Arc` 无需加锁。
///
/// A shared `name → resource` table, cloned (cheaply, via `Arc`) into each node's `Context`.
// ANCHOR: collection
#[derive(Clone, Default)]
pub struct ResourceCollection {
    inner: Arc<HashMap<String, AnyResource>>,
}

impl ResourceCollection {
    /// 从一张建好的 `name → AnyResource` 表封装出集合（Graph Builder 装配期调用一次）。
    /// Wrap a built `name → resource` map (called once at assembly time).
    pub fn from_map(m: HashMap<String, AnyResource>) -> Self {
        Self { inner: Arc::new(m) }
    }

    /// 按名取资源并还原成 `Arc<T>`；查无此名、或类型不符 → `None`。
    /// Look up a resource by name and downcast to `Arc<T>`; `None` if absent or wrong type.
    pub fn get<T: Any + Send + Sync>(&self, name: &str) -> Option<Arc<T>> {
        downcast_arc::<T>(self.inner.get(name)?.clone())
    }
}
// ANCHOR_END: collection

// ── 测试：类型擦除往返、错类型落 None、集合按名取（红→绿）──
#[cfg(test)]
mod tests {
    use super::*;

    /// 一个供测试用的资源类型：从 args 读一个整数 `v`（缺省 0）。
    struct Dummy {
        v: i64,
    }
    impl BuildResource for Dummy {
        fn build(args: &Args) -> Result<Self> {
            let v = args.get("v").and_then(|x| x.as_integer()).unwrap_or(0);
            Ok(Dummy { v })
        }
    }

    #[test]
    fn downcast_round_trips_to_same_type() {
        // Arc<i64> 擦成 AnyResource，再按 i64 还原——成功，且共享同一底层值。
        let any: AnyResource = Arc::new(42i64);
        let back = downcast_arc::<i64>(any).unwrap();
        assert_eq!(*back, 42);
    }

    #[test]
    fn downcast_wrong_type_is_none() {
        // 擦的是 i64，按 String 还原应失败（None），而非 panic 或误还原。
        let any: AnyResource = Arc::new(42i64);
        assert!(downcast_arc::<String>(any).is_none());
    }

    #[test]
    fn build_arc_erases_then_downcasts_back() {
        // build_arc 走 BuildResource::build + 显式 unsize 强转；再还原验证 args 真被读到。
        let args: Args = toml::from_str("v = 7").unwrap();
        let any = build_arc::<Dummy>(&args).unwrap();
        let d = downcast_arc::<Dummy>(any).unwrap();
        assert_eq!(d.v, 7);
    }

    #[test]
    fn collection_gets_by_name_and_type() {
        let mut m: HashMap<String, AnyResource> = HashMap::new();
        m.insert("n".to_owned(), Arc::new(100i64));
        let rc = ResourceCollection::from_map(m);
        // 名字对、类型对 → Some
        assert_eq!(rc.get::<i64>("n").map(|a| *a), Some(100));
        // 名字对、类型错 → None
        assert!(rc.get::<String>("n").is_none());
        // 查无此名 → None
        assert!(rc.get::<i64>("missing").is_none());
    }

    #[test]
    fn collection_shares_one_instance() {
        // 同一份资源被取两次，得到的是共享同一底层的两个 Arc（强引用计数 > 1）。
        let mut m: HashMap<String, AnyResource> = HashMap::new();
        m.insert("n".to_owned(), Arc::new(1i64));
        let rc = ResourceCollection::from_map(m);
        let a = rc.get::<i64>("n").unwrap();
        let b = rc.get::<i64>("n").unwrap();
        assert!(Arc::ptr_eq(&a, &b), "两次取到的应是同一份共享资源");
    }
}
