# Ch2.4 node_register! 与 inventory 编译期注册表

Ch2.3 我们把节点样板集中为几行声明。但还差最后一环：写好的节点，引擎怎么**发现**它？图配置里只写类型名字符串 `"Doubler"`，引擎得据此把节点**造出来**。本章用函数式过程宏生成注册条目，再由 inventory 的平台初始化机制登记，运行时按名字查构造器。宏展开、登记和创建业务对象发生在不同阶段。

本章续着 Ch2.3 的累积工程往下写——起点是第二十步（五宏塌缩完成的 `Doubler`），第二十一~二十二步给它加上注册表：**第二十一步**立「表 + `BuildFromPorts` 契约 + 派生宏」，**第二十二步**加函数式宏 `node_register!`、走通「按名字查出来跑」。每步都是一份**可编译、自带测试**的文件，抄进工程跑通再走下一步。想先手写一遍名字到构造器的表、再理解 inventory 的分散登记，可选读 [注册表实作](ch04b-registry-workshop.md)。

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

先隔离验证 inventory 本身。创建独立 `inventory-study/` 目录，完整 `Cargo.toml` 如下：

```toml
[package]
name = "inventory-study"
version = "0.1.0"
edition = "2021"

[workspace]

[dependencies]
inventory = "0.3"
```

本实验只需 inventory，无需 feature 开关，不依赖 MegFlow 或 Tokio。下面是完整 `src/main.rs`：

```rust
{{#include ../../../code/flow-rs/examples/registry_basics.rs}}
```

两个模块各提交一条记录，main 枚举后先按名字排序再验证结果，避免把不保证的登记顺序写进业务。这里的 run 是运行函数指针，不是后面 NodeCtor 的构造函数指针；它们共用“静态记录保存函数地址”的原理。

从实验目录运行 `cargo run`，预期输出：

```text
注册与调用通过：[("double", 6), ("increment", 4)]
```

故意删除 increment 的 submit，条目数量断言应失败；恢复后重新通过。独立练习：增加 square 模块，输入 3 应得到 9，同时更新排序后的预期结果。collect 和迭代逻辑不应修改。

教材仓库根目录的 `python3 scripts/check_registry_course.py` 会在新临时目录编译这个独立工程及上一节的标准库实验，验证精确输出；不构建 flow-rs 运行时。

### 从独立登记转到节点构造

先定义「一条注册条目」和这张表。它们全在累积工程的新模块 `src/registry.rs` 里（**本章阶段示意**：终点源码的 `NodeRegistration`/`NodeCtor` 会长出更多字段，见下方落差说明）——模块顶部先 `use crate::channel::{Receiver, Sender};` 和 `use crate::node::Actor;` 引入端口类型与 `Actor`：

```rust
{{#include ../../labs/node-steps/21/registry.rs:table}}
```

> **与终点源码的落差**：`inventory::collect!` / `registrations` / `find` 三者到终点**一字未改**，可放心照抄。但 `NodeRegistration` 与 `NodeCtor` 会随后续章节**长出更多字段**——Ch3.2 给 `NodeCtor` 加了 `&Args` 入参并把返回改成 `Result`（配置驱动 + 建图期报错）、给条目加了 `inputs`/`outputs` 端口名表；Ch4.2 又把端口从一维 `Vec` 升成分组 `Vec<Vec<_>>`（数组端口）、加了 `input_array`/`output_array` 标记表；Ch4.3 还并列加了一张**资源**注册表 `ResourceRegistration`。完整终点见 `code/flow-rs/src/registry.rs`。本章先把「名字 → 构造器」这条主干立住，字段的生长留给后面各章按需引入。

代码里的两处文档注释已点明关键：`NodeCtor` 用**裸函数指针** `fn(..)` 而非 `Box<dyn Fn(..)>`，是为了让 `NodeRegistration` 能在 `submit!` 的 **`static` 上下文里 const 构造**——函数指针（指向某个具体的 `build`）是 const 值，`Box<dyn Fn>` 要堆分配、不是 const；而 `inventory::collect!` 必须写在**定义 `NodeRegistration` 的 crate**（本累积工程）里、模块级——收集点与类型定义绑定，下游 crate 只 `submit!`、不 `collect!`。

## 4. `BuildFromPorts`：位置接线的构造器

