# 第 11 课：亲手解析 MegFlow 的端口语言

前面已经会用 syn 读取 Rust 结构体。这次我们自己设计解析规则，读懂原版
`#[outputs(out:{T0})]`。完成本课后，你应能解释每个符号，修改解析器支持一个新规则，
并判断失败发生在解析、代码生成还是运行时接线阶段。

本课代码是独立实验，放在 flow-derive 的 examples 中。它不会改变当前 inputs/outputs
宏的行为，也没有实现 DynPorts。最后输出的是待生成代码的 token，不会编译这些字段。
这些边界很重要：能打印一段代码，并不等于框架已经支持它。

## 1. 先用人话读声明

| 声明 | 端口形态 | 消息描述 | 原版输出字段形态 |
| --- | --- | --- | --- |
| `plain` | 单端口 | 任意消息 | `Sender` |
| `scalar:u32` | 单端口 | Rust 的 u32 | `SenderT<u32>` |
| `batch:[String]` | 列表 | Rust 的 String | `Vec<SenderT<String>>` |
| `routes:{T0}` | 字典 | 编号 0 的模板 | `HashMap<u64, Sender>` |
| `live:dyn T1` | 动态端口 | 编号 1 的模板 | `DynPorts<Sender>` |

冒号前是字段名；冒号后同时描述“怎样组织端点”和“传什么消息”。两个维度不能混在
一个布尔值里。列表和字典都可能有多个端点，但字典还要按地址查找；动态端口还涉及
运行期间的增减及任务管理。

`{T0}` 在这里不是 Rust 的普通类型。属性宏接收 token，由我们的解析器赋予花括号
“字典端口”的含义。`T0` 也不要求你声明 `struct T0`：它在这套语言中表示模板编号。
模板的约束必须传给后续类型推导，不能看到字段生成了 Sender 就把模板信息丢掉。

本课对照父目录原版 `flow-derive/src/ports.rs` 的有效声明规则。为方便逐步理解，实验
只生成输出字段；输入把 Sender 换为 Receiver。原版的 PortInfo 元数据生成及完整
运行时装配仍需继续实现。现有重构中的 `out[]` 是早期实验语法，不能据此宣称不再
需要原版的列表、字典和模板协议。

## 2. 先定义解析后的数据

`Port` 保存三个独立字段：name 是 Ident；shape 是四选一的枚举；message 则区分 Any、
Template 和真正的 Rust Type。这样后续代码可以用 match 穷举情况。

需要三个 crate，当前 flow-derive 已声明它们，无需新加依赖：

- syn：`Parse` 是解析约定，`ParseStream` 是读取 token 的游标，`Type` 保存 Rust 类型语法树。
- proc-macro2：提供可以在普通 example 程序中使用的 TokenStream。
- quote：将我们选好的字段名、端点类型和容器类型插入输出 token。

这里使用项目锁定的 syn 2 API，`parse_terminated(Port::parse, Token![,])` 的第二个参数
指定分隔符。不要直接照抄原版旧 syn API 的参数数量。

## 3. 按分支推进游标

解析顺序如下，写代码时也按这个顺序完成：

1. 读取字段名。若后面没有冒号，就返回 Unit + Any。
2. 消耗冒号。遇到花括号就创建 Dict，方括号创建 List，dyn 创建 Dynamic，否则是 Unit。
3. 在对应范围内读取消息类型。空范围表示 Any；单独的 T 加数字表示模板；其他交给 syn::Type。
4. 列表和字典的括号内部必须读完，不能留下多余 token。
5. 外层用 Punctuated 连续读取端口，支持逗号分隔和末尾逗号。

`braced!(content in input)` 会把花括号内部交给一个单独的游标。解析内部 u32 后，必须
检查 content 是否为空，否则 `{u32, String}` 可能只读到 u32 就被错误接受。

不要用字符串 split(',')：`Result<u32, String>` 的逗号属于类型内部。syn::Type 能
识别它，外层解析器随后才会读到端口间的逗号。

`?` 表示立即向调用者返回解析错误。原版部分解析路径有回退行为，本实验选择对畸形
声明明确报错；这是诊断策略的差异，并非声称逐个错误输入都与原版完全相同。

## 4. 完整代码与运行

先自己写枚举和 Port，再完成 Parse，最后写 output_field。遇到困难时对照完整代码：

```rust,ignore
{{#include ../../../code/flow-derive/examples/port_grammar.rs}}
```

在仓库根目录执行：

```sh
cargo run --manifest-path code/Cargo.toml -p flow-derive --example port_grammar --locked
```

程序打印五个字段，最后打印“端口形态、模板编号、嵌套类型与非法输入验证通过”。
quote 输出中的空格不影响 token 的意义。观察 routes 的值类型是 Sender，batch 的
元素类型是 `SenderT<String>`：前者通过模板元数据参与推导，后者有具体 Rust 类型。

输出函数分两步：先按 message 选择端点，再按 shape 套容器。这样不用为四种形态乘
三种消息描述手写十二份重复代码。`#name` 插入 Ident，`#endpoint` 插入已经生成的
TokenStream；它们不是运行时字符串拼接。

## 5. 让错误帮助你理解边界

实验已经检查四种错误：字典内多个类型、将定长数组写成列表、声明后多出单词、泛型
没有闭合。自行把其中一个传给 `syn::parse_str::<Ports>(...).err().unwrap()` 并打印，
观察错误信息，再解释是哪一层游标发现了问题。

