# Ch3.1 serde / toml 与图 TOML schema → 配置解析层

**Part 3 开始了。** 前两部分我们造齐了零件：消息信封（Part 1）、节点与注册表（Part 2）。但它们还是一堆散件——节点能定义、能塌缩、能被名字查到，却还没有人**按一张图纸**把它们接起来跑。Part 3 就是装配线：Ch3.1 读图纸（本章）、Ch3.2 照图纸造节点接线（Builder）、Ch3.3 把节点交给 tokio 跑起来（调度）、Ch3.4 让 `1 + 2 == 3` 端到端穿过整张图——**整本书的大里程碑**。

这一章补上第一步：把 Ch0.3 钉死的那段**图拓扑 TOML**，变成引擎能操作的**类型化结构**。

先完成本章的结构定义，再做 [Ch3.1a 配置分层实验](ch01a-config-workshop.md)：在独立工程中亲自区分解析错误、引用错误与节点参数错误。

<!-- toc -->

## 1. 从一段 TOML 到一组结构

Ch0.3 的示例可写成以下等价 TOML（空白排版不影响解析）：

```toml
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
```

引擎不能直接对着一坨文本干活。它需要把这段文本变成**结构**——`main` 是个字符串、`graphs` 是个数组、每个节点有 `name`/`ty` 和一包参数……本章的产物就是这组结构，以及「文本 → 结构」这一步。

这组结构在整条建图链里的位置：

```mermaid
flowchart LR
    T["图拓扑 TOML<br/>（文本）"] -->|"Ch3.1<br/>Config::from_toml"| C["Config 结构<br/>main / graphs / nodes / ports"]
    C -->|"Ch3.2<br/>Builder::build()"| G["MainGraph<br/>（造好节点、接好线）"]
    G -->|"Ch3.3<br/>graph.start()"| R["运行中的图<br/>（一堆 tokio 任务）"]
    R -->|"Ch3.4"| O["1 + 2 == 3 ✅"]
```

本章只负责最左边那一跳：**文本 → `Config`**。它是纯粹的**解析层**（原版把这一层叫「presentation」）——只管把字段读进结构，**不判断**「`add:a` 里的 `add` 到底存不存在」这类跨引用问题。那类校验集中在 Ch3.2 的 `build()`。原版也有配置翻译、连接检查和类型推断，不能把此处的教学分层说成原版缺少建图校验。

## 2. serde + toml：不自己写 parser

要不要手写一个 TOML 解析器？不要。这正是 Rust 生态最成熟的一块，用两块基石拼起来即可：

- **[`serde`]**：通用**序列化/反序列化框架**。它定义了「一个类型如何与数据格式互转」的抽象；`#[derive(Deserialize)]` 会为你的结构体**自动生成**解析逻辑——你写字段，它写「怎么从数据里把这些字段读出来」。
- **[`toml`]**：TOML 这个具体格式的 serde 实现。`toml::from_str::<Config>(text)` 把文本喂给 serde、按 `Config` 的派生逻辑填出一个 `Config`。

于是「解析」这件事，用户侧就一行：

```rust,ignore
let cfg: Config = toml::from_str(text)?;
```

剩下的全是**声明式**的——你把结构体长相声明清楚（哪些字段、什么类型、缺了给什么默认值），serde 负责兑现。这与 Part 2 的过程宏是同一种味道：**用「生成代码」消灭样板**，只不过这次生成器是 serde 而非我们自己写的宏。

> 这也接上了 Part 1 的依赖边界：serde / toml 都是 crates.io 上的公共 crate，任何人一套 stock 环境就能拉到。我们不碰原版那批 `registry = "megvii"` 私有依赖。

## 3. `config.rs`：四个结构

schema 里有四种东西——整份配置、一张图、一个节点、一个端口——就写四个结构，逐一对应（**本章教学子集示意**：终点 `GraphConfig` 还长了 `connections`/`resources` 两个字段，见下方 §3.3 末的教学子集声明与 §7 的真实 include）：

```rust,ignore
use serde::Deserialize;

pub type Args = toml::value::Table;      // 节点参数表（= Ch0.3 契约里的 Args）

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub main: String,                    // 入口图名
    #[serde(default)]
    pub graphs: Vec<GraphConfig>,        // 所有图
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphConfig {
    pub name: String,
    #[serde(default)] pub nodes: Vec<NodeConfig>,
    #[serde(default)] pub inputs: Vec<PortConfig>,
    #[serde(default)] pub outputs: Vec<PortConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NodeConfig {
    pub name: String,
    pub ty: String,
    #[serde(default, flatten)]
    pub args: Args,                      // name/ty 之外的键，全兜到这里
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortConfig {
    pub name: String,
    pub cap: usize,                      // channel 容量（背压）
    #[serde(default)] pub ports: Vec<String>,   // ["add:a", ..]
}
```

