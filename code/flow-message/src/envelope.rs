/*!
 * flow-message · envelope —— 消息信封与类型擦除（重写版 / rewrite）
 *
 * 契约来自 Ch0.3：Envelope::new / unpack / repack<T> / repack_inplace，
 * 外加类型擦除 seal() → SealedEnvelope 与安全 downcast。
 *
 * Message envelope + type erasure. `Envelope<M>` wraps a payload of type `M`
 * together with `EnvelopeInfo`; `seal()` erases `M` into a `SealedEnvelope`
 * (`Box<dyn AnyEnvelope + Send>`) so one channel can carry many payload types,
 * recovered downstream via std's *safe* `downcast_ref`.
 */

use std::any::Any;
use std::sync::Arc;

/// 将端口字符串转为寻址 ID。与原版相同：先解析十进制 u64，否则使用 DefaultHasher。
/// 哈希算法不承诺跨 Rust 版本稳定，不应用作永久存储协议。
// ANCHOR: str2addr
pub fn str2addr(value: &str) -> u64 {
    if let Ok(address) = value.parse::<u64>() {
        return address;
    }
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}
// ANCHOR_END: str2addr

/// 随信封一起流动的七项公共元信息，与原版 Rust EnvelopeInfo 对齐。
// ANCHOR: envelope_info
#[derive(Default, Clone)]
pub struct EnvelopeInfo {
    /// 是否跳过业务处理；标志本身不负责过滤消息。
    pub skipped: bool,
    /// 批处理权重；None 与 Some(0) 是不同状态。
    pub weight: Option<usize>,
    /// 序号；信封允许重复，使用者决定重复序号的业务规则。
    pub partial_id: Option<u64>,
    /// 来源地址。
    pub from_addr: Option<u64>,
    /// 目标地址，供 Demux 等节点寻址。
    pub to_addr: Option<u64>,
    /// 中转地址。
    pub transfer_addr: Option<u64>,
    /// 可跨线程共享的附带数据；克隆信封共享同一 Arc。
    pub extra_data: Option<Arc<dyn Any + Send + Sync>>,
}
// ANCHOR_END: envelope_info

/// 信封：把一条消息 `M` 连同元信息 `EnvelopeInfo` 一起装起来。
/// An envelope wrapping a message `M` plus its `EnvelopeInfo`.
///
/// `msg` 用 `Option<M>` 是因为「取出载荷」`unpack` 会把它拿走（留下 `None`），
/// 且允许存在**空信封**（`empty` / 载荷已被取走）。
/// `msg` is `Option<M>` so `unpack` can take the payload out (leaving `None`),
/// and so empty envelopes are representable.
// ANCHOR: envelope_struct
pub struct Envelope<M> {
    info: EnvelopeInfo,
    msg: Option<M>,
}

impl<M> Envelope<M> {
    /// 用默认元信息装一条消息。/ wrap a message with default info.
    pub fn new(msg: M) -> Self {
        Self {
            info: EnvelopeInfo::default(),
            msg: Some(msg),
        }
    }

    /// 用指定元信息装一条消息。/ wrap a message with the given info.
    pub fn with_info(msg: M, info: EnvelopeInfo) -> Self {
        Self {
            info,
            msg: Some(msg),
        }
    }

    /// 空信封（无载荷）。/ an empty envelope (no payload).
    pub fn empty() -> Self {
        Self {
            info: EnvelopeInfo::default(),
            msg: None,
        }
    }

    /// 只读访问元信息。/ read the info.
    pub fn info(&self) -> &EnvelopeInfo {
        &self.info
    }

    /// 可变访问元信息。/ mutate the info.
    pub fn info_mut(&mut self) -> &mut EnvelopeInfo {
        &mut self.info
    }

    /// 取出载荷（留下空信封）。空信封会 panic。
    /// Take the payload out (leaving the envelope empty). Panics if empty.
    pub fn unpack(&mut self) -> M {
        self.msg.take().expect("envelope has no message")
    }

