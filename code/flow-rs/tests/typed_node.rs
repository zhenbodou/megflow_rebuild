use flow_rs::prelude::*;
use std::sync::{Arc, Mutex};
#[inputs(inp: u32)]
#[outputs(out: String)]
#[derive(Default, Node, Actor, BuildFromPorts)]
struct TypedNode {}
#[methods]
impl TypedNode {
    async fn exec(&mut self) -> Result<()> {
        let mut envelope = self.inp.recv().await?;
        let value = envelope.unpack();
        self.out.send(envelope.repack(value.to_string())).await?;
        Ok(())
    }
}
node_register!("TypedPortNode", TypedNode);
#[tokio::test]
async fn typed_attributes_register_wire_preserve_metadata_and_close() {
    let node = TypedNode::default();
    assert!(node.inp.is_none() && node.out.is_none());
    let received = Arc::new(Mutex::new(Vec::new()));
    let sink = received.clone();
    let mut sandbox = Sandbox::pure("TypedPortNode").unwrap();
    sandbox.add_envelope("inp", |index| {
        (index == 0).then(|| {
            Envelope::with_info(
                42u32,
                EnvelopeInfo {
                    partial_id: Some(8),
                    ..Default::default()
                },
            )
        })
    });
    sandbox.add_envelope_check("out", move |mut envelope: Envelope<String>| {
        sink.lock()
            .unwrap()
            .push((envelope.unpack(), envelope.info().partial_id));
    });
    tokio::time::timeout(std::time::Duration::from_secs(2), sandbox.start())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(*received.lock().unwrap(), [(String::from("42"), Some(8))]);
}

#[test]
fn registration_exposes_payload_types_without_constructing_node() {
    use flow_rs::config::interlayer::MsgTypeId;
    let registration = flow_rs::registry::find("TypedPortNode").unwrap();
    assert_eq!(registration.inputs, &["inp"]);
    assert_eq!((registration.input_types)(), vec![MsgTypeId::of::<u32>()]);
    assert_eq!(registration.outputs, &["out"]);
    assert_eq!(
        (registration.output_types)(),
        vec![MsgTypeId::of::<String>()]
    );
}
