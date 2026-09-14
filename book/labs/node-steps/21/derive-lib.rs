//! 第二十一步：加派生宏 `#[derive(BuildFromPorts)]`——为节点生成「从端口构造自己」的 `build`。
//! 它按字段类型把传入的端口**填进**字段，复用第十八步写的精确分类（`is_output_port` / `type_is`），
//! 只是方向相反：`#[derive(Node)]` 在 `close` 里把输出端口**撤**成 `None`，这里在 `build` 里把端口
//! **填**进字段。相比第二十步，只多了 `derive_build_from_ports` 入口 + `expand_build_from_ports`。
//!
//! 生成的 `impl BuildFromPorts for Doubler` 用**裸名** `BuildFromPorts`/`Receiver`/`Sender`/`Actor`
//! （沿用本章的卫生化策略），故使用处（`tests/register.rs`）要 `use` 好这些名字。

use proc_macro::TokenStream;
use quote::quote;
use syn::punctuated::Punctuated;
use syn::{
    parse_macro_input, parse_quote, Data, DeriveInput, Field, FieldMutability, Ident, ImplItem,
    ItemImpl, ItemStruct, Token, Type, Visibility,
};

#[proc_macro_attribute]
pub fn inputs(args: TokenStream, item: TokenStream) -> TokenStream {
    let names = parse_macro_input!(args with Punctuated::<Ident, Token![,]>::parse_terminated);
    let item = parse_macro_input!(item as ItemStruct);
    expand_inputs(&names.into_iter().collect::<Vec<_>>(), item).into()
}

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

#[proc_macro_derive(Actor)]
pub fn derive_actor(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand_derive_actor(&input).into()
}

// ANCHOR: build_from_ports_entry
#[proc_macro_derive(BuildFromPorts)]
pub fn derive_build_from_ports(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand_build_from_ports(&input).into()
}
// ANCHOR_END: build_from_ports_entry

#[proc_macro_attribute]
pub fn methods(_args: TokenStream, item: TokenStream) -> TokenStream {
    // 属性括号里没有参数（`#[methods]`），只关心被标注的整个 `impl` 块。
    let item = parse_macro_input!(item as ItemImpl);
    expand_methods(item).into()
}

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

fn expand_inputs(names: &[Ident], mut item: ItemStruct) -> proc_macro2::TokenStream {
    let ident = item.ident.clone();
    {
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
    quote! { #item }
}

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

fn type_is(ty: &Type, name: &str) -> bool {
    matches!(ty, Type::Path(path) if path.qself.is_none()
        && path.path.segments.last().is_some_and(|segment|
            segment.ident == name && matches!(segment.arguments, syn::PathArguments::None)))
}

fn wrapped_type<'a>(ty: &'a Type, wrapper: &str) -> Option<&'a Type> {
    let Type::Path(path) = ty else { return None };
    if path.qself.is_some() {
        return None;
    }
    let segment = path.path.segments.last()?;
    if segment.ident != wrapper {
        return None;
    }
    let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return None;
    };
    if arguments.args.len() != 1 {
        return None;
    }
    match arguments.args.first()? {
        syn::GenericArgument::Type(inner) => Some(inner),
        _ => None,
    }
}

fn is_output_port(ty: &Type) -> bool {
    wrapped_type(ty, "Option").is_some_and(|inner| type_is(inner, "Sender"))
}

fn output_field_idents(input: &DeriveInput) -> Vec<Ident> {
    let mut outs = Vec::new();
    if let Data::Struct(data) = &input.data {
        for field in data.fields.iter() {
            if let Some(id) = &field.ident {
                if is_output_port(&field.ty) {
                    outs.push(id.clone());
                }
            }
        }
    }
    outs
}

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

