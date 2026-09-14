//! 第二十二步：过程宏的**第三种形态**——函数式宏 `node_register!("Doubler", Doubler)`。
//! 它在编译期把一条 `NodeRegistration` `submit!` 进 inventory 表；运行时只凭字符串就能 `find`
//! 出构造器。相比第二十一步，只多了 `NodeRegisterArgs`（自定义 `Parse`）+ `node_register` 入口
//! + `expand_node_register`。
//!
//! 与派生宏/属性宏不同，函数式宏的入口用 `#[proc_macro]`，输入是**任意 token 流**——没有现成的
//! `DeriveInput`/`ItemImpl` 可解析，得自己定义语法（这里是 `"名字", 类型路径`）。

use proc_macro::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{
    parse_macro_input, parse_quote, Data, DeriveInput, Field, FieldMutability, Ident, ImplItem,
    ItemImpl, ItemStruct, LitStr, Path, Token, Type, Visibility,
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

#[proc_macro_derive(BuildFromPorts)]
pub fn derive_build_from_ports(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand_build_from_ports(&input).into()
}

#[proc_macro_attribute]
pub fn methods(_args: TokenStream, item: TokenStream) -> TokenStream {
    let item = parse_macro_input!(item as ItemImpl);
    expand_methods(item).into()
}

// ANCHOR: node_register_entry
#[proc_macro]
pub fn node_register(input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(input as NodeRegisterArgs);
    expand_node_register(&args).into()
}
// ANCHOR_END: node_register_entry

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

// ANCHOR: node_register_args
/// `node_register!("Name", Type)` 的参数：注册名（字符串字面量）+ 节点类型路径。
/// （教学版入口与逻辑合在 lib.rs 一处，故这个解析器结构体保持**非 pub**——proc-macro crate 的
/// 根不能导出宏以外的公有项；终点把它放在 `node` 子模块里，才写成 `pub struct`。）
struct NodeRegisterArgs {
    /// 注册到表里的类型名字符串（TOML 里按它引用节点）。
    name: LitStr,
    /// 节点类型的路径（`<Type as BuildFromPorts>::build` 从它取构造器）。
    ty: Path,
}

impl Parse for NodeRegisterArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let name: LitStr = input.parse()?;
        input.parse::<Token![,]>()?;
        let ty: Path = input.parse()?;
        Ok(NodeRegisterArgs { name, ty })
    }
}
// ANCHOR_END: node_register_args

// ANCHOR: expand_node_register
/// 函数式宏 `node_register!("Name", Type)`：编译期提交一条注册。
///
/// 生成 `flow_rs::inventory::submit! { flow_rs::registry::NodeRegistration { .. } }`——全部用
/// **绝对路径** `flow_rs::`（下游视角）：`submit!` 展开成 item 级 `static`，不便要求使用处 `use`，
/// 故不走 Ch2.3 派生宏的裸名策略。本 crate 内部（Part 4 内置节点）也能用它——因为 lib.rs 里写了
/// `extern crate self as flow_rs;`，让绝对路径 `flow_rs::` 在 flow-rs 自己内部也解析得通。
fn expand_node_register(args: &NodeRegisterArgs) -> proc_macro2::TokenStream {
    let name = &args.name;
    let ty = &args.ty;
    quote! {
        flow_rs::inventory::submit! {
            flow_rs::registry::NodeRegistration {
                name: #name,
                ctor: <#ty as flow_rs::registry::BuildFromPorts>::build,
            }
        }
    }
}
// ANCHOR_END: expand_node_register

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
        let input: DeriveInput =
            parse_str("struct D { inp: Receiver, out: Option<Sender>, input_closed: bool }")
                .unwrap();
        let out = expand_build_from_ports(&input).to_string().replace(' ', "");
        assert!(out.contains("implBuildFromPortsforD"));
        assert!(out.contains("inp:ins.remove(0)"));
        assert!(out.contains("out:Some(outs.remove(0))"));
        assert!(out.contains("input_closed:false"));
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
        let item: ItemImpl =
            parse_str("impl D { pub async fn exec(&mut self) -> Result<()> { Ok(()) } }").unwrap();
        let out = expand_methods(item).to_string().replace(' ', "");
        assert!(out.contains("__megflow_exec_inner"));
        assert!(out.contains("self.input_closed=true"));
        assert!(out.contains("asyncfninitialize"));
        assert!(out.contains("asyncfnfinalize"));
    }

    #[test]
    fn methods_keeps_user_defined_lifecycle() {
        let item: ItemImpl = parse_str(
            "impl D { async fn initialize(&mut self) { setup(); } pub async fn exec(&mut self) -> Result<()> { Ok(()) } }",
        )
        .unwrap();
        let out = expand_methods(item).to_string().replace(' ', "");
        assert!(out.contains("setup()"));
        assert_eq!(out.matches("fninitialize").count(), 1);
    }

    #[test]
    fn node_register_emits_submit() {
        // node_register!("Doubler", Doubler) 生成 submit! + NodeRegistration + build 构造器。
        let args: NodeRegisterArgs = parse_str(r#""Doubler", Doubler"#).unwrap();
        let out = expand_node_register(&args).to_string().replace(' ', "");
        assert!(out.contains("flow_rs::inventory::submit!"));
        assert!(out.contains("flow_rs::registry::NodeRegistration"));
        assert!(out.contains(r#"name:"Doubler""#));
        assert!(out.contains("ctor:<Doublerasflow_rs::registry::BuildFromPorts>::build"));
    }
}
