pub mod error;

#[cfg(test)]
mod tests {
    use crate::error::{Error, Result};
    use flow_message::{Envelope, SealedEnvelope};
    use tokio::sync::mpsc;

    async fn receive(receiver: &mut mpsc::Receiver<SealedEnvelope>) -> Result<SealedEnvelope> {
        match receiver.recv().await {
            Some(message) => Ok(message),
            None => Err(Error::ChannelClosed),
        }
    }

    #[tokio::test]
    async fn distinguish_message_from_closed_queue() {
        let (sender, mut receiver) = mpsc::channel(1);
        assert!(sender.send(Envelope::new(7u32).seal()).await.is_ok());
        assert!(receive(&mut receiver).await.is_ok());
        drop(sender);
        assert!(matches!(
            receive(&mut receiver).await,
            Err(Error::ChannelClosed)
        ));
        assert_eq!(Error::ChannelClosed.to_string(), "channel closed");
        assert_eq!(
            Error::TypeMismatch.to_string(),
            "message type mismatch on recv"
        );
    }
}
