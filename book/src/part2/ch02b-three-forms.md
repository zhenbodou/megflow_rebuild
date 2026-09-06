# Ch2.2b 独立小工程：亲手实现三种过程宏

本章让你在尚未实现 MegFlow 引擎时，就能完整练习过程宏。
实验代码位于 `code/macro-labs/`，是独立 workspace，不依赖 `flow-rs`。
按三步制作；每一步有单独的可运行检查点。

## 1. 建两个 crate，理解依赖方向

从你准备学习的空目录开始：

```bash
mkdir my-macro-lab
cd my-macro-lab
cargo new --vcs none --lib derive --name macro-lab-derive
cargo new --vcs none --bin app --name macro-lab-app
```

将根目录 `Cargo.toml` 新建为：

```toml
{{#include ../../../code/macro-labs/Cargo.toml}}
```

把 `derive/Cargo.toml` 完整替换为：

```toml
{{#include ../../../code/macro-labs/derive/Cargo.toml}}
```

把 `app/Cargo.toml` 完整替换为：

```toml
{{#include ../../../code/macro-labs/app/Cargo.toml}}
```

`app → derive → syn/quote/proc-macro2` 是依赖方向。
宏 crate 不能再正常依赖 app，否则形成环。生成代码在 app 中编译，
所以它可引用 app 的类型，但这不要求宏 crate 在自身编译时认识那些类型。
例如 `quote!(impl Node {})` 对宏 crate 来说是数据，不是立即编译的 impl。

`[lib] proc-macro = true` 将 derive 设为编译器加载的过程宏库。
包名中的连字符在 `use` 中写为下划线：`macro-lab-derive` 对应 `macro_lab_derive`。
过程宏入口定义在库根；解析、生成的辅助逻辑可以拆到普通私有模块。
这些边界见 [Rust Reference 的过程宏说明](https://doc.rust-lang.org/reference/procedural-macros.html)。

## 2. 第一步：derive 追加方法

删除 `derive/src/lib.rs` 默认的 add 函数与测试，文件开头写：

```rust,ignore
use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput};
```

随后添加完整的第一个宏：

```rust,ignore
{{#include ../../../code/macro-labs/derive/src/lib.rs:derive}}
```

把 `app/src/main.rs` 完整替换为：

```rust,ignore
{{#include ../../../code/macro-labs/stages/01-main.rs}}
```

执行 `cargo run -p macro-lab-app`，预期为 `第 1 步：derive 通过`。
若先写 app、暂不写宏，导入/派生失败是这一步预期的“红”。

宏收到的是 `struct Node<T> { value: T }` 的语法。
它输出 `impl<T> Node<T> { ... }`；原结构体由编译器保留。
**不要在 derive 结果里再返回 `#input`**，否则同一个结构体会被定义两遍。
`type_name()` 返回声明名称，不是 `std::any::type_name::<Node<u32>>()` 那种完整类型路径。

## 3. 第二步：attribute 改写字段

现在让宏给结构体增加 `label: &'static str` 字段。把 lib.rs 的 syn 导入改为：

```rust,ignore
use syn::{parse_macro_input, parse_quote, DeriveInput, ItemStruct};
```

保留第一步的宏，在同一文件末尾添加：

```rust,ignore
{{#include ../../../code/macro-labs/derive/src/lib.rs:attribute}}
```

完整替换 app 文件：

```rust,ignore
{{#include ../../../code/macro-labs/stages/02-main.rs}}
```

执行 `cargo run -p macro-lab-app`，预期打印 `第 2 步：attribute 通过` 及结构体内容。
本章只给类型增加字段；字段实际取值由 `Node { ..., label: "source" }` 的构造表达式提供。

属性宏接收两个 token 流：args 是 `#[with_label(...)]` 括号内的参数，
item 是结构体定义。本宏没有参数，所以非空 args 应报告错误，不能偷偷忽略。
与 derive 不同，属性宏返回值**替换**原项：必须 `quote!(#item)` 返回改写后的结构体。
返回空 token 会把结构体删除。

代码顺序为 `#[with_label]`、`#[derive(TypeName, Debug)]`、结构体。
with_label 先接到包含 derive 属性的 item，保留它们并补字段，然后编译器继续
处理 derive。`Debug` 因此能打印新增的 label。这与 MegFlow 中
`inputs → outputs → derive(Node, Actor, BuildFromPorts)` 的协作类似。

## 4. 第三步：函数式过程宏生成表达式

在 lib.rs 顶部补入 `use syn::punctuated::Punctuated;`，并在 syn 导入中补 `Ident, Token`。
然后添加：

```rust,ignore
{{#include ../../../code/macro-labs/derive/src/lib.rs:function}}
```

现在 app 的最终完整版本如下：

```rust,ignore
{{#include ../../../code/macro-labs/app/src/main.rs}}
```

`port_names!(inp, out,)` 的输入就是括号内 token，输出是
`&["inp", "out"]`。它是在表达式位置被调用，因此必须产生合法表达式。
MegFlow 的 `node_register!` 也属于函数式过程宏，但它在模块级位置调用、生成注册项。
**同样写 `name!(...)`，既可能是 macro_rules，也可能是函数式过程宏，不能只看感叹号判断。**

运行 `cargo run -p macro-lab-app`，应输出 `三种过程宏通过`，并显示节点与两个端口。
空调用也有断言；空数组借用的元素类型由左侧 `&[&str]` 给定。

## 5. 泛型必须拆成三个位置

对于 `struct Buffer<'a, T = String, const N: usize = 2> where T: AsRef<str> { ... }`：

| 位置 | split_for_impl 的结果概念 | 原因 |
| --- | --- | --- |
| `impl` 后 | `<'a, T, const N: usize>` | 声明实现的参数，不能带默认值 |
| `Buffer` 后 | `<'a, T, N>` | 给类型传参，不再声明 const |
| 类型之后 | `where T: AsRef<str>` | 保留原类型的约束 |

因此 `impl #ig #name #tg #wc` 中每块都不能漏。
`split_for_impl` 不会猜测并自动添加 `Send`、`Clone` 等业务约束；
生成 Actor 时仍需要节点实际满足 `Send + 'static`，错误应由相关测试覆盖。

主工程的 `code/flow-derive/tests/derive_type_name.rs` 已包含生命周期、默认类型、
const 泛型和 where 的下游编译测试。注意关联函数调用不总能推断默认泛型，
测试用 `type DefaultGeneric = Generic<'static>;` 先形成具体类型，再调用方法。

## 6. 用检查点排除“漏抄一段”的问题

在本书项目根目录执行，输出目录必须尚不存在：

```bash
python3 scripts/macro_checkpoint.py --stage 1 --out /tmp/megflow-macro-step1
cd /tmp/megflow-macro-step1
cargo run -p macro-lab-app --locked
```

第二、三步回到本书项目根目录，将 `--stage` 与新目录改为 2/3 即可。
脚本从上面的同一份源码抽取对应阶段，生成独立工作区并复制锁文件；
不读取 `flow-rs`，不依赖后续课程。导出版本可能有未使用导入警告，不影响运行。
它用于核对手写结果，正常学习仍建议自己逐步修改。

最终参考实现可直接执行：

```bash
cargo run --manifest-path code/macro-labs/Cargo.toml -p macro-lab-app --locked
```

练习：给 with_label 加一个同名字段，检查错误指向原字段；把 Node 改为元组结构体，
理解为什么宏拒绝；把 port_names 的一个名字换成字符串，观察 Ident 解析失败。
写下错误发生在“解析”“宏校验”“生成后类型检查”中的哪一层，再进入真实节点宏。
