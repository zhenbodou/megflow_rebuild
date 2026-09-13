# 第 6 课：泛型、辅助属性与精确约束

本课将上一课“返回类型名称”的 derive 扩展为真实功能：把结构体的字段描述成 `载荷=7, source=inp`。这里最重要的不是字符串拼接，而是只要求实际参与描述的字段实现 Display。

## 1. 先写使用方式和手写结果

```rust,ignore
#[derive(Describe)]
struct Packet<T> {
    #[describe(rename = "载荷")]
    value: T,
    #[describe(skip)]
    cache: std::marker::PhantomData<T>,
}
```

我们期望生成 `impl<T> Packet<T> where T: core::fmt::Display`，其中 `describe(&self)` 借用 value 格式化。cache 被跳过，不读、不移动，也不为它添加约束。这个示例生成固有方法，避免在学习泛型时同时引入运行时 trait crate；正式库若需要统一 trait 接口，再按第 7 课拆包。

## 2. 辅助属性不是另一种属性过程宏

入口 `#[proc_macro_derive(Describe, attributes(describe))]` 声明 derive 能识别 `#[describe(...)]`。这个辅助属性提供配置，本身不会执行生成器。我们用 `parse_nested_meta` 处理 `skip` 与 `rename = "..."`，对未知键、重复键、错误值和互斥组合报错。

为什么不能默默忽略未知键？用户写成 `skpi` 后可能把不该输出的字段打印出来。宏的输入校验也是功能正确性的一部分。

## 3. 完整实现

该代码位于独立实验的 `derive/src/lib.rs`，不依赖 MegFlow：

```rust,ignore
{{#include ../../../code/macro-labs/derive/src/lib.rs:describe}}
```

分四次阅读，不必一次背完：

1. 入口只把编译器 TokenStream 变成 DeriveInput，交给普通函数。普通函数返回 syn::Result，便于单元测试；用户错误最后转换为 compile_error token。
2. 按字段读取属性。`Member::Named` 表示 `self.value`；`Member::Unnamed(Index)` 表示 `self.0`。这样具名、元组、单元结构体共用主体。枚举明确拒绝，因为枚举需要逐 variant match，不能套结构体访问代码。
3. 对未跳过的字段，向克隆后的 generics 添加 `字段类型: Display`。例如字段为 `Wrapper<T>`，添加的是 `Wrapper<T>: Display`，不是武断添加 `T: Display`。这让包装类型自己的实现决定真正约束。
4. `split_for_impl` 拆成 impl 参数、目标类型参数、where 子句，再生成 impl。模板中的 `&self.#member` 保证不移走字段。

