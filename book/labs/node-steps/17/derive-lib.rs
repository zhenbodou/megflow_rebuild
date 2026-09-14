//! 第十七步：过程宏三件套里的头两件——属性宏 `#[inputs]`/`#[outputs]` 改写结构体、
//! 注入端口字段；派生宏 `#[derive(Node)]` 追加 `impl Node`。
//!
//! 本步的 `#[derive(Node)]` 先用「类型 token 里出现 Sender 就当输出端口」这个**示意**判据，
//! 够本步的 Doubler 用；它的脆弱之处（会误伤业务字段 `Option<HistorySender>`）第十八步修。

use proc_macro::TokenStream;
use quote::quote;
use syn::punctuated::Punctuated;
use syn::{
    parse_macro_input, parse_quote, Data, DeriveInput, Field, FieldMutability, Ident, ItemStruct,
    Token, Type, Visibility,
};

// ── 薄入口：解析 token 流后转交下面的 expand_* 逻辑（逻辑独立才好写单元测试）──

// ANCHOR: inputs_entry
#[proc_macro_attribute]
pub fn inputs(args: TokenStream, item: TokenStream) -> TokenStream {
    // args 是属性括号里的 `inp`（或 `inp, foo`）；item 是被标注的整个 struct。
    let names = parse_macro_input!(args with Punctuated::<Ident, Token![,]>::parse_terminated);
    let item = parse_macro_input!(item as ItemStruct);
    expand_inputs(&names.into_iter().collect::<Vec<_>>(), item).into()
}
// ANCHOR_END: inputs_entry

#[proc_macro_attribute]
pub fn outputs(args: TokenStream, item: TokenStream) -> TokenStream {
    let names = parse_macro_input!(args with Punctuated::<Ident, Token![,]>::parse_terminated);
    let item = parse_macro_input!(item as ItemStruct);
    expand_outputs(&names.into_iter().collect::<Vec<_>>(), item).into()
}

#[proc_macro_derive(Node)]
pub fn derive_node(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand_derive_node(&input).into()
}

// ── 逻辑 ──

// ANCHOR: named_field
/// 造一个「命名字段」`name: ty`（私有可见性——端口是引擎内部状态）。
///
/// syn 的 `Field` **不实现 `Parse`**：单个字段有「命名 `a: T`」和「元组 `T`」两种，单看无法
/// 区分，所以不能 `parse_quote!(inp: Receiver)`，只能手工构造。但字段里的 `ty` 可以
/// `parse_quote!`——因为 `Type` **实现了** `Parse`。这种「整体不可解析、部件可解析」是 syn 常见的坑。
fn named_field(name: Ident, ty: Type) -> Field {
    Field {
        attrs: vec![],
        vis: Visibility::Inherited,
        mutability: FieldMutability::None,
        ident: Some(name),
        colon_token: Some(Default::default()),
        ty,
    }
}
// ANCHOR_END: named_field

