//! flow-rs · config —— 图配置解析层（Ch3.1）。
//!
//! Part 3 的起点：把 Ch0.3 钉死的那段**图拓扑 TOML**，反序列化成一组**类型化
//! 结构**。这是引擎的「presentation 层」——只管「文本 → 结构」这一步，**不做**
//! 跨引用校验（端口引用 `"add:a"` 里的 `add` 到底存不存在、类型对不对，留到
//! Ch3.2 的 `Builder::build()`）。原版也有配置连接检查和类型推断。
//!
//! 用两块生态基石：
//! - **`serde`**：通用序列化框架。`#[derive(Deserialize)]` 自动为结构体生成解析逻辑。
//! - **`toml`**：把 TOML 文本喂给 serde，`toml::from_str::<Config>(..)` 一步到位。
//!
//! 结构对齐 Ch0.3 契约表里的 TOML schema，是原版 `config/presentation.rs` 的
//! **教学子集**（Part 3 只覆盖 BinaryOp 用到的字段；Ch4.1 补上图**内部连接**
//! `connections`；`resources`/子图等更后面再上）。
//!
//! Graph-config layer: deserialize the pinned TOML schema into typed structs.
//! Pure `text → structs`; cross-reference validation lives in `build()` (Ch3.2).

use crate::error::{Error, Result};
use serde::{de::DeserializeOwned, Deserialize};

/// 节点参数表：TOML 里 `name`/`ty` 之外的多余键，会被 `flatten` 收进这里。
/// 等价于 Ch0.3 契约里的 `Args`（原版也用 `toml::value::Table`）。
/// Extra node keys captured here; same as the reference `Args`.
pub type Args = toml::value::Table;

/// 从参数表 `args` 里按键名取出并反序列化成 `T`——`#[derive(BuildFromPorts)]` 生成的
/// `build` 用它填充节点的**自有参数字段**（如 `BinaryOp` 的 `op: String` ← `args["op"]`）。
///
/// 这是「配置驱动」在构造侧的落点：`flatten` 在 Ch3.1 把节点私有键兜进 `args`（一个
/// `key → toml::Value` 的表），这里再把某个键**按目标字段的类型**反序列化回来。缺键、
/// 或类型对不上（`op` 写成了数字），都归到 `Error::Arg`——于是「参数不对」在 `build()`
/// 当场报错，而非等到节点运行时才 panic。这正是「校验前移到 build()」的一部分。
///
/// Deserialize one node arg by key; missing or wrong-typed → `Error::Arg`.
pub fn arg<T: DeserializeOwned>(args: &Args, key: &str) -> Result<T> {
    let value = args.get(key).ok_or_else(|| Error::Arg {
        key: key.to_owned(),
        msg: "missing".to_owned(),
    })?;
    value
        .clone()
        .try_into()
        .map_err(|e: toml::de::Error| Error::Arg {
            key: key.to_owned(),
            msg: e.to_string(),
        })
}

/// 一整份图配置：入口图名 `main` + 若干张图 `graphs`。
///
/// `#[serde(deny_unknown_fields)]`：拼错的顶层键（如 `grahps`）在**解析期**就报错，
/// 而非静默忽略——这是「校验前移」的最省事一层：serde 免费帮你挡住笔误。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// 入口图的名字：指向 `graphs` 里的某一张。
    pub main: String,
    /// 本配置里的所有图。单图场景就一张；多图/子图见 Part 4。
    #[serde(default)]
    pub graphs: Vec<GraphConfig>,
}

/// 一张图：名字 + 节点表 + 对外输入/输出端口 + 内部连接。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphConfig {
    /// 图名（与 `Config::main` 或子图引用对应）。
    pub name: String,
    /// 图里的节点实例。
    #[serde(default)]
    pub nodes: Vec<NodeConfig>,
    /// 图对外暴露的输入端口。
    #[serde(default)]
    pub inputs: Vec<PortConfig>,
    /// 图对外暴露的输出端口。
    #[serde(default)]
    pub outputs: Vec<PortConfig>,
    /// 图**内部**的节点间连接（Ch4.1）。对外端口连的是图的边界，`connections` 连的是
    /// 节点与节点——有了它，`a` 的输出才能直接喂给 `b` 的输入，不必绕一圈对外端口。
    #[serde(default)]
    pub connections: Vec<ConnConfig>,
    /// 图的**共享资源**声明（Ch4.3）：构造一次、被图内多个节点按 `name` 共享的重对象
    /// （检测模型 / 内存池）。装配期一次建好、随 `Context` 分发给每个节点。
    #[serde(default)]
    pub resources: Vec<ResourceConfig>,
}

