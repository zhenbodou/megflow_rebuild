use proc_macro2::TokenStream;
use quote::quote;
use syn::{spanned::Spanned, FnArg, ItemFn, ReturnType};

// 对照原版 flow-derive/src/cvt_func.rs（95f870bf）的两槽位语法。
// Rust 分支不使用字符串提示，保留解析避免拒绝原版合法调用。
struct CvtFnOption;
impl syn::parse::Parse for CvtFnOption {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        if !input.is_empty() && !input.peek(syn::Token![_]) {
            input.parse::<syn::LitStr>()?;
        } else {
            input.parse::<syn::Token![_]>().ok();
        }
        input.parse::<syn::Token![,]>().ok();
        if !input.is_empty() && !input.peek(syn::Token![_]) {
            input.parse::<syn::LitStr>()?;
        } else {
            input.parse::<syn::Token![_]>().ok();
        }
        Ok(Self)
    }
}

pub fn expand(args: TokenStream, function: ItemFn) -> syn::Result<TokenStream> {
    // Rust 类型仍从函数签名取得；提示语法与原版保持一致。
    let _options = syn::parse2::<CvtFnOption>(args)?;
    let sig = &function.sig;
    if sig.asyncness.is_some()
        || sig.unsafety.is_some()
        || sig.abi.is_some()
        || !sig.generics.params.is_empty()
        || sig.inputs.len() != 1
    {
        return Err(syn::Error::new(
            sig.span(),
            "转换函数必须是单参数、非泛型的安全同步 Rust 函数",
        ));
    }
    let from = match sig.inputs.first().unwrap() {
        FnArg::Typed(argument) => &argument.ty,
        other => return Err(syn::Error::new_spanned(other, "转换函数不能接收 self")),
    };
    let to = match &sig.output {
        ReturnType::Type(_, ty) => ty,
        _ => return Err(syn::Error::new(sig.span(), "转换函数必须显式声明返回类型")),
    };
    let name = &sig.ident;
    let gates: Vec<_> = function
        .attrs
        .iter()
        .filter_map(|attribute| gate(&attribute.meta).transpose())
        .collect::<syn::Result<_>>()?;
    Ok(quote! {
        #function
        #(#[#gates])*
        const _: () = {
            ::flow_rs::inventory::submit! {
                ::flow_rs::channel::ConversionRegistration {
                    from: ::flow_rs::config::interlayer::MsgTypeId::of::<#from>,
                    to: ::flow_rs::config::interlayer::MsgTypeId::of::<#to>,
                    function: |mut envelope| {
                        let envelope = envelope.downcast_mut::<::flow_rs::envelope::Envelope<#from>>()
                            .expect(concat!("type error in convert function ", stringify!(#name)));
                        let input = envelope.unpack();
                        envelope.repack(#name(input)).seal()
                    },
                }
            }
        };
    })
}

// 仅复制决定“是否存在”的属性，不能把 inline/test 等函数专属属性贴到 const 上。
fn gate(meta: &syn::Meta) -> syn::Result<Option<syn::Meta>> {
    if meta.path().is_ident("cfg") {
        return Ok(Some(meta.clone()));
    }
    if !meta.path().is_ident("cfg_attr") {
        return Ok(None);
    }
    let list = meta.require_list()?;
    let items = list.parse_args_with(
        syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
    )?;
    let mut items = items.iter();
    let predicate = items
        .next()
        .ok_or_else(|| syn::Error::new_spanned(meta, "cfg_attr 缺少条件"))?;
    let gates = items
        .filter_map(|item| gate(item).transpose())
        .collect::<syn::Result<Vec<_>>>()?;
    if gates.is_empty() {
        Ok(None)
    } else {
        Ok(Some(syn::parse_quote!(cfg_attr(#predicate, #(#gates),*))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_rust_option_forms_preserve_type_identity() {
        let function: ItemFn = syn::parse_quote!(
            fn convert(value: u32) -> u64 {
                value as u64
            }
        );
        let plain = expand(TokenStream::new(), function.clone())
            .unwrap()
            .to_string();
        for options in [
            quote!(),
            quote!(_),
            quote!(_, _),
            quote!("from"),
            quote!(_, "to"),
            quote!("from", "to"),
        ] {
            assert_eq!(
                expand(options, function.clone()).unwrap().to_string(),
                plain
            );
        }
        for bad in [quote!(123), quote!(_, _, _), quote!("from", "to",)] {
            assert!(expand(bad, function.clone()).is_err());
        }
    }

    #[test]
    fn nested_cfg_attr_keeps_only_existence_conditions() {
        let input: syn::Meta = syn::parse_quote!(cfg_attr(
            feature = "x",
            inline,
            cfg_attr(unix, cfg(any()), allow(dead_code))
        ));
        let output = gate(&input).unwrap().unwrap();
        let expected: syn::Meta =
            syn::parse_quote!(cfg_attr(feature = "x", cfg_attr(unix, cfg(any()))));
        assert_eq!(quote!(#output).to_string(), quote!(#expected).to_string());
        let function: ItemFn = syn::parse_quote!(
            #[cfg(any())]
            fn convert(x: u32) -> u32 {
                x
            }
        );
        let file: syn::File = syn::parse2(expand(TokenStream::new(), function).unwrap()).unwrap();
        let syn::Item::Const(registration) = &file.items[1] else {
            panic!("missing registration");
        };
        assert!(registration.attrs[0].path().is_ident("cfg"));
    }

    #[test]
    fn rejects_signatures_that_cannot_be_registered() {
        for source in [
            "async fn f(x: u32) -> u32 { x }",
            "fn f<T>(x: T) -> T { x }",
            "fn f(x: u32, y: u32) -> u32 { x+y }",
            "fn f(x: u32) {}",
        ] {
            assert!(expand(TokenStream::new(), syn::parse_str(source).unwrap()).is_err());
        }
    }
}
