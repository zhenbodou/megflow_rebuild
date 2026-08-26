//! flow-derive —— MegFlow 过程宏（重写版）。
//!
//! 本 crate 是一个**过程宏 crate**（`proc-macro = true`）：它导出的函数在**编译期**
//! 运行——输入是一段代码的 **token 流**，输出也是 token 流。这就是「用代码生成代码」。
//! 三件套各司其职：
//! - **`proc-macro2`**：token 流的类型（`proc_macro2::TokenStream`）。编译器自带的
//!   `proc_macro::TokenStream` 只能在宏入口用、无法单元测试；`proc-macro2` 是它的
//!   可测镜像，生态里 syn/quote 都围绕它。
//! - **`syn`**：把 token 流**解析**成语法树（如 [`DeriveInput`] = 一个带属性的
//!   struct/enum 定义）。
//! - **`quote`**：`quote! { ... }` 把语法树片段和变量（`#var`）**拼回** token 流。
//!
//! 本章（Ch2.2）实现最小派生宏 `#[derive(TypeName)]` 打通全流程；
//! `#[inputs]`/`#[outputs]`/`#[derive(Node)]`/`#[methods]` 在 Ch2.3、
//! `node_register!` 在 Ch2.4。
//!
//! A procedural-macro crate. Macros run at compile time: token stream in,
//! token stream out. `proc-macro2` (testable token type), `syn` (parse),
//! `quote` (generate).

use proc_macro::TokenStream;
use quote::quote;
use syn::punctuated::Punctuated;
use syn::{parse_macro_input, DeriveInput, Ident, ItemImpl, ItemStruct, Token};

mod node;

/// 派生宏 `#[derive(TypeName)]`：给结构体/枚举生成一个返回其类型名的**固有方法**
/// `const fn type_name() -> &'static str`。
///
/// 入门宏，刻意**自包含**——生成固有方法、不引用任何 trait，聚焦三件套本身。
/// （生成的代码引用「外部 trait 该住哪」这个话题留到 Ch2.3 的 `derive(Node)`。）
/// 类型名字符串会在 Ch2.4 的 `node_register!` 里用作注册表 key。
///
/// Derive macro generating `Foo::type_name() -> &'static str`.
#[proc_macro_derive(TypeName)]
pub fn derive_type_name(input: TokenStream) -> TokenStream {
    // ① 边界：把编译器给的 proc_macro::TokenStream 解析成 syn 语法树 DeriveInput。
    let input = parse_macro_input!(input as DeriveInput);
    // ② 逻辑：交给可单元测试的纯函数（内部用 proc_macro2::TokenStream）。
    // ③ 边界：.into() 转回 proc_macro::TokenStream 交还编译器。
    expand_type_name(&input).into()
}

// ── Ch2.3：节点宏入口（薄）。逻辑一律在 `node` 模块，便于单元测试。──

/// 属性宏 `#[inputs(a, b, ..)]`：给节点结构体注入输入端口字段 + 关闭标志。
#[proc_macro_attribute]
pub fn inputs(args: TokenStream, item: TokenStream) -> TokenStream {
    let names = parse_macro_input!(args with Punctuated::<Ident, Token![,]>::parse_terminated);
    let item = parse_macro_input!(item as ItemStruct);
    node::expand_inputs(&names.into_iter().collect::<Vec<_>>(), item).into()
}

/// 属性宏 `#[outputs(a, b, ..)]`：给节点结构体注入输出端口字段。
#[proc_macro_attribute]
pub fn outputs(args: TokenStream, item: TokenStream) -> TokenStream {
    let names = parse_macro_input!(args with Punctuated::<Ident, Token![,]>::parse_terminated);
    let item = parse_macro_input!(item as ItemStruct);
    node::expand_outputs(&names.into_iter().collect::<Vec<_>>(), item).into()
}

/// 属性宏 `#[methods]`：改写节点的固有 impl 块（包装 exec + 补齐生命周期）。
#[proc_macro_attribute]
pub fn methods(_args: TokenStream, item: TokenStream) -> TokenStream {
    let item = parse_macro_input!(item as ItemImpl);
    node::expand_methods(item).into()
}

/// 派生宏 `#[derive(Node)]`：生成 `impl Node`（close / is_all_input_closed）。
#[proc_macro_derive(Node)]
pub fn derive_node(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    node::expand_derive_node(&input).into()
}

/// 派生宏 `#[derive(Actor)]`：生成 `impl Actor`（三段式 start 循环）。
#[proc_macro_derive(Actor)]
pub fn derive_actor(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    node::expand_derive_actor(&input).into()
}

// ── Ch2.4：编译期注册表入口 ──

/// 派生宏 `#[derive(BuildFromPorts)]`：生成「从端口构造节点」的 `impl BuildFromPorts`。
#[proc_macro_derive(BuildFromPorts)]
pub fn derive_build_from_ports(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    node::expand_build_from_ports(&input).into()
}

/// 函数式宏 `node_register!("Name", Type)`：编译期把一条节点注册提交进 inventory 表。
/// 这是过程宏的**第三种形态**（函数式宏），Ch2.2/2.3 只演示了派生宏与属性宏。
#[proc_macro]
pub fn node_register(input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(input as node::NodeRegisterArgs);
    node::expand_node_register(&args).into()
}

/// 宏的**逻辑核心**：`DeriveInput` → `proc_macro2::TokenStream`。
///
/// 把「编译期 API 边界」（`proc_macro`，只入口用）与「可测的生成逻辑」
/// （`proc_macro2`）分开，是过程宏的标准工程模式——于是 `expand_*` 能被下面的
/// `#[cfg(test)]` 单元测试直接调用、比对生成的 token。
fn expand_type_name(input: &DeriveInput) -> proc_macro2::TokenStream {
    let name = &input.ident; // 结构体/枚举名，如 `Foo`
    let name_str = name.to_string(); // "Foo"
    quote! {
        impl #name {
            pub const fn type_name() -> &'static str {
                #name_str
            }
        }
    }
    // 注：此处未处理泛型（`impl<T> Foo<T>`）。泛型的 `split_for_impl` 留到 Ch2.3 的
    // `#[derive(Node)]`——那里必须处理。本入门示例针对无泛型类型。
}

#[cfg(test)]
mod tests {
    use super::expand_type_name;
    use syn::{parse_str, DeriveInput};

    #[test]
    fn expands_to_impl_with_type_name() {
        let input: DeriveInput = parse_str("struct Foo { a: i32 }").unwrap();
        let out = expand_type_name(&input).to_string();
        // 生成的 token 串里应含 `impl Foo`、方法名、以及字符串字面量 "Foo"
        assert!(out.contains("impl Foo"));
        assert!(out.contains("type_name"));
        assert!(out.contains("\"Foo\""));
    }

    #[test]
    fn works_for_enum_too() {
        let input: DeriveInput = parse_str("enum Bar { A, B }").unwrap();
        let out = expand_type_name(&input).to_string();
        assert!(out.contains("impl Bar"));
        assert!(out.contains("\"Bar\""));
    }
}
