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

/// 随信封一起流动的公共元信息。/ Metadata that travels alongside a message.
///
/// 先只放引擎当前真正要用到的两个字段；其余（weight / from_addr / …）待对应
/// 特性（重排序、寻址）落地时再按需增补——避免提前引入用不上的状态。
/// Only the two fields the engine needs right now; the rest grow in when the
/// features that use them (reorder, addressing) actually land.
#[derive(Default, Clone)]
pub struct EnvelopeInfo {
    /// 序号（可重复），用于重排序等场景。/ sequence id (may repeat), for reordering.
    pub partial_id: Option<u64>,
    /// 任意类型、可跨线程共享的附带数据。/ arbitrary, thread-shareable side data.
    pub extra_data: Option<Arc<dyn Any + Send + Sync>>,
}

/// 信封：把一条消息 `M` 连同元信息 `EnvelopeInfo` 一起装起来。
/// An envelope wrapping a message `M` plus its `EnvelopeInfo`.
///
/// `msg` 用 `Option<M>` 是因为「取出载荷」`unpack` 会把它拿走（留下 `None`），
/// 且允许存在**空信封**（`empty` / 载荷已被取走）。
/// `msg` is `Option<M>` so `unpack` can take the payload out (leaving `None`),
/// and so empty envelopes are representable.
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

// 仅当载荷可克隆时，信封才可克隆（广播 Ch4.1 会用到）。
// Envelope is Clone only when its payload is — broadcast (Ch4.1) needs this.
impl<M: Clone> Clone for Envelope<M> {
    fn clone(&self) -> Self {
        Envelope {
            info: self.info.clone(),
            msg: self.msg.clone(),
        }
    }
}

/// 类型擦除的信封 trait：擦掉泛型 `M`，只暴露**不带泛型**的方法（保持对象安全）。
/// Type-erased envelope trait: hides the payload type `M`, exposing only
/// non-generic methods so it stays object-safe (see Ch1.2 §4).
///
/// `as_any` 把自己降级成 `&dyn Any`，真正的 downcast 交给标准库那套**安全**实现——
/// 这正是相对原版「手写 unsafe transmute」的改进（Ch1.2 §5）。
/// `as_any` demotes `self` to `&dyn Any`; the actual downcast is std's *safe*
/// one — our improvement over the original's hand-rolled `unsafe` transmute.
pub trait AnyEnvelope: Any {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn is_some(&self) -> bool;
    fn is_none(&self) -> bool;
    fn info(&self) -> &EnvelopeInfo;
    fn info_mut(&mut self) -> &mut EnvelopeInfo;
}

impl<M: 'static> AnyEnvelope for Envelope<M> {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
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

/// 装箱后的类型擦除信封——channel 真正搬运的东西。`+ Send` 是跨任务通行证（Ch1.1 §3）。
/// The boxed, type-erased envelope a channel actually carries. `+ Send` is the
/// cross-task passport (Ch1.1 §3).
pub type SealedEnvelope = Box<dyn AnyEnvelope + Send>;

impl<M: 'static + Send> Envelope<M> {
    /// 封箱：擦除类型，变成可跨节点/跨线程搬运的 `SealedEnvelope`。
    /// Seal: erase the payload type into a movable `SealedEnvelope`.
    pub fn seal(self) -> SealedEnvelope {
        Box::new(self)
    }
}

// 便捷 downcast：定义在 `dyn AnyEnvelope + Send` 上（即 SealedEnvelope 的内层类型），
// 全程走 std 的安全实现，无一处 unsafe。
// Ergonomic downcast on `dyn AnyEnvelope + Send` (SealedEnvelope's inner type),
// entirely via std's safe path — zero unsafe.
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

/// 无载荷的占位信封：某些控制路径（停机信号等）需要「一个空信封」而不携带任何 `M`。
/// A payload-less placeholder envelope, for control paths that need "an empty
/// envelope" carrying no `M`. It is a second, non-generic implementor of
/// `AnyEnvelope`, proving the trait isn't tied to `Envelope<M>`.
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
