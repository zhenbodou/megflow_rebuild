use flow_rs::{config::Config, error::Error, subgraph::flatten};

// ANCHOR: definitions
const TEXT: &str = r#"
main = "top"
[[graphs]]
name = "Branch"
nodes = [{name="leaf", ty="Transform"}]
inputs = [{name="i", cap=4, ports=["leaf:inp"]}]
outputs = [{name="o", cap=4, ports=["leaf:out"]}]
[[graphs]]
name = "top"
nodes = [{name="first", ty="Branch"}, {name="second", ty="Branch"}]
inputs = [{name="in", cap=1, ports=["first:i"]}]
outputs = [{name="out", cap=2, ports=["second:o"]}]
connections = [{cap=3, ports=["first:o", "second:i"]}]
"#;
// ANCHOR_END: definitions

fn main() {
    // ANCHOR: inspect
    let config = Config::from_toml(TEXT).unwrap();
    let flat = flatten(&config).unwrap();
    let graph = flat.main_graph().unwrap();
    let names: Vec<_> = graph.nodes.iter().map(|node| node.name.as_str()).collect();
    assert_eq!(names, ["first/leaf", "second/leaf"]);
    assert_eq!(graph.inputs[0].ports, ["first/leaf:inp"]);
    assert_eq!(graph.outputs[0].ports, ["second/leaf:out"]);
    assert_eq!(
        graph.connections[0].ports,
        ["first/leaf:out", "second/leaf:inp"]
    );
    println!("节点：{names:?}");
    println!("输入：{:?}", graph.inputs[0].ports);
    println!("内部连接：{:?}", graph.connections[0].ports);
    println!("输出：{:?}", graph.outputs[0].ports);
    // 这里只观察配置，没有注册或启动任何节点。
    assert_eq!(config.graphs.len(), 2);
    assert_eq!(flat.graphs.len(), 1);
    // ANCHOR_END: inspect

    // ANCHOR: cycle
    let cyclic = TEXT.replace("ty=\"Transform\"", "ty=\"top\"");
    let config = Config::from_toml(&cyclic).unwrap();
    assert!(matches!(flatten(&config), Err(Error::SubgraphCycle(_))));
    println!("兄弟实例可复用同一图；沿祖先路径返回 top 时拒绝递归环。");
    // ANCHOR_END: cycle
}
