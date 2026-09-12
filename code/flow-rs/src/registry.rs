//! flow-rs · registry —— 编译期节点注册表（Ch2.4）。
//!
//! 引擎要按 TOML 里的**类型名字符串**（如 `"Doubler"`）把节点造出来，就需要一张
//! 「名字 → 构造器」的表。难点在于：节点定义**分散在各处**（内置节点在 flow-rs、
//! 业务节点在下游 crate），却要在**运行前**汇成一张全局表——这正是「编译期分布式
//! 注册」。
//!
//! 原版用 ctor 初始化函数和 lazy_static 管理运行时表。重写使用 inventory：
//! 宏生成静态数据及初始化入口，随应用链接后由平台初始化机制登记条目，
//! 运行时通过 inventory::iter 枚举。它并不是没有运行时初始化，也不保证遍历顺序。
//!
//! 三个部件：
//! - [`NodeRegistration`]：一条注册条目 = `{ 名字, 构造器 }`。用 [`inventory::collect`]
//!   在**本 crate**声明「要收集的就是这种条目」。
//! - [`BuildFromPorts`]：节点的**位置接线构造器** trait，由 `#[derive(BuildFromPorts)]`
//!   生成——把一串输入/输出端口按字段顺序填进结构体。
//! - `node_register!("Name", Type)`（在 flow-derive）：在**任意 crate**用
//!   `flow_rs::inventory::submit!` 提交一条 `NodeRegistration`，`ctor` 指向
//!   `<Type as BuildFromPorts>::build`。
//!
//! Compile-time distributed node registry, backed by the `inventory` crate
//! (replaces the original's hand-rolled `#[flow_rs::ln]` + lazy_static table).

use crate::channel::{Receiver, Sender};
use crate::error::Result;
use crate::node::Actor;
use crate::resource::AnyResource;

/// 节点构造器：吃**节点参数表** + **分组**的输入端口 + **分组**的输出端口，产出类型擦除的
/// `Box<dyn Actor>`（失败 → `Err`）。
///
/// 相对 Ch2.4 的签名，Ch3.2 加了两样：
/// - `&Args`：节点的自有参数（`op="+"`）从这里反序列化进字段——「配置驱动」的落点。
/// - `-> Result`：参数缺失/类型错、乃至端口对不上，都在 `build()` 当场报错。
///
/// Ch4.2 又把端口从「一维位置 `Vec`」升级成「**分组** `Vec<Vec<_>>`」以支持**数组端口**：
/// 外层每个元素对应一个端口名（与 [`NodeRegistration::inputs`]/[`outputs`] 端口名表**同序**），
/// 内层 `Vec` 是这个端口名下的那一组 channel 端——**标量端口**是恰好 1 个的组、**数组端口**
/// （扇入 Merge / 扇出 Bcast）是 N 个的组。这样一个参数就同时携带了「端口顺序」与「每端口
/// 的元数」，构造器 `remove(0)`（整组）或 `remove(0).remove(0)`（组里唯一那个）即可取用。
/// Graph Builder 正是靠这层对应把「TOML 里按名接的 channel」排成「构造器要的分组 Vec」。用
/// 裸函数指针（而非闭包）是为了让条目能在 `static` 上下文里 const 构造，满足 `inventory::submit!`。
/// A node constructor: args + grouped positional ports in, type-erased actor (or error) out.
pub type NodeCtor =
    fn(&crate::config::Args, Vec<Vec<Receiver>>, Vec<Vec<Sender>>) -> Result<Box<dyn Actor>>;

/// 一个已接线端点及其地址标签；标签属于接线，不能按 Vec 位置重建。
#[derive(Default)]
pub struct TaggedEndpoint<T> {
    pub tag: Option<u64>,
    pub endpoint: T,
}
impl<T> TaggedEndpoint<T> {
    pub fn new(endpoint: T, tag: Option<u64>) -> Self {
        Self { tag, endpoint }
    }
}
pub type TaggedNodeCtor = fn(
    &crate::config::Args,
    Vec<Vec<TaggedEndpoint<Receiver>>>,
    Vec<Vec<TaggedEndpoint<Sender>>>,
) -> Result<Box<dyn Actor>>;

