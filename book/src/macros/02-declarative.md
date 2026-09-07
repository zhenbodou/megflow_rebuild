# 第 2 课：声明宏的递归、歧义与卫生性

上一课把宏看成有规则的代码模板。本课进一步把它看成一个小解析器：每次吃掉一段输入，将结果积累下来，直到输入为空。

## 1. 先设计没有歧义的语法

我们想写 `settings! { capacity = 8; workers = 2; }`，得到键值列表。规定每一项必须以分号结束。这样表达式可以含块、函数调用或加法，宏仍能确定它在哪里结束。

不要写 `$value:expr workers` 来区分下一项：`expr` 后允许的分隔符受到语言规则限制。也不要用 `($($name:ident)* $last:ident)` 猜最后一个名字，匹配器在当前 token 就无法决定它属于重复还是 last。增加 `;` 是语法设计，不是对编译器的妥协。

## 2. 一个可运行的 TT muncher

TT 是 token tree，muncher 可以理解成“逐口吃输入的解析器”。

```rust
{{#include ../../../code/flow-derive/examples/macro_rules_advanced.rs}}
```

运行：

```bash
cargo run --manifest-path code/Cargo.toml -p flow-derive --example macro_rules_advanced --locked
```

预期打印 `声明宏进阶：递归、单次求值、片段转发全部通过`。

逐步展开 settings：入口先创建 output，再调用 `@parse output; capacity = 8; workers = 2;`；第二条规则添加 capacity，把剩余输入递归传入；再次添加 workers；最后只剩 `@parse output;`，第一条规则结束。`@parse` 只是我们约定的内部标签，并不是 Rust 关键字。

入口接收任意 token，因此必须放在内部规则后面。否则内部调用也会被入口捕获，反复包装，最终触及递归限制。空输入的输出类型无法从元素推断，所以测试显式写 `Vec<(&str, i32)>`。

实际项目若语法完全规律，优先用 `$(...)*` 直接重复生成；这里用递归是为了学习解析状态。每步重新匹配越来越短的尾部可能使总工作量呈平方增长，长配置更适合 syn 解析器。

## 3. 为什么同一个 3 得到不同答案

`forward_expr!(3)` 已经把 3 解析为表达式，第二个宏看到的是不透明的表达式片段，不能重新用字面 token `3` 拆开匹配。它只能落入 `$value:expr`。`forward_tt!(3)` 保留 token tree，因此能匹配具体 token。

`ident`、`lifetime` 和 `tt` 是可按字面 token 再匹配的特殊情况。不要为了能够拆开输入而把所有参数改成 tt：这样也会失去 Rust 内建的表达式、类型解析能力。[规则依据：Rust Reference](https://doc.rust-lang.org/stable/reference/macros-by-example.html)。

嵌套重复也有结构：输入若按“组 → 字段”捕获，输出必须按相应层级取字段。一个重复块必须包含能决定重复次数的捕获变量，不能凭空写 `$(do_something();)*`。

## 4. 跨 crate 时的名字

局部变量和标签具有定义处卫生性，其他名字可能在调用处解析；所以“宏里所有名字都不会冲突”是错误理解。调用者提供的标识符可以有意把生成定义与调用处连接起来。

导出宏中调用自己的辅助函数，应写 `$crate::helper()`。`$crate` 指定义这个宏的 crate，即使下游把依赖改名也能定位；helper 跨 crate 必须公开。若 helper 不是用户 API，可用 `#[doc(hidden)] pub` 隐藏文档，但它仍是生成代码依赖的兼容性接口。

不要直接拼接标识符：写 `$prefix$name` 不会得到一个新名字。过程宏可用 `quote::format_ident!`；声明宏最好让用户明确传入目标名字。

## 5. Edition 是语法的一部分

本项目基础实验使用 edition 2021。宏定义所在的 edition 决定片段匹配规则。2024 的 `expr` 额外接受顶层 const 块与下划线表达式；需要保留旧匹配范围时有 `expr_2021`。2021 的 `pat` 可匹配顶层 or-pattern；`pat_param` 用于较窄的模式范围。升级 edition 后应检查规则优先级是否改变，不能只看 Cargo 文件是否能解析。

## 练习与答案

1. 去掉最后一个分号会怎样？当前语法无法匹配，应该报错；若要允许省略，必须另写末项规则并放在宽泛入口之前。
2. 为什么两个设置值一个整数一个字符串会失败？输出是同一个 Vec，元素类型要一致。宏没有改变集合类型规则。
3. 把递归实现改成单层重复。答案的匹配器为 `($($name:ident = $value:expr;)*)`，输出为 `vec![$((stringify!($name), $value)),*]`，同样按顺序、各求值一次。
4. 在 main 中增加 `let output = 99;`，宏调用后断言仍为 99。内部临时变量不会赋值到调用者这个局部变量。


---

[课程首页](00-roadmap.md) · [上一课](01-basics.md) · [下一课](03-procedural.md)
