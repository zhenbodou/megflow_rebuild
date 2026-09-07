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

// ANCHOR: describe
#[proc_macro_derive(Describe, attributes(describe))]
pub fn describe(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand_describe(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn expand_describe(input: DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let syn::Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "Describe 只支持结构体",
        ));
    };
    let mut generics = input.generics.clone();
    let mut statements = Vec::new();
    let mut errors: Option<syn::Error> = None;
    for (index, field) in data.fields.iter().enumerate() {
        let mut skip = false;
        let mut rename: Option<syn::LitStr> = None;
        for attr in field.attrs.iter().filter(|a| a.path().is_ident("describe")) {
            let parsed = attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("skip") {
                    if skip {
                        return Err(meta.error("重复的 skip"));
                    }
                    skip = true;
                    Ok(())
                } else if meta.path.is_ident("rename") {
                    if rename.is_some() {
                        return Err(meta.error("重复的 rename"));
                    }
                    rename = Some(meta.value()?.parse()?);
                    Ok(())
                } else {
                    Err(meta.error("仅支持 skip 或 rename = \"名称\""))
                }
            });
            if let Err(error) = parsed {
                if let Some(previous) = &mut errors {
                    previous.combine(error);
                } else {
                    errors = Some(error);
                }
            }
        }
        if skip && rename.is_some() {
            let error = syn::Error::new_spanned(field, "skip 与 rename 不能同时使用");
            if let Some(previous) = &mut errors {
                previous.combine(error);
            } else {
                errors = Some(error);
            }
        }
        if skip {
            continue;
        }
        let member = field
            .ident
            .clone()
            .map(syn::Member::Named)
            .unwrap_or_else(|| syn::Member::Unnamed(syn::Index::from(index)));
        let label = rename.map(|s| s.value()).unwrap_or_else(|| {
            field
                .ident
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| index.to_string())
        });
        let ty = &field.ty;
        generics
            .make_where_clause()
            .predicates
            .push(parse_quote!(#ty: ::core::fmt::Display));
        statements.push(quote! {
            parts.push(::std::format!("{}={}", #label, &self.#member));
        });
    }
    if let Some(errors) = errors {
        return Err(errors);
    }
    let name = &input.ident;
    let (ig, tg, wc) = generics.split_for_impl();
    Ok(quote! {
        impl #ig #name #tg #wc {
            pub fn describe(&self) -> ::std::string::String {
                let mut parts: ::std::vec::Vec<::std::string::String> = ::std::vec::Vec::new();
                #(#statements)*
                parts.join(", ")
            }
        }
    })
}
// ANCHOR_END: describe

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_errors_are_combined() {
        let input = syn::parse_quote! {
            struct Invalid { #[describe(unknown)] a: u8, #[describe(other)] b: u8 }
        };
        let errors = expand_describe(input).unwrap_err();
        assert_eq!(errors.into_iter().count(), 2);
    }
    #[test]
    fn rejects_unsupported_and_conflicting_input() {
        for source in [
            "enum E { A }",
            "struct X { #[describe(skip, rename=\"x\")] a:u8 }",
            "struct X { #[describe(skip, skip)] a:u8 }",
            "struct X { #[describe(rename=3)] a:u8 }",
        ] {
            assert!(expand_describe(syn::parse_str(source).unwrap()).is_err());
        }
    }
}
