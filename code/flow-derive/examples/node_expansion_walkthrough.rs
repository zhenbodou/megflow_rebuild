//! 调用真实生成器，观察每一步产物。AST 检查不替代下游编译和行为测试。
#[allow(dead_code)]
#[path = "../src/node.rs"]
mod generator;
use syn::{parse_quote, DeriveInput, ItemStruct};

fn main() {
    let original: ItemStruct = parse_quote! { struct Doubler {} };
    let inputs = vec![syn::parse_str::<generator::PortSpec>("inp: u32").unwrap()];
    let outputs = vec![syn::parse_str::<generator::PortSpec>("out: String").unwrap()];

    // ANCHOR: inject_fields
    let with_inputs = generator::expand_inputs(&inputs, original);
    let with_inputs: ItemStruct = syn::parse2(with_inputs).unwrap();
    let with_outputs = generator::expand_outputs(&outputs, with_inputs);
    let final_struct: ItemStruct = syn::parse2(with_outputs.clone()).unwrap();
    let names: Vec<_> = final_struct.fields.iter()
        .map(|field| field.ident.as_ref().unwrap().to_string()).collect();
    assert_eq!(names, ["inp", "input_closed", "out"]);
    println!("步骤 1：属性宏生成字段\n{with_outputs}\n");
    // ANCHOR_END: inject_fields

    // ANCHOR: generate_impls
    let input: DeriveInput = syn::parse2(with_outputs).unwrap();
    let node_impl = generator::expand_derive_node(&input);
    let actor_impl = generator::expand_derive_actor(&input);
    let constructor = generator::expand_build_from_ports(&input);
    for (name, tokens) in [("Node", node_impl), ("Actor", actor_impl), ("BuildFromPorts", constructor)] {
        let _: syn::ItemImpl = syn::parse2(tokens.clone()).unwrap();
        println!("步骤 2：{name} 实现\n{tokens}\n");
    }
    // ANCHOR_END: generate_impls
    println!("展开追踪通过：字段顺序和三个 impl 均可解析；请继续运行下游 typed_node 测试");
}
