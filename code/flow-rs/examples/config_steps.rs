// ANCHOR: schema
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    main: String,
    #[serde(default)]
    graphs: Vec<Graph>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Graph {
    name: String,
    #[serde(default)]
    nodes: Vec<Node>,
}

#[derive(Debug, Deserialize)]
struct Node {
    name: String,
    ty: String,
    #[serde(flatten)]
    args: toml::Table,
}
// ANCHOR_END: schema

// ANCHOR: select
fn main_graph(config: &Config) -> Result<&Graph, String> {
    config
        .graphs
        .iter()
        .find(|graph| graph.name == config.main)
        .ok_or_else(|| format!("入口图不存在：{}", config.main))
}
// ANCHOR_END: select

// ANCHOR: construct
#[derive(Debug, Deserialize)]
struct BinaryArgs {
    op: String,
}

fn read_operation(node: &Node) -> Result<String, String> {
    if node.ty != "BinaryOp" {
        return Err(format!("节点 {} 的类型未注册：{}", node.name, node.ty));
    }
    let args: BinaryArgs = toml::Value::Table(node.args.clone())
        .try_into()
        .map_err(|error| format!("节点 {} 参数错误：{error}", node.name))?;
    Ok(args.op)
}
// ANCHOR_END: construct

// ANCHOR: experiment
const VALID: &str = r#"
main = "example"
[[graphs]]
name = "example"
nodes = [{name="add", ty="BinaryOp", op="+"}]
"#;

fn main() {
    let config: Config = toml::from_str(VALID).unwrap();
    let graph = main_graph(&config).unwrap();
    let node = &graph.nodes[0];
    assert_eq!(read_operation(node).unwrap(), "+");
    assert!(!node.args.contains_key("name"));
    assert!(!node.args.contains_key("ty"));
    println!("正常：example → add → BinaryOp → +");

    // 字段类型不符：解析阶段就失败。
    let wrong_main = VALID.replace("main = \"example\"", "main = 7");
    assert!(toml::from_str::<Config>(&wrong_main).is_err());
    println!("main=7：解析失败");

    // 类型正确，但引用不存在：解析成功，查找失败。
    let missing_graph = VALID.replace("main = \"example\"", "main = \"missing\"");
    let config: Config = toml::from_str(&missing_graph).unwrap();
    assert!(main_graph(&config).is_err());
    println!("main=missing：解析成功，入口查找失败");

    for (old, new, label) in [
        ("BinaryOp", "Unknown", "未知节点类型"),
        ("op=\"+\"", "op=7", "参数类型错误"),
        ("op=\"+\"", "opp=\"+\"", "参数键拼错"),
    ] {
        let changed = VALID.replace(old, new);
        let config: Config = toml::from_str(&changed).unwrap();
        let node = &main_graph(&config).unwrap().nodes[0];
        let error = read_operation(node).unwrap_err();
        println!("{label}：图配置解析成功，构造准备失败：{error}");
    }
}
// ANCHOR_END: experiment