// ANCHOR: expand_build_from_ports
/// `#[derive(BuildFromPorts)]`：生成 `impl BuildFromPorts`——把传入的端口按字段类型**填进**字段。
/// 复用第十八步的精确分类：`Option<Sender>` 是输出端口、裸 `Receiver` 是输入端口；`input_closed`
/// 标志初值 `false`；其余字段走 `Default::default()`。
fn expand_build_from_ports(input: &DeriveInput) -> proc_macro2::TokenStream {
    let name = &input.ident;
    let (ig, tg, wc) = input.generics.split_for_impl();

    let mut inits = Vec::new();
    if let Data::Struct(data) = &input.data {
        for field in data.fields.iter() {
            let Some(id) = &field.ident else { continue };
            let init = if is_output_port(&field.ty) {
                quote! { Some(outs.remove(0)) } // 输出端口：取一个 Sender、包 Some
            } else if type_is(&field.ty, "Receiver") {
                quote! { ins.remove(0) } // 输入端口：取一个 Receiver
            } else if *id == "input_closed" {
                quote! { false } // 关闭标志：初值 false
            } else {
                quote! { Default::default() } // 其余字段：默认值
            };
            inits.push(quote! { #id: #init });
        }
    }

    quote! {
        impl #ig BuildFromPorts for #name #tg #wc {
            fn build(mut ins: Vec<Receiver>, mut outs: Vec<Sender>) -> Box<dyn Actor> {
                Box::new(#name { #( #inits ),* })
            }
        }
    }
}
// ANCHOR_END: expand_build_from_ports

fn expand_derive_actor(input: &DeriveInput) -> proc_macro2::TokenStream {
    let name = &input.ident;
    let (ig, tg, wc) = input.generics.split_for_impl();
    quote! {
        impl #ig Actor for #name #tg #wc {
            fn start(mut self: Box<Self>) -> tokio::task::JoinHandle<Result<()>> {
                tokio::spawn(async move {
                    self.initialize().await;
                    let result = async {
                        while !self.is_all_input_closed() {
                            self.exec().await?;
                        }
                        Ok(())
                    }
                    .await;
                    self.close();
                    self.finalize().await;
                    result
                })
            }
        }
    }
}

/// `#[methods]`：把业务 `impl` 补全成引擎要的形状。三件事：
/// 1）把用户写的 `exec` 改名成内部方法 `__megflow_exec_inner`（业务纯逻辑留在这里）；
/// 2）生成新的 `exec` 包装：跑内部方法，把 `Err(ChannelClosed)` 翻译成「置 `input_closed`、正常返回」——
///    于是 `#[derive(Actor)]` 生成的 `while !is_all_input_closed() { exec()? }` 循环能自然收尾；
/// 3）用户没写 `initialize`/`finalize` 就补一个空默认（教学版都不带 `Context`；终点在 Ch4.3 才加 `ctx`）。
fn expand_methods(mut item: ItemImpl) -> proc_macro2::TokenStream {
    let mut has_initialize = false;
    let mut has_finalize = false;
    for impl_item in &mut item.items {
        if let ImplItem::Fn(method) = impl_item {
            match method.sig.ident.to_string().as_str() {
                "exec" => method.sig.ident = parse_quote!(__megflow_exec_inner),
                "initialize" => has_initialize = true,
                "finalize" => has_finalize = true,
                _ => {}
            }
        }
    }
    // 包装 exec：调内部方法，`ChannelClosed` 不算错，翻译成「输入已关」信号。
    item.items.push(parse_quote! {
        pub async fn exec(&mut self) -> Result<()> {
            match self.__megflow_exec_inner().await {
                Err(Error::ChannelClosed) => {
                    self.input_closed = true;
                    Ok(())
                }
                other => other,
            }
        }
    });
    if !has_initialize {
        item.items.push(parse_quote! {
            async fn initialize(&mut self) {}
        });
    }
    if !has_finalize {
        item.items.push(parse_quote! {
            async fn finalize(&mut self) {}
        });
    }
    quote! { #item }
}

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
    fn classification_uses_structure_not_substrings() {
        let input: DeriveInput = parse_str(
            "struct D { out: Option<Sender>, history: Option<HistorySender>, input_closed: bool }",
        )
        .unwrap();
        let out = expand_derive_node(&input).to_string().replace(' ', "");
        assert!(out.contains("self.out=None"));
        assert!(!out.contains("self.history=None"));
    }

    #[test]
    fn build_from_ports_wires_ports_by_position() {
        // 派生出的 build 把输入端口按位置取、输出端口包 Some、关闭标志置 false。
        let input: DeriveInput =
            parse_str("struct D { inp: Receiver, out: Option<Sender>, input_closed: bool }")
                .unwrap();
        let out = expand_build_from_ports(&input).to_string().replace(' ', "");
        assert!(out.contains("implBuildFromPortsforD"));
        assert!(out.contains("inp:ins.remove(0)")); // 输入端口：取一个 Receiver
        assert!(out.contains("out:Some(outs.remove(0))")); // 输出端口：包 Some
        assert!(out.contains("input_closed:false")); // 关闭标志初值
    }

    #[test]
    fn derive_actor_emits_three_phase_loop() {
        let input: DeriveInput = parse_str("struct D { input_closed: bool }").unwrap();
        let out = expand_derive_actor(&input).to_string().replace(' ', "");
        assert!(out.contains("implActorforD"));
        assert!(out.contains("self.initialize().await"));
        assert!(out.contains("self.exec().await?"));
        assert!(out.contains("self.finalize().await"));
    }

    #[test]
    fn methods_wraps_exec_and_fills_lifecycle() {
        // 只写了 exec，没写生命周期：改名 + 包装 + 补默认。
        let item: ItemImpl =
            parse_str("impl D { pub async fn exec(&mut self) -> Result<()> { Ok(()) } }").unwrap();
        let out = expand_methods(item).to_string().replace(' ', "");
        assert!(out.contains("__megflow_exec_inner")); // 用户 exec 被改名
        assert!(out.contains("self.input_closed=true")); // 包装吞 ChannelClosed
        assert!(out.contains("asyncfninitialize")); // 补了默认 initialize
        assert!(out.contains("asyncfnfinalize")); // 补了默认 finalize
    }

    #[test]
    fn methods_keeps_user_defined_lifecycle() {
        // 用户自己写了 initialize：保留原样，不再补默认（教学版签名不带 Context）。
        let item: ItemImpl = parse_str(
            "impl D { async fn initialize(&mut self) { setup(); } pub async fn exec(&mut self) -> Result<()> { Ok(()) } }",
        )
        .unwrap();
        let out = expand_methods(item).to_string().replace(' ', "");
        assert!(out.contains("setup()")); // 用户逻辑保留
        assert_eq!(out.matches("fninitialize").count(), 1); // 只有一份 initialize
    }
}
