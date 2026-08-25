//! flow-derive · node —— 节点相关过程宏（Ch2.3）。
//!
//! 目标：把 Ch2.1 手写 `Doubler` 的那堆样板塌缩成几行声明。这里演示过程宏的
//! **全部三种形态**（Ch2.2 只演示了派生宏）：
//!
//! - **属性宏** `#[inputs(..)]` / `#[outputs(..)]`：**改写结构体**，注入端口字段。
//! - **派生宏** `#[derive(Node)]` / `#[derive(Actor)]`：**追加** impl。
//! - **属性宏** `#[methods]`：**改写 impl 块**——重命名用户的 `exec`、生成吞掉
//!   `ChannelClosed` 的包装、补齐缺失的生命周期默认实现。
//!
//! 生成目标严格对齐 Ch2.1 已提交的架构：`Node`（`close` 撤输出 / `is_all_input_closed`
//! 读关闭标志）、`Actor::start`（三段式循环 `initialize → while !closed { exec } →
//! close → finalize`）、exec 里把 `ChannelClosed` 转成「置标志 + Ok」。
//!
//! Node-related procedural macros: attribute macros inject port fields, derive
//! macros append `Node`/`Actor` impls, and `#[methods]` rewrites the impl block.

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{
    parse_quote, Data, DeriveInput, Field, FieldMutability, Ident, ImplItem, ItemImpl, ItemStruct,
    Type, Visibility,
};

/// 造一个「命名字段」`name: ty`（默认可见性）。syn 的 `Field` 不实现 `Parse`
/// （单个字段的「命名/元组」二义），故手工构造字段字面量而非 `parse_quote!`。
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