// ANCHOR: expand_inputs
/// `#[inputs(inp)]`：给每个端口名注入一个 `name: Receiver` 字段，并（一次性）注入关闭标志
/// `input_closed: bool`——它由 `#[methods]` 生成的包装 exec 置位、被 `#[derive(Node)]` 的
/// `is_all_input_closed` 读取。三个宏靠「字段名 `input_closed` 这个约定」协作（都在本 crate 里，约定可控）。
fn expand_inputs(names: &[Ident], mut item: ItemStruct) -> proc_macro2::TokenStream {
    let ident = item.ident.clone();
    {
        // `named` 是对 item.fields 的可变借用；用一个 `{ }` 块把它限制住，块结束借用释放，
        // 才能在下面 `quote! { #item }` 里不可变地用 item（否则可变借用与其冲突）。
        let syn::Fields::Named(named) = &mut item.fields else {
            let msg = "#[inputs] 只能用于具名字段结构体（struct X { .. }）";
            return syn::Error::new_spanned(ident, msg).to_compile_error();
        };
        for name in names {
            named
                .named
                .push(named_field(name.clone(), parse_quote!(Receiver)));
        }
        named
            .named
            .push(named_field(parse_quote!(input_closed), parse_quote!(bool)));
    }
    // 重新输出整个 struct：此刻 #[outputs]/#[derive] 还挂在它身上（属性宏自上而下展开），
    // 原样保留，于是轮到它们继续跑。
    quote! { #item }
}
// ANCHOR_END: expand_inputs

// ANCHOR: expand_outputs
/// `#[outputs(out)]`：给每个端口名注入 `name: Option<Sender>`。用 `Option` 是为了让 `close()`
/// 能把它置 `None`、drop 掉 `Sender`、触发下游收到关闭。
fn expand_outputs(names: &[Ident], mut item: ItemStruct) -> proc_macro2::TokenStream {
    let ident = item.ident.clone();
    {
        let syn::Fields::Named(named) = &mut item.fields else {
            let msg = "#[outputs] 只能用于具名字段结构体（struct X { .. }）";
            return syn::Error::new_spanned(ident, msg).to_compile_error();
        };
        for name in names {
            named
                .named
                .push(named_field(name.clone(), parse_quote!(Option<Sender>)));
        }
    }
    quote! { #item }
}
// ANCHOR_END: expand_outputs

// ANCHOR: derive_node_naive
/// **单端口阶段的示意分类**：类型 token 里出现 `Sender` 就当输出端口。够本步的 Doubler 用；
/// 但它会把业务字段 `Option<HistorySender>`（类型名恰好含 Sender）也误判成端口——这个洞第十八步补。
fn type_contains(ty: &Type, name: &str) -> bool {
    quote!(#ty).to_string().contains(name)
}

fn output_field_idents(input: &DeriveInput) -> Vec<Ident> {
    let mut outs = Vec::new();
    if let Data::Struct(data) = &input.data {
        for field in data.fields.iter() {
            if let Some(id) = &field.ident {
                if type_contains(&field.ty, "Sender") {
                    outs.push(id.clone());
                }
            }
        }
    }
    outs
}

/// `#[derive(Node)]`：生成 `impl Node`。派生宏跑时属性宏已把字段注入好，所以这里看到的是
/// **完整字段列表**，只需按类型认出输出端口。
///
/// - `close`：`quote` 的重复语法 `#( self.#outs = None; )*` 为每个输出字段生成一行置 `None`。
/// - `is_all_input_closed`：读 `#[inputs]` 注入的 `self.input_closed`。
/// - `split_for_impl()` 正确处理泛型（`impl<T> Node for X<T> where ..`）——正是 Ch2.2 承诺在本章补上的点。
fn expand_derive_node(input: &DeriveInput) -> proc_macro2::TokenStream {
    let name = &input.ident;
    let (ig, tg, wc) = input.generics.split_for_impl();
    let outs = output_field_idents(input);
    quote! {
        impl #ig Node for #name #tg #wc {
            fn close(&mut self) { #( self.#outs = None; )* }
            fn is_all_input_closed(&self) -> bool { self.input_closed }
        }
    }
}
// ANCHOR_END: derive_node_naive

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_str;

    fn id(name: &str) -> Ident {
        Ident::new(name, proc_macro2::Span::call_site())
    }

    #[test]
    fn inputs_injects_receiver_and_flag() {
        let item: ItemStruct = parse_str("struct D {}").unwrap();
        let out = expand_inputs(&[id("inp")], item).to_string().replace(' ', "");
        assert!(out.contains("inp:Receiver"));
        assert!(out.contains("input_closed:bool"));
    }

    #[test]
    fn outputs_injects_option_sender() {
        let item: ItemStruct = parse_str("struct D {}").unwrap();
        let out = expand_outputs(&[id("out")], item).to_string().replace(' ', "");
        assert!(out.contains("out:Option<Sender>"));
    }

    #[test]
    fn derive_node_closes_outputs_and_reads_flag() {
        // 模拟属性宏跑完后的结构体：一个输入、一个输出、关闭标志。
        let input: DeriveInput =
            parse_str("struct D { inp: Receiver, out: Option<Sender>, input_closed: bool }").unwrap();
        let out = expand_derive_node(&input).to_string().replace(' ', "");
        assert!(out.contains("implNodeforD"));
        assert!(out.contains("self.out=None")); // 输出被 close 置 None
        assert!(out.contains("self.input_closed")); // is_all_input_closed 读标志
        assert!(!out.contains("self.inp=None")); // 输入不是输出，不该被撤
    }
}