`NodeCtor` 的签名是 `fn(Vec<Receiver>, Vec<Sender>) -> Box<dyn Actor>`——给一串端口，造一个节点。但每个节点的字段不同（`Doubler` 是 `inp`/`out`/`input_closed`），谁来把端口**填进**对应字段？这又是一件该由宏生成的样板。我们定义一个 trait，再用派生宏生成它（**本章阶段示意**：终点 `build` 会带 `&Args` 入参、返回 `Result`、端口分组成 `Vec<Vec<_>>`，见 §5 末落差说明与 `code/flow-rs/src/registry.rs`）：

```rust
{{#include ../../labs/node-steps/21/registry.rs:build_from_ports}}
```

trait 上的文档注释解释了**为什么不加 `where Self: Sized`**：`build` 没有 `self` 接收者，本会让 trait 不对象安全（除非补 `Self: Sized`）；但我们从不需要 `dyn BuildFromPorts`——只对具体类型取 `<Doubler as BuildFromPorts>::build` 这个函数指针，既然不 dyn，就不必加那句仪式。

`#[derive(BuildFromPorts)]` 生成 `build` 的逻辑，和 Ch2.3 的 `#[derive(Node)]` 一样**按字段类型分类**——而且**直接复用第十八步写的精确分类** `is_output_port`（恰好 `Option<Sender>`）/ `type_is`（恰好 `Receiver`，不靠字符串包含），只是方向相反：`derive(Node)` 在 `close` 里把输出端口**撤**成 `None`，这里在 `build` 里把端口**填**进字段（**本章阶段示意**：终点用 `port_kind` 精确分类、并处理数组/字典/类型化/参数字段，见 §5 末落差说明）：

