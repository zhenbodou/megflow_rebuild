use macro_lab_derive::{with_label, TypeName};

#[with_label]
#[derive(TypeName, Debug)]
struct Node<T> { value: T }

fn main() {
    let node = Node { value: 7u32, label: "source" };
    assert_eq!(Node::<u32>::type_name(), "Node");
    assert_eq!(node.value, 7);
    assert_eq!(node.label, "source");
    println!("第 2 步：attribute 通过，{node:?}");
}
