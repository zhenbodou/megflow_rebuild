use flow_rs::prelude::*;
#[inputs(inp: [u32])]
#[outputs(out: [u32])]
#[derive(Node, Actor, BuildFromPorts)]
struct TypedFanout {}
#[methods]
impl TypedFanout {
    async fn exec(&mut self) -> Result<()> {
        let message = self.inp[0].recv().await?;
        for output in &self.out {
            output.send(message.clone()).await?;
        }
        Ok(())
    }
}
node_register!("TypedListFanout", TypedFanout);
#[tokio::test]
async fn typed_lists_construct_send_and_close_all_members() {
    let text = r#"
main="g"
[[graphs]]
name="g"
nodes=[{name="n",ty="TypedListFanout"}]
inputs=[{name="in",cap=1,ports=["n:inp"]}]
outputs=[{name="a",cap=1,ports=["n:out"]},{name="b",cap=1,ports=["n:out"]}]
"#;
    let mut graph = Builder::default().template(text).build().unwrap();
    let input = graph.input("in").unwrap();
    assert_eq!(input.chan_tid(), MsgTypeId::of::<u32>());
    let a = graph.take_output("a").unwrap();
    let b = graph.take_output("b").unwrap();
    let task = graph.start();
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        input.send(Envelope::new(42u32)).await.unwrap();
        assert_eq!(a.recv::<u32>().await.unwrap().unpack(), 42);
        assert_eq!(b.recv::<u32>().await.unwrap().unpack(), 42);
        drop(input);
        graph.stop();
        task.await.unwrap().unwrap();
        assert!(matches!(a.recv_any().await, Err(Error::ChannelClosed)));
        assert!(matches!(b.recv_any().await, Err(Error::ChannelClosed)));
    })
    .await
    .unwrap();
}
#[test]
fn builtin_lists_keep_template_identity_in_registration() {
    for name in ["Bcast", "Merge"] {
        let reg = flow_rs::registry::find(name).unwrap();
        assert_eq!((reg.input_types)(), vec![MsgTypeId::Template(0)]);
        assert_eq!((reg.output_types)(), vec![MsgTypeId::Template(0)]);
    }
}
