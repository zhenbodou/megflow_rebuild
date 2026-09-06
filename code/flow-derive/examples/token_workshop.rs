//! 不调用编译器过程宏入口，在普通程序中练习 token -> AST -> token。
use proc_macro2::{TokenStream, TokenTree};
use quote::{format_ident, quote, quote_spanned};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Ident, LitStr, Token, Type};

// ANCHOR: grammar
// 练习语法：inp: i32, out: Vec<String>。保存类型，不执行类型检查。
struct TypedPort {
    name: Ident,
    ty: Type,
}

impl Parse for TypedPort {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(Self {
            name: input.parse()?,
            ty: {
                input.parse::<Token![:]>()?;
                input.parse()?
            },
        })
    }
}

struct Ports(Punctuated<TypedPort, Token![,]>);

impl Parse for Ports {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(Self(Punctuated::parse_terminated(input)?))
    }
}
// ANCHOR_END: grammar

// ANCHOR: expand
fn expand_port_struct(input: Ports) -> syn::Result<TokenStream> {
    let mut seen = std::collections::HashSet::new();
    let mut fields = Vec::new();
    let mut names = Vec::new();
    for port in input.0 {
        let name = port.name;
        if !seen.insert(name.to_string()) {
            return Err(syn::Error::new_spanned(name, "端口名不能重复"));
        }
        let ty = port.ty;
        fields.push(quote_spanned!(name.span()=> pub #name: #ty));
        names.push(LitStr::new(&name.to_string(), name.span()));
    }
    let struct_name = format_ident!("{}Ports", "Demo");
    Ok(quote! {
        pub struct #struct_name { #(#fields),* }
        impl #struct_name {
            pub const NAMES: &'static [&'static str] = &[#(#names),*];
        }
    })
}
// ANCHOR_END: expand

fn print_tokens(stream: TokenStream, depth: usize) {
    for token in stream {
        let indent = "  ".repeat(depth);
        match token {
            TokenTree::Group(group) => {
                println!("{indent}Group {:?}", group.delimiter());
                print_tokens(group.stream(), depth + 1);
            }
            TokenTree::Ident(ident) => println!("{indent}Ident {ident}"),
            TokenTree::Punct(punct) => {
                println!("{indent}Punct {} {:?}", punct.as_char(), punct.spacing())
            }
            TokenTree::Literal(literal) => println!("{indent}Literal {literal}"),
        }
    }
}

fn main() -> syn::Result<()> {
    print_tokens(quote!(send(value, 16)), 0);
    let parsed: Ports = syn::parse_str("inp: i32, out: Vec<String>,")?;
    let expanded = expand_port_struct(parsed)?;
    // 解析生成结果只证明语法正确；实际类型与行为仍需下游编译测试。
    let file: syn::File = syn::parse2(expanded.clone())?;
    assert_eq!(file.items.len(), 2);
    let syn::Item::Struct(item) = &file.items[0] else {
        panic!("应生成结构体")
    };
    assert_eq!(item.ident, "DemoPorts");
    assert_eq!(item.fields.len(), 2);
    println!("{expanded}");

    assert!(syn::parse_str::<Ports>("inp i32").is_err());
    let duplicate = syn::parse_str("inp: i32, inp: String")?;
    let error = expand_port_struct(duplicate).unwrap_err();
    assert_eq!(error.to_string(), "端口名不能重复");
    println!("预期诊断：{}", error.to_compile_error());
    Ok(())
}
