#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_then_unpack() {
        let mut envelope = Envelope::new(1i32);
        assert!(envelope.is_some());
        assert_eq!(envelope.unpack(), 1);
        assert!(envelope.is_none());
    }
}
