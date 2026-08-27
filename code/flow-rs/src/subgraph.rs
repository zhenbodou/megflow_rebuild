//! flow-rs · subgraph —— 子图 / 多图的**内联展开（flattening）**（Ch4.4）。
//!
//! 到 Ch4.3 为止，一份配置里虽然能写多张图（`Config.graphs: Vec<GraphConfig>`），但只有
//! `main` 那张被装配。本章让**一张图能当「可复用部件」嵌进另一张图**：主图里一个节点，若它
//! 的 `ty` 恰好等于某张图的名字，就是一次**子图引用**（无需新配置语法——与原版
//! `graph_names.contains(&ty)` 的自动识别一致）。于是「一个模型喂 N 条同构支路」这种真实
//! 拓扑，可以把那条支路写成**一张子图**、在主图里实例化 N 份。
//!
//! # 为什么是「内联展开」而非原版的「嵌套运行时」
//!
//! 原版把 `Graph` 也实现成一个 `Node`（嵌套运行时单元）：子图是个独立的、递归运行的运行时，
//! 父调度器只管调度一批 `Box<dyn Actor>`，其中某些恰好是 `Graph`。它靠**构造后注入通道**
//! （`set_port`）把父子图的边界端口对接起来——这套在原版里最省事，因为原版的端口信息本就是
//! **运行期**计算的、通道是构造后注入的。
//!
//! **但我们这版重写的装配模型完全不同**：节点在**构造时**就把 channel 作为构造器参数拿走
//! （`NodeCtor(&Args, Vec<Vec<Receiver>>, Vec<Vec<Sender>>)`，见 Ch3.2/4.2），端口名表是
//! **编译期** `&'static`（注册表）。这版里根本没有「构造后 `set_port` 注入」这一步，子图也
//! 套不进「一个编译期注册的节点」的模子。于是对**我们的架构**，最省事的选择反了过来：**在
//! 装配之前先把多图压平成一张扁平图**（本模块），之后 Ch3.2 的 `assemble` **一行不改**地跑。
//! 这又是一次「架构决定设计」——与 Ch4.3「需求决定抽象」同气：同一个问题（子图对接），因
//! 底层接线架构不同，最省事的解法正好相反。（动态子图——运行期按流的个数生成 N 份实例——
//! 确实需要嵌套运行时，故划在本教学子集之外，见章末「诚实的边界」。）
//!
//! # 压平做什么
//!
//! [`flatten`] 把 `Config{ main, graphs }` 变成 `Config{ main, graphs: vec![一张扁平图] }`：
//! - **节点**：叶子节点（`ty` 不是图名）原样保留、名字加前缀（`b1` 里的 `t` → `b1/t`）；
//!   子图节点递归展开成它内部的节点。前缀用 `/`——因为 `:` 是 `PortRef` 的「节点:端口」
//!   分隔符，节点名里不能带 `:`。
//! - **连接 / 对外端口**：每个端口引用按子图边界**解析到叶子**。引用 `b1:inp` 里 `b1` 是子图，
//!   `inp` 是它的边界输入端口 → 查 `Branch.inputs` 找到它映射的内部端口 `tf:inp` → 递归下钻，
//!   直到落在真实叶子节点上，得 `b1/tf:inp`。
//! - **资源**：取**主图**的资源。压平后就是一张图，主图的资源自然被所有（原属不同子图实例的）
//!   节点共享——这正回答了 Ch4.3 末尾的钩子「资源怎么跨图共享」。
//!
//! 压平**对没有任何子图引用的单图配置是恒等变换**（前缀为空、引用原样透传），故它插进
//! `Builder::build` 后，前几章所有单图测试一字不改地继续绿。
//!
//! Inline subgraph expansion: rewrite a multi-graph `Config` into one flat graph, so the
//! Ch3.2 assembler runs unchanged. A node whose `ty` equals a graph name is a subgraph ref.

use crate::config::{Config, ConnConfig, GraphConfig, NodeConfig, PortConfig, PortRef};
use crate::error::{Error, Result};
use std::collections::HashMap;

