use flow_rs::broker::Broker;
use std::sync::Arc;

#[derive(Clone)]
struct Created {
    id: u64,
    resource: Arc<String>,
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut broker = Broker::new();
    let first = broker.subscribe("camera".to_owned());
    let second = broker.subscribe("camera".to_owned());
    let resource = Arc::new("model".to_owned());
    first
        .publish(Created {
            id: 7,
            resource: resource.clone(),
        })
        .await;
    let task = broker.run();
    let a = first.fetch::<Created>().await.unwrap();
    let b = second.fetch::<Created>().await.unwrap();
    assert_eq!((a.id, b.id), (7, 7));
    assert!(Arc::ptr_eq(&a.resource, &b.resource));
    first.close();
    assert!(second.is_closed());
    task.await.unwrap().unwrap();
    println!("两个订阅者收到实例 7；资源共享；主题已关闭。");
}
