# 目录

[前言](introduction.md)

# 第 0 部分 · 全景与环境

- [Ch0.1 什么是 dataflow / actor，MegFlow 全景](part0/ch01-panorama.md)
- [Ch0.2 开发环境与项目骨架](part0/ch02-environment.md)
- [Ch0.3 跑通真实 flow-rs，钉死验收标准](part0/ch03-reference.md)

# 第 1 部分 · 消息与异步地基

- [Ch1.1 Rust 复习：并发下的所有权、借用、生命周期 + 错误处理](part1/ch01-rust-review.md)
- [Ch1.2 泛型、trait、trait 对象 dyn、Any 与 downcast](part1/ch02-traits-dyn.md)
- [Ch1.3 实现 Envelope 消息信封与类型擦除消息层](part1/ch03-envelope.md)
- [Ch1.4 async/await、Future、tokio 入门 → channel 封装](part1/ch04-async-channel.md)

# 第 2 部分 · 节点与过程宏

- [Ch2.1 Node / Actor trait、端口、exec 循环（手写不用宏）]()
- [Ch2.2 过程宏入门：proc-macro2 / syn / quote]()
- [Ch2.3 实现 inputs / outputs / derive(Node) / methods 宏]()
- [Ch2.4 node_register! 与 inventory 编译期注册表]()

# 第 3 部分 · 图与运行时

- [Ch3.1 serde / toml 与图 TOML schema → 配置解析层]()
- [Ch3.2 Graph Builder：装配节点与 channel]()
- [Ch3.3 tokio 调度：spawn actor、start/stop、优雅停机]()
- [Ch3.4 端到端跑通 BinaryOp（大里程碑）+ Sandbox 测试框架]()

# 第 4 部分 · 内置节点与高级特性

- [Ch4.1 transform / noop / bcast 广播 + add_cvt_func]()
- [Ch4.2 merge / demux / reorder（多路复用与重排序）]()
- [Ch4.3 Resource 与 Context：共享模型 / 内存池]()
- [Ch4.4 子图 subgraph、多图 graphs、动态子图]()

# 第 5 部分 · 兼容 · 优化 · 收尾

- [Ch5.1 对齐真实 API，跑真实算法仓风格的图 + pplcore 边界]()
- [Ch5.2 优化与更少 bug：逐条对比原版]()
- [Ch5.3 全景回顾 + 进阶指路]()
