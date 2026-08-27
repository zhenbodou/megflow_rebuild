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
use syn::parse::{Parse, ParseStream};
use syn::{
    parse_quote, Data, DeriveInput, Field, FieldMutability, Ident, ImplItem, ItemImpl, ItemStruct,
    LitStr, Path, Token, Type, Visibility,
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

/// 一个端口声明：端口名 + 是否为**数组端口**（名字后跟 `[]`）。
///
/// `#[inputs(inp, inps[])]` 解析成 `[PortSpec{inp, array:false}, PortSpec{inps, array:true}]`。
/// 标量端口一个名字对应**一条** channel 端；数组端口一个名字对应**一组** channel 端
/// （`Vec<Receiver>` / `Vec<Sender>`）——这是扇入（Merge）/ 扇出（Bcast）的地基（Ch4.2）。
/// 语法上我们用**裸的空方括号** `name[]`；原版写作 `name:[T0]`（带每端口类型变量），但重写
/// 版的 channel 在字段层是**未类型化**的（都搬 `SealedEnvelope`），无需那套类型变量机制，
/// 故取更简的写法（方括号内即便写了东西也一律忽略）。
pub struct PortSpec {
    /// 端口名（注入结构体的字段名，也是注册表端口名表里的名字）。
    pub name: Ident,
    /// 是否数组端口（名字后带 `[]`）。
    pub array: bool,
}

impl Parse for PortSpec {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let name: Ident = input.parse()?;
        // 名字后可选 `[...]` → 标记为数组端口；括号内内容一律忽略（消费掉即可）。
        let array = input.peek(syn::token::Bracket);
        if array {
            let _bracket_content;
            syn::bracketed!(_bracket_content in input);
        }
        Ok(PortSpec { name, array })
    }
}

