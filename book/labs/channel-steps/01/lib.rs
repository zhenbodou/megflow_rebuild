#[cfg(test)]
mod tests {
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn a_number_crosses_the_queue() {
        let (sender, mut receiver) = mpsc::channel::<u32>(1);
        sender.send(7).await.unwrap();
        assert_eq!(receiver.recv().await, Some(7));
        drop(sender);
        assert_eq!(receiver.recv().await, None);
    }
}
