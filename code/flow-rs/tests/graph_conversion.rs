use flow_rs::prelude::*;
#[derive(Clone)]
struct Number(u32);
#[derive(Clone)]
struct Text(String);
#[add_cvt_func]
fn render(value: Number) -> Text {
    Text(value.0.to_string())
}

#[inputs(inp: Number)]
#[outputs(out: Number)]
#[derive(Node, Actor, BuildFromPorts)]
struct Source {}
#[methods]
impl Source {
    async fn exec(&mut self) -> Result<()> {
        self.out.send(self.inp.recv().await?).await
    }
}
#[inputs(inp: Text)]
#[outputs(out: Text)]
#[derive(Node, Actor, BuildFromPorts)]
struct Sink {}
#[methods]
impl Sink {
    async fn exec(&mut self) -> Result<()> {
        self.out.send(self.inp.recv().await?).await
    }
}
node_register!("ConversionSource", Source);
node_register!("ConversionSink", Sink);
const GRAPH: &str = r#"
main="g"
[[graphs]]
name="g"
nodes=[{name="source",ty="ConversionSource"},{name="sink",ty="ConversionSink"}]
inputs=[{name="in",cap=1,ports=["source:inp"]}]
outputs=[{name="out",cap=1,ports=["sink:out"]}]
connections=[{cap=1,ports=["source:out","sink:inp"]}]
"#;
#[tokio::test]
async fn builder_selects_type_and_converts_without_manual_channel_setup() {
    let mut graph = Builder::default().template(GRAPH).build().unwrap();
    let input = graph.input("in").unwrap();
    let output = graph.take_output("out").unwrap();
    let task = graph.start();
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        input
            .send(Envelope::with_info(
                Number(42),
                EnvelopeInfo {
                    partial_id: Some(7),
                    ..Default::default()
                },
            ))
            .await
            .unwrap();
        let mut result = output.recv::<Text>().await.unwrap();
        assert_eq!(result.info().partial_id, Some(7));
        assert_eq!(result.unpack().0, "42");
        drop(input);
        graph.stop();
        task.await.unwrap().unwrap();
    })
    .await
    .unwrap();
}

#[test]
fn incompatible_direction_fails_before_start() {
    let text = GRAPH
        .replace("ConversionSource", "TemporaryType")
        .replace("ConversionSink", "ConversionSource")
        .replace("TemporaryType", "ConversionSink");
    assert!(matches!(
        Builder::default().template(text).build(),
        Err(flow_rs::error::Error::ChannelTypeMismatch)
    ));
}

#[tokio::test]
async fn mixed_boundary_types_compete_without_losing_or_duplicating_messages() {
    use flow_rs::config::interlayer::MsgTypeId;
    // 同一图输入接 Number/Text 两种消费者，只存在 Number→Text 转换。
    // 输入必须选 Number，输出必须选 Text，两个选择都是唯一可行候选。
    let text = r#"
main="g"
[[graphs]]
name="g"
nodes=[{name="number",ty="ConversionSource"},{name="text",ty="ConversionSink"}]
inputs=[{name="in",cap=1,ports=["number:inp","text:inp"]}]
outputs=[{name="out",cap=1,ports=["number:out","text:out"]}]
"#;
    let mut graph = Builder::default().template(text).build().unwrap();
    let input = graph.input("in").unwrap();
    let output = graph.take_output("out").unwrap();
    assert_eq!(input.chan_tid(), MsgTypeId::of::<Number>());
    assert_eq!(output.chan_tid(), MsgTypeId::of::<Text>());
    let task = graph.start();
    graph.stop();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let producer = tokio::spawn(async move {
            for value in 0..100u32 {
                input
                    .send(Envelope::with_info(
                        Number(value),
                        EnvelopeInfo {
                            partial_id: Some(value as u64),
                            ..Default::default()
                        },
                    ))
                    .await
                    .unwrap();
            }
        });
        let mut values = Vec::new();
        loop {
            match output.recv::<Text>().await {
                Ok(mut envelope) => {
                    let id = envelope.info().partial_id.unwrap();
                    let value: u32 = envelope.unpack().0.parse().unwrap();
                    assert_eq!(id, value as u64);
                    values.push(value);
                }
                Err(Error::ChannelClosed) => break,
                Err(error) => panic!("unexpected receive error: {error}"),
            }
        }
        producer.await.unwrap();
        task.await.unwrap().unwrap();
        values.sort_unstable();
        assert_eq!(values, (0..100).collect::<Vec<_>>());
    })
    .await
    .unwrap();
}