/// 把「主图 + 若干被引用的子图」压平成**一张**扁平图，返回一份新的 `Config`（`main` 不变、
/// `graphs` 只剩那张扁平图）。装配前调用（见 `Builder::build`）；之后 `assemble` 照旧。
///
/// 无子图引用时是**恒等变换**：叶子节点前缀为空（名字不变）、引用原样透传，产出的扁平图与
/// 原主图逐字段等价。有环（某图沿引用链直接/间接引用自己）→ `Err(SubgraphCycle)`。
///
/// Flatten a multi-graph config into one graph; identity when there are no subgraph refs.
pub fn flatten(config: &Config) -> Result<Config> {
    // 名字 → 图 的索引：判断「某个 ty 是不是子图引用」+ 递归时取子图定义，都靠它。
    let graphs: HashMap<&str, &GraphConfig> =
        config.graphs.iter().map(|g| (g.name.as_str(), g)).collect();
    let main = config
        .main_graph()
        .ok_or_else(|| Error::MainGraphNotFound(config.main.clone()))?;

    // 递归展开主图：叶子节点收进 flat_nodes（带前缀名），连接收进 flat_conns（引用已解析到叶子）。
    let mut flat_nodes: Vec<NodeConfig> = Vec::new();
    let mut flat_conns: Vec<ConnConfig> = Vec::new();
    let mut ancestors: Vec<String> = Vec::new();
    expand(
        main,
        "",
        &graphs,
        &mut ancestors,
        &mut flat_nodes,
        &mut flat_conns,
    )?;

    // 对外输入/输出 = 主图的对外端口，引用同样按子图边界解析到叶子（主图前缀为空）。
    // expand(main) 已先把整棵子图树校验为无环，故这里的 resolve 递归必然终止（见 resolve_ref）。
    let flat_inputs = resolve_ports(main, "", &graphs, &main.inputs)?;
    let flat_outputs = resolve_ports(main, "", &graphs, &main.outputs)?;

    let flat = GraphConfig {
        name: main.name.clone(),
        nodes: flat_nodes,
        inputs: flat_inputs,
        outputs: flat_outputs,
        connections: flat_conns,
        // 资源取主图的：压平后一张图，主图资源被所有节点共享（回答 Ch4.3 的跨图共享钩子）。
        resources: main.resources.clone(),
    };
    Ok(Config {
        main: config.main.clone(),
        graphs: vec![flat],
    })
}

/// 递归展开一张图 `g`（当前名字前缀 `prefix`，形如 `"b1/"` 或顶层的 `""`）：
/// 叶子节点带前缀 push 进 `flat_nodes`，子图节点递归下钻，连接解析后 push 进 `flat_conns`。
///
/// **环检测**：`ancestors` 是当前正在展开的图名栈（从主图到此处的引用路径）。若 `g` 已在栈上，
/// 说明沿引用链兜回了自己 → `SubgraphCycle`。注意兄弟复用（`b1`/`b2` 都是 `Branch`）不是环：
/// `b1` 展开完会把 `Branch` 弹出栈，`b2` 再展开时栈上并无 `Branch`。
fn expand(
    g: &GraphConfig,
    prefix: &str,
    graphs: &HashMap<&str, &GraphConfig>,
    ancestors: &mut Vec<String>,
    flat_nodes: &mut Vec<NodeConfig>,
    flat_conns: &mut Vec<ConnConfig>,
) -> Result<()> {
    if ancestors.iter().any(|a| a == &g.name) {
        return Err(Error::SubgraphCycle(g.name.clone()));
    }
    ancestors.push(g.name.clone());

    // 节点：ty 是图名 → 子图引用，递归展开（前缀追加 `节点名/`）；否则叶子，带前缀原样收下。
    for nd in &g.nodes {
        if let Some(sub) = graphs.get(nd.ty.as_str()) {
            let child_prefix = format!("{}{}/", prefix, nd.name);
            expand(
                sub,
                &child_prefix,
                graphs,
                ancestors,
                flat_nodes,
                flat_conns,
            )?;
        } else {
            flat_nodes.push(NodeConfig {
                name: format!("{}{}", prefix, nd.name),
                ty: nd.ty.clone(),
                args: nd.args.clone(),
            });
        }
    }

    // 内部连接：每个端口引用解析到叶子（子图边界端口会被下钻穿透，可能一变多）。
    for conn in &g.connections {
        flat_conns.push(ConnConfig {
            cap: conn.cap,
            ports: resolve_refs(g, prefix, graphs, &conn.ports)?,
        });
    }

    ancestors.pop();
    Ok(())
}

