use crate::config::interlayer::MsgTypeId;
use flow_message::SealedEnvelope;
use std::{
    collections::HashMap,
    sync::{LazyLock, RwLock},
};

pub type CvtF = fn(SealedEnvelope) -> SealedEnvelope;
static TABLE: LazyLock<RwLock<HashMap<(MsgTypeId, MsgTypeId), CvtF>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

pub fn add_cvt_func_impl(from: MsgTypeId, to: MsgTypeId, function: CvtF) {
    TABLE.write().unwrap().insert((from, to), function);
}

pub(super) fn lookup(from: MsgTypeId, to: MsgTypeId) -> Option<CvtF> {
    TABLE.read().unwrap().get(&(from, to)).copied()
}

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
