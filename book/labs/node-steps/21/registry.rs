//! 第二十一步：编译期节点注册表的「表」与「构造器契约」。
//!
//! 本模块只放两样东西：
//! 1）一条注册条目 `NodeRegistration`（类型名 → 构造器），以及用 inventory 收集/枚举它们的
//!    `collect!` / `registrations` / `find`；
//! 2）「从端口构造节点」的契约 `BuildFromPorts`——它的 `build` 由 `#[derive(BuildFromPorts)]`
//!    生成（见 derive/src/lib.rs）。
//!
//! 本步先立「表 + 契约 + 派生宏」；下一步（第二十二步）再用函数式宏 `node_register!` 把条目
//! 提交进表，走通「按名字查出来跑」。
//!
//! **教学版单端口**：端口是一维 `Vec`、`build` 不带 `&Args`、也不返回 `Result`。全书终点
//! `code/flow-rs/src/registry.rs` 会长出分组数组端口、参数字段、`Result` 以及对偶的资源注册表
//! （Ch3.2 / Ch4.2 / Ch4.3 陆续引入）。

use crate::channel::{Receiver, Sender};
use crate::node::Actor;

// ANCHOR: table
/// 节点构造器：吃一串输入端口 + 一串输出端口，产出类型擦除的 `Box<dyn Actor>`。
///
/// 用**裸函数指针** `fn(..)` 而不是 `Box<dyn Fn(..)>`，是为了让条目能在 `submit!` 的 `static`
/// 上下文里 const 构造——函数指针（指向某个具体的 `build`）是 const 值；`Box<dyn Fn>` 要堆分配、
/// 不是 const。用 `fn` 指针，条目才能被静态保存。
pub type NodeCtor = fn(Vec<Receiver>, Vec<Sender>) -> Box<dyn Actor>;

/// 注册表里的一条条目：类型名 + 构造器。
pub struct NodeRegistration {
    pub name: &'static str,
    pub ctor: NodeCtor,
}

// 声明「本 crate 收集 NodeRegistration 条目」。必须与被收集类型同 crate、写在 item 位置（模块级）。
inventory::collect!(NodeRegistration);

/// 枚举所有已注册节点。登记顺序**没有保证**，调用方不能把它当业务约定。
pub fn registrations() -> impl Iterator<Item = &'static NodeRegistration> {
    inventory::iter::<NodeRegistration>.into_iter()
}

/// 按类型名查一条注册。Part 3 的 Graph Builder 会用它把 TOML 里的类型名解析成构造器。
pub fn find(name: &str) -> Option<&'static NodeRegistration> {
    registrations().find(|r| r.name == name)
}
// ANCHOR_END: table

// ANCHOR: build_from_ports
/// 「从端口构造节点」的契约。`#[derive(BuildFromPorts)]` 为每个节点生成它的 `build`——
/// 按字段类型把传入的端口**填进**对应字段（与 Ch2.3 `#[derive(Node)]` 按字段类型认端口是同一
/// 套路，只是这次是往字段里「填」、而非 `close` 里「撤」）。
///
/// **为什么不加 `where Self: Sized`？** `build` 没有 `self` 接收者，本会让 trait 不对象安全
/// （除非补 `Self: Sized`）。但我们从不需要 `dyn BuildFromPorts`——只对**具体类型**取
/// `<Doubler as BuildFromPorts>::build` 这个函数指针。既然不 dyn，就不必加那句仪式。
pub trait BuildFromPorts {
    fn build(ins: Vec<Receiver>, outs: Vec<Sender>) -> Box<dyn Actor>;
}
// ANCHOR_END: build_from_ports
