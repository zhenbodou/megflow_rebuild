use flow_rs::prelude::*;
const GRAPH: &str = r#"
main="g"
[[graphs]]
name="g"
nodes=[{name="route",ty="Demux"}]
inputs=[{name="in",cap=1,ports=["route:inp"]}]
outputs=[{name="a",cap=1,ports=["route:out:7"]},{name="b",cap=1,ports=["route:out:camera:42"]}]
"#;
fn message(value: u32, address: u64) -> Envelope<u32> {
    Envelope::with_info(
        value,
        EnvelopeInfo {
            to_addr: Some(address),
            partial_id: Some(3),
            ..Default::default()
        },
    )
}
#[tokio::test]
async fn static_demux_routes_empty_and_data_and_ignores_closed_targets() {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        let mut graph = Builder::default().template(GRAPH).build().unwrap();
        let input = graph.input("in").unwrap();
        let a = graph.take_output("a").unwrap();
        let b = graph.take_output("b").unwrap();
        let task = graph.start();
        let address = flow_rs::envelope::str2addr("camera:42");
        input.send(message(99, 99)).await.unwrap(); // 未知地址不产生输出
        input.send(message(42, address)).await.unwrap();
        let mut received = b.recv::<u32>().await.unwrap();
        assert_eq!(received.unpack(), 42);
        assert_eq!(received.info().partial_id, Some(3));
        assert_eq!(received.info().to_addr, Some(address));
        let mut empty = Envelope::<u32>::empty();
        empty.info_mut().to_addr = Some(7);
        input.send(empty).await.unwrap();
        assert!(a.recv::<u32>().await.unwrap().is_none());
        drop(b);
        input.send(message(0, address)).await.unwrap(); // 已关闭目标不能令节点退出
        input.send(message(7, 7)).await.unwrap();
        assert_eq!(a.recv::<u32>().await.unwrap().unpack(), 7);
        drop(input);
        graph.stop();
        task.await.unwrap().unwrap();
        assert!(matches!(a.recv_any().await, Err(Error::ChannelClosed)));
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn missing_destination_panics_in_node_task() {
    let mut graph = Builder::default().template(GRAPH).build().unwrap();
    let input = graph.input("in").unwrap();
    let task = graph.start();
    input.send(Envelope::new(1u32)).await.unwrap();
    drop(input);
    graph.stop();
    let result = tokio::time::timeout(std::time::Duration::from_secs(3), task)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(result, Err(Error::TaskJoin(_))));
}
#[test]
fn demux_registration_preserves_original_template_and_dictionary_shape() {
    let registration = flow_rs::registry::find("Demux").unwrap();
    assert_eq!((registration.input_types)(), vec![MsgTypeId::Template(0)]);
    assert_eq!((registration.output_types)(), vec![MsgTypeId::Template(0)]);
    assert!(registration.output_is_dict("out"));
}
