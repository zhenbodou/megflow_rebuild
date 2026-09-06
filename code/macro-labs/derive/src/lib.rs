use proc_macro::TokenStream;
use quote::quote;
use syn::punctuated::Punctuated;
use syn::{parse_macro_input, parse_quote, DeriveInput, Ident, ItemStruct, Token};

// ANCHOR: derive
#[proc_macro_derive(TypeName)]
pub fn type_name(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let text = name.to_string();
    let (ig, tg, wc) = input.generics.split_for_impl();
    quote! {
        impl #ig #name #tg #wc {
            pub fn type_name() -> &'static str { #text }
        }
    }
    .into()
}
// ANCHOR_END: derive

// ANCHOR: attribute
#[proc_macro_attribute]
pub fn with_label(args: TokenStream, item: TokenStream) -> TokenStream {
    if !args.is_empty() {
        return syn::Error::new_spanned(
            proc_macro2::TokenStream::from(args),
            "#[with_label] 不接受参数",
        )
        .to_compile_error()
        .into();
    }
    let mut item = parse_macro_input!(item as ItemStruct);
    let syn::Fields::Named(fields) = &mut item.fields else {
        return syn::Error::new_spanned(&item.ident, "请使用具名字段结构体")
            .to_compile_error()
            .into();
    };
    if let Some(field) = fields
        .named
        .iter()
        .find(|f| f.ident.as_ref().is_some_and(|id| id == "label"))
    {
        return syn::Error::new_spanned(field, "label 字段已存在")
            .to_compile_error()
            .into();
    }
    fields.named.push(parse_quote!(pub label: &'static str));
    // 属性宏替换原项，所以必须把修改后的 item 返回。
    quote!(#item).into()
}
// ANCHOR_END: attribute

// ANCHOR: function
#[proc_macro]
pub fn port_names(input: TokenStream) -> TokenStream {
    let names = parse_macro_input!(input with Punctuated::<Ident, Token![,]>::parse_terminated);
    let strings: Vec<_> = names.iter().map(|name| name.to_string()).collect();
    // 调用位置是表达式；输出为借用数组，调用者给出 &[&str] 类型。
    quote!(&[#(#strings),*]).into()
}
// ANCHOR_END: function
