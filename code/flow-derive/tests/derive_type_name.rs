//! 集成测试：从**下游视角**真正 `use` 宏、展开、断言运行时行为。
//!
//! 集成测试（`tests/` 目录）是**独立的编译单元**，能像真实用户那样使用过程宏——
//! 这正是过程宏 crate 内部单元测试做不到的：那里只能测 `expand_*` 逻辑函数，
//! 无法「展开」`#[proc_macro_derive]` 入口本身（`proc_macro::TokenStream` 只在
//! 编译器的宏上下文里可用）。两种测试各补一半，合起来才完整。
//!
//! Integration test: use the derive macro as a downstream user would.

use flow_derive::TypeName;

#[derive(TypeName)]
#[allow(dead_code)] // 测试夹具：字段仅为让宏有真实输入，运行时不读取
struct Widget {
    x: i32,
}

#[derive(TypeName)]
#[allow(dead_code)] // 测试夹具：变体不构造
enum Color {
    Red,
    Green,
}

#[test]
fn struct_reports_its_name() {
    assert_eq!(Widget::type_name(), "Widget");
}

#[test]
fn enum_reports_its_name() {
    assert_eq!(Color::type_name(), "Color");
}
