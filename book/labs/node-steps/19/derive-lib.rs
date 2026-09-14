//! 第十九步：加派生宏 `#[derive(Actor)]`，生成固定的三段式 `start` 循环。于是 Doubler 里
//! 手写的 `impl Actor` 可以删掉。相比第十八步，只多了 `derive_actor` 入口 + `expand_derive_actor`。

use proc_macro::TokenStream;
use quote::quote;
use syn::punctuated::Punctuated;
use syn::{
    parse_macro_input, parse_quote, Data, DeriveInput, Field, FieldMutability, Ident, ItemStruct,
    Token, Type, Visibility,
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

// ANCHOR: expand_derive_actor
/// `#[derive(Actor)]`：生成 `impl Actor`——固定的三段式 `start` 循环。它只调固有方法
/// （`initialize`/`exec`/`finalize`）与 `Node` 的 `is_all_input_closed`/`close`，不依赖任何
/// 字段信息，故几乎是常量模板，仅按类型名 + 泛型参数化。
///
/// `?` 只提前退出**内层** future；外层仍执行 `close`/`finalize`——即便业务出错也保证收尾。
/// （教学版 `start` 不带 `Context`；全书终点在 Ch4.3 给它加上 `ctx`，且只穿过 `initialize(&ctx)`。）
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
// ANCHOR_END: expand_derive_actor

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
    fn derive_actor_emits_three_phase_loop() {
        let input: DeriveInput = parse_str("struct D { input_closed: bool }").unwrap();
        let out = expand_derive_actor(&input).to_string().replace(' ', "");
        assert!(out.contains("implActorforD"));
        assert!(out.contains("tokio::spawn"));
        assert!(out.contains("self.initialize().await")); // 教学版无 ctx
        assert!(out.contains("!self.is_all_input_closed()"));
        assert!(out.contains("self.exec().await?"));
        assert!(out.contains("self.close()"));
        assert!(out.contains("self.finalize().await"));
    }
}
