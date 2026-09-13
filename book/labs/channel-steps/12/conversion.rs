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