/// `#[inputs(a, b[], ..)]`：为每个端口注入字段——标量端口 `name: Receiver`，数组端口
/// `name: Vec<Receiver>`（扇入）——并（一次性）注入关闭标志 `input_closed: bool`。标志由
/// `#[methods]` 生成的包装 `exec` 置位、被 `#[derive(Node)]` 的 `is_all_input_closed` 读取。
///
/// 端口的类型写作**裸名** `Receiver`（要求使用处 `use flow_rs::channel::Receiver`）。
/// 用绝对路径 `::flow_rs::..` 做卫生化留到后续（见章末「边界」）。
pub fn expand_inputs(specs: &[PortSpec], mut item: ItemStruct) -> TokenStream2 {
    let ident = item.ident.clone();
    {
        let syn::Fields::Named(named) = &mut item.fields else {
            let msg = "#[inputs] 只能用于具名字段结构体（struct X { .. }）";
            return syn::Error::new_spanned(ident, msg).to_compile_error();
        };
        for spec in specs {
            // 数组端口 → Vec<Receiver>（一名多端，扇入）；标量端口 → 单个 Receiver。
            let ty: Type = if spec.array {
                parse_quote!(Vec<Receiver>)
            } else {
                parse_quote!(Receiver)
            };
            named.named.push(named_field(spec.name.clone(), ty));
        }
        named
            .named
            .push(named_field(parse_quote!(input_closed), parse_quote!(bool)));
    }
    quote! { #item }
}

/// `#[outputs(a, b[], ..)]`：为每个端口注入字段——标量端口 `name: Option<Sender>`，数组端口
/// `name: Vec<Sender>`（扇出/广播）。标量用 `Option` 是为了让 `close()` 能把它置 `None` →
/// drop 掉 `Sender` → 下游收到关闭；数组则靠 `close()` 里 `.clear()` 达到同样效果。
pub fn expand_outputs(specs: &[PortSpec], mut item: ItemStruct) -> TokenStream2 {
    let ident = item.ident.clone();
    {
        let syn::Fields::Named(named) = &mut item.fields else {
            let msg = "#[outputs] 只能用于具名字段结构体（struct X { .. }）";
            return syn::Error::new_spanned(ident, msg).to_compile_error();
        };
        for spec in specs {
            // 数组端口 → Vec<Sender>（一名多端，扇出）；标量端口 → Option<Sender>。
            let ty: Type = if spec.array {
                parse_quote!(Vec<Sender>)
            } else {
                parse_quote!(Option<Sender>)
            };
            named.named.push(named_field(spec.name.clone(), ty));
        }
    }
    quote! { #item }
}

/// 收集输出端口字段：`(字段名, 是否数组端口)`。判据仍是「类型 token 含 `Sender`」；
/// 其中类型再含 `Vec` 的（`Vec<Sender>`）即数组端口，否则是标量端口（`Option<Sender>`）。
/// `close()` 据此选择「置 `None`」还是「清空 `Vec`」。
fn output_fields(input: &DeriveInput) -> Vec<(Ident, bool)> {
    let mut outs = Vec::new();
    if let Data::Struct(data) = &input.data {
        for f in data.fields.iter() {
            if let Some(id) = &f.ident {
                if type_contains(&f.ty, "Sender") {
                    outs.push((id.clone(), type_contains(&f.ty, "Vec")));
                }
            }
        }
    }
    outs
}

/// `#[derive(Node)]`：生成 `impl Node`。
/// - `close`：撤掉每个输出端口——标量 `self.#id = None;`、数组 `self.#id.clear();`。
///   两者都会 drop 掉底层 `Sender`，触发下游 `ChannelClosed`（关闭涟漪）。
/// - `is_all_input_closed`：读关闭标志 `self.input_closed`（由 `#[inputs]` 注入）。
///
/// 用 `split_for_impl()` 正确处理泛型（`impl<T> Node for X<T> where ..`）——这正是
/// Ch2.2 里 `TypeName` 刻意后置、承诺在本章补上的点。
pub fn expand_derive_node(input: &DeriveInput) -> TokenStream2 {
    let name = &input.ident;
    let (ig, tg, wc) = input.generics.split_for_impl();
    let closes = output_fields(input).into_iter().map(|(id, is_array)| {
        if is_array {
            quote! { self.#id.clear(); } // 数组输出：清空 Vec → drop 掉每个 Sender
        } else {
            quote! { self.#id = None; } // 标量输出：置 None → drop 掉 Sender
        }
    });
    quote! {
        impl #ig Node for #name #tg #wc {
            fn close(&mut self) {
                #( #closes )*
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

// ── Ch2.4：编译期注册表（`#[derive(BuildFromPorts)]` + `node_register!`）──

/// `#[derive(BuildFromPorts)]`：生成 `impl BuildFromPorts`——「从参数+端口构造节点」。
///
/// 一次字段遍历同时干三件事（Ch3.2 相对 Ch2.4 的升级，Ch4.2 又加了「数组端口」维度）：
/// 1. **接线**（位置）：构造器现在收的是**分组**的端口——`ins: Vec<Vec<Receiver>>` /
///    `outs: Vec<Vec<Sender>>`，每个内层 `Vec` 是**一个端口名**下的那一组 channel 端
///    （标量端口 = 恰好 1 个的组，数组端口 = N 个的组）。于是：
///    - 标量输入 `Receiver` → `ins.remove(0).remove(0)`（取该端口组里唯一的 Receiver）；
///    - 数组输入 `Vec<Receiver>` → `ins.remove(0)`（整组搬走）；
///    - 标量输出 `Option<Sender>` → `Some(outs.remove(0).remove(0))`；
///    - 数组输出 `Vec<Sender>` → `outs.remove(0)`（整组搬走）；
///    - 名为 `input_closed` → `false`。
/// 2. **端口名表 + 数组标记表**：`INPUTS`/`OUTPUTS` 收字段名（遍历顺序，与 `build` 消费
///    `ins`/`outs` 的顺序严格同序）；并行的 `INPUT_ARRAY`/`OUTPUT_ARRAY` 记每个端口是否
///    数组端口。Graph Builder 靠名表把「TOML 里按名接的 channel」排成「构造器要的分组
///    Vec」，靠数组标记表决定「一个端口能接几条边」（标量端口重复接 → PortAlreadyConnected，
///    数组端口可接多条 → 扇入/扇出）。
/// 3. **自有参数**：其余字段（如 `op: String`）改用 `flow_rs::config::arg(args, "字段名")?`
///    从节点参数表按字段名反序列化。于是 `build` 需要 `&Args` 入参、并返回 `Result`。
///
/// 裸名 `Receiver`/`Sender`/`Actor`/`Result`/`BuildFromPorts` 沿用 Ch2.3 的策略（要求
/// 使用处 `use`）；新引入的 `Args`/`arg` 用**绝对路径** `flow_rs::config::..`，免得再逼
/// 使用处多写两个 `use`（与 `node_register!` 的绝对路径策略一致）。
pub fn expand_build_from_ports(input: &DeriveInput) -> TokenStream2 {
    let name = &input.ident;
    let (ig, tg, wc) = input.generics.split_for_impl();

    let mut inits = Vec::new();
    let mut input_names: Vec<LitStr> = Vec::new();
    let mut output_names: Vec<LitStr> = Vec::new();
    let mut input_array: Vec<bool> = Vec::new();
    let mut output_array: Vec<bool> = Vec::new();
    if let Data::Struct(data) = &input.data {
        for f in data.fields.iter() {
            let Some(id) = &f.ident else { continue };
            let init = if type_contains(&f.ty, "Sender") {
                let is_array = type_contains(&f.ty, "Vec");
                output_names.push(LitStr::new(&id.to_string(), id.span()));
                output_array.push(is_array);
                if is_array {
                    quote! { outs.remove(0) } // 数组输出：整组 Vec<Sender> 搬走
                } else {
                    quote! { Some(outs.remove(0).remove(0)) } // 标量：取组里唯一的 Sender
                }
            } else if type_contains(&f.ty, "Receiver") {
                let is_array = type_contains(&f.ty, "Vec");
                input_names.push(LitStr::new(&id.to_string(), id.span()));
                input_array.push(is_array);
                if is_array {
                    quote! { ins.remove(0) } // 数组输入：整组 Vec<Receiver> 搬走
                } else {
                    quote! { ins.remove(0).remove(0) } // 标量：取组里唯一的 Receiver
                }
            } else if *id == "input_closed" {
                quote! { false }
            } else {
                // 自有参数字段：按字段名从 args 反序列化（配置驱动的构造侧落点）。
                let key = LitStr::new(&id.to_string(), id.span());
                quote! { flow_rs::config::arg(args, #key)? }
            };
            inits.push(quote! { #id: #init });
        }
    }

    quote! {
        impl #ig BuildFromPorts for #name #tg #wc {
            const INPUTS: &'static [&'static str] = &[ #( #input_names ),* ];
            const OUTPUTS: &'static [&'static str] = &[ #( #output_names ),* ];
            const INPUT_ARRAY: &'static [bool] = &[ #( #input_array ),* ];
            const OUTPUT_ARRAY: &'static [bool] = &[ #( #output_array ),* ];
            fn build(
                args: &flow_rs::config::Args,
                mut ins: Vec<Vec<Receiver>>,
                mut outs: Vec<Vec<Sender>>,
            ) -> Result<Box<dyn Actor>> {
                Ok(Box::new(#name { #( #inits ),* }))
            }
        }
    }
}

/// `node_register!("Name", Type)` 的参数：注册名（字符串字面量）+ 节点类型路径。
pub struct NodeRegisterArgs {
    /// 注册到表里的类型名字符串（TOML 里按它引用节点）。
    pub name: LitStr,
    /// 节点类型的路径（`<Type as BuildFromPorts>::build` 从它取构造器）。
    pub ty: Path,
}

impl Parse for NodeRegisterArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let name: LitStr = input.parse()?;
        input.parse::<Token![,]>()?;
        let ty: Path = input.parse()?;
        Ok(NodeRegisterArgs { name, ty })
    }
}

/// 函数式宏 `node_register!("Name", Type)`：在编译期提交一条注册。
///
/// 生成 `flow_rs::inventory::submit! { flow_rs::registry::NodeRegistration { .. } }`——
/// 全部用**绝对路径** `flow_rs::`（下游视角）：`submit!` 生成的是 item 级 `static`，不便
/// 要求使用处 `use`，故不走 Ch2.3 派生宏的裸名策略。本 crate 内部（Part 4 内置节点）也能
/// 用这个宏——因为 lib.rs 里写了 `extern crate self as flow_rs;`，让绝对路径 `flow_rs::`
/// 在 flow-rs 自己内部也解析得通（无需 `proc-macro-crate` 那套按调用位置改前缀的机制）。
pub fn expand_node_register(args: &NodeRegisterArgs) -> TokenStream2 {
    let name = &args.name;
    let ty = &args.ty;
    quote! {
        flow_rs::inventory::submit! {
            flow_rs::registry::NodeRegistration {
                name: #name,
                inputs: <#ty as flow_rs::registry::BuildFromPorts>::INPUTS,
                outputs: <#ty as flow_rs::registry::BuildFromPorts>::OUTPUTS,
                input_array: <#ty as flow_rs::registry::BuildFromPorts>::INPUT_ARRAY,
                output_array: <#ty as flow_rs::registry::BuildFromPorts>::OUTPUT_ARRAY,
                ctor: <#ty as flow_rs::registry::BuildFromPorts>::build,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::{parse_str, ItemImpl, ItemStruct};

    fn id(s: &str) -> Ident {
        Ident::new(s, proc_macro2::Span::call_site())
    }

    /// 标量端口声明（`name`）。/ scalar port spec.
    fn scalar(s: &str) -> PortSpec {
        PortSpec {
            name: id(s),
            array: false,
        }
    }

    /// 数组端口声明（`name[]`）。/ array port spec.
    fn array_port(s: &str) -> PortSpec {
        PortSpec {
            name: id(s),
            array: true,
        }
    }

    #[test]
    fn inputs_injects_receiver_and_flag() {
        let item: ItemStruct = parse_str("struct D {}").unwrap();
        let out = expand_inputs(&[scalar("inp")], item)
            .to_string()
            .replace(' ', "");
        assert!(out.contains("inp:Receiver"));
        assert!(out.contains("input_closed:bool"));
    }

    #[test]
    fn inputs_array_injects_vec_receiver() {
        // 数组输入端口 `inps[]` → 注入 `inps: Vec<Receiver>`（扇入 Merge 的地基）。
        let item: ItemStruct = parse_str("struct D {}").unwrap();
        let out = expand_inputs(&[array_port("inps")], item)
            .to_string()
            .replace(' ', "");
        assert!(out.contains("inps:Vec<Receiver>"));
        assert!(out.contains("input_closed:bool"));
    }

    #[test]
    fn outputs_injects_option_sender() {
        let item: ItemStruct = parse_str("struct D {}").unwrap();
        let out = expand_outputs(&[scalar("out")], item)
            .to_string()
            .replace(' ', "");
        assert!(out.contains("out:Option<Sender>"));
    }

    #[test]
    fn outputs_array_injects_vec_sender() {
        // 数组输出端口 `out[]` → 注入 `out: Vec<Sender>`（扇出 Bcast 的地基）。
        let item: ItemStruct = parse_str("struct D {}").unwrap();
        let out = expand_outputs(&[array_port("out")], item)
            .to_string()
            .replace(' ', "");
        assert!(out.contains("out:Vec<Sender>"));
    }

    #[test]
    fn derive_node_closes_outputs_and_reads_flag() {
        // 模拟属性宏跑完后的结构体：一个输入、一个输出、关闭标志。
        let input: DeriveInput =
            parse_str("struct D { inp: Receiver, out: Option<Sender>, input_closed: bool }")
                .unwrap();
        let out = expand_derive_node(&input).to_string().replace(' ', "");
        assert!(out.contains("implNodeforD"));
        assert!(out.contains("self.out=None")); // 标量输出被 close 置 None
        assert!(out.contains("self.input_closed")); // is_all_input_closed 读标志
        assert!(!out.contains("self.inp=None")); // 输入不是输出，不该被撤
    }

    #[test]
    fn derive_node_clears_array_output() {
        // 数组输出端口 `out: Vec<Sender>`：close 用 `.clear()`（drop 掉每个 Sender），
        // 而非 `= None`（那是标量端口的做法）。
        let input: DeriveInput =
            parse_str("struct D { inp: Receiver, out: Vec<Sender>, input_closed: bool }").unwrap();
        let out = expand_derive_node(&input).to_string().replace(' ', "");
        assert!(out.contains("self.out.clear()")); // 数组输出：清空 Vec
        assert!(!out.contains("self.out=None")); // 不走标量的置 None
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

    #[test]
    fn build_from_ports_wires_ports() {
        // 模拟属性宏跑完后的结构体，验证按字段类型/名字生成的接线 + 端口名表 + 数组标记表。
        let input: DeriveInput =
            parse_str("struct D { inp: Receiver, out: Option<Sender>, input_closed: bool }")
                .unwrap();
        let out = expand_build_from_ports(&input).to_string().replace(' ', "");
        assert!(out.contains("implBuildFromPortsforD"));
        // 分组构造器：标量输入取「组里唯一的 Receiver」→ remove(0).remove(0)
        assert!(out.contains("inp:ins.remove(0).remove(0)"));
        // 标量输出取「组里唯一的 Sender」→ Some(remove(0).remove(0))
        assert!(out.contains("out:Some(outs.remove(0).remove(0))"));
        assert!(out.contains("input_closed:false")); // 关闭标志初值
                                                     // 端口名表：与填充顺序同序，交给 Graph Builder 做「名字→位置」的桥
        assert!(out.contains("constINPUTS"));
        assert!(out.contains("&[\"inp\"]"));
        assert!(out.contains("constOUTPUTS"));
        assert!(out.contains("&[\"out\"]"));
        // 数组标记表：标量端口 → false
        assert!(out.contains("constINPUT_ARRAY:&'static[bool]=&[false]"));
        assert!(out.contains("constOUTPUT_ARRAY:&'static[bool]=&[false]"));
        // build 收分组端口 Vec<Vec<_>>、返回 Result、包 Ok
        assert!(out.contains("ins:Vec<Vec<Receiver>>"));
        assert!(out.contains("outs:Vec<Vec<Sender>>"));
        assert!(out.contains("->Result<Box<dynActor>>"));
        assert!(out.contains("Ok(Box::new(D"));
    }

    #[test]
    fn build_from_ports_array_ports() {
        // 数组输入 + 数组输出：整组搬走（不再 remove(0).remove(0)），数组标记表为 true。
        let input: DeriveInput =
            parse_str("struct D { inps: Vec<Receiver>, out: Vec<Sender>, input_closed: bool }")
                .unwrap();
        let out = expand_build_from_ports(&input).to_string().replace(' ', "");
        // 数组输入：整组 Vec<Receiver> 搬走 → 只 remove(0) 一次（不再取内层）
        assert!(out.contains("inps:ins.remove(0)"));
        assert!(!out.contains("inps:ins.remove(0).remove(0)"));
        // 数组输出：整组 Vec<Sender> 搬走 → 不包 Some、不取内层
        assert!(out.contains("out:outs.remove(0)"));
        assert!(!out.contains("out:Some("));
        // 数组标记表：数组端口 → true
        assert!(out.contains("constINPUT_ARRAY:&'static[bool]=&[true]"));
        assert!(out.contains("constOUTPUT_ARRAY:&'static[bool]=&[true]"));
    }

    #[test]
    fn build_from_ports_deserializes_arg_fields() {
        // 多输入 + 输出 + 自有参数：验证 INPUTS/OUTPUTS 按序、arg 字段走 config::arg。
        let input: DeriveInput = parse_str(
            "struct D { op: String, a: Receiver, b: Receiver, input_closed: bool, c: Option<Sender> }",
        )
        .unwrap();
        let out = expand_build_from_ports(&input).to_string().replace(' ', "");
        // 自有参数按字段名从 args 反序列化（取代 Ch2.4 的 Default::default）
        assert!(out.contains("op:flow_rs::config::arg(args,\"op\")?"));
        // 端口名表按声明顺序：输入 a、b；输出 c
        assert!(out.contains("&[\"a\",\"b\"]"));
        assert!(out.contains("&[\"c\"]"));
        // 标量端口按位置填，取组里唯一的端（remove(0).remove(0)），且与名表同序
        assert!(out.contains("a:ins.remove(0).remove(0)"));
        assert!(out.contains("b:ins.remove(0).remove(0)"));
        assert!(out.contains("c:Some(outs.remove(0).remove(0))"));
    }

    #[test]
    fn node_register_emits_submit() {
        let args: NodeRegisterArgs = parse_str("\"D\", D").unwrap();
        let out = expand_node_register(&args).to_string().replace(' ', "");
        // 绝对路径提交一条 NodeRegistration，端口名表 + ctor 都指向 <D as BuildFromPorts>::..
        assert!(out.contains("flow_rs::inventory::submit!"));
        assert!(out.contains("flow_rs::registry::NodeRegistration"));
        assert!(out.contains("name:\"D\""));
        assert!(out.contains("BuildFromPorts>::INPUTS"));
        assert!(out.contains("BuildFromPorts>::OUTPUTS"));
        assert!(out.contains("BuildFromPorts>::INPUT_ARRAY"));
        assert!(out.contains("BuildFromPorts>::OUTPUT_ARRAY"));
        assert!(out.contains("BuildFromPorts>::build"));
    }
}
