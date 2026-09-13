pub struct Envelope<M> {
    msg: Option<M>,
}

impl<M> Envelope<M> {
    pub fn new(msg: M) -> Self {
        Self {
            msg: Some(msg),
        }
    }

    pub fn unpack(&mut self) -> M {
        self.msg.take().expect("envelope has no message")
    }

    pub fn is_none(&self) -> bool {
        self.msg.is_none()
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