四个结构和 §1 的 TOML 逐行对得上。但有三个 serde 属性值得停下来讲透——它们是这一章真正的「学 Rust / 学生态」的点。

### 3.1 `#[serde(flatten)]`：把多余的键兜进 `args`

看 `NodeConfig`。TOML 里一个节点是 `{name="add", ty="BinaryOp", op="+"}`——`name` 和 `ty` 是**每个**节点都有的固定字段，而 `op="+"` 是 `BinaryOp` **自己**的参数；换个节点可能是 `threshold=0.5` 或者根本没有额外参数。固定字段用具名字段接，**数量不定的自有参数**怎么办？

`#[serde(flatten)]` 就是答案：它让 `args` 这个字段**吸收掉所有没被其它具名字段认领的键**。解析 `{name="add", ty="BinaryOp", op="+"}` 时，`name`/`ty` 先被同名字段吃掉，剩下的 `op="+"` 无处可去——`flatten` 把它收进 `args`（一个 `toml::value::Table`，即 key→value 的表）。于是 Ch0.3 里节点构造器读的那个 `args["op"]`，正是这么来的。

**这就是「配置驱动」的机制底座**：节点的私有参数不必在引擎里写死任何字段，TOML 写什么、`args` 里就有什么，原样交给节点自己解释。

### 3.2 为什么 `NodeConfig` 偏偏不加 `deny_unknown_fields`

你会注意到：`Config`/`GraphConfig`/`PortConfig` 三个都挂了 `#[serde(deny_unknown_fields)]`，唯独 `NodeConfig` 没有。这不是疏漏，是一条 serde 的**硬性限制**：

> `#[serde(flatten)]` 与 `#[serde(deny_unknown_fields)]` **不能共存**。

这是 Serde 官方声明的不支持组合，不应推断成任何组合都会在编译时被拒绝。而 `NodeConfig` 的本意就是**要收**未知键（那正是 `args`），所以它天然不能要 deny。反过来，`PortConfig` 没有兜底字段，未知键一定是笔误，加上 deny 正好挡住。

### 3.3 `deny_unknown_fields` 与 `default`：校验前移的免费第一层

`deny_unknown_fields` 换来一件实在的好处：**把拼写错误的发现时机，从「运行时行为诡异」提前到「解析当场报错」**。比如把 `graphs` 敲成了 `grahps`：

```toml
main = "g"
grahps = []     # ← 拼错了
```

没有 deny 的话，serde 会**默默忽略** `grahps`、把 `graphs` 当成空——你直到运行时发现「图怎么是空的」才反应过来。加了 deny，`toml::from_str` 当场返回 `Err`：`unknown field 'grahps'`。这是「校验前移」最省事的一层——**不用写一行校验代码，serde 免费帮你挡住笔误**。

配套的 `#[serde(default)]` 管另一头：`nodes`/`inputs`/`outputs`/`ports` 都标了 default，所以 TOML 里**省略**它们时，得到的是空 `Vec` 而非解析失败——一张只有输入没有输出的图、一个没有额外参数的节点，都能正常解析。「未知键报错」与「缺省键给默认」这两件事各由一个属性负责，不冲突。

而 `main` 字段**没有** default、也不是 `Option`——它是**必填**的。缺了 `main`，`toml::from_str` 直接报 `missing field 'main'`。这是我们用类型系统表达的一条约束：一份图配置必须指明入口图。

> **教学子集声明**：这四个结构是原版 `config/presentation.rs` 的**精简版**——只覆盖 BinaryOp 里程碑用到的字段。原版还有 `connections`（图内节点互连）、`resources`（共享资源）、子图 `features`、`include`（拆分多文件）等。它们分别留到 Part 4 的相应章节，需要同时实现字段解析、后续转换与行为测试；只加字段不能实现这些配置的业务语义。

## 4. `PortRef`：零拷贝拆 `"node:port"`

