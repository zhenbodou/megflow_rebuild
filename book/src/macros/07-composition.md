# 第 7 课：路径、属性组合与 AST 改写

复杂宏的问题往往出现在“调用者换了写法”之后。本课学习如何让生成代码在不同模块、依赖名字和条件编译配置下保持正确。

## 1. 过程宏并不知道你的类型到底是什么

`syn::Type::Path` 表示一段类型路径。它能告诉你写的是 `Vec<Receiver>`，不能告诉你 `type Input = Receiver` 中 Input 最终等于哪个类型，也不能判定同名 Receiver 来自哪个库。

因此不能用 `ty.to_token_stream().to_string().contains("Sender")` 识别端口：`HistorySender` 也会中招。即使用 AST 精确匹配最后一个名字，也只能验证语法形状。更可靠的接口让端口属性记录明确角色，或让生成代码调用 trait，由编译器检查类型能力。

练习：比较 `Receiver`、`my::Receiver`、`Input` 三个 AST。参考答案：前两者是不同长度的路径，第三个只是单段路径；解析器无法仅靠这些 token 证明它们类型相同。

## 2. 生成路径要能经受依赖改名

假设用户在 Cargo.toml 写 `engine = { package = "flow-rs", path = "..." }`，生成代码仍写 `flow_rs::Context` 就可能找不到 crate。过程宏没有声明宏的 `$crate` 机制。

可以设计显式配置，例如 `#[node(crate = engine)]`。这里是**接口设计示例，当前 MegFlow 宏尚未实现这个参数**。解析值应是 syn::Path，输出时 `quote!(#runtime::Context)`，避免仅允许单个 Ident 而拒绝合法路径。

另一种方案是使用 [proc-macro-crate](https://docs.rs/proc-macro-crate/latest/proc_macro_crate/) 查询下游清单：处理 FoundCrate::Itself 与 FoundCrate::Name 两种结果；不要查询失败后悄悄猜名字。显式覆盖路径仍有价值，例如运行时经由门面库重导出时。

验证应建立只有 engine 依赖的独立下游工程，禁止原名依赖同时存在，否则可能掩盖失败。还要测试运行时 crate 内调用、外部调用以及生成代码需要的辅助项可见性。`extern crate self as flow_rs` 仅解决一部分内部路径问题。

## 3. 为什么要分运行时与 derive 两个 crate

过程宏 crate 是编译时运行的代码生成器；trait、消息类型与业务函数属于目标程序。常见依赖方向为：应用依赖运行时，运行时可重导出 derive，derive 只生成指向运行时的 token 而不反向依赖运行时。这样避免依赖环。

生成代码中的第三方依赖也需要规划。例如输出 `inventory::submit!` 会要求调用者能找到 inventory；输出运行时公开重导出的路径则由运行时维护该依赖。这不是把依赖隐藏后就无需维护：重导出的路径仍是宏与运行时之间的协议。

Describe 示例使用 `::std::...`，因此避免普通局部同名导入干扰，但明确要求 std。若要支持 no_std，应先决定返回值是否需要 alloc，并建立 no_std 下游测试；把路径机械替换成 core 并不能解决 Vec/String 的分配需求。

## 4. 属性宏替换原项，derive 追加新项

`#[with_label]` 必须返回修改后的整个结构体，漏掉 `quote!(#item)` 就等于删除定义。derive 则保留原定义、追加 impl；把原结构体再次输出会重复定义。

第 5 课将 with_label 放在 derive 之前，使生成字段参与后续 derive。设计多个主动属性宏时，要明确哪个先变换、哪个依赖变换结果，分别编译两种顺序，决定支持两者还是给出明确限制。辅助属性只是配置，不应按主动属性宏理解。

保留未消费的属性，包括文档、可见性、cfg 和其他 derive。尤其不要只从 `ident`、`fields` 重建结构体而丢掉 generics 与 where。

## 5. cfg 必须作用于一致的代码

设一个字段带 `#[cfg(feature = "metrics")]`。宏若生成访问该字段的代码，必须确保访问与字段在相同配置下存在，否则关闭 feature 后字段消失而访问残留。

不能假定不同宏入口总能看到相同的 cfg 处理阶段。为真实组合编写 feature 开/关的下游测试，检查宏实际收到的输入。如果宏负责复制 cfg，复制的是配置条件，不是盲目把所有字段属性贴到表达式上。条件字段、构造表达式、匹配模式、约束都可能需要同步处理。

本课程 Describe 测试尚未覆盖 feature 矩阵，不据此宣称它能处理所有属性组合。

## 6. 用 Visit / VisitMut / Fold 改 AST

只读取 AST 用 `syn::visit::Visit`；原地修改用 `visit_mut::VisitMut`；消费旧树并返回新树用 `fold::Fold`，分别开启对应 feature。处理完整函数、impl 通常需要 syn 的 full feature。第 4 课的简单端口语法不用引入整树遍历。

例如要收集函数调用路径，可以重写 visit_expr_call：记录当前调用，然后调用 syn 提供的默认遍历继续走子表达式。忘记递归，嵌套调用就被漏掉。

不要仅按标识符文本把某个函数体里的所有 x 改名：局部变量遮蔽、模式绑定与宏调用内 token 都可能改变含义。AST 遍历不是类型解析器或完整重构工具。对需要语义解析的任务，调整宏输入设计通常比猜测更可靠。

## 自测

对每个生成名字标明来源：用户输入、宏内部局部变量、运行时公开路径、标准库路径。若不能解释它会绑定到哪里，先缩小例子；编译成功后再测依赖改名、同名导入和条件编译。这就是从“在我的例子里可用”走向可维护库的关键步骤。


---

[课程首页](00-roadmap.md) · [上一课](06-generics.md) · [下一课](08-engineering.md)
