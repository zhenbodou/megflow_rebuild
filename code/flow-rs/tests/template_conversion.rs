use flow_rs::prelude::*;
#[derive(Clone)]
struct Raw(u32);
#[derive(Clone)]
struct Stored(u32);
#[add_cvt_func]
fn store(value: Raw) -> Stored {
    Stored(value.0 + 1)
}

#[inputs(inp: T0)]
#[outputs(out: T0)]
#[derive(Node, Actor, BuildFromPorts)]
struct Relay {}
#[methods]
impl Relay {
    async fn exec(&mut self) -> Result<()> {
        let message = self.inp.recv::<Stored>().await?;
        assert_eq!(message.info().partial_id, Some(9));
        self.out.as_ref().unwrap().send(message).await
    }
}
// 仅提供明确 Raw 约束，不从共享队列消费数据；exec 立即结束。
#[inputs(inp: Raw)]
#[derive(Node, Actor, BuildFromPorts)]
struct RawConstraint {}
#[methods]
impl RawConstraint {
    async fn exec(&mut self) -> Result<()> {
        self.input_closed = true;
        Ok(())
    }
}
#[inputs(inp: Stored)]
#[outputs(out: Stored)]
#[derive(Node, Actor, BuildFromPorts)]
struct Sink {}
#[methods]
impl Sink {
    async fn exec(&mut self) -> Result<()> {
        self.out.send(self.inp.recv().await?).await
    }
}
node_register!("ConvertedTemplateRelay", Relay);
node_register!("RawTypeConstraint", RawConstraint);
node_register!("StoredTypeSink", Sink);
#[tokio::test]
async fn inferred_template_receiver_converts_before_node_exec() {
    let text = r#"
main="g"
[[graphs]]
name="g"
nodes=[{name="r",ty="ConvertedTemplateRelay"},{name="raw",ty="RawTypeConstraint"},{name="sink",ty="StoredTypeSink"}]
inputs=[{name="in",cap=1,ports=["r:inp","raw:inp"]}]
outputs=[{name="out",cap=1,ports=["sink:out"]}]
connections=[{cap=1,ports=["r:out","sink:inp"]}]
"#;
    let mut graph = Builder::default().template(text).build().unwrap();
    let input = graph.input("in").unwrap();
    // Raw 是唯一候选：存在 Raw→Stored，不存在 Stored→Raw。
    assert_eq!(input.chan_tid(), MsgTypeId::of::<Raw>());
    let output = graph.take_output("out").unwrap();
    let task = graph.start();
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        input
            .send(Envelope::with_info(
                Raw(41),
                EnvelopeInfo {
                    partial_id: Some(9),
                    ..Default::default()
                },
            ))
            .await
            .unwrap();
        assert_eq!(output.recv::<Stored>().await.unwrap().unpack().0, 42);
        drop(input);
        graph.stop();
        task.await.unwrap().unwrap();
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn untyped_sender_with_type_uses_port_to_channel_direction() {
    let (mut sender, receiver) = flow_rs::channel::channel_with_type(1, MsgTypeId::of::<Stored>());
    sender.with_type(&MsgTypeId::of::<Raw>());
    sender.send(Envelope::new(Raw(6))).await.unwrap();
    assert_eq!(receiver.recv::<Stored>().await.unwrap().unpack().0, 7);
}
