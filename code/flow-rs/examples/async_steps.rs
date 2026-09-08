//! 异步入门：每步只观察一个机制，先用 Tokio 原生队列再回到引擎封装。
use std::cell::Cell;
use tokio::sync::mpsc;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    // ANCHOR: lazy_future
    let calls = Cell::new(0);
    let work = async {
        calls.set(calls.get() + 1);
        7u32
    };
    assert_eq!(calls.get(), 0);
    let value = work.await;
    assert_eq!(value, 7);
    assert_eq!(calls.get(), 1);
    // ANCHOR_END: lazy_future
    println!("步骤 1：创建 Future 不执行块内业务，await 驱动后得到 7");

    // ANCHOR: backpressure
    let (sender, mut receiver) = mpsc::channel::<u32>(1);
    sender.send(10).await.unwrap();
    {
        let second = sender.send(20);
        tokio::pin!(second);
        assert!(futures_util::poll!(&mut second).is_pending());
        assert_eq!(receiver.recv().await, Some(10));
        second.await.unwrap();
    }
    assert_eq!(receiver.recv().await, Some(20));
    // ANCHOR_END: backpressure
    println!("步骤 2：容量满时发送 Pending，取走一条后发送继续");

    // ANCHOR: shutdown
    let producer_sender = sender.clone();
    let producer = tokio::spawn(async move {
        for value in [30, 40, 50] {
            producer_sender.send(value).await.unwrap();
        }
        // 任务结束释放 producer_sender。
    });
    drop(sender); // 必须释放主任务保留的这份，recv 才能最终返回 None。
    let mut collected = Vec::new();
    while let Some(value) = receiver.recv().await {
        collected.push(value);
    }
    producer.await.unwrap();
    assert_eq!(collected, [30, 40, 50]);
    // ANCHOR_END: shutdown
    println!("步骤 3：并发收发得到 [30, 40, 50]，全部发送端释放后正常退出");
}