    /// 复用同一份元信息，换一个新载荷 `T`，产出**新类型**的信封。
    /// Reuse the same info but with a new payload `T`, yielding a fresh
    /// `Envelope<T>`. This is how a node maps `Envelope<In>` → `Envelope<Out>`
    /// while preserving metadata.
    pub fn repack<T>(&self, msg: T) -> Envelope<T> {
        Envelope {
            info: self.info.clone(),
            msg: Some(msg),
        }
    }

    /// 原地替换载荷（类型不变）。/ replace the payload in place (same type).
    pub fn repack_inplace(&mut self, msg: M) {
        self.msg = Some(msg);
    }

    /// 拿走内部载荷与元信息，留下一个空信封（元信息被克隆保留在新信封里）。
    /// Move the payload out into a new envelope, leaving `self` empty.
    pub fn take(&mut self) -> Envelope<M> {
        Envelope {
            info: self.info.clone(),
            msg: self.msg.take(),
        }
    }

    /// 只读借用载荷。空信封会 panic。/ borrow the payload; panics if empty.
    pub fn get_ref(&self) -> &M {
        self.msg.as_ref().expect("envelope has no message")
    }

    /// 可变借用载荷。空信封会 panic。/ borrow the payload mutably; panics if empty.
    pub fn get_mut(&mut self) -> &mut M {
        self.msg.as_mut().expect("envelope has no message")
    }

    /// 是否含载荷。/ whether a payload is present.
    pub fn is_some(&self) -> bool {
        self.msg.is_some()
    }

    /// 是否为空。/ whether the envelope is empty.
    pub fn is_none(&self) -> bool {
        self.msg.is_none()
    }
}
// ANCHOR_END: envelope_struct

// 仅当载荷可克隆时，信封才可克隆（广播 Ch4.1 会用到）。
// Envelope is Clone only when its payload is — broadcast (Ch4.1) needs this.
// ANCHOR: envelope_clone
impl<M: Clone> Clone for Envelope<M> {
    fn clone(&self) -> Self {
        Envelope {
            info: self.info.clone(),
            msg: self.msg.clone(),
        }
    }
}
// ANCHOR_END: envelope_clone

/// 类型擦除的信封 trait：擦掉泛型 `M`，只暴露**不带泛型**的方法（保持对象安全）。
/// Type-erased envelope trait: hides the payload type `M`, exposing only
/// non-generic methods so it stays object-safe (see Ch1.2 §4).
///
/// `as_any` 把自己降级成 `&dyn Any`，真正的 downcast 交给标准库那套**安全**实现——
/// 这正是相对原版「手写 unsafe transmute」的改进（Ch1.2 §5）。
/// `as_any` demotes `self` to `&dyn Any`; the actual downcast is std's *safe*
/// one — our improvement over the original's hand-rolled `unsafe` transmute.
// ANCHOR: any_envelope_trait
pub trait AnyEnvelope: Any {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    /// 在**类型擦除**下克隆自己：擦掉 `M` 后，标准库的 `Clone` 已无从谈起（trait 对象
    /// 不是 `Sized`、也不知道具体类型怎么复制）。于是把克隆能力**烙进 trait**——每个具体
    /// 实现者自己知道怎么克隆一份、再重新封箱。这正是 `dyn-clone` crate 在背后生成的东西，
    /// 我们手写它（Ch1.3 类型擦除的回访，Ch4.2 广播 `Bcast` 的前提）。
    /// Clone under type erasure: each concrete impl clones itself and re-seals.
    fn clone_box(&self) -> SealedEnvelope;
    fn is_some(&self) -> bool;
    fn is_none(&self) -> bool;
    fn info(&self) -> &EnvelopeInfo;
    fn info_mut(&mut self) -> &mut EnvelopeInfo;
}
// ANCHOR_END: any_envelope_trait

