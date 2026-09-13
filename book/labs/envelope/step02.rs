#[derive(Default, Clone)]
pub struct EnvelopeInfo {
    pub partial_id: Option<u64>,
}

pub struct Envelope<M> {
    info: EnvelopeInfo,
    message: Option<M>,
}

impl<M> Envelope<M> {
    pub fn new(message: M) -> Self {
        Self {
            info: EnvelopeInfo::default(),
            message: Some(message),
        }
    }

    pub fn unpack(&mut self) -> M {
        self.message.take().expect("envelope has no message")
    }

    pub fn is_none(&self) -> bool {
        self.message.is_none()
    }
    pub fn info(&self) -> &EnvelopeInfo {
        &self.info
    }
    pub fn info_mut(&mut self) -> &mut EnvelopeInfo {
        &mut self.info
    }

    pub fn repack<T>(&self, message: T) -> Envelope<T> {
        Envelope {
            info: self.info.clone(),
            message: Some(message),
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
