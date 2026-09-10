//! 转换注册表：只查询已注册的直接边，不推导转换链。
use crate::{config::interlayer::MsgTypeId, envelope::SealedEnvelope};
use std::{
    collections::HashMap,
    sync::{LazyLock, RwLock},
};

// ANCHOR: cvt_f_type
pub type CvtF = fn(SealedEnvelope) -> SealedEnvelope;
// ANCHOR_END: cvt_f_type
/// 属性宏提交的静态登记。用函数取得 TypeId，初始化时才调用。
pub struct ConversionRegistration {
    pub from: fn() -> MsgTypeId,
    pub to: fn() -> MsgTypeId,
    pub function: CvtF,
}
inventory::collect!(ConversionRegistration);

static TABLE: LazyLock<RwLock<HashMap<(MsgTypeId, MsgTypeId), CvtF>>> = LazyLock::new(|| {
    let mut table = HashMap::new();
    for registration in inventory::iter::<ConversionRegistration> {
        table.insert(
            ((registration.from)(), (registration.to)()),
            registration.function,
        );
    }
    RwLock::new(table)
});

/// 与原版注册规则一致：同一有向类型对后注册覆盖先注册。
pub fn add_cvt_func_impl(from: MsgTypeId, to: MsgTypeId, func: CvtF) {
    TABLE.write().unwrap().insert((from, to), func);
}

pub(super) fn lookup(from: MsgTypeId, to: MsgTypeId) -> Option<CvtF> {
    TABLE.read().unwrap().get(&(from, to)).copied()
}

/// 原版 ChannelStorage::guess 的类型选择规则；完整存储/建图接入另行迁移。
pub fn guess_channel_type(
    tx: &std::collections::HashSet<MsgTypeId>,
    rx: &std::collections::HashSet<MsgTypeId>,
) -> crate::error::Result<MsgTypeId> {
    use crate::error::Error;
    let abstract_type = |t: &MsgTypeId| matches!(t, MsgTypeId::Any | MsgTypeId::Template(_));
    if tx
        .iter()
        .chain(rx)
        .all(|t| matches!(t, MsgTypeId::Template(_)))
    {
        return Err(Error::TemplateInferFault);
    }
    if tx.iter().chain(rx).all(abstract_type) {
        return Ok(MsgTypeId::Any);
    }
    let table = TABLE.read().unwrap();
    let mut python_candidate = None;
    for candidate in tx.iter().chain(rx).filter(|t| !abstract_type(t)) {
        let accepts_all = tx
            .iter()
            .filter(|t| !abstract_type(t))
            .all(|source| source == candidate || table.contains_key(&(*source, *candidate)));
        let reaches_all = rx
            .iter()
            .filter(|t| !abstract_type(t))
            .all(|target| target == candidate || table.contains_key(&(*candidate, *target)));
        if accepts_all && reaches_all {
            if !matches!(candidate, MsgTypeId::Python(_)) {
                return Ok(*candidate);
            }
            python_candidate.get_or_insert(*candidate);
        }
    }
    python_candidate.ok_or(Error::ChannelTypeMismatch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::Envelope;
    fn first(_: SealedEnvelope) -> SealedEnvelope {
        Envelope::new(1u32).seal()
    }
    fn second(_: SealedEnvelope) -> SealedEnvelope {
        Envelope::new(2u32).seal()
    }

    #[test]
    fn directed_lookup_overwrite_and_snapshot() {
        struct Source;
        struct Target;
        struct Third;
        let (a, b, c) = (
            MsgTypeId::of::<Source>(),
            MsgTypeId::of::<Target>(),
            MsgTypeId::of::<Third>(),
        );
        add_cvt_func_impl(a, b, first);
        let cached = lookup(a, b).unwrap();
        assert!(lookup(b, a).is_none());
        add_cvt_func_impl(b, c, first);
        assert!(lookup(a, c).is_none());
        add_cvt_func_impl(a, b, second);
        let mut old = cached(Envelope::new(()).seal());
        let mut new = lookup(a, b).unwrap()(Envelope::new(()).seal());
        assert_eq!(old.downcast_mut::<Envelope<u32>>().unwrap().unpack(), 1);
        assert_eq!(new.downcast_mut::<Envelope<u32>>().unwrap().unpack(), 2);
    }
}
