# Ch2.3a 宏工程化：错误定位、trybuild 与生命周期

会生成代码只是第一步。要独立维护 MegFlow 的宏，还必须能回答：非法输入在哪里
报错？泛型代码是否真能编译？节点执行失败之后是否仍然释放资源？本章逐个解决。

## 1. 先分清四类失败

| 层次 | 例子 | 谁负责发现 |
| --- | --- | --- |
| token/语法解析 | `node_register!(123, X)` 注册名不是字符串 | `syn::LitStr` 解析 |
| 宏自己的规则 | `#[inputs(inp, inp)]` 重复端口 | 生成字段前的校验 |
| 生成后编译 | `Actor` 的状态类型不满足 `Send` | rustc 的类型与借用检查 |
| 运行行为 | exec 返回 Err 后跳过 finalize | 引擎集成测试 |

对前三类，应让用户在编译时得到能定位到自己代码的说明；第四类即使编译全绿，
仍可能丢消息或泄漏业务资源。不要只检查展开文本里出现了 `finalize` 就下结论。

## 2. 将用户错误变成 syn::Error

打开 `code/flow-derive/src/node.rs`，依次检查 `PortSpec::parse`、
`expand_inputs`、`expand_outputs`。当前版本做了这些检查：

1. `name[]` 的括号必须为空。`inp[16]` 既不是容量配置，也不是固定长度数组，不应被忽略。
2. 输入/输出属性只接受 `struct X { ... }`，不接受元组结构体。
3. 先将原字段名装进 HashSet，再逐个插入端口名；插入失败表示重复或与业务字段冲突。
4. `input_closed` 是当前实现注入的控制字段，拒绝手写同名字段。

错误使用 `syn::Error::new_spanned(&spec.name, "...")` 指向用户的端口名，
然后 `.to_compile_error()` 返回 token。对于正在读的括号内容，
`content.error("...")` 会把位置落到当前游标。
`parse_macro_input!` 自带解析失败转编译错误的路径，普通 `expand_*` 函数则由我们显式处理。

为什么不 `unwrap()`？`parse_quote!` 中完全由作者控制的模板若写错，是实现者自己的 bug；
用户输入则是宏的公共接口，应给出清楚的诊断，不能以“过程宏 panic”结束。
若希望一次报告多处独立错误，可用 `syn::Error::combine` 合并，再统一返回。
当前端口校验报告首个错误，尚未收集所有问题。

## 3. trybuild：把“应该编译失败”写成自动测试

`trybuild` 是本书新增的测试 crate，放在 `flow-derive/Cargo.toml` 的
`[dev-dependencies]` 中，不进入业务程序的正常依赖。
测试入口是一个普通 Rust 集成测试：

```rust,ignore
{{#include ../../../code/flow-derive/tests/ui.rs}}
```