端口引用 `"add:a"` 是接线的最小单位——它得拆成「哪个节点」+「哪个端口」，Ch3.2 的 Builder 才知道把 channel 接到哪。我们给它一个专门的类型（**本章示意**：终点 `PortRef` 多一个 `tag: Option<u64>` 字段、`parse` 委托给 `interlayer::Port::parse` 支持 `"node:port:tag"` 地址标签，见 §7 真实 include）：

```rust,ignore
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortRef<'a> {
    pub node: &'a str,   // ':' 左侧
    pub port: &'a str,   // ':' 右侧
}

impl<'a> PortRef<'a> {
    pub fn parse(s: &'a str) -> Result<Self> {
        match s.split_once(':') {
            Some((node, port)) if !node.is_empty() && !port.is_empty() => {
                Ok(PortRef { node, port })
            }
            _ => Err(Error::BadPortRef(s.to_owned())),
        }
    }
}
```

这里有个顺带的 Rust 练习点：**`PortRef` 借用 `&str`，而非持有 `String`**。注意那个生命周期参数 `<'a>`——它声明 `node`/`port` 这两个引用**借自**传进来的 `s`，活得不能比 `s` 久。为什么不干脆存两个 `String`？因为解析结果只在「接线那一刻」用一下，没必要为此各堆分配一份拷贝——`split_once` 返回的本就是原串的两个切片，直接借过来即可。这是 Rust 里「零拷贝解析」的典型手法：**用生命周期把「借来的」这件事写进类型**，编译器替你保证不会悬垂。

`split_once(':')` 干净利落：有冒号就切成两半，没有就返回 `None`。加上「两侧都不能空」的守卫（挡掉 `"add:"`、`":a"`），其余情况一律 `Err(Error::BadPortRef)`——这是本章给 `Error` 枚举**按需**添的新变体。

顺带，配置解析失败也进了 `Error`：

```rust,ignore
#[error("config parse error: {0}")]
Toml(#[from] toml::de::Error),      // #[from]：toml::from_str 的错误能被 `?` 直接抬升
#[error("bad port reference {0:?}, expected \"node:port\"")]
BadPortRef(String),
```

那个 `#[from]` 是 Ch1.1 就用过的 thiserror 便利——它自动生成 `From<toml::de::Error>`，于是 `Config::from_toml` 里一个 `?` 就能把底层解析错误抬成引擎自己的 `Error`。错误枚举**按需生长**：这一章真正构造了这两个变体，才把它们加进去。

## 5. 解析错误与跨引用错误分层处理

最后一个分工要讲明白。`Config` 上有个便利方法：

```rust,ignore
impl Config {
    pub fn main_graph(&self) -> Option<&GraphConfig> {
        self.graphs.iter().find(|g| g.name == self.main)
    }
}
```

注意它返回 `Option`——`main` 指向一张不存在的图时，它返回 `None`，**而不是** `Err`。这是刻意的：**本层只负责「解析成结构」和「按结构查找」，不负责裁定「这份配置合不合法」**。「main 指向的图不存在」「端口引用指向的节点不存在」「输入类型对不上」——这些**跨引用**的校验，全部集中到 Ch3.2 的 `build()` 去做。

为什么这么切？因为跨引用校验需要**全局视野**（要同时看到所有节点、所有端口才能判断一个引用对不对），而它天然属于「装配」这道工序。把它放在 `build()` 里，我们就有了一个**单一的、集中的校验点**。当前实现通过 `?` 返回遇到的错误，并不一次收集所有错误；原版也在配置处理阶段校验连接与推断类型。本章的解析层只做它该做的：把文本变成结构，把结构里能免费查的（拼写、必填、格式）用 serde 属性查掉。

## 6. 本章完整文件与独立构建

前面为了讲解分段展示了类型和方法；下面给出同一基础阶段的完整工程。它不引用后续的资源、模板推导或运行时模块，也不混入最终源码的资源测试。

先在一个不属于其他 Cargo 工作区的新目录建立以下文件：

```text
config-basic/
├── Cargo.toml
└── src/
    ├── lib.rs
    ├── error.rs
    └── config.rs
```

### Cargo.toml

```toml
{{#include ../../labs/config-basic/Cargo.toml}}
```

Serde 的 derive feature 引入派生宏，生成 Deserialize 实现；toml 提供具体文本格式的解析器及 Value/Table；thiserror 为两个错误分支生成标准错误 trait。这里不依赖 flow-rs，因此也不会意外使用后续图构造功能。`[workspace]` 让实验拥有独立工作区根。

