use flow_rs::prelude::*;

// 无需声明 Rust 类型 T0/T12：宏将其解释为框架模板。
#[inputs(inp: T0, other: T12)]
#[outputs(out: {T0})]
#[derive(Default, Node, Actor, BuildFromPorts)]
struct TemplateNode {}
#[methods]
impl TemplateNode {
    async fn exec(&mut self) -> Result<()> {
        Ok(())
    }
}
node_register!("TemplateMetadataProbe", TemplateNode);

#[test]
fn template_numbers_survive_attribute_and_derive_expansion() {
    let node = TemplateNode::default();
    let _: &Receiver = &node.inp;
    let _: &Receiver = &node.other;
    let _: &std::collections::HashMap<u64, Sender> = &node.out;
    let registration = flow_rs::registry::find("TemplateMetadataProbe").unwrap();
    assert_eq!(
        (registration.input_types)(),
        vec![MsgTypeId::Template(0), MsgTypeId::Template(12)]
    );
    assert_eq!((registration.output_types)(), vec![MsgTypeId::Template(0)]);
    assert!(registration.output_is_dict("out"));
    assert!(!registration.output_is_array("out"));
}

#[test]
fn wholly_template_connections_use_original_graph_level_any_fallback() {
    let text = r#"
main="g"
[[graphs]]
name="g"
nodes=[{name="n",ty="TemplateMetadataProbe"}]
inputs=[{name="in",cap=1,ports=["n:inp"]}]
outputs=[{name="out",cap=1,ports=["n:out:7"]}]
"#;
    let graph = Builder::default().template(text).build().unwrap();
    assert_eq!(graph.input("in").unwrap().chan_tid(), MsgTypeId::Any);
}
