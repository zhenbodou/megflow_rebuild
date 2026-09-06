use macro_lab_derive::{port_names, with_label, TypeName};

#[with_label]
#[derive(TypeName, Debug)]
struct Node<T> {
    value: T,
}

fn main() {
    let node = Node {
        value: 7u32,
        label: "source",
    };
    assert_eq!(Node::<u32>::type_name(), "Node");
    assert_eq!(node.label, "source");
    assert_eq!(node.value, 7);
    let ports: &[&str] = port_names!(inp, out,);
    assert_eq!(ports, &["inp", "out"]);
    let empty: &[&str] = port_names!();
    assert!(empty.is_empty());
    println!("三种过程宏通过：{node:?}，端口={ports:?}");
}
