# Ch2.2 第十六步：只生成刚刚手写的 impl Node

沿用第十五步的工程。先只替换一个东西：让派生宏生成 `impl Node for Doubler`，业务处理和 Actor::start 仍手写。端口属性、自动注册、Context 和动态端口都不是本步前提。

## 1. 宏要生成的究竟是什么

上一章 Node 的核心实现只有两件事：关闭 output，返回 input_closed。本步限定输入结构体具有这两个同名字段；后面再扩展为可声明端口的通用宏。先把这条窄规则验证清楚，不用一次处理数组、字典和动态端口。

事件列表是教学观测，不属于所有节点的接口。因此把 `record("close")` 从 Node::close 移到手写 Actor::start 调用 close 的前一行；宏只负责真正的端口关闭，不要求用户结构体都有 record 方法。之前的生命周期测试继续保留。

## 2. 创建过程宏 crate

过程宏在编译时执行，因此单独放进 derive 子目录。编译器先构建它，再用它处理 flow-rs 源文件中的派生请求。它不依赖 flow-rs，不会形成“运行时依赖宏、宏又依赖运行时”的 Cargo 循环；生成代码里的 flow_rs 路径由使用宏的工程解析。

**替换根 Cargo.toml，新增 derive/Cargo.toml 和 derive/src/lib.rs**。根目录仍是你一路写来的 flow-rs，不要建立第二份运行时。

完整 **Cargo.toml**：

```toml
{{#include ../../labs/node-steps/16/Cargo.toml}}
```

workspace.members 将 derive 纳入同一工作区；dependencies 中的本地路径让运行时库可以使用派生宏。message 仍保留原有路径依赖。

完整 **derive/Cargo.toml**：

```toml
{{#include ../../labs/node-steps/16/derive-Cargo.toml}}
```

`proc-macro = true` 告诉编译器这是过程宏库。三个依赖的职责不同：syn 2 解析输入 token，full feature 支持本步单测使用的 ItemImpl 语法树；quote 把模板和语法树片段拼成输出 token；proc-macro2 提供可以在普通单测里操作的 TokenStream。编译器提供的 proc_macro 不需要写到 dependencies。

完整 **derive/src/lib.rs**：

```rust
{{#include ../../labs/node-steps/16/derive-lib.rs}}
```

## 3. 从入口走到生成结果

derive_node 接收编译器的 TokenStream，parse_macro_input! 将它变为 DeriveInput；语法错误会生成编译诊断。expand_node 是普通辅助函数，因此单元测试可以直接调用它，不必每次都启动另一个 Rust 编译器。

依次阅读两个 match：先要求输入是 struct，再要求它有具名字段。然后用迭代器检查 output 和 input_closed 是否存在。这里检查字段名，不假装能从语法树得知任意类型别名的最终类型；生成方法是否能调用、返回值是否为 bool，仍由 Rust 类型检查负责。

new_spanned 将诊断关联到输入语法。失败通过 into_compile_error 变成编译错误 token，不让宏内部直接 panic。当前诊断定位到整个声明，后面更复杂的端口语法再提高定位精度。

成功时取出结构体名，split_for_impl 分开 impl 泛型、类型泛型和 where 条件。quote! 中的 `#name` 是插值，不是运行时变量：生成代码时插入对应 token。输出只是追加一个 impl，派生宏不会删除原结构体。

两个测试分别验证缺字段的诊断、泛型和 where 条件保留。语法树测试只能证明输出的形状；下一节会在真实节点上编译运行，验证行为。

## 4. 用宏替换手写实现

**替换 src/lib.rs 和 src/node.rs**，其他运行时源文件、配置、通道、测试和消息库全部不变。

完整 **src/lib.rs**：

```rust
{{#include ../../labs/node-steps/16/lib.rs}}
```

新增的 `extern crate self as flow_rs;` 给当前 crate 一个自引用名称。生成代码写的是 `::flow_rs::node::Node`；在库内部使用宏时，这个别名也让同一路径成立。这不是下载新 crate，也没有创建第二个运行时。

完整 **src/node.rs**：

```rust
{{#include ../../labs/node-steps/16/node.rs}}
```

对照上一章：Doubler 增加 derive，手写的 impl Node 删除，Actor::start 仍存在；业务 exec 没有改成新的“宏专用写法”。绝不能同时保留手写 impl Node，否则两份实现冲突。

## 5. 运行、排错与独立练习

第一次加入依赖后，在 flow-rs 目录运行：

```bash
cargo test --workspace
```

预期宏 crate 的 2 项单元测试、原来 3 项节点测试和全部 Part 1 测试通过；之后可以加 --offline。维护脚本 `python3 scripts/check_basic_channel_course.py` 会从第一步累计构建到本步，并且包含整个 workspace，避免漏跑宏 crate 测试。

排错实验：把 Doubler.input_closed 字段名改掉，编译应报告 `Node requires field input_closed`；恢复后通过。再暂时保留一份手写 impl Node，观察重复实现错误，理解派生宏生成的也是普通 Rust 实现。

独立练习：用不同结构体名但相同两个字段派生 Node，先检查 close 后输出端点的克隆也观察到关闭，再检查输入标志。不要给宏添加业务乘二逻辑；宏生成的是结构规则，业务留在节点里。

本步只覆盖两个固定字段。下一章的 inputs、outputs、methods 和 Actor 生成仍需分别扩展并通过同一累计工程验证，不能直接复制带 Context 和动态端口的最终过程宏文件。
