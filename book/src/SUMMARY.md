# 目录

[前言](introduction.md)

# 第 0 部分 · 全景与环境

- [Ch0.1 什么是 dataflow / actor，MegFlow 全景](part0/ch01-panorama.md)
- [Ch0.2 开发环境与项目骨架](part0/ch02-environment.md)
- [Ch0.3 对照原版示例，建立首个验收标准](part0/ch03-reference.md)
- [Ch0.4 完整重构的验收账本](part0/ch04-completeness-audit.md)
- [Ch0.5 从看懂代码到独立开发：学习方法与完整示范](part0/ch05-learn-to-build.md)

# 第 1 部分 · 消息与异步地基

- [Ch1.1 Rust 复习：并发下的所有权、借用、生命周期 + 错误处理](part1/ch01-rust-review.md)
- [Ch1.2 泛型、trait、trait 对象 dyn、Any 与 downcast](part1/ch02-traits-dyn.md)
- [Ch1.3 实现 Envelope 消息信封与类型擦除消息层](part1/ch03-envelope.md)
- [Ch1.4 async/await、Future、tokio 入门 → channel 封装](part1/ch04-async-channel.md)
- [Ch1.4a 异步三步实验：Future、背压与关闭](part1/ch04a-async-workshop.md)
- [Ch1.4b 通道协议：容量、批量、限时与竞争接收](part1/ch04b-channel-protocols.md)
- [Ch1.4c 类型化与默认端点](part1/ch04c-typed-endpoints.md)
- [Ch1.4d 类型信息与转换表](part1/ch04d-type-conversion.md)
- [Ch1.4e 取消、超时与任务错误](part1/ch04e-cancellation-errors.md)

# 独立专题 · Rust 宏：从入门到专家实践

- [学习路线与验收标准](macros/00-roadmap.md)
- [第 1 课：宏是什么，怎样读展开结果](macros/01-basics.md)
- [第 2 课：声明宏的递归、歧义与卫生性](macros/02-declarative.md)
- [第 3 课：过程宏与编译阶段](macros/03-procedural.md)
- [第 4 课：proc-macro2、syn、quote 实操](macros/04-tokens.md)
- [第 5 课：从零创建三种过程宏](macros/05-three-forms.md)
- [第 6 课：泛型、辅助属性与精确约束](macros/06-generics.md)
- [第 7 课：路径、属性组合与 AST 改写](macros/07-composition.md)
- [第 8 课：诊断、编译测试与运行语义](macros/08-engineering.md)
- [第 9 课：性能、兼容性与发布维护](macros/09-maintenance.md)
- [第 10 课：crate 协作与 MegFlow 毕业实战](macros/10-megflow.md)

# 第 2 部分 · 用宏实现节点

- [Ch2.1 Node / Actor trait、端口、exec 循环（手写不用宏）](part2/ch01-node-trait.md)
- [Ch2.1a 手写节点实作：逐步装配与错误收尾](part2/ch01a-manual-actor-workshop.md)
- [Ch2.3 实现 inputs / outputs / derive(Node) / methods 宏](part2/ch03-node-macros.md)
- [Ch2.3b 从手写实现追踪宏展开](part2/ch03b-expansion-walkthrough.md)
- [Ch2.4 node_register! 与 inventory 注册表](part2/ch04-registry.md)
- [Ch2.4b 注册表实作：从函数指针到按名构造](part2/ch04b-registry-workshop.md)

# 第 3 部分 · 图与运行时

- [Ch3.1 serde / toml 与图 TOML schema → 配置解析层](part3/ch01-config.md)
- [Ch3.1a 配置实作：解析成功之后，还缺什么](part3/ch01a-config-workshop.md)
- [Ch3.2 Graph Builder：装配节点与 channel](part3/ch02-graph-builder.md)
- [Ch3.2a 接线实作：把名字变成真实端点](part3/ch02a-wiring-workshop.md)
- [Ch3.3 tokio 调度：spawn actor、start/stop、优雅停机](part3/ch03-scheduler.md)
- [Ch3.4 端到端跑通 BinaryOp（大里程碑）+ Sandbox 测试框架](part3/ch04-binaryop-e2e.md)

# 第 4 部分 · 内置节点与高级特性

- [Ch4.1 节点成链：内部连接 connections + 类型无关直通节点 Transform / Noop](part4/ch01-connections-transform.md)
- [Ch4.2 数组端口与广播/汇聚：Bcast / Merge（扇出与扇入）](part4/ch02-array-ports-bcast-merge.md)
- [Ch4.3 Resource 与 Context：共享模型 / 内存池](part4/ch03-resource-context.md)
- [Ch4.4 子图 subgraph、多图 graphs、动态子图](part4/ch04-subgraph.md)
- [Ch4.4a 子图实作：亲手追踪递归展开](part4/ch04a-subgraph-workshop.md)
- [Ch4.5 Reorder：恢复连续序列与原版对照](part4/ch05-reorder.md)

- [Ch4.6 算法消息：从矩形与跟踪状态开始](part4/ch06-algorithm-messages.md)

# 第 5 部分 · 兼容 · 优化 · 收尾

- [Ch5.1 对齐真实 API，跑真实算法仓风格的图 + pplcore 边界](part5/ch01-prelude-api-alignment.md)
- [Ch5.2 优化与更少 bug：逐条对比原版](part5/ch02-optimizations-vs-original.md)
- [Ch5.3 全景回顾与完整框架毕业验收](part5/ch03-retrospective-and-next-steps.md)
