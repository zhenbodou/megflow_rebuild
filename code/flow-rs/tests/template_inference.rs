use flow_rs::prelude::*;
#[inputs(inp: T0)]
#[outputs(out: T0)]
#[derive(Node, Actor, BuildFromPorts)]
struct Relay {}
#[methods]
impl Relay {
    async fn exec(&mut self) -> Result<()> {
        self.out
            .as_ref()
            .unwrap()
            .send_any(self.inp.recv_any().await?)
            .await
    }
}
node_register!("TemplateRelayTest", Relay);
macro_rules! typed_relay {
    ($node:ident, $ty:ty, $name:literal) => {
        #[inputs(inp: $ty)]
        #[outputs(out: $ty)]
        #[derive(Node, Actor, BuildFromPorts)]
        struct $node {}
        #[methods]
        impl $node {
            async fn exec(&mut self) -> Result<()> {
                self.out.send(self.inp.recv().await?).await
            }
        }
        node_register!($name, $node);
    };
}
typed_relay!(Numbers, u32, "TemplateNumbersTest");
typed_relay!(Strings, String, "TemplateStringsTest");
const GRAPH: &str = r#"
main="g"
[[graphs]]
name="g"
nodes=[{name="a",ty="TemplateRelayTest"},{name="b",ty="TemplateRelayTest"},{name="number",ty="TemplateNumbersTest"},{name="string",ty="TemplateStringsTest"}]
inputs=[{name="number",cap=1,ports=["a:inp"]},{name="string",cap=1,ports=["b:inp"]}]
outputs=[{name="number",cap=1,ports=["number:out"]},{name="string",cap=1,ports=["string:out"]}]
connections=[{cap=1,ports=["a:out","number:inp"]},{cap=1,ports=["b:out","string:inp"]}]
"#;
#[tokio::test]
async fn concrete_types_propagate_to_boundaries_without_crossing_node_instances() {
    let mut graph = Builder::default().template(GRAPH).build().unwrap();
    let numbers = graph.input("number").unwrap();
    let strings = graph.input("string").unwrap();
    assert_eq!(numbers.chan_tid(), MsgTypeId::of::<u32>());
    assert_eq!(strings.chan_tid(), MsgTypeId::of::<String>());
    let nr = graph.take_output("number").unwrap();
    let sr = graph.take_output("string").unwrap();
    let task = graph.start();
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        numbers.send(Envelope::new(42u32)).await.unwrap();
        strings
            .send(Envelope::new("hello".to_owned()))
            .await
            .unwrap();
        assert_eq!(nr.recv::<u32>().await.unwrap().unpack(), 42);
        assert_eq!(sr.recv::<String>().await.unwrap().unpack(), "hello");
        drop((numbers, strings));
        graph.stop();
        task.await.unwrap().unwrap();
    })
    .await
    .unwrap();
}
#[test]
fn conflicting_concrete_constraints_fail_before_tasks_start() {
    let text = r#"
main="g"
[[graphs]]
name="g"
nodes=[{name="n",ty="TemplateNumbersTest"},{name="r",ty="TemplateRelayTest"},{name="s",ty="TemplateStringsTest"}]
connections=[{cap=1,ports=["n:out","r:inp"]},{cap=1,ports=["r:out","s:inp"]}]
"#;
    assert!(matches!(
        Builder::default().template(text).build(),
        Err(Error::ChannelTypeMismatch)
    ));
}
