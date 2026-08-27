//! 集成测试：Ch4.1 类型无关直通节点——`Transform`（1 入 1 出转发）与 `NoopConsumer`
//! （只吸收、不产出的汇）。
//!
//! 这两个节点都走**未类型化**通道 API（`recv_any`/`send_any`）：它们搬运的是**已封箱**
//! 的 `SealedEnvelope`，全程不拆封、不关心里面装的是 `i32` 还是 `String`。于是同一个
//! `Transform` 既能转发整数流、也能转发字符串流——「类型无关」正是这样被测出来的。这与
//! `BinaryOp` 形成对照：后者 `recv::<i32>()` 把类型钉死在节点里，前者把类型留给上下游。
//!
//! Type-agnostic passthrough/sink nodes, exercised via the `Sandbox` harness.

use flow_rs::sandbox::Sandbox;
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn transform_passes_through_i32_stream() {
    // 喂一串 i32，Transform 原样转发到 out——顺序、内容都不变。
    let got = Arc::new(Mutex::new(Vec::new()));
    let sink = got.clone();
    let mut sb = Sandbox::pure("Transform").unwrap();
    sb.add_data("inp", vec![1i32, 2, 3])
        .add_check("out", move |v: i32| sink.lock().unwrap().push(v));
    sb.start().await.unwrap();
    assert_eq!(*got.lock().unwrap(), vec![1, 2, 3]);
}

#[tokio::test]
async fn transform_is_type_agnostic_over_strings() {
    // 同一个 Transform，这次搬运 String——它不绑定具体消息类型（recv_any/send_any），
    // 换成任何 `Send + 'static` 的载荷都照转不误。
    let got = Arc::new(Mutex::new(Vec::new()));
    let sink = got.clone();
    let mut sb = Sandbox::pure("Transform").unwrap();
    sb.add_data("inp", vec!["a".to_string(), "bc".to_string()])
        .add_check("out", move |v: String| sink.lock().unwrap().push(v));
    sb.start().await.unwrap();
    assert_eq!(
        *got.lock().unwrap(),
        vec!["a".to_string(), "bc".to_string()]
    );
}

#[tokio::test]
async fn noop_consumer_drains_and_finishes() {
    // NoopConsumer 只有输入、没有输出：把所有消息吸收丢弃，输入耗尽后干净收工。
    // 断言点是「start() 返回 Ok 且不挂起」——一个终止数据流分支的汇（sink）。
    let mut sb = Sandbox::pure("NoopConsumer").unwrap();
    sb.add_data("inp", vec![1i32, 2, 3, 4]);
    sb.start().await.unwrap();
}
