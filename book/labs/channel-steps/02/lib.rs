#[cfg(test)]
mod tests {
    use flow_message::{Envelope, SealedEnvelope};
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn an_envelope_crosses_the_queue() {
        let (sender, mut receiver) = mpsc::channel::<SealedEnvelope>(1);
        let mut message = Envelope::new(7u32);
        message.info_mut().partial_id = Some(42);
        assert!(sender.send(message.seal()).await.is_ok());
        let mut sealed = receiver.recv().await.unwrap();
        let typed = sealed.downcast_mut::<Envelope<u32>>().unwrap();
        assert_eq!(typed.info().partial_id, Some(42));
        assert_eq!(typed.unpack(), 7);
        drop(sender);
        assert!(receiver.recv().await.is_none());
    }
}
