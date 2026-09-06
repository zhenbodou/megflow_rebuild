use macro_lab_derive::TypeName;

#[derive(TypeName)]
struct Node<T> { value: T }

fn main() {
    let node = Node { value: 7u32 };
    assert_eq!(Node::<u32>::type_name(), "Node");
    assert_eq!(node.value, 7);
    println!("第 1 步：derive 通过");
}
