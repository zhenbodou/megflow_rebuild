pub struct Envelope<M> {
    message: Option<M>,
}

impl<M> Envelope<M> {
    pub fn new(message: M) -> Self {
        Self {
            message: Some(message),
        }
    }

    pub fn unpack(&mut self) -> M {
        self.message.take().expect("envelope has no message")
    }

    pub fn is_none(&self) -> bool {
        self.message.is_none()
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
}
