use flow_rs::prelude::*;
#[inputs(inp)]
#[outputs(out: {})]
#[derive(Node, Actor, BuildFromPorts)]
struct Router {}
#[methods]
impl Router {
    async fn exec(&mut self) -> Result<()> {
        let message = self.inp.recv_any().await?;
        let address = message.info().to_addr.expect("address required");
        if let Some(output) = self.out.get(&address) {
            output.send_any(message).await.ok();
        }
        Ok(())
    }
}
node_register!("DictionaryRouterTest", Router);
const GRAPH: &str = r#"
main="g"
[[graphs]]
name="g"
nodes=[{name="r",ty="DictionaryRouterTest"}]
inputs=[{name="in",cap=1,ports=["r:inp"]}]
outputs=[{name="seven",cap=1,ports=["r:out:7"]},{name="answer",cap=1,ports=["r:out:42"]}]
"#;
#[tokio::test]
async fn dictionary_routes_and_closes_all_outputs() {
    let mut graph = Builder::default().template(GRAPH).build().unwrap();
    let input = graph.input("in").unwrap();
    let seven = graph.take_output("seven").unwrap();
    let answer = graph.take_output("answer").unwrap();
    let task = graph.start();
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        for address in [99, 42, 7] {
            input
                .send(Envelope::with_info(
                    address as u32,
                    EnvelopeInfo {
                        to_addr: Some(address),
                        partial_id: Some(8),
                        ..Default::default()
                    },
                ))
                .await
                .unwrap();
        }
        let mut message = answer.recv::<u32>().await.unwrap();
        assert_eq!(message.unpack(), 42);
        assert_eq!(message.info().partial_id, Some(8));
        assert_eq!(seven.recv::<u32>().await.unwrap().unpack(), 7);
        drop(input);
        graph.stop();
        task.await.unwrap().unwrap();
        assert!(matches!(seven.recv_any().await, Err(Error::ChannelClosed)));
        assert!(matches!(answer.recv_any().await, Err(Error::ChannelClosed)));
    })
    .await
    .unwrap();
}

#[inputs(inp: {u32})]
#[outputs(out: {String})]
#[derive(Node, Actor, BuildFromPorts)]
struct TypedDictionary {}
#[methods]
impl TypedDictionary {
    async fn exec(&mut self) -> Result<()> {
        let mut message = self.inp.get(&7).unwrap().recv().await?;
        let value = message.unpack().to_string();
        self.out.get(&42).unwrap().send(message.repack(value)).await
    }
}
node_register!("TypedDictionaryTest", TypedDictionary);
#[tokio::test]
async fn typed_dictionary_receives_and_sends_through_builder() {
    let text = r#"
main="g"
[[graphs]]
name="g"
nodes=[{name="r",ty="TypedDictionaryTest"}]
inputs=[{name="in",cap=1,ports=["r:inp:7"]}]
outputs=[{name="out",cap=1,ports=["r:out:42"]}]
"#;
    let mut graph = Builder::default().template(text).build().unwrap();
    let input = graph.input("in").unwrap();
    let output = graph.take_output("out").unwrap();
    let task = graph.start();
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        input.send(Envelope::new(23u32)).await.unwrap();
        assert_eq!(output.recv::<String>().await.unwrap().unpack(), "23");
        drop(input);
        graph.stop();
        task.await.unwrap().unwrap();
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn typed_dictionary_metadata_and_duplicate_key_replacement() {
    use flow_rs::registry::TaggedEndpoint;
    assert_eq!(TypedDictionary::input_types(), vec![MsgTypeId::of::<u32>()]);
    assert_eq!(
        TypedDictionary::output_types(),
        vec![MsgTypeId::of::<String>()]
    );
    let (first, first_rx) = flow_rs::channel::channel(1);
    let (second, second_rx) = flow_rs::channel::channel(1);
    let actor = TypedDictionary::build_tagged(
        &Default::default(),
        vec![vec![]],
        vec![vec![
            TaggedEndpoint::new(first, Some(42)),
            TaggedEndpoint::new(second, Some(42)),
        ]],
    )
    .unwrap();
    // 同键后写覆盖：旧端点立即释放，新端点仍由 actor 持有。
    let duration = std::time::Duration::from_millis(30);
    assert!(matches!(
        first_rx.try_recv_any(duration).await,
        Err(Error::ChannelClosed)
    ));
    assert!(second_rx.try_recv_any(duration).await.unwrap().is_none());
    drop(actor);
    assert!(matches!(
        second_rx.try_recv_any(duration).await,
        Err(Error::ChannelClosed)
    ));
}
#[test]
#[should_panic(expected = "dict port need a tag")]
fn connected_dictionary_requires_an_explicit_tag() {
    let _ = Builder::default()
        .template(GRAPH.replace("r:out:7", "r:out"))
        .build();
}
