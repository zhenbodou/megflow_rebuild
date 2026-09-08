# Ch2.4 node_register! 与 inventory 编译期注册表

Ch2.3 我们把节点样板塌缩成了几行声明。但还差最后一环：写好的节点，引擎怎么**发现**它？图配置（Part 3 的 TOML）里只写一个类型名字符串 `"Doubler"`，引擎得据此把对应的节点**造出来**。这一章补上这张「名字 → 构造器」的表——用**函数式宏**（过程宏的第三种形态）+ `inventory` crate，让节点在编译期自动登记。写完这章，Part 2 收官。

先完成 [注册表实作](ch04b-registry-workshop.md)，手写名字到构造器的表，再理解 inventory 的分散登记。

<!-- toc -->

## 1. 问题：一张分布式的、编译期就位的表

引擎装配图时，手上只有 TOML 里的一个字符串：

```toml,ignore
[[nodes]]
name = "d"
ty = "Doubler"     # ← 只有这个类型名
```

它要凭 `"Doubler"` 这个名字，造出一个 `Box<dyn Actor>`。所以引擎需要一张表：**类型名 → 构造器**。

难点不在「表」本身，在于表的**来源是分散的**：

- 内置节点（transform/bcast/… Part 4）定义在 **flow-rs** 里；
- 业务节点（真实算法仓里的检测/跟踪/告警节点）定义在**下游 crate** 里。

它们分布在互不相识的 crate 中，却要在**程序跑起来之前**汇成同一张全局表。这就是「**编译期分布式注册**」——每个节点在自己的定义处「报个到」，链接时自动汇总。

原版的 `node_register!` 经 `submit!` 生成 `#[flow_rs::ctor]` 初始化函数，调用运行时注册接口写入 lazy_static 管理的表。原版没有 `#[flow_rs::ln]` 这个入口；源码路径是 `flow-derive/src/node.rs`、`internal.rs` 与 `flow-rs/src/registry.rs`。

我们使用公共 crate `inventory` 管理类型化的分散条目。它生成静态数据和初始化入口，链接进应用后由平台初始化机制登记，运行时枚举。它也有初始化成本，不能未经测量就宣称比原版快；Ch2.4a 用完整实验解释这条路径。

## 2. inventory：三个动作

`inventory` 的心智模型只有三个动作：

- **`inventory::collect!(T)`**：在**定义 `T` 的 crate**里声明「要收集的条目类型是 `T`」。必须与 `T` 同 crate、写在 item 位置（模块级，不在函数体内）。
- **`inventory::submit! { EXPR }`**：在**任意 crate**提交一条 `T` 类型的条目。`EXPR` 必须能在 `static` 上下文里 const 构造。可以有任意多处 `submit!`，分散在任意多个 crate。
- **`inventory::iter::<T>`**：一个实现了 `IntoIterator<Item = &'static T>` 的值，枚举**所有** `submit!` 进来的条目。

关键是分开宏展开、链接和初始化三个阶段。`submit!` 生成静态数据与初始化入口；链接器保留对应项；平台初始化时完成登记；`iter` 在运行时遍历已登记的条目。枚举顺序没有保证：

```mermaid
flowchart TB
    subgraph crateA["flow-rs（内置节点）"]
        A1["submit! { Doubler 条目 }"]
        A2["submit! { Transform 条目 }"]
    end
    subgraph crateB["下游 crate（业务节点）"]
        B1["submit! { MyDetector 条目 }"]
    end
    A1 --> L["链接静态数据与初始化入口"]
    A2 --> L
    B1 --> L
    L --> C["平台初始化：登记条目"]
    C --> I["inventory::iter::&lt;NodeRegistration&gt;<br/>运行时枚举全表"]
    I --> R["find(&quot;Doubler&quot;) → 构造器"]
```

两者都涉及初始化；本章的直接收益是用公共库承载注册基础设施。节点类型必须进入最终应用的链接结果，登记顺序不能作为业务约定。

## 3. 注册表模块：`registry.rs`

先定义「一条注册条目」和这张表。全在新模块 `flow-rs/src/registry.rs`：