/// 注册表里的一条条目：类型名 + 端口名表 + 数组标记表 + 构造器。
/// One registry entry: type name, port-name tables, array-ness tables, and constructor.
pub struct NodeRegistration {
    /// 节点类型名——TOML 里按它引用节点（注册表 key）。
    pub name: &'static str,
    /// 输入端口名，**按构造器填充顺序**（= 结构体里 `Receiver`/`Vec<Receiver>` 字段的声明顺序）。
    /// Graph Builder 用它把命名 channel 排成 `ctor` 要的分组 `Vec`。
    pub inputs: &'static [&'static str],
    /// 输出端口名，同上（= `Sender`/`Vec<Sender>` 字段的声明顺序）。
    pub outputs: &'static [&'static str],
    /// 与 `inputs` 并行：每个输入端口是否**数组端口**（`Vec<Receiver>` → true，扇入）。
    pub input_dict: &'static [bool],
    pub output_dict: &'static [bool],
    pub input_array: &'static [bool],
    /// 与 `outputs` 并行：每个输出端口是否**数组端口**（`Vec<Sender>` → true，扇出）。
    pub output_array: &'static [bool],
    /// 与 `inputs`/`outputs` 并行：每个端口是否**动态端口**（`DynPorts<..>` → true，Ch4.9a）。
    /// 建图期（Ch4.9b）靠它识别一条连接是否指向动态子图，从而改走 `set_port_dynamic` 注入。
    pub input_dyn: &'static [bool],
    pub output_dyn: &'static [bool],
    /// 该类型的构造器（`<T as BuildFromPorts>::build`）。
    pub input_types: fn() -> Vec<crate::config::interlayer::MsgTypeId>,
    pub output_types: fn() -> Vec<crate::config::interlayer::MsgTypeId>,
    pub ctor: NodeCtor,
    pub tagged_ctor: TaggedNodeCtor,
}

impl NodeRegistration {
    pub fn input_is_dict(&self, port: &str) -> bool {
        self.inputs
            .iter()
            .position(|p| *p == port)
            .and_then(|i| self.input_dict.get(i))
            .copied()
            .unwrap_or(false)
    }
    pub fn output_is_dict(&self, port: &str) -> bool {
        self.outputs
            .iter()
            .position(|p| *p == port)
            .and_then(|i| self.output_dict.get(i))
            .copied()
            .unwrap_or(false)
    }

    /// 该输入端口是否数组端口（查名表定位、再取并行的 `input_array`；查无此端口 → false）。
    /// Whether the named input port is an array (variadic) port.
    pub fn input_is_array(&self, port: &str) -> bool {
        self.inputs
            .iter()
            .position(|&p| p == port)
            .map(|i| self.input_array[i])
            .unwrap_or(false)
    }

    /// 该输出端口是否数组端口。/ whether the named output port is an array port.
    pub fn output_is_array(&self, port: &str) -> bool {
        self.outputs
            .iter()
            .position(|&p| p == port)
            .map(|i| self.output_array[i])
            .unwrap_or(false)
    }

    /// 该输入端口是否**动态端口**（`DynPorts<Receiver..>`；查无此端口或表更短 → false）。
    /// 用防御式 `.get(i)`（同 dict）：手写 `BuildFromPorts` 靠 `INPUT_DYN = &[]` 默认值即安全。
    pub fn input_is_dyn(&self, port: &str) -> bool {
        self.inputs
            .iter()
            .position(|p| *p == port)
            .and_then(|i| self.input_dyn.get(i))
            .copied()
            .unwrap_or(false)
    }
    /// 该输出端口是否**动态端口**（`DynPorts<Sender..>`）。
    pub fn output_is_dyn(&self, port: &str) -> bool {
        self.outputs
            .iter()
            .position(|p| *p == port)
            .and_then(|i| self.output_dyn.get(i))
            .copied()
            .unwrap_or(false)
    }
}

// 声明「本 crate 收集 `NodeRegistration` 条目」。`collect!` 必须与被收集类型同 crate，
// 且在 item 位置（模块级）。各处 submit! 的条目随应用链接后通过初始化机制登记。
inventory::collect!(NodeRegistration);