公开包的版本约束不等于精确版本；Cargo.lock 记录实际解析版本。首次构建后保留锁文件。维护脚本从主工程锁文件选取缓存的依赖版本并离线检查。

### src/lib.rs

```rust
{{#include ../../labs/config-basic/src/lib.rs}}
```

### src/error.rs

```rust
{{#include ../../labs/config-basic/src/error.rs}}
```

`Toml(#[from] ...)` 同时保存底层解析错误并生成 From 转换，因而 from_toml 中的问号能传播成此处的 Error。BadPortRef 保存错误输入的拥有型 String：调用者可在原始配置释放之后显示错误。这与成功 PortRef 借用原文的选择不同。

### src/config.rs（包括全部六项测试）

```rust
{{#include ../../labs/config-basic/src/config.rs}}
```

from_toml 负责反序列化；main_graph 只查找引用，缺失时返回 None。后者不会消费 Config：返回的 GraphConfig 引用要求原 Config 继续存活。测试使用 String 的内存地址验证 node 切片来自原文，既检查内容也检查没有为该字段额外复制字符串。

## 7. 验收与排错

从 config-basic 目录执行：

```bash
cargo test
cargo test --offline
```

首次命令准备依赖和锁文件；缓存齐全后第二次应离线成功。预期是六项测试通过、零失败、退出码 0。测试分别验证完整 BinaryOp 字段和参数、缺省列表、main 的必填及类型、未知字段、借用式引用拆分和畸形引用。耗时与测试执行顺序不属于断言。

教材仓库根目录的维护命令为：

```bash
python3 scripts/check_basic_config_course.py
```

它在全新临时目录复制上面四份完整文件，离线运行测试，不复制最终 flow-rs。接着完成 [配置分层实验](ch01a-config-workshop.md)，验证“文本能解析”与“能按类型构造业务节点”是两件事。

故意破坏两处再恢复：

1. 删除 Config 上的 deny_unknown_fields，未知字段测试应失败，因为 grahps 被忽略。观察错误发生的阶段，不要靠修改断言让坏行为变绿。
2. 删除 graphs 的 default，只包含 main 的输入应解析失败。解释 default 处理缺失字段，而不是把错误类型改成正确类型。

独立练习：增加同名图检测，并分别验证找不到入口和重复名字的错误。不要在 Deserialize 派生宏里寻找“全局唯一”开关；这是需要自己编写的跨对象检查。

本章基础 schema 不是原版完整配置协议。当前教学选择 deny_unknown_fields，也不自动证明原版同层具有相同的拒绝规则。include、参数覆盖、图端口重写、连接和模板推导需要逐项对照原版配置流水线，不能仅靠添加字段完成。

此外，本章 split_once 只按第一个冒号分割：`add:a:7` 会暂时把 `a:7` 整体视作端口名。后续带标签端口课必须升级解析规则并增加测试；不要把这一阶段的借用演示当成标签协议的最终实现。

## 小结

- **Part 3 开工**：从散件到装配线。本章补上第一跳——**图拓扑 TOML → 类型化 `Config` 结构**。
- **serde + toml**：不自己写 parser。`#[derive(Deserialize)]` 声明式生成解析逻辑，`toml::from_str` 一行到位——和过程宏同源的「用生成代码消灭样板」。
- **三个 serde 属性**：`flatten` 把节点的自有参数兜进 `args`（配置驱动的底座）；`flatten` 与 `deny_unknown_fields` **不能共存**（一收一拒）；`deny_unknown_fields` + `default` 是「校验前移」的免费第一层（笔误当场报错、缺省键给默认、必填缺失即失败）。
- **`PortRef` 零拷贝**：用生命周期 `<'a>` 把「借用原串」写进类型，`split_once` 拆 `"node:port"`。
- **分工**：解析层只「发现」（`main_graph` 返回 `Option`），跨引用校验集中到 Ch3.2 的 `build()`；这说明本书当前的分工，不代表已覆盖原版的全部校验。

下一章 **Ch3.2 · Graph Builder**：拿着这份 `Config`，从 Ch2.4 的注册表 `find` 出每个节点的构造器、按 `PortRef` 把 channel 接到节点的端口上、装出一张 `MainGraph`——**注册表与配置层在这里合流**。跨引用校验也在这一章落地：节点不存在、端口接不上，`build()` 当场报错。

[`serde`]: https://docs.rs/serde
[`toml`]: https://docs.rs/toml
