//! Ch1.3：消息信封与类型擦除，不包含后续算法消息模型。
pub mod envelope;

pub use envelope::{str2addr, AnyEnvelope, DummyEnvelope, Envelope, EnvelopeInfo, SealedEnvelope};