它会为每个小程序调用 Rust 编译器，要求编译失败，并将诊断与同名 `.stderr` 文件比较。
见 [trybuild 官方测试说明](https://docs.rs/trybuild/latest/trybuild/)。

例如 `tests/ui/duplicate_ports.rs` 的完整内容：

```rust,ignore
{{#include ../../../code/flow-derive/tests/ui/duplicate_ports.rs}}
```

预期诊断不是“任何错误都算通过”，而是下面这份已审核的输出：

```text
{{#include ../../../code/flow-derive/tests/ui/duplicate_ports.stderr}}
```

在项目根目录运行：

```bash
cargo test --manifest-path code/Cargo.toml -p flow-derive --test ui --locked
```

五个用例覆盖非空数组括号、重复端口、业务字段冲突、错误结构体形态和错误注册名。
不能把这些文件直接放到 `tests/` 根下当成普通集成测试，否则 Cargo 会认为它们应该编译成功。

第一次新增失败用例，没有 `.stderr` 时，trybuild 会把结果写入 `wip/`，测试失败。
先读诊断，确认没有混入“找不到依赖”等无关错误，再把它作为预期文件保存。
已有用例需要更新时，可以执行：

```bash
TRYBUILD=overwrite cargo test --manifest-path code/Cargo.toml -p flow-derive --test ui --locked
```

**这条命令是更新预期，不是证明正确**。更新后检查 `.stderr` 差异，然后去掉
`TRYBUILD=overwrite` 再运行一次。编译器版本变化也可能改变诊断格式，要先判断
是格式变化还是宏行为回退，不能见红就重新生成。

## 4. 真实修复：为什么 exec 错误会绕过 finalize

旧生成代码将 `self.exec().await?` 放在最外层 `tokio::spawn(async move { ... })` 中。
`?` 提前返回的是**包含它的 async 块**，所以后面的 `self.close()`、
`self.finalize().await` 都没有机会执行。

原版 `../megflow/flow-derive/src/actor.rs` 会把执行循环放进内层 async，
先保存结果，再收尾。当前参考实现已对齐这个控制流：

```rust,ignore
let result = async {
    while !self.is_all_input_closed() {
        self.exec().await?;
    }
    Ok(())
}.await;
self.close();
self.finalize().await;
result
```

这段是实际宏输出的控制流，完整生成器见 Ch2.3。
`.await` 后得到一个 `Result` 值；它可以先存着，执行其他动作，然后返回。
`?` 没有被移除，只是放到了正确的返回边界。

验证不能只搜源码。打开 `code/flow-rs/tests/derive_node.rs` 的 `Failing` 节点：
initialize、exec、finalize 分别记录事件；exec 故意返回带 key 的业务错误；
finalize 内检查输出已经关闭。测试同时验证：

- 事件恰为 `initialize → exec → finalize`，没有漏调或重复调用。
- 任务返回原来的错误，而不是把失败改成成功。
- 输出接收方收到关闭，并在两秒内完成。

运行：

```bash
cargo test --manifest-path code/Cargo.toml -p flow-rs --test derive_node --locked
```

这个测试证明普通 `Result::Err` 路径的收尾，不证明 panic、任务强制取消时
也会执行异步 finalize。后两者需要独立的取消/监督机制，不能把这里的修复夸大。

## 5. 属性、辅助属性与卫生性

`#[inputs]` 和 `#[methods]` 是主动执行的属性过程宏；
`#[state]` 是 `#[derive(BuildFromPorts)]` 用 `attributes(state)` 声明的辅助属性，
它本身不生成代码，只让 derive 在字段上识别“这个字段走 Default，不从 TOML 取值”。
宏生成时应保留不属于自己的属性，例如 `#[cfg(...)]`、文档注释和其他 derive。
本项目的 #[state] 只能说明构造策略，不代表运行时自动初始化业务资源。

过程宏的名字会受到调用处作用域影响。当前生成器使用裸名 `Receiver`、`Actor`、
`Error` 等，调用者需要从 `flow_rs::prelude::*` 导入它们。换一个同名类型可能
导致错误绑定，这是现存限制，不是 Rust 自动替我们解决的事。

稳健设计通常采用明确路径、统一的运行时重导出和可配置 crate 路径。
`extern crate self as flow_rs` 只解决 flow-rs 在自身内部引用自己的情况；
不能解决 Cargo 中将依赖重命名为 `engine` 的情况。
`proc-macro-crate` 是处理 Cargo 依赖改名的一种工具，但当前工程尚未使用它，
不能仅添加依赖就宣称支持改名，还需要只有改名依赖的独立下游编译测试。

宏也无法从语法树中解析 Rust 类型别名的最终指向。
当前实现已把 contains 替换为 `Type::Path` 和容器泛型实参检查，精确识别四种
已有端口形态。回归测试 `node_close_does_not_erase_business_type_containing_sender`
证明 `Option<HistorySender>` 不会因名字含 Sender 被错误清空。它仍不能辨认
类型别名或与引擎类型同名的第三方类型，后续完整端口协议需要明确的类型信息。

## 6. 展开检查与维护顺序

可选工具 [cargo-expand](https://github.com/dtolnay/cargo-expand) 能打印宏展开结果。
它是 Cargo 子命令，不是业务依赖。已安装时在 `code/` 执行：

```bash
cargo expand -p flow-rs --test derive_node
```

如果提示没有该子命令，可按其官方说明安装，或先用 Ch2.2a 的普通程序打印生成器结果。
不要把 `cargo expand` 输出重新编译一次当成全部证据；文本会丢失部分卫生性信息，
最终证据仍是原始调用文件在真实编译器中通过以及行为测试通过。

每次修改宏按顺序做：写一个能暴露问题的最小调用文件；分清解析还是生成问题；
修改一个生成器；运行对应成功/失败用例；验证涉及的运行行为；更新书中片段。
如果给端口宏加数组语法，至少同时检查零端口、单端口、多端口、尾逗号、重复名，
以及生成字段顺序与注册表接线顺序。能解释这些用例为什么必要，才算具备维护能力。