/// 「从端口构造节点」的 trait，由 `#[derive(BuildFromPorts)]` 生成实现。
///
/// 生成的 `build` 把**分组**的 `ins`/`outs`（`Vec<Vec<_>>`，每组对应一个端口名）按**字段
/// 声明顺序**填入结构体：标量输入字段取组里唯一的 `Receiver`（`ins.remove(0).remove(0)`）、
/// 数组输入字段整组搬走（`ins.remove(0)`）、标量输出包成 `Some`、数组输出整组搬走、
/// `input_closed` 置 `false`、其余字段（节点自有参数）用 [`crate::config::arg`] 从 `args`
/// 按字段名反序列化。`INPUTS`/`OUTPUTS` 与并行的 `INPUT_ARRAY`/`OUTPUT_ARRAY` 是同一次字段
/// 遍历里收集的端口名表 + 数组标记表（与填充顺序同序），交给 Graph Builder 做「名字 → 位置」
/// 的桥、并决定每个端口能接几条边。
///
/// 不设 `where Self: Sized`——我们只以 `<ConcreteType as BuildFromPorts>::build` 取函数
/// 指针 / `::INPUTS` 取常量，从不需要 `dyn BuildFromPorts`。
/// Constructs a node from args + grouped ports; impl generated by `#[derive(BuildFromPorts)]`.
pub trait BuildFromPorts {
    /// 输入端口名，按 `build` 消费 `ins` 的顺序。/ input port names, in fill order.
    const INPUTS: &'static [&'static str];
    /// 输出端口名，按 `build` 消费 `outs` 的顺序。/ output port names, in fill order.
    const OUTPUTS: &'static [&'static str];
    /// 与 `INPUTS` 并行的数组标记：每个输入端口是否 `Vec<Receiver>`（数组端口）。
    const INPUT_DICT: &'static [bool] = &[];
    const OUTPUT_DICT: &'static [bool] = &[];
    /// 与 `INPUTS`/`OUTPUTS` 并行的动态端口标记（`DynPorts<..>` → true，Ch4.9a）。默认 `&[]`
    /// ——手写 `BuildFromPorts`（如 `tests/tagged_constructor.rs`）无需声明，`input_is_dyn` 防御式回退 false。
    const INPUT_DYN: &'static [bool] = &[];
    const OUTPUT_DYN: &'static [bool] = &[];
    const INPUT_ARRAY: &'static [bool];
    /// 与 `OUTPUTS` 并行的数组标记：每个输出端口是否 `Vec<Sender>`（数组端口）。
    const OUTPUT_ARRAY: &'static [bool];

    /// 与端口名表同序；旧的无类型手写构造器默认声明 Any。
    fn input_types() -> Vec<crate::config::interlayer::MsgTypeId> {
        vec![crate::config::interlayer::MsgTypeId::Any; Self::INPUTS.len()]
    }
    fn output_types() -> Vec<crate::config::interlayer::MsgTypeId> {
        vec![crate::config::interlayer::MsgTypeId::Any; Self::OUTPUTS.len()]
    }

    /// 标量和列表端口不消费标签，沿用 build；字典实现必须覆盖此方法。
    fn build_tagged(
        args: &crate::config::Args,
        ins: Vec<Vec<TaggedEndpoint<Receiver>>>,
        outs: Vec<Vec<TaggedEndpoint<Sender>>>,
    ) -> Result<Box<dyn Actor>> {
        Self::build(
            args,
            ins.into_iter()
                .map(|group| group.into_iter().map(|p| p.endpoint).collect())
                .collect(),
            outs.into_iter()
                .map(|group| group.into_iter().map(|p| p.endpoint).collect())
                .collect(),
        )
    }

    /// 用 `args` 填自有参数、按顺序接好**分组**端口，返回擦除后的节点（失败 → `Err`）。
    /// Fill args, wire grouped ports by order; return the type-erased node (or error).
    fn build(
        args: &crate::config::Args,
        ins: Vec<Vec<Receiver>>,
        outs: Vec<Vec<Sender>>,
    ) -> Result<Box<dyn Actor>>;
}

/// 枚举所有已注册节点（初始化登记的全局表）。
/// Iterate over every registered node.
pub fn registrations() -> impl Iterator<Item = &'static NodeRegistration> {
    inventory::iter::<NodeRegistration>.into_iter()
}

/// 按类型名查一条注册。Part 3 的 Graph Builder 会用它把 TOML 里的类型名解析成构造器。
/// Look up one registration by type name.
pub fn find(name: &str) -> Option<&'static NodeRegistration> {
    registrations().find(|r| r.name == name)
}

// ── 资源注册表（Ch4.3）─────────────────────────────────────────────────────
// 与上面的节点注册表**对偶**，但简单得多：资源没有端口、没有 arity，只有「类型名 → 构造器」。
// 构造器吃一份配置参数 `args`、产出类型擦除的 [`AnyResource`]（`Arc<dyn Any+Send+Sync>`）。
// 复用同一套 [`inventory`] 机制：`resource_register!`（flow-derive）在**任意 crate** `submit!`
// 一条条目，经初始化登记到一张全局表，`find_resource` 按名查出。
// A dual, simpler registry for resources: name → constructor, same `inventory` backing.

/// 资源构造器：吃配置参数、产出类型擦除的共享资源（失败 → `Err`）。裸函数指针，可 const 构造。
/// A resource constructor: args in, type-erased shared resource (or error) out.
pub type ResCtor = fn(&crate::config::Args) -> Result<AnyResource>;

/// 资源注册表的一条条目：类型名 + 构造器。对偶于 [`NodeRegistration`]，但没有端口/arity 信息。
/// One resource-registry entry: type name + constructor (dual to `NodeRegistration`).
pub struct ResourceRegistration {
    /// 资源类型名——TOML 的 `[[graphs]].resources` 里按它引用（注册表 key）。
    pub ty: &'static str,
    /// 该类型的构造器（`resource::build_arc::<T>`）。
    pub ctor: ResCtor,
}

// 声明「本 crate 收集 `ResourceRegistration` 条目」。各处 `resource_register!` 的条目通过初始化机制登记。
inventory::collect!(ResourceRegistration);

/// 枚举所有已注册资源类型（初始化登记的全局表）。
/// Iterate over every registered resource type.
pub fn resource_registrations() -> impl Iterator<Item = &'static ResourceRegistration> {
    inventory::iter::<ResourceRegistration>.into_iter()
}

/// 按类型名查一条资源注册。Graph Builder 用它把 `resources` 里的 `ty` 解析成构造器。
/// Look up one resource registration by type name.
pub fn find_resource(ty: &str) -> Option<&'static ResourceRegistration> {
    resource_registrations().find(|r| r.ty == ty)
}
