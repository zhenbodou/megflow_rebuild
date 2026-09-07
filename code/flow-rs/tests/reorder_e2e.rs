//! 使用原版 exec 方法原文和测试 I/O 适配器，逐项比较重写节点输出与失败类别。
use flow_rs::envelope::{Envelope, EnvelopeInfo, SealedEnvelope};
use flow_rs::{error::Error, graph::Builder, sandbox::Sandbox};
use futures_util::FutureExt;
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

mod reference {
    use super::*;
    use std::cell::RefCell;
    type Result<T> = std::result::Result<T, ()>;
    type Context = ();
    #[derive(Default)]
    struct Input(VecDeque<SealedEnvelope>);
    impl Input {
        async fn recv_any(&mut self) -> Result<SealedEnvelope> {
            self.0.pop_front().ok_or(())
        }
    }
    #[derive(Default)]
    struct Output(RefCell<Vec<SealedEnvelope>>);
    impl Output {
        async fn send_any(&self, message: SealedEnvelope) -> Result<()> {
            self.0.borrow_mut().push(message);
            Ok(())
        }
    }
    #[derive(Default)]
    struct ReferenceReorder {
        inp: Input,
        out: Output,
        cache: BTreeMap<u64, SealedEnvelope>,
        seq_id: u64,
    }
    include!("reference/reorder_exec.rs");

    pub async fn run(messages: Vec<Envelope<String>>) -> (Vec<(u64, Option<String>)>, bool) {
        let count = messages.len();
        let mut node = ReferenceReorder::default();
        node.inp.0.extend(messages.into_iter().map(Envelope::seal));
        // 最后额外一次 exec 代表输入关闭，以检验缓存缺口。
        let result = std::panic::AssertUnwindSafe(async {
            for _ in 0..=count {
                node.exec(&()).await.unwrap();
            }
        })
        .catch_unwind()
        .await;
        let outputs = node
            .out
            .0
            .into_inner()
            .into_iter()
            .map(|mut message| {
                let envelope = message.downcast_mut::<Envelope<String>>().unwrap();
                observe(envelope)
            })
            .collect();
        (outputs, result.is_ok())
    }
}

fn observe(message: &mut Envelope<String>) -> (u64, Option<String>) {
    (
        message.info().partial_id.unwrap(),
        message.is_some().then(|| message.unpack()),
    )
}

fn message(id: u64, value: &str) -> Envelope<String> {
    Envelope::with_info(
        value.to_owned(),
        EnvelopeInfo {
            partial_id: Some(id),
            ..Default::default()
        },
    )
}

async fn run_rewrite(messages: Vec<Envelope<String>>) -> (Vec<(u64, Option<String>)>, bool) {
    let collected = Arc::new(Mutex::new(Vec::new()));
    let sink = collected.clone();
    let mut source = messages.into_iter();
    let mut sandbox = Sandbox::pure("Reorder").unwrap();
    sandbox.add_envelope("inp", move |_| source.next());
    sandbox.add_envelope_check("out", move |mut envelope: Envelope<String>| {
        sink.lock().unwrap().push(observe(&mut envelope));
    });
    let result = sandbox.start().await;
    if let Err(error) = &result {
        assert!(
            matches!(error, Error::TaskJoin(_)),
            "协议断言应导致节点 panic，而非其他错误：{error}"
        );
    }
    let outputs = collected.lock().unwrap().clone();
    (outputs, result.is_ok())
}

fn permutations(items: &mut [u64], offset: usize, result: &mut Vec<Vec<u64>>) {
    if offset == items.len() {
        result.push(items.to_vec());
        return;
    }
    for index in offset..items.len() {
        items.swap(offset, index);
        permutations(items, offset + 1, result);
        items.swap(offset, index);
    }
}

#[tokio::test]
async fn all_720_permutations_match_original_method() {
    tokio::time::timeout(std::time::Duration::from_secs(30), async {
        let mut cases = Vec::new();
        permutations(&mut [0, 1, 2, 3, 4, 5], 0, &mut cases);
        assert_eq!(cases.len(), 720);
        for ids in cases {
            let messages: Vec<_> = ids
                .iter()
                .map(|id| message(*id, &format!("frame-{id}")))
                .collect();
            let expected = reference::run(messages.clone()).await;
            assert!(expected.1);
            assert_eq!(
                expected.0.iter().map(|item| item.0).collect::<Vec<_>>(),
                [0, 1, 2, 3, 4, 5]
            );
            assert_eq!(run_rewrite(messages).await, expected, "输入排列 {ids:?}");
        }
    })
    .await
    .expect("有限的排列集应全部收尾");
}

