//! 手写节点终点：只依赖已学过的信封、Tokio 和标准库，不调用引擎节点宏。
use flow_message::{Envelope, EnvelopeInfo};
use std::sync::{Arc, Mutex};
use tokio::{sync::mpsc, task::JoinHandle};
type Result<T> = std::result::Result<T, &'static str>;
type Events = Arc<Mutex<Vec<&'static str>>>;

// ANCHOR: contracts
trait Node {
    fn close(&mut self);
    fn is_all_input_closed(&self) -> bool;
}
trait Actor: Node + Send + 'static {
    fn start(self: Box<Self>) -> JoinHandle<Result<()>>;
}
// ANCHOR_END: contracts

// ANCHOR: state
struct Doubler {
    inp: mpsc::Receiver<Envelope<i32>>,
    out: Option<mpsc::Sender<Envelope<i32>>>,
    input_closed: bool,
    events: Events,
}
// ANCHOR_END: state

// ANCHOR: business
impl Doubler {
    async fn initialize(&mut self) {
        self.events.lock().unwrap().push("initialize");
    }
    async fn exec(&mut self) -> Result<()> {
        match self.inp.recv().await {
            Some(mut envelope) => {
                self.events.lock().unwrap().push("exec");
                let value = envelope.unpack();
                if value < 0 {
                    return Err("negative input");
                }
                if let Some(out) = &self.out {
                    out.send(envelope.repack(value * 2))
                        .await
                        .map_err(|_| "output closed")?;
                }
            }
            None => self.input_closed = true,
        }
        Ok(())
    }
    async fn finalize(&mut self) {
        assert!(self.out.is_none(), "先关闭输出，再执行 finalize");
        self.events.lock().unwrap().push("finalize");
    }
}
// ANCHOR_END: business

// ANCHOR: lifecycle
impl Node for Doubler {
    fn close(&mut self) {
        self.out = None;
        self.events.lock().unwrap().push("close");
    }
    fn is_all_input_closed(&self) -> bool {
        self.input_closed
    }
}
impl Actor for Doubler {
    fn start(mut self: Box<Self>) -> JoinHandle<Result<()>> {
        tokio::spawn(async move {
            self.initialize().await;
            let result = async {
                while !self.is_all_input_closed() {
                    self.exec().await?;
                }
                Ok(())
            }
            .await;
            self.close();
            self.finalize().await;
            result
        })
    }
}
// ANCHOR_END: lifecycle

// ANCHOR: assemble
async fn run(values: Vec<i32>) -> (Vec<(i32, Option<u64>)>, Vec<&'static str>, Result<()>) {
    let (source, inp) = mpsc::channel(1);
    let (out, mut sink) = mpsc::channel(1);
    let events: Events = Default::default();
    let actor: Box<dyn Actor> = Box::new(Doubler {
        inp,
        out: Some(out),
        input_closed: false,
        events: events.clone(),
    });
    let handle = actor.start();
    let producer = tokio::spawn(async move {
        for (id, value) in values.into_iter().enumerate() {
            let envelope = Envelope::with_info(
                value,
                EnvelopeInfo {
                    partial_id: Some(id as u64),
                    ..Default::default()
                },
            );
            if source.send(envelope).await.is_err() {
                break;
            }
        }
    });
    let mut outputs = Vec::new();
    while let Some(mut envelope) = sink.recv().await {
        outputs.push((envelope.unpack(), envelope.info().partial_id));
    }
    producer.await.expect("生产任务不应 panic");
    let result = handle.await.expect("节点任务不应 panic");
    let events = events.lock().unwrap().clone();
    (outputs, events, result)
}
// ANCHOR_END: assemble

#[tokio::main(flavor = "current_thread")]
async fn main() {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        let (outputs, events, result) = run(vec![1, 2, 3]).await;
        assert_eq!(outputs, [(2, Some(0)), (4, Some(1)), (6, Some(2))]);
        assert_eq!(
            events,
            ["initialize", "exec", "exec", "exec", "close", "finalize"]
        );
        assert_eq!(result, Ok(()));
        let (outputs, events, result) = run(vec![1, -1, 3]).await;
        assert_eq!(outputs, [(2, Some(0))]);
        assert_eq!(events, ["initialize", "exec", "exec", "close", "finalize"]);
        assert_eq!(result, Err("negative input"));
        println!("手写节点通过：正常处理、元信息、错误传播、关闭与 finalize");
    })
    .await
    .expect("节点应正常收尾，不能挂起");
}
