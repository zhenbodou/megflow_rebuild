/*!
 * flow-message · envelope（第三步）—— 元信息 + 带类型的信封
 *
 * 这一步先把「带类型的信封」写完整：七项 `EnvelopeInfo` 元信息、
 * 载荷 `Option<M>` 的装取与换包（`repack`），以及按需的 `Clone`。
 * 类型擦除（`seal` / `SealedEnvelope` / `downcast`）留到第四步。
 */

use std::any::Any;
use std::sync::Arc;

/// 将端口字符串转为寻址 ID。与原版相同：先解析十进制 u64，否则使用 DefaultHasher。
/// 哈希算法不承诺跨 Rust 版本稳定，不应用作永久存储协议。
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

/// 随信封一起流动的七项公共元信息，与原版 Rust EnvelopeInfo 对齐。
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