```rust,ignore
use crate::channel::{Receiver, Sender};
use crate::node::Actor;

/// 节点构造器：吃一串输入端口 + 一串输出端口，产出类型擦除的 Box<dyn Actor>。
pub type NodeCtor = fn(Vec<Receiver>, Vec<Sender>) -> Box<dyn Actor>;

/// 注册表里的一条条目：类型名 + 构造器。
pub struct NodeRegistration {
    pub name: &'static str,
    pub ctor: NodeCtor,
}

// 声明「本 crate 收集 NodeRegistration 条目」。必须与被收集类型同 crate、在 item 位置。
inventory::collect!(NodeRegistration);

/// 枚举所有已注册节点。
pub fn registrations() -> impl Iterator<Item = &'static NodeRegistration> {
    inventory::iter::<NodeRegistration>.into_iter()
}

/// 按类型名查一条注册。Part 3 的 Graph Builder 会用它把 TOML 类型名解析成构造器。
pub fn find(name: &str) -> Option<&'static NodeRegistration> {
    registrations().find(|r| r.name == name)
}
```

两个值得停下的点：

**① `NodeCtor` 为什么是裸函数指针 `fn(..)`，而不是 `Box<dyn Fn(..)>`？** 因为 `NodeRegistration` 要能在 `submit!` 的 **`static` 上下文里 const 构造**。函数指针（指向一个具体的 `build` 函数）是 const 值；`Box<dyn Fn>` 需要堆分配，不是 const。用 `fn` 指针，条目可以静态保存。

**② `collect!` 的位置约束。** 它必须写在**定义 `NodeRegistration` 的 crate**（flow-rs）里、模块级。这是 inventory 的硬性要求——收集点与类型定义绑定。下游 crate 只 `submit!`，不 `collect!`。

## 4. `BuildFromPorts`：位置接线的构造器

`NodeCtor` 的签名是 `fn(Vec<Receiver>, Vec<Sender>) -> Box<dyn Actor>`——给一串端口，造一个节点。但每个节点的字段不同（`Doubler` 是 `inp`/`out`/`input_closed`），谁来把端口**填进**对应字段？这又是一件该由宏生成的样板。我们定义一个 trait，再用派生宏生成它：

```rust,ignore
pub trait BuildFromPorts {
    fn build(ins: Vec<Receiver>, outs: Vec<Sender>) -> Box<dyn Actor>;
}
```

> **为什么不加 `where Self: Sized`？** 一个没有 `self` 接收者的关联函数，会让 trait 不对象安全（除非加 `Self: Sized`）。但我们从不需要 `dyn BuildFromPorts`——只以 `<Doubler as BuildFromPorts>::build` 对**具体类型**取函数指针。既然不 dyn，就不必加那句仪式。

`#[derive(BuildFromPorts)]` 生成 `build` 的逻辑，和 Ch2.3 的 `#[derive(Node)]` 一样**按字段类型/名字分类**——只是这次是往字段里**填**端口，而非撤：

```rust,ignore
pub fn expand_build_from_ports(input: &DeriveInput) -> TokenStream2 {
    let name = &input.ident;
    let (ig, tg, wc) = input.generics.split_for_impl();

    let mut inits = Vec::new();
    if let Data::Struct(data) = &input.data {
        for f in data.fields.iter() {
            let Some(id) = &f.ident else { continue };
            let init = if type_contains(&f.ty, "Sender") {
                quote! { Some(outs.remove(0)) }      // 输出端口：取一个 Sender，包 Some
            } else if type_contains(&f.ty, "Receiver") {
                quote! { ins.remove(0) }             // 输入端口：取一个 Receiver
            } else if *id == "input_closed" {
                quote! { false }                     // 关闭标志：初值 false
            } else {
                quote! { Default::default() }        // 其余字段：默认值
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
```

对 `Doubler` 它生成：

```rust,ignore
impl BuildFromPorts for Doubler {
    fn build(mut ins: Vec<Receiver>, mut outs: Vec<Sender>) -> Box<dyn Actor> {
        Box::new(Doubler {
            inp: ins.remove(0),
            out: Some(outs.remove(0)),
            input_closed: false,
        })
    }
}
```

两个约定要讲清：

**① 端口按位置对应。** `ins.remove(0)`/`outs.remove(0)` 按**字段声明顺序**依次取——第一个声明的输入端口拿 `ins[0]`，第二个拿（remove 后的）新 `ins[0]`，以此类推。所以「声明顺序 = 传入端口顺序」是本章的约定，够用。真实场景里，端口该按 TOML 里的**名字**（`PortInfo`）接线，而非位置——那需要配置层，留到 **Part 3** 的 Graph Builder。