```rust
{{#include ../../labs/node-steps/21/derive-lib.rs:expand_build_from_ports}}
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

```rust
{{#include ../../labs/node-steps/22/derive-lib.rs:node_register_args}}
```

> 这正是 Ch2.3 里「`Field` 不实现 `Parse`」那条经验的另一面：syn 里 `LitStr`、`Path`、`Token![,]` **都实现了 `Parse`**，可以直接 `input.parse()`。自定义 `Parse` 就是把这些基础件按你的语法拼起来。入口处用 `parse_macro_input!(input as NodeRegisterArgs)` 驱动它。
>
> **教学版这里是非 pub 的 `struct`**：`proc-macro` crate 的**根**（`derive/src/lib.rs`）不能导出宏以外的公有项，而教学版把宏入口与展开逻辑合在这一个文件里，所以解析器结构体只能非 pub（反正同文件内用，本就不需要 pub）。终点 `code/flow-derive/` 把它挪进 `node` 子模块，才写成 `pub struct`，供入口 `parse_macro_input!(input as node::NodeRegisterArgs)` 跨模块引用。

拿到 `name`/`ty` 后，生成一个 `submit!`（**本章阶段示意**：终点的条目还带端口名表 `INPUTS`/`OUTPUTS`、数组标记 `INPUT_ARRAY`/`OUTPUT_ARRAY`、类型表等字段，见下方落差说明）：

```rust
{{#include ../../labs/node-steps/22/derive-lib.rs:expand_node_register}}
```

`ctor` 那行是点睛：`<Doubler as BuildFromPorts>::build` 是个**函数项**，在 `ctor: NodeCtor`（fn 指针）的位置会自动强制成 fn 指针——于是条目 const 可构造、可作为静态条目。

> **§3~§5 与终点源码的落差小结**：本章为把「编译期分布式注册」这条主干讲清，`NodeCtor` / `NodeRegistration` / `BuildFromPorts::build` / `expand_build_from_ports` / `expand_node_register` 五处都用了**Ch2.4 阶段的单端口简化形态**（端口是一维位置 `Vec`、无参数、`build` 不返回 `Result`）。终点 `code/flow-rs/src/registry.rs` 与 `code/flow-derive/src/node.rs` 里它们已升级为：`build(&Args, Vec<Vec<Receiver>>, Vec<Vec<Sender>>) -> Result<..>`（Ch3.2 配置驱动 + Ch4.2 分组数组端口），条目并列 `INPUTS`/`OUTPUTS`/`INPUT_ARRAY`/`OUTPUT_ARRAY`/类型表，`node_register!` 一并提交这些字段，另有对偶的资源注册表（Ch4.3）。差异按章逐步引入，此处只需理解 inventory 主干；`NodeRegisterArgs` 解析器的解析逻辑到终点未变，只是终点把它挪进 `node` 子模块、写成 `pub struct`（见上）。

**卫生化：这里用绝对路径 `flow_rs::`，而 Ch2.3 派生宏用裸名。** 为什么不一致？

- Ch2.3 的派生宏生成 `impl Node for Doubler`，`Node` 用**裸名**——因为它出现在能被使用处 `use` 覆盖的位置。
- `node_register!` 生成的是 **item 级的 `static`**（`submit!` 展开成静态变量），不方便要求使用处 `use` 好 `NodeRegistration`/`inventory`。所以这里硬编码**绝对路径** `flow_rs::`（下游视角）。

当前框架在 lib.rs 中写 `extern crate self as flow_rs;`，使内部调用也能解析 `flow_rs::`。这解决了 crate 内自引用，不解决下游在 Cargo.toml 中给依赖改名的情况。后者才需要考虑 `proc-macro-crate` 或显式传入路径；具体方案见宏专题第 7 课。item 位置本身并不禁止 use；选择明确路径是为了减少生成代码对调用处导入的依赖。

为了让 `flow_rs::inventory::submit!` 能被下游定位到，flow-rs 还 `pub use inventory;` 重导出了这个 crate（`lib.rs` 里一行）——这样下游无需自己再依赖 inventory。

## 6. 端到端：注册 → 查找 → 构造 → 运行

集成测试 `tests/register.rs`（累积工程里的独立测试 crate，扮演「下游使用者」）把整条链走通。节点定义和 Ch2.3 一模一样，只多挂一个 `#[derive(BuildFromPorts)]` 和一行 `node_register!`：

```rust
{{#include ../../labs/node-steps/22/register.rs:node_def}}
```

然后**只凭字符串**把它造出来跑：

```rust
{{#include ../../labs/node-steps/22/register.rs:lookup}}
```

`find("Doubler")` 命中的，正是 `node_register!` 生成并随应用链接、初始化登记的那条。这就是 Part 3 Graph Builder 的底座：**它拿到 TOML 里的类型名，`find` 出构造器，把节点造出来接进图**。

> **与终点的落差**：上面正文里 `ctor` 的签名就是主干形态——`(reg.ctor)(vec![in_rx], vec![out_tx])`、`node.start()` 无参。全书终点 `code/flow-rs/tests/register.rs` 里它会长成 `(reg.ctor)(&flow_rs::config::Args::new(), vec![vec![in_rx]], vec![vec![out_tx]])`——构造器多了 `&Args` 入参（Ch3.2）、端口按分组 `vec![vec![..]]` 传（Ch4.2 数组端口）、`start(Context::anonymous())` 带空上下文（Ch4.3）。那些多出来的参数都是后续章节按需引入的，主干「名字 → 构造器 → 跑出 `[2,4,6]`」完全一致。

顺带，**同名巧合第三次出现**：`BuildFromPorts` 既是 trait（`flow_rs::registry`，类型命名空间）又是派生宏（`flow_derive`，宏命名空间），和 `Node`/`Actor` 一样共存——测试里两个都 `use` 了，各归其位。

## 7. 测试：两层

- **单元测试**（累积工程 `derive/src/lib.rs` 的 `mod tests` 内）：`build_from_ports_wires_ports_by_position` 断言生成的 `build` 里输入端口按位置 `ins.remove(0)`、输出端口 `Some(outs.remove(0))`、`input_closed: false`；`node_register_emits_submit` 断言 `node_register!` 生成 `flow_rs::inventory::submit!` + `flow_rs::registry::NodeRegistration` + `<Doubler as ..BuildFromPorts>::build`。（终点这些断言随字段生长扩充——还核对 `INPUTS`/`OUTPUTS`/`INPUT_ARRAY` 等，跑 `cargo test -p flow-derive --manifest-path code/Cargo.toml` 可见。）
- **集成测试**（`register.rs`，2 个）：`doubler_is_registered` 验证 `find`/`registrations` 能按名字查到、查不到不存在的名字；`build_via_registry_and_run` 走完「名字 → 构造 → 跑出 `[2,4,6]`」。**注册是真的经过了 linker section**——集成测试是独立 crate，它的 `submit!` 与 flow-rs 里的 `collect!` 在链接时汇合，证明跨 crate 收集成立。

## 8. 本章终点与复现

**起点**：Ch2.3 结束时的累积工程（第二十步——五个节点宏，能塌缩出 `Doubler`）。

**两步长出注册表**（每步都是一份可编译、自带测试的文件，抄进累积工程跑通再走下一步）：

- **第二十一步**：给工程加 `inventory` 依赖（`Cargo.toml`）；新建 `src/registry.rs`（`NodeRegistration` + `inventory::collect!` + `registrations`/`find` + `BuildFromPorts` 契约），`src/lib.rs` 加 `pub mod registry;` + `pub use inventory;` + `extern crate self as flow_rs;`；`derive/src/lib.rs` 加 `#[derive(BuildFromPorts)]`。新增独立集成测试 `tests/register.rs`，先直接 `build()` 跑通「填端口 → 造节点」。
- **第二十二步**：`derive/src/lib.rs` 加函数式宏 `node_register!`（第三种宏形态：`NodeRegisterArgs` 解析器 + `expand_node_register`）；`tests/register.rs` 补上 `node_register!("Doubler", Doubler)` 一行 + 按名字查找/构造/运行的端到端测试。

**验收命令**（照抄可跑）：

```bash
python3 scripts/check_basic_channel_course.py   # 从空目录累积构建二十二步，每步 cargo test
```

**对照成品**（可选）——全书终点的 `flow-derive` 单元测试与跨 crate 注册端到端：

```bash
cargo test -p flow-derive --manifest-path code/Cargo.toml --locked              # BuildFromPorts / node_register! 展开单测
cargo test --manifest-path code/Cargo.toml -p flow-rs --test register --locked  # 跨 crate 注册端到端
```

正文 §3~§7 的每个代码块都由 `{{#include}}` 取自 labs 第二十一~二十二步的可编译文件，抄进去就能跑（§3 开头那份 `inventory-study/` 独立实验，是 `inventory` 这个第三方 crate 的入门示范，仍由仓库 `code/` 提供、`scripts/check_registry_course.py` 单独验证）。教学版是**单端口版**——`NodeCtor`/`NodeRegistration`/`BuildFromPorts::build`/`expand_*` 都用一维位置 `Vec`、无参数、`build` 不返回 `Result`；全书终点 `code/flow-rs/src/registry.rs` 与 `code/flow-derive/src/node.rs` 已按 Ch3.2/4.2/4.3 长出 `&Args`、`Result`、分组数组端口与对偶的资源注册表，差异见各处落差说明与 [注册表实作](ch04b-registry-workshop.md)。

## 小结

- **编译期分布式注册**：节点分散在各 crate，却要在运行前汇成一张全局表。用 inventory 的静态条目与初始化机制承载注册，替换原版自建的 ctor + lazy_static 路径；仍需验证业务语义与成本。
- **三个动作**：`collect!`（定义 crate 声明收集）、`submit!`（任意 crate 提交、须 const 构造）、`iter`（枚举）。`NodeCtor` 用**裸函数指针**正是为了让条目能进 `static` 上下文。
- **`BuildFromPorts` 派生宏**：按字段类型/名字**位置接线**（`ins`/`outs` 按声明顺序 `remove`）。命名接线（TOML `PortInfo`）留到 Part 3。
- **函数式宏 `node_register!`**：过程宏的第三种形态。自定义 Parse 解析 `"名字", 类型`，生成 submit。明确路径减少对调用处 use 的依赖；内部自别名与下游依赖改名是两个不同问题。

**Part 2 至此收官。** 我们有了：`Node`/`Actor` 节点契约（Ch2.1）、过程宏三件套与三种宏形态（Ch2.2/2.3/2.4）、一套把节点样板塌缩掉的宏（Ch2.3）、以及一张编译期就位的节点注册表（Ch2.4）。节点能定义、能塌缩、能被发现——**万事俱备，只差把它们按图接起来跑**。

下一部分 **Part 3 · 图与运行时**才是引擎真正成形的地方：**Ch3.1** 用 serde/toml 定义图的 TOML schema 与配置解析层；**Ch3.2** 写 Graph Builder，按配置从注册表（就是本章这张表！）造出节点、用 channel 接线；**Ch3.3** 用 tokio 调度这些 actor、实现优雅停机；最终 **Ch3.4** 端到端跑通一个 `BinaryOp` 计算图——那是整本书的**大里程碑**：第一个真正能跑的引擎。

[`inventory`]: https://docs.rs/inventory
