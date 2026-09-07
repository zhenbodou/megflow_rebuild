# megflow-rebuild

一本手把手的中文学习型 mdbook（`book/`）+ 其配套参考实现（`code/`），目标是指导你从零完成 MegFlow 的完整 Rust 重构；当前实现及尚待补齐的能力见验收账本。

## 目录结构

- `book/` —— mdbook 源。`cd book && mdbook serve` 本地预览，`mdbook build` 产出静态站点。
- `code/` —— cargo workspace 参考实现（`flow-message` / `flow-derive` / `flow-rs`）。`cd code && cargo test` 全绿。
- `docs/specs/` —— 设计文档（spec）。
- `docs/plans/` —— 逐 Part 实现计划。

## 快速开始

```bash
# 读书
cd book && mdbook serve --open

# 构建参考实现
cd code && cargo build --workspace && cargo test --workspace
```

## 依赖与边界

参考实现只用 crates.io 公共 crate，不依赖任何闭源/私有注册表。详见 `docs/specs/`。


## Rust 宏学习与验收

宏学习从独立的 [Rust 宏完整教程](book/src/macros/00-roadmap.md) 开始，共 10 课，
依次学习声明宏、Token/AST、三种过程宏、泛型约束、属性组合、诊断测试和发布维护，
最后回到 MegFlow 实战。每阶段配代码、运行命令和练习。
独立实验位于 `code/macro-labs/`，不依赖完整引擎；分步代码可以用
`scripts/macro_checkpoint.py` 导出（用法见教程第 5 课）。

在项目根目录执行：

```bash
cargo test --manifest-path code/Cargo.toml --workspace --locked
python3 scripts/check_macro_course.py
```

完整 Rust 功能仍在补齐中，当前缺口及原版证据见
[完整重构验收账本](book/src/part0/ch04-completeness-audit.md)。
宏课程的通过不等于动态子图、插件服务等功能已经对齐。