/// 把一组对外端口声明的引用逐个解析到叶子（`cap`/`name` 原样保留）。用于主图 inputs/outputs。
fn resolve_ports(
    g: &GraphConfig,
    prefix: &str,
    graphs: &HashMap<&str, &GraphConfig>,
    ports: &[PortConfig],
) -> Result<Vec<PortConfig>> {
    ports
        .iter()
        .map(|pc| {
            Ok(PortConfig {
                name: pc.name.clone(),
                cap: pc.cap,
                ports: resolve_refs(g, prefix, graphs, &pc.ports)?,
            })
        })
        .collect()
}

/// 解析一组端口引用，把每个引用解析出的叶子引用平摊进一个 `Vec`。
fn resolve_refs(
    g: &GraphConfig,
    prefix: &str,
    graphs: &HashMap<&str, &GraphConfig>,
    refs: &[String],
) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for r in refs {
        resolve_ref(g, prefix, graphs, r, &mut out)?;
    }
    Ok(out)
}

/// 把**一个**端口引用 `r`（在图 `g`、前缀 `prefix` 语境下）解析成若干**叶子**引用，追加进 `out`。
///
/// - `r = "节点:端口"`。先在 `g.nodes` 里按名找这个节点（找不到 → `UnknownNode`）。
/// - 若该节点是**叶子**（`ty` 不是图名）：直接产出带前缀的叶子引用 `prefix+节点:端口`。叶子端口
///   是否真的存在，不在这里查——留给 `assemble`（它有编译期端口名表），与无子图时的行为一致。
/// - 若该节点是**子图引用**：`端口` 是子图的**边界端口名**，去子图的 `inputs`/`outputs` 里找它
///   映射到的内部端口（找不到 → `UnknownPort`），带上追加了 `节点名/` 的前缀**递归下钻**每个
///   内部端口。边界端口可映射到多个内部端口（如子图内的边界输出接了多个源），故一个引用可能
///   解析出多个叶子引用（扇入形态；assemble 会按 mpsc 规则校验）。
///
/// **递归终止性**：本函数沿「节点 ty 是图名」这条边下钻，可达的图是 `expand` 从 `g` 出发可达图
/// 的子集。`flatten` 总是先 `expand(main)` 把整棵树校验为无环，再调用本函数解析主图对外端口 /
/// `expand` 在节点递归之后才解析本图连接——故调用本函数时，其可达子树必已被证明无环，递归必然终止。
fn resolve_ref(
    g: &GraphConfig,
    prefix: &str,
    graphs: &HashMap<&str, &GraphConfig>,
    r: &str,
    out: &mut Vec<String>,
) -> Result<()> {
    let pref = PortRef::parse(r)?;
    let nd = g
        .nodes
        .iter()
        .find(|n| n.name == pref.node)
        .ok_or_else(|| Error::UnknownNode(pref.node.to_owned()))?;

    match graphs.get(nd.ty.as_str()) {
        // 子图引用：把边界端口映射到内部端口，逐个下钻。
        Some(sub) => {
            let decl = sub
                .inputs
                .iter()
                .chain(sub.outputs.iter())
                .find(|p| p.name == pref.port)
                .ok_or_else(|| Error::UnknownPort {
                    node: pref.node.to_owned(),
                    port: pref.port.to_owned(),
                })?;
            let child_prefix = format!("{}{}/", prefix, pref.node);
            for inner in &decl.ports {
                resolve_ref(sub, &child_prefix, graphs, inner, out)?;
            }
            Ok(())
        }
        // 叶子节点：产出带前缀的叶子引用。
        None => {
            out.push(format!("{}{}:{}", prefix, pref.node, pref.port));
            Ok(())
        }
    }
}

// ── 测试：钉死压平的恒等性、前缀展开、边界解析、环检测、多层嵌套（红→绿）──
#[cfg(test)]
mod tests {
    use super::*;