/// 判断一个类型的 token 串（去空格后）是否包含某子串——用来**按类型名分类端口**。
/// 这正是原版 flow-derive 的做法（它把 `Sender`/`Receiver` 等类型名列成常量表）。
fn type_contains(ty: &Type, needle: &str) -> bool {
    quote!(#ty).to_string().replace(' ', "").contains(needle)
}

/// `#[inputs(a, b, ..)]`：为每个名字注入 `name: Receiver`，并（一次性）注入关闭标志
/// `input_closed: bool`。标志由 `#[methods]` 生成的包装 `exec` 置位、被
/// `#[derive(Node)]` 的 `is_all_input_closed` 读取。
///
/// 端口的类型写作**裸名** `Receiver`（要求使用处 `use flow_rs::channel::Receiver`）。
/// 用绝对路径 `::flow_rs::..` 做卫生化留到后续（见章末「边界」）。
pub fn expand_inputs(names: &[Ident], mut item: ItemStruct) -> TokenStream2 {
    let ident = item.ident.clone();
    {
        let syn::Fields::Named(named) = &mut item.fields else {
            let msg = "#[inputs] 只能用于具名字段结构体（struct X { .. }）";
            return syn::Error::new_spanned(ident, msg).to_compile_error();
        };
        for n in names {
            named
                .named
                .push(named_field(n.clone(), parse_quote!(Receiver)));
        }
        named
            .named
            .push(named_field(parse_quote!(input_closed), parse_quote!(bool)));
    }
    quote! { #item }
}

/// `#[outputs(a, b, ..)]`：为每个名字注入 `name: Option<Sender>`。
/// 用 `Option` 是为了让 `close()` 能把它置 `None` → drop 掉 `Sender` → 下游收到关闭。
pub fn expand_outputs(names: &[Ident], mut item: ItemStruct) -> TokenStream2 {
    let ident = item.ident.clone();
    {
        let syn::Fields::Named(named) = &mut item.fields else {
            let msg = "#[outputs] 只能用于具名字段结构体（struct X { .. }）";
            return syn::Error::new_spanned(ident, msg).to_compile_error();
        };
        for n in names {
            named
                .named
                .push(named_field(n.clone(), parse_quote!(Option<Sender>)));
        }
    }
    quote! { #item }
}

/// 收集结构体里「类型 token 含 `Sender`」的字段名——即输出端口。
fn output_field_idents(input: &DeriveInput) -> Vec<Ident> {
    let mut outs = Vec::new();
    if let Data::Struct(data) = &input.data {
        for f in data.fields.iter() {
            if let Some(id) = &f.ident {
                if type_contains(&f.ty, "Sender") {
                    outs.push(id.clone());
                }
            }
        }
    }
    outs
}

/// `#[derive(Node)]`：生成 `impl Node`。
/// - `close`：把每个输出端口置 `None`（drop `Sender`，触发下游 `ChannelClosed`）。
/// - `is_all_input_closed`：读关闭标志 `self.input_closed`（由 `#[inputs]` 注入）。
///
/// 用 `split_for_impl()` 正确处理泛型（`impl<T> Node for X<T> where ..`）——这正是
/// Ch2.2 里 `TypeName` 刻意后置、承诺在本章补上的点。
pub fn expand_derive_node(input: &DeriveInput) -> TokenStream2 {
    let name = &input.ident;
    let (ig, tg, wc) = input.generics.split_for_impl();
    let outs = output_field_idents(input);
    quote! {
        impl #ig Node for #name #tg #wc {
            fn close(&mut self) {
                #( self.#outs = None; )*
            }
            fn is_all_input_closed(&self) -> bool {
                self.input_closed
            }
        }
    }
}

/// `#[derive(Actor)]`：生成 `impl Actor`——固定的三段式 `start` 循环。
/// 它只调用固有方法（`initialize`/`exec`/`finalize`）与 `Node` 的
/// `is_all_input_closed`/`close`，故几乎是常量模板，仅按类型名 + 泛型参数化。
pub fn expand_derive_actor(input: &DeriveInput) -> TokenStream2 {
    let name = &input.ident;
    let (ig, tg, wc) = input.generics.split_for_impl();
    quote! {
        impl #ig Actor for #name #tg #wc {
            fn start(mut self: Box<Self>) -> tokio::task::JoinHandle<Result<()>> {
                tokio::spawn(async move {
                    self.initialize().await;
                    while !self.is_all_input_closed() {
                        self.exec().await?;
                    }
                    self.close();
                    self.finalize().await;
                    Ok(())
                })
            }
        }
    }
}

/// `#[methods]`：改写节点的固有 `impl` 块。
/// 1. 把用户写的 `exec` 重命名为私有 `__megflow_exec_inner`；
/// 2. 生成新的 `exec` 包装：调用 inner，若得到 `Err(ChannelClosed)` 则「置关闭标志 +
///    返回 Ok」——于是**用户的 exec 里可以直接 `recv().await?`**，无需手写关闭处理；
/// 3. 若用户没写 `initialize`/`finalize`，补上空默认实现（`#[derive(Actor)]` 的循环
///    会调用它们）。
///
/// 假设 `exec` 的签名是 `async fn exec(&mut self) -> Result<()>`（本章 worker 节点约定）。
pub fn expand_methods(mut item: ItemImpl) -> TokenStream2 {
    let mut has_init = false;
    let mut has_final = false;
    for it in item.items.iter() {
        if let ImplItem::Fn(f) = it {
            match f.sig.ident.to_string().as_str() {
                "initialize" => has_init = true,
                "finalize" => has_final = true,
                _ => {}
            }
        }
    }

    let mut new_items: Vec<ImplItem> = Vec::new();
    for it in item.items.into_iter() {
        match it {
            ImplItem::Fn(f) if f.sig.ident == "exec" => {
                // ① inner：保留原签名与函数体，仅换名。
                let mut inner = f.clone();
                inner.sig.ident = Ident::new("__megflow_exec_inner", f.sig.ident.span());
                // ② wrapper：保留原签名，替换函数体为「调 inner + 吞 ChannelClosed」。
                let mut wrapper = f;
                wrapper.block = parse_quote!({
                    match self.__megflow_exec_inner().await {
                        Err(Error::ChannelClosed) => {
                            self.input_closed = true;
                            Ok(())
                        }
                        other => other,
                    }
                });
                new_items.push(ImplItem::Fn(inner));
                new_items.push(ImplItem::Fn(wrapper));
            }
            other => new_items.push(other),
        }
    }
    // ③ 补齐缺失的生命周期默认实现。
    if !has_init {
        new_items.push(parse_quote!(
            async fn initialize(&mut self) {}
        ));
    }
    if !has_final {
        new_items.push(parse_quote!(
            async fn finalize(&mut self) {}
        ));
    }
    item.items = new_items;
    quote! { #item }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::{parse_str, ItemImpl, ItemStruct};

    fn id(s: &str) -> Ident {
        Ident::new(s, proc_macro2::Span::call_site())
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
        let out = expand_outputs(&[id("out")], item)
            .to_string()
            .replace(' ', "");
        assert!(out.contains("out:Option<Sender>"));
    }

    #[test]
    fn derive_node_closes_outputs_and_reads_flag() {
        // 模拟属性宏跑完后的结构体：一个输入、一个输出、关闭标志。
        let input: DeriveInput =
            parse_str("struct D { inp: Receiver, out: Option<Sender>, input_closed: bool }")
                .unwrap();
        let out = expand_derive_node(&input).to_string().replace(' ', "");
        assert!(out.contains("implNodeforD"));
        assert!(out.contains("self.out=None")); // 输出被 close 撤掉
        assert!(out.contains("self.input_closed")); // is_all_input_closed 读标志
        assert!(!out.contains("self.inp=None")); // 输入不是输出，不该被撤
    }

    #[test]
    fn derive_actor_emits_three_phase_loop() {
        let input: DeriveInput = parse_str("struct D { input_closed: bool }").unwrap();
        let out = expand_derive_actor(&input).to_string().replace(' ', "");
        assert!(out.contains("implActorforD"));
        assert!(out.contains("tokio::spawn"));
        assert!(out.contains("self.initialize().await"));
        assert!(out.contains("!self.is_all_input_closed()"));
        assert!(out.contains("self.exec().await?"));
        assert!(out.contains("self.close()"));
        assert!(out.contains("self.finalize().await"));
    }

    #[test]
    fn derive_node_handles_generics() {
        // 兑现 Ch2.2 承诺：泛型走 split_for_impl，生成 `impl<T> Node for G<T>`。
        let input: DeriveInput = parse_str(
            "struct G<T> { inp: Receiver, out: Option<Sender>, input_closed: bool, _m: std::marker::PhantomData<T> }",
        )
        .unwrap();
        let out = expand_derive_node(&input).to_string().replace(' ', "");
        assert!(out.contains("impl<T>NodeforG<T>"));
    }

    #[test]
    fn methods_wraps_exec_and_fills_lifecycle() {
        let item: ItemImpl =
            parse_str("impl D { async fn exec(&mut self) -> Result<()> { self.inp.recv::<i32>().await?; Ok(()) } }")
                .unwrap();
        let out = expand_methods(item).to_string().replace(' ', "");
        // 原 exec 被改名为内部方法，且其函数体（recv）保留在内部方法里
        assert!(out.contains("__megflow_exec_inner"));
        assert!(out.contains("recv::<i32>"));
        // 生成的包装 exec 吞掉 ChannelClosed 并置标志
        assert!(out.contains("Err(Error::ChannelClosed)"));
        assert!(out.contains("self.input_closed=true"));
        // 缺失的生命周期被补齐
        assert!(out.contains("asyncfninitialize"));
        assert!(out.contains("asyncfnfinalize"));
    }

    #[test]
    fn methods_keeps_user_defined_lifecycle() {
        let item: ItemImpl = parse_str(
            "impl D { async fn initialize(&mut self) { self.n = 1; } async fn exec(&mut self) -> Result<()> { Ok(()) } }",
        )
        .unwrap();
        let out = expand_methods(item).to_string();
        // 用户自定义的 initialize 保留其函数体，不被默认实现覆盖（只应出现一次）
        assert!(out.contains("self . n = 1"));
        assert_eq!(out.matches("fn initialize").count(), 1);
    }
}
