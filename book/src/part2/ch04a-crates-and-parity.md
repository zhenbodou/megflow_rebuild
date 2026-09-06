# Ch2.4a 注册实操、crate 协作与原版宏清单

本章收拢 Part 2：先用可运行的 inventory 实验理解注册，再把所有相关 crate
放回它们的编译和运行阶段，最后逐项核对原版宏。前面的学习实验不等于引擎功能已经对齐。

## 1. inventory 中登记的是什么

项目的 `NodeRegistration` 保存名称、端口名表、数组标记和构造函数指针。
登记的是“如何构造节点”，不是已经启动的节点对象。真正调用构造器发生在图装配时。
否则宏展开期间就去打开模型文件或启动异步任务，会混淆编译环境与业务运行环境。

先忽略 Actor 和 channel，运行这个完整例子：

```rust,ignore
{{#include ../../../code/flow-rs/examples/registry_basics.rs}}
```

项目根目录命令：

```bash
cargo run --manifest-path code/Cargo.toml -p flow-rs --example registry_basics --locked
```

预期结果为 `[("double", 6), ("increment", 4)]`。
`run: fn(i32) -> i32` 是函数指针，不是调用结果。`submit!` 里保存它，
之后 `(p.run)(3)` 才执行计算。两个模块无需互相导入，却登记到同一个 Plugin 表。