/// 一个节点实例：图内名 `name` + 注册类型名 `ty` + 其余键作为构造参数 `args`。
///
/// **注意这里没有 `deny_unknown_fields`**——恰恰相反，「未知键」正是我们要的：
/// `op="+"` 这类节点自有参数会被 `#[serde(flatten)]` 收进 `args`，原样交给节点的
/// 构造器（Ch0.3 里 `args["op"]` 读到的就是它）。serde 有一条**硬性限制**：
/// 官方不支持将 `flatten` 与 `deny_unknown_fields` 组合使用；这不意味着任何
/// 组合都会触发编译错误。本结构体需要收集节点私有参数，因此不加 deny。
#[derive(Debug, Clone, Deserialize)]
pub struct NodeConfig {
    /// 节点在本图内的实例名（图内唯一）。
    pub name: String,
    /// 节点的注册类型名：对应 `node_register!("BinaryOp", ..)` 里那个字符串。
    pub ty: String,
    /// `name`/`ty` 之外的所有键，打包成参数表交给节点构造器。
    #[serde(default, flatten)]
    pub args: Args,
}

/// 一个**资源**声明：资源名 `name` + 注册类型名 `ty` + 其余键作为构造参数 `args`（Ch4.3）。
///
/// 与 [`NodeConfig`] 同构——`name`/`ty` 之外的键由 `#[serde(flatten)]` 兜进 `args`，原样交给
/// 资源构造器（`resource::BuildResource::build`）。同样因 `flatten` ⊥ `deny_unknown_fields`
/// **不加 deny**。资源与节点的区别只在语义：资源**构造一次、被多个节点按 `name` 共享**，
/// 而非每节点各造一份（共享模型 / 内存池）。
///
/// A resource declaration: name + registered type + flattened construction args.
#[derive(Debug, Clone, Deserialize)]
pub struct ResourceConfig {
    /// 资源在本图内的名字（节点按它借用；图内唯一）。
    pub name: String,
    /// 资源的注册类型名：对应 `resource_register!("Counter", ..)` 里那个字符串。
    pub ty: String,
    /// `name`/`ty` 之外的所有键，打包成参数表交给资源构造器。
    #[serde(default, flatten)]
    pub args: Args,
}

/// 一个对外端口：端口名 + channel 容量 `cap` + 接到哪些「节点:端口」。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortConfig {
    /// 对外端口名（`graph.input("a")` 里用的就是它）。
    pub name: String,
    /// 这条 channel 的容量（缓冲多少条消息）——满了触发背压。
    pub cap: usize,
    /// 端口引用列表，每项形如 `"节点名:端口名"`（如 `"add:a"`）。
    #[serde(default)]
    pub ports: Vec<String>,
}

/// 一条**内部连接**：channel 容量 `cap` + 挂在这条 channel 上的一组「节点:端口」（Ch4.1）。
///
/// 与 [`PortConfig`] 的关键区别是**没有名字**——内部连接是匿名的，它不对外暴露，只把
/// 图里两个（或多个）节点端口接到同一条 channel 上。方向不写死在配置里，而是**由端口
/// 角色推断**：引用里指向某节点**输出端口**的那一端是发送方、指向**输入端口**的那一端
/// 是接收方（Ch3.2 的注册表 `inputs`/`outputs` 端口名表就是判据）。mpsc 单消费者要求
/// 一条连接至少有 1 个接收端和 1 个发送端；多个接收端竞争同一队列。
/// 若要求每个消费者都收到一份消息，应通过 bcast 连接不同队列。
///
/// An anonymous internal edge: a channel plus the `"node:port"` refs hung on it.
/// Direction is inferred per endpoint from its port role (output → sender, input → receiver).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnConfig {
    /// 这条 channel 的容量（缓冲多少条消息）——满了触发背压。
    pub cap: usize,
    /// 挂在这条 channel 上的端口引用，每项形如 `"节点名:端口名"`（如 `"add1:c"`）。
    #[serde(default)]
    pub ports: Vec<String>,
}

