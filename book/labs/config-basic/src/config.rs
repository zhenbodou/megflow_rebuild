use crate::error::{Error, Result};
use serde::Deserialize;

pub type Args = toml::value::Table;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub main: String,
    #[serde(default)]
    pub graphs: Vec<GraphConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphConfig {
    pub name: String,
    #[serde(default)]
    pub nodes: Vec<NodeConfig>,
    #[serde(default)]
    pub inputs: Vec<PortConfig>,
    #[serde(default)]
    pub outputs: Vec<PortConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NodeConfig {
    pub name: String,
    pub ty: String,
    #[serde(default, flatten)]
    pub args: Args,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortConfig {
    pub name: String,
    pub cap: usize,
    #[serde(default)]
    pub ports: Vec<String>,
}

impl Config {
    pub fn from_toml(text: &str) -> Result<Self> {
        Ok(toml::from_str(text)?)
    }

    pub fn main_graph(&self) -> Option<&GraphConfig> {
        self.graphs.iter().find(|graph| graph.name == self.main)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortRef<'a> {
    pub node: &'a str,
    pub port: &'a str,
}

impl<'a> PortRef<'a> {
    pub fn parse(text: &'a str) -> Result<Self> {
        match text.split_once(':') {
            Some((node, port)) if !node.is_empty() && !port.is_empty() => Ok(Self { node, port }),
            _ => Err(Error::BadPortRef(text.to_owned())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = r#"
main = "example"
[[graphs]]
name = "example"
nodes = [{ name = "add", ty = "BinaryOp", op = "+" }]
inputs = [
    { name = "a", cap = 16, ports = ["add:a"] },
    { name = "b", cap = 16, ports = ["add:b"] }
]
outputs = [{ name = "c", cap = 16, ports = ["add:c"] }]
"#;

    #[test]
    fn reads_graph_and_preserves_args() {
        let config = Config::from_toml(EXAMPLE).unwrap();
        assert_eq!(config.main, "example");
        let graph = config.main_graph().unwrap();
        assert_eq!(graph.nodes.len(), 1);
        assert_eq!(graph.nodes[0].name, "add");
        assert_eq!(graph.nodes[0].ty, "BinaryOp");
        assert_eq!(graph.nodes[0].args["op"].as_str(), Some("+"));
        assert_eq!(graph.nodes[0].args.len(), 1);
        assert_eq!(graph.inputs.len(), 2);
        assert_eq!(graph.inputs[0].cap, 16);
        assert_eq!(graph.inputs[1].ports, ["add:b"]);
        assert_eq!(graph.outputs[0].name, "c");
        assert_eq!(graph.outputs[0].ports, ["add:c"]);
    }

    #[test]
    fn optional_lists_default_to_empty() {
        let config = Config::from_toml("main = 'missing'").unwrap();
        assert!(config.graphs.is_empty());
        assert!(config.main_graph().is_none());
    }

    #[test]
    fn missing_and_mistyped_main_are_errors() {
        assert!(matches!(
            Config::from_toml("graphs = []"),
            Err(Error::Toml(_))
        ));
        assert!(matches!(Config::from_toml("main = 7"), Err(Error::Toml(_))));
    }

    #[test]
    fn unknown_fields_are_not_silently_ignored() {
        let error = Config::from_toml("main = 'x'\ngrahps = []").unwrap_err();
        assert!(error.to_string().contains("grahps"));
    }

    #[test]
    fn references_borrow_the_input() {
        let text = String::from("add:a");
        let reference = PortRef::parse(&text).unwrap();
        assert_eq!(
            reference,
            PortRef {
                node: "add",
                port: "a"
            }
        );
        assert_eq!(reference.node.as_ptr(), text.as_ptr());
    }

    #[test]
    fn malformed_references_are_errors() {
        for text in ["", "adda", "add:", ":a"] {
            assert!(matches!(PortRef::parse(text), Err(Error::BadPortRef(_))));
        }
    }
}