// 要能在类型擦除下克隆，封箱前的载荷 `M` 必须可 `Clone`（否则擦除后无从复制）；`Send`
// 是跨任务通行证（`SealedEnvelope` 恒为 `+ Send`）。于是 `AnyEnvelope` 的实现前提从
// 「`M: 'static`」收紧到「`M: 'static + Send + Clone`」——这与原版靠 `dyn-clone` 隐式要求
// 载荷可克隆是同一层约束，只是我们把它写在明面上。
// Payloads must be `Clone` (to clone after erasure) and `Send`; mirrors the
// original's implicit `dyn-clone` requirement, made explicit here.
// ANCHOR: any_envelope_impl
impl<M: 'static + Send + Clone> AnyEnvelope for Envelope<M> {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn clone_box(&self) -> SealedEnvelope {
        Box::new(self.clone())
    }
    fn is_some(&self) -> bool {
        self.msg.is_some()
    }
    fn is_none(&self) -> bool {
        self.msg.is_none()
    }
    fn info(&self) -> &EnvelopeInfo {
        &self.info
    }
    fn info_mut(&mut self) -> &mut EnvelopeInfo {
        &mut self.info
    }
}
// ANCHOR_END: any_envelope_impl

/// 装箱后的类型擦除信封——channel 真正搬运的东西。`+ Send` 是跨任务通行证（Ch1.1 §3）。
/// The boxed, type-erased envelope a channel actually carries. `+ Send` is the
/// cross-task passport (Ch1.1 §3).
// ANCHOR: sealed_envelope
pub type SealedEnvelope = Box<dyn AnyEnvelope + Send>;

// 让**封箱后的**信封也能克隆：标准库对 `Box<T>` 只在 `T: Clone` 时给 `Clone`，而 trait
// 对象 `dyn AnyEnvelope + Send` 不是 `Clone`（也不 `Sized`），故那条 blanket 不适用、
// 与本实现不冲突。这里手写一条：克隆一个 `SealedEnvelope` = 转调其 `clone_box`。这正是
// `dyn-clone` 的 `clone_trait_object!` 宏生成的实现——我们把它摊开来看清楚。
// `Box<dyn Trait>` isn't `Clone` for free; delegate to `clone_box` (the dyn-clone pattern).
impl Clone for SealedEnvelope {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

impl<M: 'static + Send + Clone> Envelope<M> {
    /// 封箱：擦除类型，变成可跨节点/跨线程搬运的 `SealedEnvelope`。
    /// Seal: erase the payload type into a movable `SealedEnvelope`.
    pub fn seal(self) -> SealedEnvelope {
        Box::new(self)
    }
}
// ANCHOR_END: sealed_envelope

// 便捷 downcast：定义在 `dyn AnyEnvelope + Send` 上（即 SealedEnvelope 的内层类型），
// 全程走 std 的安全实现，无一处 unsafe。
// Ergonomic downcast on `dyn AnyEnvelope + Send` (SealedEnvelope's inner type),
// entirely via std's safe path — zero unsafe.
// ANCHOR: safe_downcast
impl dyn AnyEnvelope + Send {
    /// 认领回具体信封类型 `T` 的只读引用；猜错得 `None`。
    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        self.as_any().downcast_ref::<T>()
    }
    /// 认领回具体信封类型 `T` 的可变引用；猜错得 `None`。
    pub fn downcast_mut<T: Any>(&mut self) -> Option<&mut T> {
        self.as_any_mut().downcast_mut::<T>()
    }
    /// 内层是否确为具体类型 `T`。/ whether the erased value is a `T`.
    pub fn is<T: Any>(&self) -> bool {
        self.as_any().is::<T>()
    }
}
// ANCHOR_END: safe_downcast

/// 无载荷的占位信封：某些控制路径（停机信号等）需要「一个空信封」而不携带任何 `M`。
/// A payload-less placeholder envelope, for control paths that need "an empty
/// envelope" carrying no `M`. It is a second, non-generic implementor of
/// `AnyEnvelope`, proving the trait isn't tied to `Envelope<M>`.
// ANCHOR: dummy_envelope
#[derive(Default, Clone)]
pub struct DummyEnvelope;

impl DummyEnvelope {
    /// 封箱成 `SealedEnvelope`。/ seal into a `SealedEnvelope`.
    pub fn seal(self) -> SealedEnvelope {
        Box::new(self)
    }
}