    /// 便捷断言：找到扁平图里某条内部连接（按其端口列表精确匹配）。
    fn has_conn(g: &GraphConfig, ports: &[&str]) -> bool {
        g.connections
            .iter()
            .any(|c| c.ports.iter().map(String::as_str).eq(ports.iter().copied()))
    }

    /// 无子图引用的单图 → 压平是恒等变换：节点名不变、连接原样。这是「前几章测试一字不改
    /// 继续绿」的保证。
    #[test]
    fn flatten_is_identity_without_subgraphs() {
        let toml = r#"
main = "g"
[[graphs]]
name = "g"
nodes = [{name="a", ty="Transform"}, {name="b", ty="Transform"}]
inputs = [{name="in", cap=8, ports=["a:inp"]}]
outputs = [{name="out", cap=8, ports=["b:out"]}]
connections = [{cap=8, ports=["a:out", "b:inp"]}]
"#;
        let cfg = Config::from_toml(toml).unwrap();
        let flat = flatten(&cfg).unwrap();
        assert_eq!(flat.graphs.len(), 1);
        let g = &flat.graphs[0];
        let names: Vec<&str> = g.nodes.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b"], "叶子节点名不加前缀");
        assert_eq!(g.inputs[0].ports, vec!["a:inp".to_string()]);
        assert_eq!(g.outputs[0].ports, vec!["b:out".to_string()]);
        assert!(has_conn(g, &["a:out", "b:inp"]), "连接原样保留");
    }

    /// 主图 `top` 引用两份 `Branch` 子图 → 展开成带前缀的叶子节点，内部连接与边界解析都正确，
    /// 主图资源被带上。
    const SHARED: &str = r#"
main = "top"
[[graphs]]
name = "Branch"
nodes = [{name="tf", ty="Transform"}, {name="t", ty="Tally", res="counter"}]
inputs = [{name="inp", cap=16, ports=["tf:inp"]}]
outputs = [{name="out", cap=16, ports=["t:out"]}]
connections = [{cap=16, ports=["tf:out", "t:inp"]}]
[[graphs]]
name = "top"
resources = [{name="counter", ty="Counter"}]
nodes = [{name="bc", ty="Bcast"}, {name="b1", ty="Branch"}, {name="b2", ty="Branch"}]
inputs = [{name="in", cap=16, ports=["bc:inp"]}]
outputs = [{name="o1", cap=16, ports=["b1:out"]}, {name="o2", cap=16, ports=["b2:out"]}]
connections = [{cap=16, ports=["bc:out", "b1:inp"]}, {cap=16, ports=["bc:out", "b2:inp"]}]
"#;

    #[test]
    fn flatten_expands_and_prefixes_subgraph_nodes() {
        let cfg = Config::from_toml(SHARED).unwrap();
        let flat = flatten(&cfg).unwrap();
        let g = &flat.graphs[0];
        assert_eq!(g.name, "top");

        // 子图节点 b1/b2 被展开掉，留下带前缀的叶子；bc 是主图自己的叶子，不加前缀。
        let mut names: Vec<&str> = g.nodes.iter().map(|n| n.name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, vec!["b1/t", "b1/tf", "b2/t", "b2/tf", "bc"]);
        assert!(
            !g.nodes.iter().any(|n| n.ty == "Branch"),
            "子图节点不应出现在扁平图里"
        );

        // 叶子节点的 args 随实例保留：两份 Tally 都带 res="counter"。
        for tally in g.nodes.iter().filter(|n| n.ty == "Tally") {
            assert_eq!(
                tally.args.get("res").and_then(|v| v.as_str()),
                Some("counter")
            );
        }
    }

