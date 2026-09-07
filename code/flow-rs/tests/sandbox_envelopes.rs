use flow_rs::prelude::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn envelope_source_is_lazy_and_stops_on_none() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let source_calls = calls.clone();
    let received = Arc::new(Mutex::new(Vec::new()));
    let sink = received.clone();
    let mut sandbox = Sandbox::pure("Transform").unwrap();
    sandbox.add_envelope("inp", move |index| {
        source_calls.lock().unwrap().push(index);
        match index {
            0 => Some(Envelope::with_info(
                7u32,
                EnvelopeInfo {
                    to_addr: Some(42),
                    ..Default::default()
                },
            )),
            1 => Some(Envelope::<u32>::empty()),
            _ => None,
        }
    });
    sandbox.add_envelope_check("out", move |mut envelope: Envelope<u32>| {
        sink.lock().unwrap().push((
            envelope.info().to_addr,
            envelope.is_some().then(|| envelope.unpack()),
        ));
    });
    assert!(
        calls.lock().unwrap().is_empty(),
        "数据源不能在 start 之前执行"
    );
    tokio::time::timeout(std::time::Duration::from_secs(2), sandbox.start())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(*calls.lock().unwrap(), [0, 1, 2]);
    assert_eq!(
        *received.lock().unwrap(),
        [(Some(42), Some(7)), (None, None)]
    );
}

#[tokio::test]
async fn wrong_checker_type_must_fail_instead_of_passing_with_zero_callbacks() {
    let mut sandbox = Sandbox::pure("Transform").unwrap();
    sandbox.add_items("inp", vec![String::from("not-an-integer")]);
    sandbox.add_check("out", |_: u32| panic!("错误类型不应进入回调"));
    let result = tokio::time::timeout(std::time::Duration::from_secs(2), sandbox.start())
        .await
        .unwrap();
    assert!(matches!(result, Err(Error::TypeMismatch)));
}

static FINALIZED: AtomicBool = AtomicBool::new(false);

#[inputs(inp)]
#[outputs(out)]
#[derive(Node, Actor, BuildFromPorts)]
struct FinalizeProbe {}

#[methods]
impl FinalizeProbe {
    async fn exec(&mut self) -> Result<()> {
        let message = self.inp.recv_any().await?;
        if let Some(out) = &self.out {
            out.send_any(message).await?;
        }
        Ok(())
    }
    async fn finalize(&mut self) {
        // 让 checker 先失败；start 必须等待这里完成才可返回错误。
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        FINALIZED.store(true, Ordering::SeqCst);
    }
}
node_register!("SandboxFinalizeProbe", FinalizeProbe);

#[tokio::test]
async fn checker_panic_is_reported_after_node_has_finished() {
    FINALIZED.store(false, Ordering::SeqCst);
    let mut sandbox = Sandbox::pure("SandboxFinalizeProbe").unwrap();
    sandbox.add_items("inp", vec![1u32]);
    sandbox.add_check("out", |_: u32| panic!("intentional checker failure"));
    let result = tokio::time::timeout(std::time::Duration::from_secs(2), sandbox.start())
        .await
        .unwrap();
    assert!(matches!(result, Err(Error::TaskJoin(_))));
    assert!(
        FINALIZED.load(Ordering::SeqCst),
        "不能丢下尚未收尾的节点任务"
    );
}

#[tokio::test]
async fn original_data_callback_is_lazy_indexed_and_preserves_count() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let log = calls.clone();
    let values = Arc::new(Mutex::new(Vec::new()));
    let sink = values.clone();
    let mut sandbox = Sandbox::pure("Transform").unwrap();
    sandbox.add_data("inp", move |i| {
        log.lock().unwrap().push(i);
        (i < 3).then_some(i as i32 * 2)
    });
    sandbox.add_check("out", move |v: i32| sink.lock().unwrap().push(v));
    assert!(calls.lock().unwrap().is_empty());
    tokio::time::timeout(std::time::Duration::from_secs(2), sandbox.start())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(*calls.lock().unwrap(), [0, 1, 2, 3]);
    assert_eq!(*values.lock().unwrap(), [0, 2, 4]);
}

#[inputs(inp)]
#[derive(Node, Actor, BuildFromPorts)]
struct RejectSource {}
#[methods]
impl RejectSource {
    async fn exec(&mut self) -> Result<()> {
        // 接收一条后主动失败，剩余源数据仍须按原版遍历到 None。
        self.inp.recv_any().await?;
        Err(Error::ChannelClosed)
    }
}
node_register!("SandboxRejectSource", RejectSource);

#[tokio::test]
async fn closed_node_does_not_truncate_source_callback_sequence() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let log = calls.clone();
    let mut sandbox = Sandbox::pure("SandboxRejectSource").unwrap();
    sandbox.add_data("inp", move |i| {
        log.lock().unwrap().push(i);
        (i < 100).then_some(i)
    });
    tokio::time::timeout(std::time::Duration::from_secs(2), sandbox.start())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(*calls.lock().unwrap(), (0..=100).collect::<Vec<_>>());
}

#[tokio::test]
async fn last_registration_replaces_source_and_checker_before_start() {
    let received = Arc::new(Mutex::new(Vec::new()));
    let sink = received.clone();
    let mut sandbox = Sandbox::pure("Transform").unwrap();
    sandbox.add_data("inp", |_| -> Option<u32> { panic!("旧源不应执行") });
    sandbox.add_check("out", |_: u32| panic!("旧检查器不应执行"));
    sandbox.add_envelope("inp", |i| (i == 0).then(|| Envelope::new(42u32)));
    sandbox.add_envelope_check("out", move |mut e: Envelope<u32>| {
        sink.lock().unwrap().push(e.unpack())
    });
    tokio::time::timeout(std::time::Duration::from_secs(2), sandbox.start())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(*received.lock().unwrap(), [42]);
}