#[tokio::test]
async fn duplicates_gaps_missing_ids_and_empty_payloads_match_original() {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let mut empty = Envelope::<String>::empty();
        empty.info_mut().partial_id = Some(0);
        let cases = vec![
            vec![],
            vec![message(1, "old"), message(1, "new"), message(0, "zero")],
            vec![message(0, "first"), message(0, "late")],
            vec![message(0, "zero"), message(2, "gap")],
            vec![Envelope::new(String::from("missing-id"))],
            vec![message(1, "one"), empty],
        ];
        for messages in cases {
            let expected = reference::run(messages.clone()).await;
            assert_eq!(run_rewrite(messages).await, expected);
        }
        let overwritten = run_rewrite(vec![
            message(1, "old"),
            message(1, "new"),
            message(0, "zero"),
        ])
        .await;
        assert_eq!(
            overwritten,
            (
                vec![(0, Some("zero".into())), (1, Some("new".into()))],
                true
            )
        );
    })
    .await
    .unwrap();
}

// ANCHOR: graph_config
const REORDER_GRAPH: &str = r#"
main = "ordered"
[[graphs]]
name = "ordered"
nodes = [{name="reorder", ty="Reorder"}]
inputs = [{name="inp", cap=1, ports=["reorder:inp"]}]
outputs = [{name="out", cap=1, ports=["reorder:out"]}]
"#;
// ANCHOR_END: graph_config

#[tokio::test]
async fn graph_preserves_full_metadata_and_shares_extra_data() {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let mut graph = Builder::default().template(REORDER_GRAPH).build().unwrap();
        let input = graph.input("inp").unwrap();
        let output = graph.take_output("out").unwrap();
        let handle = graph.start();
        let extra = Arc::new(String::from("shared-frame-context"));
        let source_extra = extra.clone();
        let feeder = tokio::spawn(async move {
            for id in [2, 0, 1] {
                input
                    .send(Envelope::with_info(
                        id.to_string(),
                        EnvelopeInfo {
                            skipped: true,
                            weight: Some(id as usize),
                            partial_id: Some(id),
                            from_addr: Some(10),
                            to_addr: Some(20),
                            transfer_addr: Some(30),
                            extra_data: Some(source_extra.clone()),
                        },
                    ))
                    .await
                    .unwrap();
            }
        });
        // 释放图持有的输入发送端；feeder 的克隆仍可把剩余消息发完。
        graph.stop();
        for id in 0..3 {
            let mut envelope = output.recv::<String>().await.unwrap();
            assert_eq!(envelope.info().partial_id, Some(id));
            assert!(envelope.info().skipped);
            assert_eq!(envelope.info().weight, Some(id as usize));
            assert_eq!(envelope.info().from_addr, Some(10));
            assert_eq!(envelope.info().to_addr, Some(20));
            assert_eq!(envelope.info().transfer_addr, Some(30));
            let carried = envelope
                .info()
                .extra_data
                .clone()
                .unwrap()
                .downcast::<String>()
                .unwrap();
            assert!(Arc::ptr_eq(&extra, &carried));
            assert_eq!(envelope.unpack(), id.to_string());
        }
        assert!(matches!(
            output.recv::<String>().await,
            Err(Error::ChannelClosed)
        ));
        feeder.await.unwrap();
        handle.await.unwrap().unwrap();
    })
    .await
    .expect("容量 1 的图也应在并发收发下退出");
}

#[tokio::test]
async fn closed_downstream_does_not_prevent_draining_input() {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let mut graph = Builder::default().template(REORDER_GRAPH).build().unwrap();
        let input = graph.input("inp").unwrap();
        drop(graph.take_output("out").unwrap());
        let handle = graph.start();
        graph.stop();
        for id in [2, 1, 0] {
            input.send(message(id, "data")).await.unwrap();
        }
        drop(input);
        handle.await.unwrap().unwrap();
    })
    .await
    .unwrap();
}

// ANCHOR: streaming_prefix
#[tokio::test]
async fn emits_contiguous_prefix_before_input_closes() {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let mut graph = Builder::default().template(REORDER_GRAPH).build().unwrap();
        let input = graph.input("inp").unwrap();
        let output = graph.take_output("out").unwrap();
        let handle = graph.start();
        input.send(message(0, "zero")).await.unwrap();
        assert_eq!(output.recv::<String>().await.unwrap().unpack(), "zero");
        // 输入仍然打开。未来序号先缓存，直到缺失的 1 到达。
        for id in [3, 2, 1] {
            input.send(message(id, &id.to_string())).await.unwrap();
        }
        for id in 1..=3 {
            let mut received = output.recv::<String>().await.unwrap();
            assert_eq!(received.info().partial_id, Some(id));
            assert_eq!(received.unpack(), id.to_string());
        }
        drop(input);
        graph.stop();
        assert!(matches!(
            output.recv::<String>().await,
            Err(Error::ChannelClosed)
        ));
        handle.await.unwrap().unwrap();
    })
    .await
    .expect("连续前缀应在输入关闭之前输出，不能等全量收齐才排序");
}
// ANCHOR_END: streaming_prefix
