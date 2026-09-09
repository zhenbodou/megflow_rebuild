//! 端口 DSL 实验：不接入运行时，先验证解析与字段生成。
use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
    Ident, Token, Type,
};

#[derive(Debug, PartialEq)]
enum Shape {
    Unit,
    List,
    Dict,
    Dynamic,
}
enum Message {
    Any,
    Template(usize),
    Rust(Type),
}
struct Port {
    name: Ident,
    shape: Shape,
    message: Message,
}

fn message(input: ParseStream) -> syn::Result<Message> {
    if input.is_empty() || input.peek(Token![,]) {
        return Ok(Message::Any);
    }
    let ty: Type = input.parse()?;
    if let Type::Path(path) = &ty {
        if path.qself.is_none()
            && path.path.leading_colon.is_none()
            && path.path.segments.len() == 1
        {
            let segment = &path.path.segments[0];
            if segment.arguments.is_empty() {
                if let Some(index) = segment
                    .ident
                    .to_string()
                    .strip_prefix('T')
                    .and_then(|s| s.parse().ok())
                {
                    return Ok(Message::Template(index));
                }
            }
        }
    }
    Ok(Message::Rust(ty))
}

impl Parse for Port {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let name = input.parse()?;
        if !input.peek(Token![:]) {
            return Ok(Self {
                name,
                shape: Shape::Unit,
                message: Message::Any,
            });
        }
        input.parse::<Token![:]>()?;
        let (shape, message) = if input.peek(syn::token::Brace) {
            let content;
            syn::braced!(content in input);
            let ty = message(&content)?;
            if !content.is_empty() {
                return Err(content.error("字典端口括号内只能有一个消息类型"));
            }
            (Shape::Dict, ty)
        } else if input.peek(syn::token::Bracket) {
            let content;
            syn::bracketed!(content in input);
            let ty = message(&content)?;
            if !content.is_empty() {
                return Err(content.error("列表端口括号内只能有一个消息类型"));
            }
            (Shape::List, ty)
        } else if input.peek(Token![dyn]) {
            input.parse::<Token![dyn]>()?;
            (Shape::Dynamic, message(input)?)
        } else {
            (Shape::Unit, message(input)?)
        };
        Ok(Self {
            name,
            shape,
            message,
        })
    }
}
struct Ports(Punctuated<Port, Token![,]>);
impl Parse for Ports {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(Self(input.parse_terminated(Port::parse, Token![,])?))
    }
}
impl Port {
    fn output_field(&self) -> TokenStream {
        let name = &self.name;
        let endpoint = match &self.message {
            Message::Rust(ty) => quote!(flow_rs::channel::SenderT<#ty>),
            Message::Any | Message::Template(_) => quote!(flow_rs::channel::Sender),
        };
        let field_type = match self.shape {
            Shape::Unit => endpoint,
            Shape::List => quote!(Vec<#endpoint>),
            Shape::Dict => quote!(std::collections::HashMap<u64, #endpoint>),
            Shape::Dynamic => quote!(flow_rs::node::DynPorts<#endpoint>),
        };
        quote!(#name: #field_type)
    }
}
fn main() {
    let ports: Ports =
        syn::parse_str("plain, scalar:u32, batch:[String], routes:{T0}, live:dyn T1,").unwrap();
    assert_eq!(ports.0.len(), 5);
    let routes = &ports.0[3];
    assert_eq!(routes.shape, Shape::Dict);
    assert!(matches!(routes.message, Message::Template(0)));
    assert!(matches!(ports.0[4].message, Message::Template(1)));
    for port in &ports.0 {
        println!("{}", port.output_field());
    }
    // Rust 类型内部的逗号属于类型语法，不是端口分隔符。
    let nested: Ports = syn::parse_str("out:{Result<u32, String>}, next").unwrap();
    assert_eq!(nested.0.len(), 2);
    assert!(matches!(nested.0[0].message, Message::Rust(_)));
    for invalid in [
        "out:{u32, String}",
        "out:[u32; 2]",
        "out:{u32} garbage",
        "out:Result<",
    ] {
        assert!(
            syn::parse_str::<Ports>(invalid).is_err(),
            "不应接受 {invalid}"
        );
    }
    println!("端口形态、模板编号、嵌套类型与非法输入验证通过");
}