    #[test]
    fn flatten_rewrites_internal_and_boundary_edges() {
        let cfg = Config::from_toml(SHARED).unwrap();
        let flat = flatten(&cfg).unwrap();
        let g = &flat.graphs[0];

        // 子图内部连接被带前缀搬上来。
        assert!(has_conn(g, &["b1/tf:out", "b1/t:inp"]), "b1 内部边");
        assert!(has_conn(g, &["b2/tf:out", "b2/t:inp"]), "b2 内部边");
        // 主图连接里的子图边界输入 b1:inp 被下钻解析到叶子 b1/tf:inp。
        assert!(has_conn(g, &["bc:out", "b1/tf:inp"]), "边界输入解析到叶子");
        assert!(has_conn(g, &["bc:out", "b2/tf:inp"]));

        // 主图对外输出里的子图边界输出 b1:out 被下钻解析到叶子 b1/t:out。
        let o1 = g.outputs.iter().find(|p| p.name == "o1").unwrap();
        assert_eq!(o1.ports, vec!["b1/t:out".to_string()], "边界输出解析到叶子");
        let o2 = g.outputs.iter().find(|p| p.name == "o2").unwrap();
        assert_eq!(o2.ports, vec!["b2/t:out".to_string()]);

        // 主图输入指向普通叶子 bc:inp，原样。
        let inp = g.inputs.iter().find(|p| p.name == "in").unwrap();
        assert_eq!(inp.ports, vec!["bc:inp".to_string()]);

        // 资源从主图带上。
        assert_eq!(g.resources.len(), 1);
        assert_eq!(g.resources[0].name, "counter");
    }

    /// 多层嵌套：`outer` 引用 `mid`、`mid` 引用叶子节点 → 前缀逐层叠加成 `m/leaf`。
    #[test]
    fn flatten_handles_nested_subgraphs() {
        let toml = r#"
main = "outer"
[[graphs]]
name = "mid"
nodes = [{name="leaf", ty="Transform"}]
inputs = [{name="i", cap=8, ports=["leaf:inp"]}]
outputs = [{name="o", cap=8, ports=["leaf:out"]}]
[[graphs]]
name = "outer"
nodes = [{name="m", ty="mid"}]
inputs = [{name="in", cap=8, ports=["m:i"]}]
outputs = [{name="out", cap=8, ports=["m:o"]}]
"#;
        let cfg = Config::from_toml(toml).unwrap();
        let flat = flatten(&cfg).unwrap();
        let g = &flat.graphs[0];
        let names: Vec<&str> = g.nodes.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["m/leaf"], "两层前缀叠加");
        // 对外端口两层下钻到最底层叶子。
        assert_eq!(g.inputs[0].ports, vec!["m/leaf:inp".to_string()]);
        assert_eq!(g.outputs[0].ports, vec!["m/leaf:out".to_string()]);
    }

    /// 互相引用的两张图 → 环，报 `SubgraphCycle`（而非爆栈）。
    #[test]
    fn flatten_detects_cycle() {
        let toml = r#"
main = "a"
[[graphs]]
name = "a"
nodes = [{name="nb", ty="b"}]
[[graphs]]
name = "b"
nodes = [{name="na", ty="a"}]
"#;
        let cfg = Config::from_toml(toml).unwrap();
        let err = flatten(&cfg).unwrap_err();
        assert!(
            matches!(err, Error::SubgraphCycle(ref n) if n == "a"),
            "实际：{err:?}"
        );
    }

    /// 引用一个子图不存在的边界端口 → `UnknownPort`。
    #[test]
    fn flatten_rejects_unknown_boundary_port() {
        let toml = r#"
main = "top"
[[graphs]]
name = "Sub"
nodes = [{name="t", ty="Transform"}]
inputs = [{name="inp", cap=8, ports=["t:inp"]}]
outputs = [{name="out", cap=8, ports=["t:out"]}]
[[graphs]]
name = "top"
nodes = [{name="s", ty="Sub"}, {name="k", ty="NoopConsumer"}]
connections = [{cap=8, ports=["s:nosuchport", "k:inp"]}]
"#;
        let cfg = Config::from_toml(toml).unwrap();
        let err = flatten(&cfg).unwrap_err();
        assert!(
            matches!(err, Error::UnknownPort { ref node, ref port } if node == "s" && port == "nosuchport"),
            "实际：{err:?}"
        );
    }

    /// `main` 指向不存在的图 → `MainGraphNotFound`（与既有 assemble 行为同变体）。
    #[test]
    fn flatten_missing_main_errors() {
        let cfg = Config::from_toml("main=\"nope\"\n[[graphs]]\nname=\"x\"\n").unwrap();
        let err = flatten(&cfg).unwrap_err();
        assert!(matches!(err, Error::MainGraphNotFound(ref m) if m == "nope"));
    }
}