impl AnyEnvelope for DummyEnvelope {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn clone_box(&self) -> SealedEnvelope {
        Box::new(self.clone())
    }
    fn is_some(&self) -> bool {
        false
    }
    fn is_none(&self) -> bool {
        true
    }
    fn info(&self) -> &EnvelopeInfo {
        // 占位信封不携带元信息；引擎当前不会对它调用 info()。
        // Placeholder carries no info; the engine never calls info() on it today.
        unimplemented!("DummyEnvelope has no EnvelopeInfo")
    }
    fn info_mut(&mut self) -> &mut EnvelopeInfo {
        unimplemented!("DummyEnvelope has no EnvelopeInfo")
    }
}
// ANCHOR_END: dummy_envelope

// ── 测试先行（RED→GREEN）：下面这组测试钉死 Ch0.3 的对外契约 ──
// Tests pin the Ch0.3 public contract.
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn new_then_unpack() {
        let mut e = Envelope::new(1i32);
        assert!(e.is_some());
        assert_eq!(e.unpack(), 1i32);
        assert!(e.is_none()); // unpack 取走后信封为空
    }

    #[test]
    fn repack_carries_info_and_changes_type() {
        let mut src = Envelope::new(7i32);
        src.info_mut().partial_id = Some(42);
        // 换载荷类型：i32 → String，元信息随行 / info rides along across the type change
        let mut dst = src.repack(String::from("hi"));
        assert_eq!(dst.info().partial_id, Some(42));
        assert_eq!(dst.unpack(), "hi");
    }

    #[test]
    fn repack_inplace_keeps_type() {
        let mut e = Envelope::new(1i32);
        e.repack_inplace(99);
        assert_eq!(e.unpack(), 99);
    }

    #[test]
    fn type_erasure_seal_then_downcast() {
        let sealed: SealedEnvelope = Envelope::new(3i32).seal();
        assert!(sealed.is::<Envelope<i32>>());
        assert!(!sealed.is::<Envelope<String>>());
        // 认领回具体类型：猜错得 None（安全），猜对拿到 &mut
        assert!(sealed.downcast_ref::<Envelope<String>>().is_none());
        let mut sealed = sealed;
        let inner = sealed.downcast_mut::<Envelope<i32>>().unwrap();
        assert_eq!(inner.unpack(), 3i32);
    }

    #[test]
    fn dummy_is_empty() {
        let sealed: SealedEnvelope = DummyEnvelope.seal();
        assert!(sealed.is_none());
        assert!(sealed.is::<DummyEnvelope>());
        assert!(sealed.downcast_ref::<Envelope<i32>>().is_none());
    }

    #[test]
    fn sealed_envelope_is_cloneable() {
        // Ch4.2 广播 Bcast 的前提：**类型擦除后仍能克隆**。封箱擦掉了 `M`，克隆能力
        // 必须在封箱那一刻就烙进 trait（`clone_box`）。克隆出的副本与原件各自独立，
        // 都能安全 downcast 回具体类型。
        let sealed: SealedEnvelope = Envelope::new(5i32).seal();
        let mut a = sealed.clone(); // 走 `impl Clone for Box<dyn AnyEnvelope + Send>`
        let mut b = sealed; // 原件
        assert_eq!(a.downcast_mut::<Envelope<i32>>().unwrap().unpack(), 5);
        assert_eq!(b.downcast_mut::<Envelope<i32>>().unwrap().unpack(), 5);
    }

    #[test]
    fn cloned_sealed_envelope_carries_info() {
        // 克隆连同元信息一起复制（广播出去的每一份都带着相同的 partial_id）。
        let mut e = Envelope::new(1i32);
        e.info_mut().partial_id = Some(7);
        let sealed: SealedEnvelope = e.seal();
        let clone = sealed.clone();
        assert_eq!(clone.info().partial_id, Some(7));
    }

    #[test]
    fn extra_data_shared_via_arc() {
        let mut e = Envelope::new(1i32);
        e.info_mut().extra_data = Some(Arc::new(String::from("meta")));
        let got = e
            .info()
            .extra_data
            .as_ref()
            .unwrap()
            .downcast_ref::<String>()
            .unwrap();
        assert_eq!(got, "meta");
    }
}
