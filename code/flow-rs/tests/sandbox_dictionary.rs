use flow_rs::prelude::*;
use std::sync::{Arc, Mutex};
#[tokio::test]
async fn sandbox_demux_uses_zero_tag_and_keeps_plain_callback_port_names() {
    let received = Arc::new(Mutex::new(Vec::new()));
    let record = received.clone();
    let mut sandbox = Sandbox::pure("Demux").unwrap();
    sandbox.add_envelope("inp", |index| {
        (index < 3).then(|| {
            Envelope::with_info(
                index as u32,
                EnvelopeInfo {
                    to_addr: Some(if index == 1 { 99 } else { 0 }),
                    partial_id: Some(index as u64),
                    ..Default::default()
                },
            )
        })
    });
    sandbox.add_envelope_check("out", move |mut envelope: Envelope<u32>| {
        record
            .lock()
            .unwrap()
            .push((envelope.unpack(), envelope.info().partial_id));
    });
    tokio::time::timeout(std::time::Duration::from_secs(3), sandbox.start())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(*received.lock().unwrap(), [(0, Some(0)), (2, Some(2))]);
}

#[inputs(inp: {u32})]
#[outputs(out: u32)]
#[derive(Node, Actor, BuildFromPorts)]
struct DictionaryInput {}
#[methods]
impl DictionaryInput {
    async fn exec(&mut self) -> Result<()> {
        self.out.send(self.inp.get(&0).unwrap().recv().await?).await
    }
}
node_register!("SandboxDictionaryInput", DictionaryInput);
#[tokio::test]
async fn sandbox_also_assigns_zero_to_dictionary_inputs() {
    let received = Arc::new(Mutex::new(Vec::new()));
    let record = received.clone();
    let mut sandbox = Sandbox::pure("SandboxDictionaryInput").unwrap();
    sandbox.add_data("inp", |i| (i < 2).then_some(i as u32));
    sandbox.add_check("out", move |value: u32| {
        record.lock().unwrap().push(value);
    });
    tokio::time::timeout(std::time::Duration::from_secs(3), sandbox.start())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(*received.lock().unwrap(), [0, 1]);
}
