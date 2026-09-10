# Ch3.1a 配置实作：解析成功之后，还缺什么

本节接在配置结构之后、Builder 之前。完成后，你应该能够从空目录写出一个配置读取程序，解释每个错误由谁发现，再把这段程序接到上一节的注册表。先不创建 channel：我们要先弄清楚将要创建什么。

需要的前置知识是结构体、`Vec`、借用、`Option`、`Result` 和上一节的函数指针注册表。如果你还分不清 `name` 与 `ty`，先记住这个例子：`add_left` 和 `add_right` 可以是两个实例的名字，它们的类型都叫 `BinaryOp`，但可以持有不同参数。

## 1. 从空目录开始，先运行最小解析

在练习目录中执行：

```sh
cargo new config-steps
cd config-steps
```

将 `Cargo.toml` 写成：

```toml
[package]
name = "config-steps"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { version = "1", features = ["derive"] }
toml = "0.8.23"
```

`serde` 定义反序列化接口；启用 `derive` 后才能使用派生宏。`toml` 负责读 TOML 格式，两者的版本号不是同一套版本。本节沿用主工程的 TOML 0.8 系列。第一次运行需要下载依赖；成功后保留生成的 `Cargo.lock`。

先把下面的结构写进 `src/main.rs`：

```rust,ignore
{{#include ../../../code/flow-rs/examples/config_steps.rs:schema}}
```

再在文件末尾写临时入口：

```rust
fn main() {
    let config: Config = toml::from_str("main = \"example\"").unwrap();
    assert_eq!(config.main, "example");
    assert!(config.graphs.is_empty());
    println!("第一次解析成功");
}
```

执行 `cargo run`，应看到 `第一次解析成功`。暂时没使用全部字段，出现未使用字段的警告是正常的。现在把 `main` 改成数字 `7` 再运行：这次 `unwrap()` 会 panic，因为解析返回了 `Err`。`unwrap` 用在本节固定实验数据上帮助暴露失败；框架处理用户输入时应返回错误，让调用者决定如何展示。

`Config` 字段 `main: String` 要求数据是字符串。`graphs` 上的 `default` 只处理缺失字段，缺失时调用 `Vec::default()` 得到空数组；它不会把错误类型自动修正成数组。`[[graphs]]` 在 TOML 中表示追加一张图的表，图里面的 `nodes = [...]` 则是节点列表。

## 2. 查入口：字符串正确，不等于引用正确

把下面函数放在结构定义之后、`main` 之前：

```rust,ignore
{{#include ../../../code/flow-rs/examples/config_steps.rs:select}}
```

这行链式调用可拆成三个动作：`iter()` 借用图列表逐个查看；`find` 找到第一个满足名字条件的图，返回 `Option<&Graph>`；`ok_or_else` 把 `None` 转成附带上下文的 `Err(String)`。闭包里的 `format!` 只在找不到时执行。

返回值借用 `config` 中的图，没有复制整张图。这里只有一个输入引用，Rust 的生命周期省略规则能确定返回引用来自它；完整关系相当于 `fn main_graph<'a>(config: &'a Config) -> Result<&'a Graph, String>`。函数返回后，调用者必须仍然持有这份配置。

`main = "missing"` 完全符合 `String` 的要求。因此，serde 不会替你检查有没有一张图叫 `missing`。这需要我们写查找逻辑。该函数也没有检查重名图：只要找到第一张就返回；后续 Builder 必须另行处理名字唯一性。

## 3. 读取参数：宏无法凭空知道业务要求

继续加入：

```rust,ignore
{{#include ../../../code/flow-rs/examples/config_steps.rs:construct}}
```

`Node` 的 `name`、`ty` 由固定字段接收，其余键由 `flatten` 收进 `toml::Table`。例如 `op = 7` 会成为合法的 TOML 整数，所以第一次解析**能成功**。

等到知道类型是 `BinaryOp` 后，我们才知道 `op` 必须是字符串。`BinaryArgs` 表达这个要求，第二次反序列化便能检查它：

1. `node.args.clone()` 复制参数表，因为本函数只借用了节点，而下一步转换需要拥有数据。
2. `toml::Value::Table(...)` 把表包装成 TOML 值。
3. `try_into()` 尝试把这个值转换成目标类型。左边的 `let args: BinaryArgs` 告诉编译器目标是什么。
4. `map_err` 给原始错误加上节点名；`?` 遇到错误就结束当前函数，否则拿到成功值。

这里的 `#[derive(Deserialize)]` 是过程宏，`#[serde(flatten)]` 是该宏理解的辅助属性。宏在编译时生成读取字段的实现，**用户的 TOML 文本在程序运行时才被读取**。想进一步理解这两种时刻，回看宏专题第 3、5、6 课。