impl Config {
    /// 把一段 TOML 文本解析成 `Config`。解析失败 → `Err(Error::Toml)`。
    /// Parse a TOML string into `Config`.
    pub fn from_toml(s: &str) -> Result<Self> {
        Ok(toml::from_str(s)?)
    }

    /// 取入口图（`name == self.main` 的那张）；找不到返回 `None`。
    /// 「main 指向一张不存在的图」是一种配置错误——本层只**发现**（返回 None），
    /// 由 Ch3.2 的 `build()` 决定如何**报错**（校验集中在 build）。
    /// The entry graph, or `None` if `main` names no graph.
    pub fn main_graph(&self) -> Option<&GraphConfig> {
        self.graphs.iter().find(|g| g.name == self.main)
    }
}

/// 一个端口引用 `"节点名:端口名"` 拆开后的**借用视图**——零拷贝，字段借自原串。
///
/// 端口引用是图接线的最小单位：`inputs`/`outputs`/（后续的）`connections` 里的每个
/// `"add:a"` 都要拆成「哪个节点」+「哪个端口」，Ch3.2 的 Builder 靠它把 channel 接到
/// 节点字段上。这里用 `&str` 借用而非 `String` 拥有，是本章顺带的一个 Rust 练习点：
/// 解析结果只在接线那一刻用一下，没必要各自持有一份堆分配的拷贝。
///
/// Borrowed view of a `"node:port"` reference; zero-copy, fields borrow from `s`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortRef<'a> {
    /// 节点实例名（`:` 左侧）。
    pub node: &'a str,
    /// 端口名（`:` 右侧）。
    pub port: &'a str,
}

impl<'a> PortRef<'a> {
    /// 解析 `"node:port"`。缺少 `:`、或任一侧为空 → `Err(Error::BadPortRef)`。
    /// Parse `"node:port"`; missing colon or empty side → `BadPortRef`.
    pub fn parse(s: &'a str) -> Result<Self> {
        match s.split_once(':') {
            Some((node, port)) if !node.is_empty() && !port.is_empty() => {
                Ok(PortRef { node, port })
            }
            _ => Err(Error::BadPortRef(s.to_owned())),
        }
    }
}

// ── 测试：钉死 schema 解析与端口引用拆分（红→绿）──
#[cfg(test)]
mod tests {
    use super::*;

    /// Ch0.3 契约里那段一字不差的 BinaryOp 图配置。
    const BINARY_OP: &str = r#"
main = "example"
[[graphs]]
name = "example"
nodes = [
    {name="add", ty="BinaryOp", op="+"},
]
inputs = [
    {name="a", cap=16, ports=["add:a"]},
    {name="b", cap=16, ports=["add:b"]}
]
outputs = [{name="c", cap=16, ports=["add:c"]}]
"#;

    #[test]
    fn parses_binary_op_graph() {
        let cfg = Config::from_toml(BINARY_OP).unwrap();
        assert_eq!(cfg.main, "example");
        assert_eq!(cfg.graphs.len(), 1);

        let g = &cfg.graphs[0];
        assert_eq!(g.name, "example");
        // 节点：name / ty / 以及 flatten 进 args 的 op
        assert_eq!(g.nodes.len(), 1);
        assert_eq!(g.nodes[0].name, "add");
        assert_eq!(g.nodes[0].ty, "BinaryOp");
        assert_eq!(
            g.nodes[0].args.get("op").and_then(|v| v.as_str()),
            Some("+")
        );
        // 对外端口 a / b / c
        assert_eq!(g.inputs.len(), 2);
        assert_eq!(g.inputs[0].name, "a");
        assert_eq!(g.inputs[0].cap, 16);
        assert_eq!(g.inputs[0].ports, vec!["add:a".to_string()]);
        assert_eq!(g.inputs[1].ports, vec!["add:b".to_string()]);
        assert_eq!(g.outputs.len(), 1);
        assert_eq!(g.outputs[0].name, "c");
        assert_eq!(g.outputs[0].ports, vec!["add:c".to_string()]);
    }

