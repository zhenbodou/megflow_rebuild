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

## 8. 把列表语法接入真实宏

字典完成后，再处理原版的 `inp:[T0]`、`out:[u32]`。不要把它理解成 Rust 的定长数组：
方括号在这里属于端口语言，表示一个名字下可以接多个端点。最终字段是 Vec。

| 声明 | 生成的输出字段类型 | 消息元数据 |
| --- | --- | --- |
| `out:[]` | `Vec<Sender>` | Any |
| `out:[u32]` | `Vec<SenderT<u32>>` | Rust(u32 的 TypeId) |
| `out:[T0]` | `Vec<Sender>`，另有模板辅助属性 | Template(0) |
| `out[]` | `Vec<Sender>` | Any，保留早期教学语法 |

### 第一步：在解析冒号后识别方括号

在 PortSpec::parse 中，读取冒号后检查 Bracket，使用 bracketed! 得到内部游标。
空内容表示没有具体消息类型，否则交给 syn::Type 解析一个类型；检查内部读完，
然后返回 array=true、dict=false。

为什么不能直接把整个 `[u32]` 当成 syn::Type？那样得到的是 Rust 的切片类型，无法
直接区分“列表里每个端点收 u32”和“一个端点收某种容器”。端口解析器需要先取出
自己的容器语法，再解析里面的 Rust 类型。

`[Result<u32, String>]` 内有一个完整类型，应当通过；`[u32; 4]` 是定长数组形式，
端口列表不支持这个长度；`[u32, String]` 有两个类型，也必须报错。不要只读 u32 就
忽略剩下的 token。

### 第二步：先判定列表，再选元素类型

字段生成先判断 spec.array。具体载荷选择 SenderT/ReceiverT，模板与 Any 选择
未类型化端点，再包一层 Vec。只有接下来才处理字典、普通端口。

顺序有意义：如果先看到 payload=u32 就生成 `SenderT<u32>`，列表形态会被丢掉。
模板编号仍通过 port_field 的 port_template 属性传递，不需要为 Vec 再造一套模板
编号规则。

### 第三步：派生宏识别类型化列表

port_kind 原来只认识 `Vec<Sender>` 和 `Vec<Receiver>`，现在也要认识元素为
`SenderT<T>/ReceiverT<T>` 的 Vec。类型描述函数先剥开 Vec，再读端点的载荷类型。
否则 BuildFromPorts 可能把它当成普通业务参数，从 TOML 中反序列化整个端口字段。

这一逻辑仍按语法结构匹配。`Vec<HistorySender>` 不是输出端口，`Vec<Option<Sender>>`
也不是本书支持的列表端口形态，不能因为名字中出现 Sender 就误判。

### 第四步：构造时逐元素转换

Builder 交给构造器的是未类型化端点。对于 `Vec<SenderT<u32>>`，不能直接把
`Vec<Sender>` 填进去；Rust 不会自动将一个容器的所有元素都执行 Into。

生成的普通列表构造表达式改为：

```rust,ignore
outs.remove(0).into_iter().map(Into::into).collect()
```

remove 移出整个端口组；into_iter 消费每个端点；Into 根据字段要求变成 SenderT；
collect 重新组成目标 Vec。未类型化列表也能用同一表达式，元素使用恒等转换。

如果节点同时含字典，生成的 build_tagged 则先取 `.endpoint`，再 `.into()`。
这条分支也必须更新，否则“只有列表的节点”能编译，“列表加字典的节点”却不能编译。

### 第五步：让内置节点也使用原版声明

Bcast 改为输入 `inp:T0`、输出 `out:[T0]`；Merge 改为输入 `inps:[T0]`、输出
`out:T0`。这与父目录对应节点的声明一致，并让输入输出的类型关联进入已实现的图
推导。节点的收发算法与端口语法是不同部分，本次没有借改宏重新定义广播或汇聚行为。

### 第六步：运行并解释验证

下面测试使用类型化列表输入与两个列表输出。必须证明消息到达两路、图能结束、
两个接收端都观察到关闭；只检查 Vec 长度并不能验证端点转换和生命周期。

```rust,ignore
{{#include ../../../code/flow-rs/tests/typed_list_ports.rs}}
```

运行：

```sh
cargo test --manifest-path code/Cargo.toml -p flow-rs --test typed_list_ports --locked
cargo test --manifest-path code/Cargo.toml -p flow-derive --locked
```

独立练习：将 TypedFanout 的 u32 改成 String，同时修改发送数据和断言；再把它改为
T0，改用 recv_any/send_any。先解释为什么后者不能从字段推断具体 Rust 载荷，再验证
它是否仍保持两路转发和关闭。

列表和字典都已进入真实宏，但动态端口仍需要 DynPorts 及相关运行时支持。第 3 节
独立实验打印出的动态字段不能作为动态框架已完成的证据。