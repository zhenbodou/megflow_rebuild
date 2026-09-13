#[derive(Default, Clone)]
pub struct EnvelopeInfo {
    pub partial_id: Option<u64>,
}

pub struct Envelope<M> {
    info: EnvelopeInfo,
    msg: Option<M>,
}

impl<M> Envelope<M> {
    pub fn new(msg: M) -> Self {
        Self {
            info: EnvelopeInfo::default(),
            msg: Some(msg),
        }
    }

    pub fn unpack(&mut self) -> M {
        self.msg.take().expect("envelope has no message")
    }

    pub fn is_none(&self) -> bool {
        self.msg.is_none()
    }
    pub fn info(&self) -> &EnvelopeInfo {
        &self.info
    }
    pub fn info_mut(&mut self) -> &mut EnvelopeInfo {
        &mut self.info
    }

    pub fn repack<T>(&self, msg: T) -> Envelope<T> {
        Envelope {
            info: self.info.clone(),
            msg: Some(msg),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Envelope;

    #[test]
    fn move_payload_out() {
        let mut envelope = Envelope::new(String::from("frame"));
        assert_eq!(envelope.unpack(), "frame");
        assert!(envelope.is_none());
    }

    #[test]
    fn conversion_keeps_sequence_and_original_payload() {
        let mut original = Envelope::new(7u32);
        original.info_mut().partial_id = Some(42);
        let mut converted = original.repack(String::from("seven"));
        assert_eq!(converted.info().partial_id, Some(42));
        assert_eq!(converted.unpack(), "seven");
        assert_eq!(original.unpack(), 7);
    }
}
