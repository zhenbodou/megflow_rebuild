# Ch2.3b 从手写实现追踪宏展开

前置：完成手写节点实作，并学过宏专题中的 Token、syn、quote 与三种过程宏。
本课把两条线接起来：每看到一个宏，都能指认它替你生成的手写代码。

## 1. 先分工，再写宏

| 手写时重复的代码 | 由谁生成 | 为什么是这种宏 |
| --- | --- | --- |
| 输入/输出字段 | inputs、outputs 属性宏 | 需要修改原结构体，derive 不能加字段 |
| close、输入结束判断 | Node derive | 原类型保留，追加 trait impl |
| spawn、循环、收尾 | Actor derive | 追加调度接口实现 |
| 用户 exec 的错误包装、默认钩子 | methods 属性宏 | 需要读取并改写 impl 内的方法 |
| 接线与参数构造 | BuildFromPorts derive | 从结构体字段生成构造代码 |
| 注册条目 | node_register! | 在定义位置生成登记项 |

不要一开始同时实现六项。先让一个属性能生成正确字段，再写一个 Node impl，
然后才加入生命周期和构造器。每一步先写出预期 Rust 代码，再决定怎样用 quote 表达。

## 2. 用真实生成器看中间产物

运行：

```bash
cargo run --manifest-path code/Cargo.toml -p flow-derive --example node_expansion_walkthrough --locked
```

示例通过 `#[path = "../src/node.rs"]` 引入生成器模块，作为普通程序调用它。
这里不在编译器的过程宏入口里，所以使用 proc_macro2 的 token 类型；这正是将宏入口
与 expand 函数分离的好处。示例没有复制另一份生成器，修改实际实现后会看到新结果。

起点是一棵 `struct Doubler {}` 的语法树，再解析两个端口声明 inp: u32、out: String。

```rust,ignore
{{#include ../../../code/flow-derive/examples/node_expansion_walkthrough.rs:inject_fields}}
```

第一次展开生成输入字段和 input_closed；parse2 把 token 重新解析为 ItemStruct，交给
输出属性扩展。最终三个字段依次是 inp、input_closed、out。这个顺序可以直接断言，
但只断言顺序不能证明字段类型正确或构造器能编译。

操作题：把空结构体改成含 `value: u32` 的结构体，先预测新字段列表，再修改断言。
参考答案：原有 value 保留在前，之后追加 inp、input_closed、out。若已有 inp，宏应
给出字段冲突诊断，不能静默覆盖业务字段。

## 3. 追加 impl，不再重复输出结构体

```rust,ignore
{{#include ../../../code/flow-derive/examples/node_expansion_walkthrough.rs:generate_impls}}
```

DeriveInput 是 derive 可接收的结构体/枚举/联合体语法模型。它与 ItemStruct 不同，
因此这里重新 parse2 成 DeriveInput，再分别生成 impl。derive 的输出应是新增实现；
如果再次输出完整结构体，会出现重复定义。

阅读终端输出时，按下面的清单找具体代码：

- Node：类型化 out 被 Default 替换，释放旧发送端；is_all_input_closed 读取控制字段。
- Actor：`self: Box<Self>` 把节点移入任务；内层 async 保存 exec 的错误；外层 close 后 finalize。
- BuildFromPorts：INPUTS/OUTPUTS 名表来自字段；输入/输出队列从分组向量取出，into 转换为类型化端点。

本实验使用最终生成器，因此 Actor 带 Context 参数。上一课的独立最小节点还没有资源
机制。资源章节扩展签名后，最终节点的 initialize 必须接受 &Context，这不是宏自动
猜出来的参数，而是我们明确写在生成模板里的协议。

## 4. 如何阅读 quote 模板

以字段名 id 为例，`self.#id` 表示把 syn::Ident 放到点号之后，不是打印变量 id 的
运行时值。`#(#closes)*` 表示按 closes 集合重复输出多段语句，不是生成运行时 for 循环。
泛型要分别放在 impl 参数、目标类型参数和 where 子句位置，不能把整个泛型声明当
字符串复制三遍。相关基础见宏专题第 4、6 课。

如果看到长串输出不要急着读完：先找 impl 目标，再找方法签名，最后跟踪一个字段。
终端的空格不代表最终代码风格，TokenStream::to_string 是展示，不是格式化器。

## 5. 从“能解析”继续走到“能编译、行为一致”

本例将三个生成结果解析为 ItemImpl，只证明语法结构成立。它没有实际调用生成代码，
也没有定义所有运行时接口。下一步必须运行真实下游：

```bash
cargo test --manifest-path code/Cargo.toml -p flow-rs --test typed_node --locked
cargo test --manifest-path code/Cargo.toml -p flow-rs --test derive_node --locked
cargo test --manifest-path code/Cargo.toml -p flow-derive --test ui --locked
```

typed_node 检查宏生成的端口能被装配并运行，元信息与关闭行为保持；derive_node 检查
实际生命周期等行为；ui 检查非法输入的诊断。这三层不能互相代替。

## 独立练习与参考方向

1. 不看生成器，写出一个只有 out 的 Node::close，再对照终端展开。类型化输出应设为
   Default，Option 输出设为 None，数组 clear；它们都释放发送端，但 Rust 赋值方式不同。
2. 把 Actor 的问号移出内层 async，预测哪个行为测试失败。参考方向：exec 返回普通错误
   后的 finalize 事件，语法解析仍然可能成功。
3. 给输入属性加重复端口名，判断错误应该由 syn 语法解析还是领域规则检查发现。
   参考答案：两个合法标识符的列表语法正确，重复属于宏自己的规则，需主动检查。
4. 解释为什么把全部端口类型名转成字符串做 contains 不可靠。参考答案：业务类型
   HistorySender 也包含 Sender，而类型别名的语义无法通过字符串包含判断。

当你能从手写代码写出预期展开，再自己构造 syn/quote 实现并跑下游验证，才真正完成
“用宏消除重复代码”这一阶段。原版模板、字典和动态端口仍需要后续完整实现，不能
仅靠本课的标量例子宣称整个原版宏系统已经对齐。
