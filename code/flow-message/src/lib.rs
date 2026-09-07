//! flow-message —— MegFlow 消息层（重写版）。
//!
//! 核心类型 `Envelope<M>`（消息信封）与类型擦除的 `AnyEnvelope` / `SealedEnvelope`。
//! flow-message —— message layer of MegFlow (rewrite): the `Envelope<M>` message
//! wrapper plus the type-erased `AnyEnvelope` / `SealedEnvelope`.

pub mod envelope;

pub use envelope::{str2addr, AnyEnvelope, DummyEnvelope, Envelope, EnvelopeInfo, SealedEnvelope};
