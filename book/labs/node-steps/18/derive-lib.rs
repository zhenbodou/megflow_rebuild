//! 第十八步：把 `#[derive(Node)]` 的端口分类从「字符串包含」升级为**按语法树精确匹配**。
//! 于是业务字段 `Option<HistorySender>`（类型名恰好含 Sender）不再被误判成输出端口。
//!
//! 相比第十七步，只有 `derive_node` 用到的分类逻辑（`type_contains` → 精确版）变了，
//! `#[inputs]`/`#[outputs]` 一字未动。

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

// ANCHOR: classify
/// 按语法树精确匹配路径末段，不用字符串包含（那会误伤 `HistorySender`）。要求恰好是 `name`
/// 这一段、且不带泛型实参（`Sender`✓、`HistorySender`✗、`Sender<T>`✗）。
fn type_is(ty: &Type, name: &str) -> bool {
    matches!(ty, Type::Path(path) if path.qself.is_none()
        && path.path.segments.last().is_some_and(|segment|
            segment.ident == name && matches!(segment.arguments, syn::PathArguments::None)))
}

/// 若 `ty` 是 `Wrapper<Inner>`（恰好一个类型实参）就取出 `Inner`，否则 `None`。
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

/// 输出端口 = 恰好 `Option<Sender>`（本教学版单端口）。于是 `Option<HistorySender>` 因内层
/// 不是 `Sender` 而被排除，不再误撤业务字段。（终点还长出数组/字典/类型化端口，见章末「示意」标注。）
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
// ANCHOR_END: classify

/// `#[derive(Node)]`：生成 `impl Node`（结构同第十七步，只是端口分类换成上面的精确版）。
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
    fn classification_uses_structure_not_substrings() {
        // 一个真输出端口 + 一个业务字段（类型名含 Sender，但不是 Option<Sender>）。
        let input: DeriveInput = parse_str(
            "struct D { out: Option<Sender>, history: Option<HistorySender>, input_closed: bool }",
        )
        .unwrap();
        let out = expand_derive_node(&input).to_string().replace(' ', "");
        assert!(out.contains("self.out=None")); // 真端口被撤
        assert!(!out.contains("self.history=None")); // 业务字段不被误撤
    }
}
