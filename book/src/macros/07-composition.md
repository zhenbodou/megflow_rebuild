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

### 动手：分别编译字段存在与不存在的程序

从第 6 课已完成的 `code/macro-labs` 工程继续。派生实现保持不变，将 `app/Cargo.toml` 更新为以下完整内容：

```toml
{{#include ../../../code/macro-labs/app/Cargo.toml}}
```

`metrics = []` 声明本包可选的 feature，不启用额外依赖。未写 default 列表，所以普通构建默认不开启 metrics。接着新增 `app/tests/composition.rs`，完整内容如下：

```rust
{{#include ../../../code/macro-labs/app/tests/composition.rs}}
```

这里需要三个对应关系：结构体字段带条件、构造表达式的字段赋值带同一条件、预期输出按同一条件切换。cfg 会决定哪些代码参与编译；它不是运行时 if，也不会在同一个已编译程序中动态切换字段。

从教材仓库根目录运行两次：

```bash
cargo test --manifest-path code/macro-labs/Cargo.toml -p macro-lab-app --test composition --no-default-features --locked
cargo test --manifest-path code/macro-labs/Cargo.toml -p macro-lab-app --test composition --no-default-features --features metrics --locked
```

两次都应有一项测试通过。第一次 describe 返回 `value=7`，第二次返回 `value=7, samples=2`。这验证了本例中字段 cfg 与 Describe 的组合，不证明所有属性宏顺序或泛型条件约束都正确。

排错实验：只删除构造表达式中 samples 上方的 cfg，再运行关闭 feature 的命令，编译器应报告不存在 samples 字段；开启 feature 时仍能通过。恢复后再删除结构体字段上的 cfg，关闭 feature 的构造会缺少字段。两种失败说明字段定义与构造必须一起验证。

## 6. 用 Visit / VisitMut / Fold 改 AST

只读取 AST 用 `syn::visit::Visit`；原地修改用 `visit_mut::VisitMut`；消费旧树并返回新树用 `fold::Fold`，分别开启对应 feature。处理完整函数、impl 通常需要 syn 的 full feature。第 4 课的简单端口语法不用引入整树遍历。

例如要收集函数调用路径，可以重写 visit_expr_call：记录当前调用，然后调用 syn 提供的默认遍历继续走子表达式。忘记递归，嵌套调用就被漏掉。

不要仅按标识符文本把某个函数体里的所有 x 改名：局部变量遮蔽、模式绑定与宏调用内 token 都可能改变含义。AST 遍历不是类型解析器或完整重构工具。对需要语义解析的任务，调整宏输入设计通常比猜测更可靠。

### 从空目录写一个遍历程序

这个实验是独立的普通可执行程序，不是过程宏库，也不依赖 MegFlow。先创建目录：

```bash
mkdir -p ast-walk/src
cd ast-walk
```

将 `Cargo.toml` 写成以下完整内容：

```toml
{{#include ../../labs/ast-walk/Cargo.toml}}
```

syn 的 full 提供完整表达式语法；visit、visit-mut、fold 分别提供三套遍历 trait，这些 feature 不是相互替代的。quote 提供 ToTokens，把语法树转成 token，便于打印与核对。这里没有直接操作 proc_macro2 的类型，故不需要把它列为直接依赖；quote 与 syn 会通过依赖关系使用它。

`[workspace]` 使实验成为独立工作区，避免意外继承父目录的其他包。第一次构建准备依赖后保留生成的 Cargo.lock。此处只解析字符串中的 Rust 语法，不编译那个字符串里的函数，所以无需定义 outer、inner、hidden 或 secret。

`src/main.rs` 完整内容：

```rust
{{#include ../../labs/ast-walk/src/main.rs}}
```

### 逐段理解遍历为何能找到嵌套调用

`Calls(Vec<String>)` 是元组结构体，self.0 就是保存调用路径的列表。`Visit<'ast>` 借用语法树：方法拿到 `&'ast ExprCall`，读取路径并复制成字符串，不移动树里的表达式。

ExprCall.func 是装箱的 Expr。`&*expression.func` 先解引用 Box，再借用内部表达式，用 if let 判断它是否为路径调用。我们只收集这一类；方法调用 `value.run()` 是另一种节点 ExprMethodCall，不能由当前测试推断也会被收集。

调用默认的 `syn::visit::visit_expr_call(self, expression)` 后，遍历才会继续进入实参，发现 inner。这不是递归调用我们自己的同名方法，而是调用 syn 的遍历函数，让它访问子节点。若遗漏这行，就只记录 outer。

### 三种遍历接口的所有权差别

| 接口 | 参数 | 本例作用 |
| --- | --- | --- |
| Visit | 借用 `&Expr` | 收集 outer、inner，不修改输入 |
| VisitMut | 可变借用 `&mut Expr` | 将无后缀整数 1 改为 2 |
| Fold | 消费 Expr，返回 Expr | 将无后缀整数 2 改为 3，返回新树 |

ReplaceOne 中的 `*literal = ...` 替换被借用的字面量。构造新 LitInt 时保留原 span，维持源码定位。suffix 检查使规则只作用于无后缀整数；`1u32` 不在本实验改写范围内。base10_parse 失败时变为 None，不会误判为目标值。

ReplaceTwo 接管字面量所有权，必须在每个分支返回一个 LitInt。最后 `let expression = ...` 将消费旧树得到的新树重新绑定为同名变量，旧值已被移动，不能继续使用。

最容易误解的是 `hidden!(secret(1))`：syn 把它保存为宏调用及内部 token，默认遍历不会猜测这些 token 是否代表表达式，因此不会收集 secret，也不会把宏内部的 1 改成 2 或 3。宏可接受自己的 DSL；随意按 Rust 表达式解析它，可能破坏本来合法的输入。

### 运行、预期结果与练习

从 ast-walk 目录执行 `cargo run`，精确业务输出为：

```text
函数调用：outer, inner
字面量依次从 1 改为 2、3；宏内部 token 保持 1
```

程序同时断言两次改写后的 token 文本。这里比较文本仅用于观察这个固定表达式的变化，不证明生成代码类型正确或卫生性正确。若要用作真实宏变换，还需编译下游调用程序。

1. 删除默认递归调用再运行，第一个路径列表断言应失败。恢复后通过。
2. 将输入 inner(1) 改为 inner(1u32)，预测两个替换器都不会改它，再相应调整预期文本核对。
3. 增加 ExprMethodCall 的访问方法，收集方法名；用 `outer(value.run())` 验证，而不是期待 visit_expr_call 自动处理所有调用形态。

维护者从教材仓库根目录执行：

```bash
python3 scripts/check_macro_composition_course.py
```

脚本在新临时目录构建 AST 实验，并分别编译 Describe 的 metrics 关闭和开启版本。它使用离线依赖缓存，不复制 flow-rs 运行时。该检查已接入 CI；它仍只是第 7 课这两个实验的证据，依赖改名、no_std 和全部属性组合还需要各自的下游工程。

## 自测

对每个生成名字标明来源：用户输入、宏内部局部变量、运行时公开路径、标准库路径。若不能解释它会绑定到哪里，先缩小例子；编译成功后再测依赖改名、同名导入和条件编译。这就是从“在我的例子里可用”走向可维护库的关键步骤。


---

[课程首页](00-roadmap.md) · [上一课](06-generics.md) · [下一课](08-engineering.md)
