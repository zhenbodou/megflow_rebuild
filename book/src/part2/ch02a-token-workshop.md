# Ch2.2a 把代码当数据：逐个学会 proc-macro2、syn 和 quote

本章接在 Ch2.2 后。先不碰完整节点，目标是亲眼看到一段端口声明被拆开，
再拼成结构体。这三个 crate 的分工一旦看清，后面的几百行节点宏就能分段阅读。

## 1. 先查自己的版本，不混抄不同版本的教程

在项目根目录运行：

```bash
cargo tree --manifest-path code/Cargo.toml -p flow-derive --depth 1
```

本次锁定的直接依赖为 `proc-macro2 1.0.107`、`quote 1.0.47`、`syn 2.0.119`，
测试依赖为 `trybuild 1.0.120`。以随书 `Cargo.lock` 和实际命令输出为准。
`Cargo.toml` 的 `syn = "2"` 是兼容版本范围，`Cargo.lock` 才是本次解析到的具体版本。
`--locked` 要求 Cargo 不擅自更新锁文件，不代表禁止下载依赖；`--offline` 才是不访问网络。

原版 `../megflow/flow-derive/Cargo.toml` 使用 syn 1，本书代码使用 syn 2。
例如原版 `ImplItem::Method` 在当前实现中是 `ImplItem::Fn`，
原版属性的 `.path` 在当前代码中通过 `.path()` 访问。在线 `latest` 可能已经
指向更新大版本，查 API 时应选择对应版本，不能机械复制最新示例。

## 2. proc_macro 和 proc-macro2 到底差在哪

`proc_macro` 由 Rust 工具链提供，不必在 Cargo 依赖中写版本。
真正的宏入口使用它的 `TokenStream`，因为这是 rustc 传给宏的接口。
它并非只能写在入口函数里：入口调用的辅助函数也可使用它；限制是需要
编译器提供的宏执行上下文，普通测试直接构造它会出问题。

