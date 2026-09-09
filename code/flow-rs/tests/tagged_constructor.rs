use flow_rs::prelude::*;
use flow_rs::registry::TaggedEndpoint;

#[inputs(inp)]
#[outputs(out)]
#[derive(Node, Actor)]
struct TagProbe {}
#[methods]
impl TagProbe {
    async fn exec(&mut self) -> Result<()> {
        self.out
            .as_ref()
            .unwrap()
            .send_any(self.inp.recv_any().await?)
            .await
    }
}
impl BuildFromPorts for TagProbe {
    const INPUTS: &'static [&'static str] = &["inp"];
    const OUTPUTS: &'static [&'static str] = &["out"];
    const INPUT_ARRAY: &'static [bool] = &[false];
    const OUTPUT_ARRAY: &'static [bool] = &[false];
    fn build(
        _: &flow_rs::config::Args,
        _: Vec<Vec<Receiver>>,
        _: Vec<Vec<Sender>>,
    ) -> Result<Box<dyn Actor>> {
        panic!("Builder must use the tagged constructor")
    }
    fn build_tagged(
        _: &flow_rs::config::Args,
        mut ins: Vec<Vec<TaggedEndpoint<Receiver>>>,
        mut outs: Vec<Vec<TaggedEndpoint<Sender>>>,
    ) -> Result<Box<dyn Actor>> {
        let inp = ins.remove(0).remove(0);
        let out = outs.remove(0).remove(0);
        assert_eq!(inp.tag, Some(7));
        assert_eq!(out.tag, Some(flow_rs::envelope::str2addr("camera:42")));
        Ok(Box::new(Self {
            inp: inp.endpoint,
            out: Some(out.endpoint),
            input_closed: false,
        }))
    }
}
node_register!("TaggedConstructorProbe", TagProbe);

#[tokio::test]
async fn tags_survive_nested_leaf_rewrite_and_all_connection_directions() {
    let text = r#"
main="main"
[[graphs]]
name="main"
nodes=[{name="branch",ty="branch"}]
inputs=[{name="in",cap=1,ports=["branch:in"]}]
outputs=[{name="out",cap=1,ports=["branch:out"]}]
[[graphs]]
name="branch"
nodes=[{name="a",ty="TaggedConstructorProbe"},{name="b",ty="TaggedConstructorProbe"}]
inputs=[{name="in",cap=1,ports=["a:inp:7"]}]
outputs=[{name="out",cap=1,ports=["b:out:camera:42"]}]
connections=[{cap=1,ports=["a:out:camera:42","b:inp:7"]}]
"#;
    let mut graph = Builder::default().template(text).build().unwrap();
    let input = graph.input("in").unwrap();
    let output = graph.take_output("out").unwrap();
    let task = graph.start();
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        input.send(Envelope::new(23u32)).await.unwrap();
        assert_eq!(output.recv::<u32>().await.unwrap().unpack(), 23);
        drop(input);
        graph.stop();
        task.await.unwrap().unwrap();
    })
    .await
    .unwrap();
}