    #[test]
    fn node_args_capture_extra_keys_but_not_name_ty() {
        let toml = r#"
main="g"
[[graphs]]
name="g"
nodes=[{name="n", ty="T", alpha=1, beta="two", flag=true}]
"#;
        let cfg = Config::from_toml(toml).unwrap();
        let args = &cfg.graphs[0].nodes[0].args;
        // 多余键（含多种类型）全部落进 args
        assert_eq!(args.get("alpha").and_then(|v| v.as_integer()), Some(1));
        assert_eq!(args.get("beta").and_then(|v| v.as_str()), Some("two"));
        assert_eq!(args.get("flag").and_then(|v| v.as_bool()), Some(true));
        // name / ty 是结构体字段，不该混进 args
        assert!(!args.contains_key("name"));
        assert!(!args.contains_key("ty"));
    }

    #[test]
    fn main_graph_selects_by_name() {
        let toml = r#"
main="second"
[[graphs]]
name="first"
[[graphs]]
name="second"
"#;
        let cfg = Config::from_toml(toml).unwrap();
        assert_eq!(cfg.main_graph().unwrap().name, "second");
    }

    #[test]
    fn main_graph_none_when_missing() {
        let cfg = Config::from_toml("main=\"nope\"\n[[graphs]]\nname=\"first\"\n").unwrap();
        assert!(cfg.main_graph().is_none());
    }

    #[test]
    fn port_ref_splits_node_and_port() {
        let r = PortRef::parse("add:a").unwrap();
        assert_eq!(r.node, "add");
        assert_eq!(r.port, "a");
    }

    #[test]
    fn port_ref_rejects_malformed() {
        // 无冒号 / 右空 / 左空 / 全空，都该被拒
        for bad in ["adda", "add:", ":a", ""] {
            assert!(matches!(PortRef::parse(bad), Err(Error::BadPortRef(_))));
        }
    }

    #[test]
    fn parses_graph_resources() {
        // Ch4.3：resources 声明 = name/ty + flatten 进 args 的额外键（capacity）。
        // 加了 resources 字段后，deny_unknown_fields 的 GraphConfig 才认得这个键。
        let toml = r#"
main = "g"
[[graphs]]
name = "g"
resources = [{name="pool", ty="MemPool", capacity=1024}]
"#;
        let cfg = Config::from_toml(toml).unwrap();
        let g = &cfg.graphs[0];
        assert_eq!(g.resources.len(), 1);
        assert_eq!(g.resources[0].name, "pool");
        assert_eq!(g.resources[0].ty, "MemPool");
        assert_eq!(
            g.resources[0]
                .args
                .get("capacity")
                .and_then(|v| v.as_integer()),
            Some(1024)
        );
    }

    #[test]
    fn graph_without_resources_still_parses() {
        // resources 是 #[serde(default)]：老图（无 resources 键）照样解析成空表。
        let cfg = Config::from_toml(BINARY_OP).unwrap();
        assert!(cfg.graphs[0].resources.is_empty());
    }

    #[test]
    fn unknown_top_level_field_is_rejected() {
        // deny_unknown_fields：拼错的键（grahps）在解析期即报错——校验前移的免费一层。
        let toml = "main=\"g\"\ngrahps=[]\n";
        assert!(matches!(Config::from_toml(toml), Err(Error::Toml(_))));
    }

    #[test]
    fn missing_main_is_error() {
        // main 是必填字段（非 Option、无 default）：缺了就解析失败。
        let toml = "[[graphs]]\nname=\"g\"\n";
        assert!(matches!(Config::from_toml(toml), Err(Error::Toml(_))));
    }
}