**② `Box<Doubler>` → `Box<dyn Actor>` 的自动 unsize。** `Box::new(Doubler{..})` 是 `Box<Doubler>`，而 `build` 返回 `Box<dyn Actor>`。在 return 位置 Rust 自动做 unsize coercion——前提是 `Doubler: Actor`，这由同时挂的 `#[derive(Actor)]` 保证。

## 5. `node_register!`：第三种宏形态

Ch2.2/2.3 我们写了派生宏和属性宏。过程宏还有第三种形态——**函数式宏**（function-like macro，形如 `foo!(...)`）。`node_register!("Doubler", Doubler)` 就是它：把一条注册**提交**进表。

函数式宏的入口用 `#[proc_macro]`（不是 `#[proc_macro_derive]`/`#[proc_macro_attribute]`），且它的输入是**任意 token 流**——没有现成的 `DeriveInput`/`ItemStruct` 可解析，得**自己定义语法**。我们要解析的是 `"名字", 类型路径`，于是自定义一个 `Parse`：

```rust,ignore
pub struct NodeRegisterArgs {
    pub name: LitStr,   // "Doubler"
    pub ty: Path,       // Doubler（可以是带路径的 crate::foo::Bar）
}

impl Parse for NodeRegisterArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let name: LitStr = input.parse()?;   // 先读字符串字面量
        input.parse::<Token![,]>()?;         // 吃掉逗号
        let ty: Path = input.parse()?;        // 再读类型路径
        Ok(NodeRegisterArgs { name, ty })
    }
}
```

> 这正是 Ch2.3 里「`Field` 不实现 `Parse`」那条经验的另一面：syn 里 `LitStr`、`Path`、`Token![,]` **都实现了 `Parse`**，可以直接 `input.parse()`。自定义 `Parse` 就是把这些基础件按你的语法拼起来。入口处用 `parse_macro_input!(input as node::NodeRegisterArgs)` 驱动它。

拿到 `name`/`ty` 后，生成一个 `submit!`：

```rust,ignore
pub fn expand_node_register(args: &NodeRegisterArgs) -> TokenStream2 {
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
```

`ctor` 那行是点睛：`<Doubler as BuildFromPorts>::build` 是个**函数项**，在 `ctor: NodeCtor`（fn 指针）的位置会自动强制成 fn 指针——于是条目 const 可构造、可作为静态条目。

**卫生化：这里用绝对路径 `flow_rs::`，而 Ch2.3 派生宏用裸名。** 为什么不一致？

- Ch2.3 的派生宏生成 `impl Node for Doubler`，`Node` 用**裸名**——因为它出现在能被使用处 `use` 覆盖的位置。
- `node_register!` 生成的是 **item 级的 `static`**（`submit!` 展开成静态变量），不方便要求使用处 `use` 好 `NodeRegistration`/`inventory`。所以这里硬编码**绝对路径** `flow_rs::`（下游视角）。

这也顺带解释了 Ch2.3 章末埋的伏笔——为什么「Part 4 内置节点在 flow-rs 内部用宏时」才需要 `proc-macro-crate`：`flow_rs::` 前缀是「下游 crate 里 flow-rs 叫 `flow_rs`」的假设，一旦在 **flow-rs 自己内部**用 `node_register!`，前缀就得换成 `crate::`。`proc-macro-crate` 正是编译期查出「当前 crate 里 flow-rs 叫什么」来生成正确前缀的。本章使用者都在 `flow-rs/tests/`（下游视角），绝对路径够用，proper 卫生化留到真正需要时。

为了让 `flow_rs::inventory::submit!` 能被下游定位到，flow-rs 还 `pub use inventory;` 重导出了这个 crate（`lib.rs` 里一行）——这样下游无需自己再依赖 inventory。

## 6. 端到端：注册 → 查找 → 构造 → 运行

集成测试 `flow-rs/tests/register.rs` 把整条链走通。节点定义和 Ch2.3 一模一样，只多挂一个 `#[derive(BuildFromPorts)]` 和一行 `node_register!`：

```rust,ignore
#[inputs(inp)]
#[outputs(out)]
#[derive(Node, Actor, BuildFromPorts)]
struct Doubler {}

#[methods]
impl Doubler {
    async fn exec(&mut self) -> Result<()> {
        let mut e = self.inp.recv::<i32>().await?;
        if let Some(out) = self.out.as_ref() {
            out.send(Envelope::new(e.unpack() * 2)).await?;
        }
        Ok(())
    }
}

node_register!("Doubler", Doubler);   // ← 编译期登记
```