`struct X<T = u32>` 的默认值属于定义，不能原样复制成 `impl<T = u32>`；而目标类型位置也只能写 `X<T>`。手工拼字符串很容易弄错，split_for_impl 就是为这个区分准备的。生命周期与 const 泛型也由它保留。参见 [syn Generics API](https://docs.rs/syn/2.0.119/syn/struct.Generics.html)。

## 4. 必须在下游真正编译

```rust,ignore
{{#include ../../../code/macro-labs/app/tests/describe.rs}}
```

运行：

```bash
cargo test --manifest-path code/macro-labs/Cargo.toml --locked
```

测试覆盖默认类型参数、生命周期、const 泛型、已有 where、重命名、跳过、不实现 Display 的类型，以及元组和单元结构体。生成器的两个单元测试另外检查多个错误合并及非法属性。

`Marker<NotDisplay>` 能调用 describe，才证明没有误加 `T: Display`。只对 u32 测试不能发现这个问题。这个示例仍使用 std、会分配字符串、不支持 enum，且用户若已有同名方法会遇到重复定义错误；这些都是具体接口边界。

## 5. Span 与约束错误

未知属性用 `meta.error` 指向属性位置；互斥选项指向字段；多个独立字段错误用 `combine` 合并。若生成语句导致 trait 错误难以定位，可用 `quote_spanned!` 将模板关联到字段类型的 span，再用 trybuild 确认诊断实际落点。Span 是位置与名字解析上下文，不是只改行号的装饰。

## 6. 从上一课继续：本章完整工程

本章保留第 5 课的 TypeName、with_label 和 port_names，新增 Describe 及其测试。前面的 describe 代码块是新增部分；下面给出完整文件，避免追加时漏掉导入、重复入口或把原来的宏删除。

```text
macro-labs/
├── Cargo.toml
├── Cargo.lock                  # 由 Cargo 生成并保留
├── derive/
│   ├── Cargo.toml
│   └── src/lib.rs
└── app/
    ├── Cargo.toml
    ├── src/main.rs
    └── tests/describe.rs
```

根 `Cargo.toml` 完整内容：

```toml
{{#include ../../../code/macro-labs/Cargo.toml}}
```

`derive/Cargo.toml` 完整内容：

```toml
{{#include ../../../code/macro-labs/derive/Cargo.toml}}
```

依赖沿用上一课：proc-macro2 在普通函数中表示 token；syn 的 full feature 解析完整结构体与字段；quote 把字段访问和 where 约束插入生成代码。本章没有新增运行时反射库。`proc-macro = true` 使这个包作为编译期代码生成器构建。

`app/Cargo.toml` 完整内容：

```toml
{{#include ../../../code/macro-labs/stages/04-app.toml}}
```

这个阶段还没有 metrics feature，也没有第 7 课的 composition 测试。应用只直接依赖宏库；生成的 describe 使用标准库，因此应用无需自行依赖 syn 或 quote。

`derive/src/lib.rs` 完整内容：

```rust
{{#include ../../../code/macro-labs/derive/src/lib.rs}}
```

按以下次序核对：文件顶部公共导入；三种已有宏；Describe 的编译器入口；接收 DeriveInput 的普通生成函数；最后是生成器单元测试。`#[cfg(test)]` 中的测试只在 cargo test 时编译，不会随应用运行。

`app/src/main.rs` 保留第 5 课的完整入口：

```rust
{{#include ../../../code/macro-labs/app/src/main.rs}}
```

`app/tests/describe.rs` 的完整内容已在第 4 节给出。tests 目录中的每个文件是独立测试 crate，必须自己导入 Describe；main.rs 的 use 不会自动传播到测试文件。main 验证三种旧宏，测试文件验证新增 derive，两者都要继续通过。

从自己的 macro-labs 根目录执行：

```bash
cargo run -p macro-lab-app
cargo test --workspace
```

预期 main 输出三种过程宏通过的信息；生成器的两个单元测试和应用的一个 Describe 测试全部通过。没有第 7 课的 feature 组合用例。第一次构建下载依赖后保留 Cargo.lock，此后可增加 `--locked --offline`，用固定依赖验证。

教材提供第 4 个独立检查点，对应本课（检查点编号不是课程编号）。从教材仓库根目录执行：

```bash
python3 scripts/macro_checkpoint.py --stage 4 --out /tmp/megflow-macros-lesson6
cargo test --manifest-path /tmp/megflow-macros-lesson6/Cargo.toml --workspace --locked --offline
```

导出器拒绝覆盖已有目录。它输出上述阶段文件和锁文件，只复制 describe 测试，不带后续 composition 测试。完整课程检查 `python3 scripts/check_macro_course.py` 会在临时目录验证四个检查点，因此不会用最终应用的测试掩盖本阶段缺少文件的问题。

## 7. 手写约束，再对照生成器

对于本课开头的 Packet，先手写如下预期实现理解数据流。这是用于对照的展开示意，不能与 derive 生成的方法同时加入同一个程序，否则会产生重复方法。

```rust,ignore
impl<T> Packet<T>
where
    T: std::fmt::Display,
{
    pub fn describe(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        parts.push(format!("{}={}", "载荷", &self.value));
        parts.join(", ")
    }
}
```

只有读取 value 的格式化语句，cache 完全不出现在生成方法中。输入参数是 `&self`，格式化借用字段，不会把 value 移出结构体。返回的 String 拥有自己的内容，因而不借用 Packet，可以在 Packet 释放后继续使用。

生成器先克隆 generics，是因为新增 where 约束只应作用于新 impl，不应修改原结构体的定义。它针对“实际格式化的字段类型”添加 Display，而不是遍历所有泛型参数一律加约束。测试 `Marker<NotDisplay>` 正是为了发现后一种过度限制。

## 练习与参考方案

1. 增加 `#[describe(rename = "")]` 的非空校验：解析 LitStr 后用 value().is_empty() 判断，用 new_spanned 指向该字面量，不能在运行时才检查。
2. 支持枚举：先为每个 variant 构造匹配模式；具名字段用名字绑定，元组用生成的独立标识符，单元 variant 不绑定；再分别格式化。只为实际绑定且格式化的字段添加约束。先测试三种形状再合并解析器。
3. 为什么不给所有泛型参数加 Display？参数可能只存在于跳过字段或 PhantomData 中；这样的限制没有业务依据，会拒绝原本可用的类型。
4. 为什么宏不能直接知道 T 是否实现 Display？展开阶段拿到的是语法，不是 rustc 完成推导后的类型信息；生成 where 约束交给编译器验证。


---

[课程首页](00-roadmap.md) · [上一课](05-three-forms.md) · [下一课](07-composition.md)
