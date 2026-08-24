# megflow-rebuild

一本手把手的中文学习型 mdbook（`book/`）+ 其配套参考实现（`code/`），指导你从零用 Rust 重写 MegFlow 引擎核心。

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