`collect!` 声明哪种类型可收集，必须与该类型在同一 crate；`submit!`
在模块级生成登记项；`iter::<T>` 在运行时枚举。枚举次序没有保证，因此示例先排序。
这些接口约定见 [inventory 官方文档](https://docs.rs/inventory/latest/inventory/)。

### 不要把 inventory 讲成完全没有运行时机制

静态条目与初始化入口由宏生成并随程序链接，但当前 inventory 0.3.24 的实现
通过平台初始化机制登记条目；它不是简单在链接时生成一个数组、随后遍历 section。
可以在本地该 crate 的 `src/lib.rs` 查 `__do_submit`、`__ctor` 和 `Registry`。
因此不能未经测量就宣称比 lazy_static“零运行时开销”。
同样，模块中 submit 的先后顺序不是“同名后注册覆盖前注册”的约定。
引擎需要明确处理重名注册；当前 find 取第一个匹配项，还没有给出完整的重名校验。

## 2. 把实验接回 node_register!

按以下顺序读当前源文件，每次只跟一个数据流：

1. `flow-derive/src/node.rs` 的 `NodeRegisterArgs`：将 `"Doubler", Doubler` 解析为 LitStr 和 Path。
2. `expand_node_register`：把名字插进 `NodeRegistration.name`，把类型插进 `<Doubler as BuildFromPorts>::build`。
3. `flow-rs/src/registry.rs` 的 `NodeRegistration` 与 `inventory::collect!`：定义条目数据。
4. `find("Doubler")`：枚举匹配；返回的是构造说明，而不是节点。
5. `flow-rs/tests/register.rs`：给构造器传 Args 和端口组，启动节点，检查三个输出及退出。

`<#ty as Trait>::method` 是完全限定语法，明确选择某个 trait 的关联项。
生成时插入类型路径，运行时通过函数指针调用；这两步不要混为一个“宏会创建节点”。

运行真实节点注册测试：

```bash
cargo test --manifest-path code/Cargo.toml -p flow-rs --test register --locked
```

resource_register 采用同一个参数解析器，但登记 ResourceRegistration，
构造器是 `build_arc::<T>`，没有输入/输出表。对应业务测试在 Part 4 的 resource_e2e。

## 3. 涉及的 crate，在哪个阶段起作用

| crate/工具 | 当前采用的用途 | 必须掌握的接口或约束 |
| --- | --- | --- |
| Rust 自带 `proc_macro` | 编译器宏入口 | TokenStream 输入/输出，宏执行上下文 |
| `proc-macro2` | 可测试的 token 表示 | TokenTree、Span、TokenStream、边界转换 |
| `syn` 2 | 解析和改写 Rust 语法 | Parse、Punctuated、DeriveInput、ItemStruct、ItemImpl、Error |
| `quote` | 生成 token | # 插值、重复、ToTokens、format_ident、quote_spanned |
| `trybuild` | 编译诊断回归测试 | compile_fail、.stderr、审核后更新预期 |
| `inventory` | 分散提交的类型化注册 | collect、submit、iter，无顺序保证 |
| `tokio` | 生成代码的任务执行 | spawn、JoinHandle；#[tokio::test] 建测试运行时 |
| `serde` | 构造器参数及配置反序列化 | Deserialize、derive feature、flatten/default 辅助属性 |
| `toml` | 配置格式与 Args 数据 | TOML 值转为类型；错误必须传回装配调用者 |
| `thiserror` | 引擎错误类型派生 | Error derive、error/from 属性；它不负责捕获 panic |
| `futures-util` | 节点中的异步组合 | join! 等生成/组合 future，不会自行创建线程 |
| `cargo-expand`（可选） | 看真实调用处展开 | 工具安装与 crate 依赖不同 |

不是每个 crate 都要加入过程宏库。比如生成结果里有 `tokio::spawn`，
要能在**调用者编译环境**找到 Tokio；只给 flow-derive 加 Tokio 没用。
本项目流向为 flow-rs 依赖 flow-derive，并在 prelude 重导出宏；
下游若直接使用 Tokio 的属性宏，也需在自己的 Cargo.toml 中声明依赖。

Serde 的 `Deserialize` 与同名 derive 分属类型/宏命名空间；
`#[serde(default)]` 是 derive 读取的配置，不会自己执行反序列化。
同理，Node trait 规定运行时方法，Node derive 只负责生成 impl，两者不能互相替代。

`ctor`、`lazy_static`、`proc-macro-crate` 不属于当前 flow-derive 的直接依赖；
需要讨论原版机制或完善改名支持时再引入具体实现与测试，不能在依赖表里写了名字就算教过。
原版还有 anyhow 和开启 `span-locations` 的 proc-macro2；它们服务于原版错误表示
及位置处理，不意味着重写必须照搬所有依赖，也不意味着可以删除相关行为。

## 4. 原版宏清单：不能遗漏哪些能力

下表来自本地 `../megflow/flow-derive/src/lib.rs` 的入口，固定参照版本见 Ch0.4。
“部分”表示名字相同但能力、语法或生命周期有差距，不是已完成兼容。

| 原版宏 | 原版源码模块 | 当前重写状态及必须补齐的内容 |
| --- | --- | --- |
| inputs / outputs | ports、lib | 部分：标量/数组教学语法；类型化、字典、动态端口与信息表待补 |
| Node derive | node | 部分：关闭与标志；动态接线、状态、空消息转发和统计待补 |
| Actor derive（含 local） | actor | 部分：spawn 与普通错误收尾；local、性能统计及空信号协议待补 |
| Parser derive | internal | 未实现原版内部声明解析派生 |
| node_register! | node/internal | 部分：inventory 构造器注册；原版注册接口仍需对照 |
| methods | methods | 部分：当前包装 exec；原版还处理参数适配、同步/异步、更新回调、validator/filter |
| opt_register! | pass | 未实现优化 pass 注册 |
| resource_register! | resource | 部分：资源构造注册，生命周期和原版接口待补 |
| submit! | internal | 未实现对应公共入口 |
| feature! | lib/internal | 未实现原版图特性功能 |
| atest / amain | lib | 未实现原版入口；当前测试用 Tokio 属性宏，不能说已兼容 |
| add_cvt_func | cvt_func | 未实现转换函数登记及调用链 |

本书自己的 TypeName、BuildFromPorts 是教学/实现辅助宏，不是用来填平上述缺项的替代名称。
原版 `methods` 会生成 Actor 实现，当前需额外 derive Actor；因此原版
`#[derive(Default, Node)] + #[methods]` 不能仅靠复制进当前项目就视为成功。

## 5. 从熟练使用到能独立维护的验收

完成本专题后，先独立做五个任务：

1. 不看模板，从空目录写出本章三种宏的实验，说明每个 crate 为什么在这个 Cargo.toml 中。
2. 对一种带生命周期、where 与 const 泛型的类型生成方法，并让下游真正编译通过。
3. 给一个非法端口语法添加带 span 的错误，再写 trybuild 用例证明不是其他编译错误。
4. 修改 Actor 生成代码时，说明每个 `?` 退出哪个 async 块，并验证错误路径 finalize 恰一次。
5. 从上表挑一种原版宏，列出所有可观察行为，再写成功、失败和业务测试；不能只移植宏入口函数名。

本专题已提供前四项的参考实现、命令和测试。第五项要随着动态端口、配置更新、
资源和优化器继续完成，仍在完整重构验收范围内。“精通”不是记住几个 API，
而是能用这些方法独立迁移并维护完整宏系统。