然后**只凭字符串**把它造出来跑：

```rust,ignore
#[tokio::test]
async fn build_via_registry_and_run() {
    let reg = find("Doubler").expect("Doubler 已注册");   // 名字 → 注册条目
    let (in_tx, in_rx) = channel(8);
    let (out_tx, mut out_rx) = channel(8);
    let node = (reg.ctor)(vec![in_rx], vec![out_tx]);      // 构造器 → Box<dyn Actor>

    let handle = node.start();
    for v in [1i32, 2, 3] { in_tx.send(Envelope::new(v)).await.unwrap(); }
    drop(in_tx);

    let mut got = Vec::new();
    while let Ok(mut e) = out_rx.recv::<i32>().await { got.push(e.unpack()); }
    assert_eq!(got, vec![2, 4, 6]);                        // 与手写版行为一致
    handle.await.unwrap().unwrap();
}
```

`find("Doubler")` 命中的，正是 `node_register!` 生成并随应用链接、初始化登记的那条。这就是 Part 3 Graph Builder 的底座：**它拿到 TOML 里的类型名，`find` 出构造器，把节点造出来接进图**。

顺带，**同名巧合第三次出现**：`BuildFromPorts` 既是 trait（`flow_rs::registry`，类型命名空间）又是派生宏（`flow_derive`，宏命名空间），和 `Node`/`Actor` 一样共存——测试里两个都 `use` 了，各归其位。

## 7. 测试：两层

- **单元测试**（`node.rs` 内，2 个新增）：`build_from_ports_wires_ports` 断言生成的 `build` 里 `inp: ins.remove(0)`、`out: Some(outs.remove(0))`、`input_closed: false`；`node_register_emits_submit` 断言 `node_register!` 生成 `flow_rs::inventory::submit!` + `NodeRegistration` + `<D as ..BuildFromPorts>::build`。
- **集成测试**（`register.rs`，2 个）：`doubler_is_registered` 验证 `find`/`registrations` 能按名字查到、查不到不存在的名字；`build_via_registry_and_run` 走完「名字 → 构造 → 跑出 `[2,4,6]`」。**注册是真的经过了 linker section**——集成测试是独立 crate，它的 `submit!` 与 flow-rs 里的 `collect!` 在链接时汇合，证明跨 crate 收集成立。

## 小结

- **编译期分布式注册**：节点分散在各 crate，却要在运行前汇成一张全局表。用 inventory 的静态条目与初始化机制承载注册，替换原版自建的 ctor + lazy_static 路径；仍需验证业务语义与成本。
- **三个动作**：`collect!`（定义 crate 声明收集）、`submit!`（任意 crate 提交、须 const 构造）、`iter`（枚举）。`NodeCtor` 用**裸函数指针**正是为了让条目能进 `static` 上下文。
- **`BuildFromPorts` 派生宏**：按字段类型/名字**位置接线**（`ins`/`outs` 按声明顺序 `remove`）。命名接线（TOML `PortInfo`）留到 Part 3。
- **函数式宏 `node_register!`**：过程宏的**第三种形态**。自定义 `Parse` 解析 `"名字", 类型`，生成 `submit!`。它用**绝对路径** `flow_rs::`（下游视角），与派生宏的裸名策略对照——也点明了 Part 4 内部用宏时 `proc-macro-crate` 的必要性。

**Part 2 至此收官。** 我们有了：`Node`/`Actor` 节点契约（Ch2.1）、过程宏三件套与三种宏形态（Ch2.2/2.3/2.4）、一套把节点样板塌缩掉的宏（Ch2.3）、以及一张编译期就位的节点注册表（Ch2.4）。节点能定义、能塌缩、能被发现——**万事俱备，只差把它们按图接起来跑**。

下一部分 **Part 3 · 图与运行时**才是引擎真正成形的地方：**Ch3.1** 用 serde/toml 定义图的 TOML schema 与配置解析层；**Ch3.2** 写 Graph Builder，按配置从注册表（就是本章这张表！）造出节点、用 channel 接线；**Ch3.3** 用 tokio 调度这些 actor、实现优雅停机；最终 **Ch3.4** 端到端跑通一个 `BinaryOp` 计算图——那是整本书的**大里程碑**：第一个真正能跑的引擎。

[`inventory`]: https://docs.rs/inventory