随后独立完成三项练习：

1. 添加 `out:{}`，断言它是 Dict + Any；添加 `out:{T12}`，断言模板编号是 12。
2. 添加 `out:{std::string::String}`，解释为什么它是 Rust 类型而不是模板。
3. 给 output_field 增加输入方向参数，生成 Receiver/ReceiverT，并写断言检查两种方向。

参考判断：空内容在 message 函数的第一行返回 Any；模板只识别不带泛型参数的单段
路径；输入方向只改变端点类型，不能改变字典的 u64 键或模板编号。

## 6. 接到框架还缺什么

下一步需要将解析结果同时用于字段和 PortInfo 生成，让构造器接收标签并按标签建立
HashMap，随后连接 TOML 的 `node:port:tag`、类型推导和子图端口转写。Demux 才能
通过 `to_addr` 找到真正的下游。动态端口还需要专门的运行时协议。

回到 [Demux 路由实验](../part4/ch07-demux.md)，现在你应能指出两个实验的连接点：
宏生成的 HashMap 字段必须由 Builder 填入地址和 Sender，route 才有可查询的数据。
完成这条链路才算支持字典端口；本课的解析器和打印结果只是其中可单独验证的一步。

## 7. 把模板编号带过两个宏阶段

现在回到真实的 `flow-derive/src/node.rs`，而不是前面的独立 example。字典端口装配
已在 [Ch4.7b](../part4/ch07b-dictionary-ports.md) 接通；接下来要解决一个更隐蔽的问题：
属性宏读到 T0 后生成 Sender，后面的派生宏怎么知道它原来是 T0？

只看字段类型已经无法恢复信息。Any 和 T0 都生成未类型化端点，但前者没有模板关系，
后者必须与同一节点实例上的其他 T0 端口关联。正确做法是保留一份编译阶段的元数据。

### 第一步：精确识别模板语法

新增 template_index，输入 syn::Type，输出 `Option<usize>`。只接受没有前导冒号、
没有限定类型、没有泛型实参的单段路径；标识符去掉开头 T 后必须能解析成 usize。
所以 T0 和 T12 是模板，`module::T0`、`T0<u32>` 和 Thing 都不是。

这里的 Option 表示“它是否使用模板语法”，不是“推导是否成功”。宏现在只解析声明，
还没有看到 TOML，更不知道用户将这个节点实例接到哪条通道。

### 第二步：生成字段时保留辅助属性

生成具体 Rust 端点类型前，排除识别出的模板类型。模板使用 Receiver/Sender；字典
模板使用 HashMap，值也是未类型化端点。随后 port_field 给字段附加：

```rust,ignore
#[port_template(0)]
out: std::collections::HashMap<u64, flow_rs::channel::Sender>
```

这不是运行时字段，不会为每个消息额外分配内存。它是给后续派生宏读取的辅助属性。
在 lib.rs 的 derive 声明中登记 `attributes(port_template)`；BuildFromPorts 同时
保留已有的 state。否则编译器不认识这个属性，宏生成的代码仍会报错。

### 第三步：从辅助属性生成类型描述

field_message_type 先查 port_template。有则用 syn::LitInt 解析编号，生成
`MsgTypeId::Template(index)`；没有才从字段类型生成 Rust TypeId 或 Any。
重复属性必须报错，不能选择第一个就忽略第二个。错误通过 to_compile_error 返回，
避免让宏自身 panic，读者才能在声明位置看到可定位的诊断。

同一个编号可以出现在多个端口上。这里的重复检查只针对“同一个字段写了两次辅助
属性”，不禁止输入 inp:T0 和输出 out:{T0} 共享模板编号。

### 第四步：验证“Rust 类型消失，框架编号保留”

完整测试如下。故意不声明 Rust 的 T0、T12；如果字段仍生成 `ReceiverT<T0>`，编译就会
失败。显式字段类型断言检查生成的是未类型化端点，注册表断言检查编号没有丢失。

```rust,ignore
{{#include ../../../code/flow-rs/tests/template_metadata.rs}}
```

运行：

```sh
cargo test --manifest-path code/Cargo.toml -p flow-rs --test template_metadata --locked
```

独立练习：把 other 的 T12 改为 T0，先预测 input_types 的返回值再运行。随后把两个
out/ inp 的编号改成不同编号，解释它们为什么不再声明同一组关系。不要为了编译通过
去声明一个空 struct T0，那会混淆宏语言和 Rust 类型系统。

### 原版推导接下来要做什么

父目录 `flow-rs/src/config/postprocess/type_infer.rs` 的 associated_ports 会查找
同一节点中的同编号端口；infer_conn 递归查看相关连接，选择通道类型并将结果传播到
关联端口。编号不是全局编号：两个独立节点实例的 T0 不能仅因数字相同就合并。

还要区分原版的两层行为：底层 ChannelStorage::guess 对全模板端点返回
TemplateInferFault；上层 infer_graph 对该错误有回退到 Any 的处理。不能仅照搬
底层错误就断言原版所有纯模板图都失败，也不能把所有模板一开始都改成 Any。

实际宏已能保留编号；[Ch3.2b](../part3/ch02b-template-inference.md) 已接入静态压平图的
跨连接推导，替换了此前 Builder 的临时拒绝。模板元数据测试现在检查全模板图的 Any
回退。端点转换装配已在该节继续接入，原版完整图作用域仍需补齐。
