use flow_rs::config::interlayer::{MsgType, Port, PortInfo, PortType};
use flow_rs::envelope::str2addr;
#[test]
fn tagged_port_parser_preserves_reference_split_and_address_rules() {
    assert_eq!(
        Port::parse("router:out").unwrap(),
        (("router", "out"), None)
    );
    assert_eq!(
        Port::parse("router:out:42").unwrap(),
        (("router", "out"), Some(42))
    );
    assert_eq!(
        Port::parse("r:p:camera:1").unwrap(),
        (("r", "p"), Some(str2addr("camera:1")))
    );
    assert_eq!(
        Port::parse("r:p:").unwrap(),
        (("r", "p"), Some(str2addr("")))
    );
    assert_eq!(Port::parse("r:p: 42").unwrap().1, Some(str2addr(" 42")));
    assert_eq!(Port::parse(":").unwrap(), (("", ""), None));
    assert!(Port::parse("router").is_err());
}
#[test]
fn dynamic_shape_is_independent_of_payload_type_and_tag() {
    for shape in [
        PortType::Unit,
        PortType::List,
        PortType::Dict,
        PortType::Dyn,
    ] {
        let port = Port {
            node_type: "Example".into(),
            node_name: "n".into(),
            port_info: PortInfo {
                name: "out".into(),
                ty: shape,
                mty: MsgType::of::<u32>(),
            },
            port_tag: Some(7),
        };
        assert_eq!(port.is_dyn(), shape == PortType::Dyn);
    }
}