`proc-macro2` 是普通依赖，代码导入名为 `proc_macro2`。
它提供能用于普通程序和测试的 token 表示，因此我们让 `expand_*`
函数接收语法树、返回 `proc_macro2::TokenStream`，入口负责 `.into()` 转换。
这不是两份源代码，而是同一段语法在两个 API 边界间转换。
参见 [proc-macro2 官方说明](https://docs.rs/proc-macro2/latest/proc_macro2/)。

## 3. TokenTree 的四种形态

对于 `send(value, 16)`，观察程序会输出：

```text
Ident send
Group Parenthesis
  Ident value
  Punct , Alone
  Literal 16
```

`Ident` 是名字；`Literal` 是字面量；`Punct` 是标点；`Group` 保存一对括号
及内部 token。括号中的内容没有丢失，而是下一层树。`<T>` 不要想当然当成
与 `(...)` 一样的 Group：尖括号还参与运算符语法，需要解析器判断。

一个 token 的 `Span` 保存源码位置和名字解析相关信息。生成错误时借用
用户标识符的 span，编译器就能把下划线画在用户写错的位置。
把所有 token 先转为字符串再解析会丢失这些信息，也容易误把 `MySenderConfig`
当成 `Sender`。字符串适合打印观察，不适合承担类型系统。

## 4. syn：从 token 到可操作的结构

| 你想读的语法 | 使用的类型/API | 项目落点 |
| --- | --- | --- |
| derive 标注的类型 | `DeriveInput` | Node、Actor、TypeName |
| 结构体完整定义 | `ItemStruct`、`Fields`、`Field` | inputs、outputs |
| impl 及方法 | `ItemImpl`、`ImplItem::Fn` | methods |
| Rust 类型 | `Type` | 端口声明、字段分类 |
| 自定义小语法 | `Parse`、`ParseStream` | PortSpec、NodeRegisterArgs |
| 带逗号列表 | `Punctuated<T, Token![,]>` | 多端口 |
| 一段完整生成代码 | `File` | 检查输出语法 |

`ParseStream` 像一个只向前读的游标。`parse()?` 读取一个语法单元；
`peek(...)` 看下一项但不取走；`?` 将解析错误交给调用者。
`Punctuated::parse_terminated` 负责循环和尾逗号，我们只写一个元素如何解析。

本章练习语法为 `inp: i32, out: Vec<String>,`。这是**独立的语法解析练习**，
并没有宣称引擎已经支持原版全部类型化端口；目前引擎 PortSpec 仍是 `inp` / `inps[]`。

把下面这段看成一个可执行的语法定义：先读名字，再读冒号，最后读类型。

```rust,ignore
{{#include ../../../code/flow-derive/examples/token_workshop.rs:grammar}}
```

`input.parse::<Token![:]>()?` 的返回值不用保存，但不能省略它，
否则后面的 `Type` 解析器会面对一个不该出现的冒号。对于 `Vec<String>`，
让 `syn::Type` 处理嵌套泛型，不要自己按字符串中的逗号拆分类型。

### features 是编译时开关

`derive` 提供派生输入的数据结构，`parsing` 提供解析，`printing` 支持输出 token，
这些在本项目使用的 syn 2 默认配置中开启。`full` 让我们能处理 `ItemImpl`
及方法体等完整语法。`visit`、`visit-mut`、`fold` 是遍历/改写树的额外接口，
当前端口宏只遍历直接字段，暂时不需要开启；要递归分析每个表达式时再增加。
`extra-traits` 用于为语法树增加 Debug/比较等能力，不是解析代码必需的开关。
本书使用的开关和 API 可用 `cargo doc -p syn@2.0.119 --no-deps` 在本地对应版本文档中核对。

## 5. quote：区分模板里的名字和插入的变量

`quote!(impl #name {})` 中，`impl` 是直接生成的关键字，`#name` 是插值。
如果 `name` 是 `Ident`，它生成标识符；如果变量是 String，它生成字符串字面量。
把 `"Demo"` 直接插在结构体名字处会生成非法 Rust，所以动态名字用 `format_ident!`。

`#(#fields),*` 的意思是依次插入 fields 中的片段、用逗号分隔。
`#(#closes)*` 则不自动添加分隔符，closes 的每个片段自己应带分号。
这里的 `#` 是 quote 的语法，不能与 macro_rules 的 `$` 混用。
参见 [quote 的插值说明](https://docs.rs/quote/latest/quote/)。

本章生成器的完整实现如下：

```rust,ignore
{{#include ../../../code/flow-derive/examples/token_workshop.rs:expand}}
```

先校验重复名，再生成字段和名字列表，最后一次性返回结构体与 impl。
`quote_spanned!` 给新生成的字段片段设置位置；已插入的 token 可以保留自己的 span。
`syn::Error::new_spanned` 表达用户输入错误，`to_compile_error()` 将其变成编译器能读的代码。

## 6. 完整运行与练习

源码文件是 `code/flow-derive/examples/token_workshop.rs`，包含上述两段及观察程序。
新增文件时使用完整文件，不能只粘贴展开函数而漏掉 `use` 和 `main`。

```bash
cargo run --manifest-path code/Cargo.toml -p flow-derive --example token_workshop --locked
```

预期先打印 token 树，再打印 `DemoPorts` 和 impl，最后打印预期的重复名诊断。
程序同时断言生成文件含两个 item、结构体有两个字段；这些检查证明语法结构，
并不证明字段类型存在或满足 trait。下一章用真实调用者让 rustc 完成后半段检查。

练习：把输入改成 `inp i32`，应在解析阶段失败；改成 `inp: i32, inp: String`，
应在校验阶段失败；把 `i32` 改成 `DoesNotExist`，解析仍能通过，这证明 syn
只认识类型语法，不替代 Rust 的类型检查。最后把生成结果放进普通 Rust 文件编译，
才会得到找不到类型的错误。