Serde 官方说明 `flatten` 不支持与 `deny_unknown_fields` 组合使用；不要把它解释成任何组合都一定触发编译错误。本例的外层节点收集额外参数，内层 `BinaryArgs` 未设置拒绝未知字段，因此额外参数会被忽略；`opp="+"` 报错的原因是缺少必填 `op`。[Serde 的 flatten 文档](https://serde.rs/attr-flatten.html)

我们暂时用一个 `if` 代表只有一种节点的注册查找。接上上一节的注册表时，用 `node.ty` 查构造函数，把 `args` 交给该函数；每种节点解释自己的参数。最终工程的 `config::arg<T>` 按字段取值，`BuildFromPorts` 宏生成相应调用。本实验的整表转换是在展示相同的分层思路，不要直接替换最终 API。

还有一个边界：`op` 是字符串也不意味着支持对应的运算。原版 BinaryOp 对未知操作符会在执行时失败；本节没有擅自改成构造时拒绝。配置类型检查和业务规则检查需要分别定义与对照。

## 4. 一次只改一个条件，验证错误发生在哪层

删掉第一步的临时 `main`，加入完整实验入口和配置常量：

```rust,ignore
{{#include ../../../code/flow-rs/examples/config_steps.rs:experiment}}
```

再次 `cargo run`。先预测，再核对结果：

| 修改 | 图配置解析 | 后续结果 |
| --- | --- | --- |
| 不修改 | 成功 | 找到 example、add、BinaryOp，读到 `+` |
| `main = 7` | 失败 | 不应尝试装图 |
| `main = "missing"` | 成功 | 查不到入口图 |
| `ty = "Unknown"` | 成功 | 查不到节点构造器 |
| `op = 7` | 成功 | 节点参数无法转换为字符串 |
| `opp = "+"` | 成功 | 节点缺少 `op` 参数 |

程序包含断言，任何结果不符合预期都会失败；具体依赖错误文本可能随补丁版本变化，不要求背诵。`VALID.replace(...)` 每次从原始正确配置构造一个独立坏配置，防止多个错误互相遮蔽。

仓库内的完整文件是 `code/flow-rs/examples/config_steps.rs`，依次包含本节的结构、查找、参数转换和入口，没有依赖任何引擎模块。也可以在仓库根目录执行：

```sh
python3 scripts/check_config_course.py
```

脚本创建临时独立工程，仅加入 serde/toml，然后运行所有断言。它使用 `--offline`，需要先有依赖缓存；你在自己的新工程首次执行普通 `cargo run` 即可下载。

## 5. 从这个实验走到 MegFlow 的 Builder

前面各步拼接后的 `src/main.rs` 完整内容如下。用它核对文件顺序和唯一的 main 入口；替换整份文件后仍运行同一个 `cargo run`，不依赖框架模块。

```rust
{{#include ../../../code/flow-rs/examples/config_steps.rs}}
```

现在可以解释 Builder 的输入，但我们还没有生成运行图。接下来需要根据注册表取得输入、输出端口信息，再验证 `add:a` 指向实际节点的输入端口、分配 channel、把端点交给构造器。`String` 无法表达这些跨对象关系。

原版并非缺少这类校验：参考提交 `95f870bf` 的 `flow-rs/src/config/mod.rs::translate_conn` 会检查空连接、节点和端口；`config/postprocess/mod.rs::proc` 会执行连接检查及类型推断。当前重构的简化 Builder 不能仅凭“能提前报错”就宣称比原版完整。完整配置流程还包括预处理、类型推断和优化等，验收账本继续跟踪这些缺口。

本实验特意只声明 `main/graphs/nodes`，因此完整示例中的 `inputs/outputs` 会被本实验的 `deny_unknown_fields` 拒绝。做完这里，回到 Ch3.1 的四种结构，补入 `PortConfig`、图的 `inputs/outputs`，再进入 Ch3.2。不要将这个练习文件冒充最终的配置 schema。

## 6. 合上答案，完成三道迁移练习

- 把 `graphs` 改成 `grahps`：预测是否会使用默认空数组。验收：解析必须失败，因为未知字段检查先发现了拼写错误。
- 声明两个同为 `BinaryOp` 的节点，实例名不同、参数分别为 `+` 和 `*`，用循环读取它们。验收：得到两个不同操作符，解释为什么 `ty` 可以相同。
- 给 `main_graph` 增加重名图检查，构造两张同名图。验收：明确报重名，不能悄悄选第一张；保留原先找不到入口的错误。提示：先用 `HashSet` 记录出现过的名字，再查入口。

如果做第三题时不知道检查应该写在哪，先画出数据流：文本 → 结构 → 全局名字检查 → 入口选择。你正在自己设计配置处理步骤；这正是下一章开发 Builder 需要的能力。
