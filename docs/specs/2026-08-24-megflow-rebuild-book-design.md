# MegFlow 重写 · 学习型 mdbook —— 设计文档 (spec)

- 日期：2026-08-24
- 状态：待用户评审
- 作者：Claude + douzhenbo
- 关联：原框架源码 `/data/algorithm_warehouse/megflow`（`coreteam/megflow.git`）

---

## 1. 背景与目标

用户想通过**从零用 Rust 重写 MegFlow 框架**来达到三个目的：

1. **掌握整个 MegFlow**（算法仓的运行框架）。
2. **顺便学会 Rust**（尤其是不熟的异步、trait 对象、过程宏、生命周期）。
3. 最终产出一个**功能逻辑一致、实现有优化、bug 更少**的 MegFlow 优化版。

交付物是一本 **mdbook**，它手把手、通俗易懂、够详细地指导用户一步步开发；看完书即可独立开发出这个优化版框架。

### 关键决策（已与用户确认）

| 维度 | 决策 |
|---|---|
| Rust 水平 | **有语法基础、异步不熟** → 重点补 async / trait 对象 / 宏 / 生命周期 + 用到的 crate；语言基础只做必要复习 |
| 重写范围 | **核心引擎**：channel 消息传递 + graph 图构建 + node 节点 + registry 注册 + TOML 配置解析，能加载 TOML 跑通真实 dataflow |
| 兼容目标 | **尽量可替换原版**：API / 宏 / TOML schema / 消息概念对齐真实 flow-rs，`crate` 直接沿用 `flow-rs`/`flow-message`/`flow-derive` 之名 |
| 推进方式 | **TDD、每章能编译能运行**；对本书本身也用 TDD——先写出并跑通参考实现，再落成书稿 |
| 结构路线 | **路线 A：忠实纵切 + 即时补 Rust**（吸收「早期端到端跑通」的正反馈，拒绝「前置知识灌输」） |

### 非目标（本书不覆盖，仅在末章指路）

- 图优化器（petgraph / pattern rewrite / iso 同构匹配，原版 `config/optimizer/*` 约 4K 行）。
- 调试器 / devtool / OpenTelemetry（原版 `debug/*`）。
- C FFI (`flow-cffi`) 与 Python 加载 (`loader/python`, pyo3/numpy/stackful)。
- 与闭源 `pplcore-std` / `pplcore-rs` / `mpp` 的二进制链接（见 §7 诚实边界）。

---

## 2. 框架分析：我们在重写什么

原框架 `/data/algorithm_warehouse/megflow` 是一个 Rust workspace，约 3 万行：

| 子 crate | 行数 | 职责 | 本书是否重写 |
|---|---|---|---|
| **flow-rs** | 22K | 引擎核心：channel、graph、node、registry、config(TOML+优化器)、rt(运行时)、loader(FFI)、debug | ✅ 核心子集 |
| **flow-message** | 3K | 消息类型：`Envelope`（在 flow-rs）之上的 `algo_base`（Frame/Item/Image/Feature/Rect，张量 blob-proxy） | ✅ 传输层 + ⚠️ algo_base 做到能跑真实图的最小子集 |
| **flow-derive** | 2K | 14 个过程宏（节点注册等） | ✅ 核心宏 |
| flow-cffi / flow-python | 1K / 0.4K | C / Python 绑定 | ❌ 末章指路 |
| flow-plugins | 2.8K | 内置插件节点 | 部分（内置节点在本书 Part 4 自己实现） |

### 2.1 引擎核心思想

**actor 模型 dataflow**：每个节点是一个 actor，拥有若干输入/输出**端口**；端口之间用**异步 channel** 连接；框架把每个节点 spawn 成一个 tokio 任务，**反复调用其 `exec`**；节点在 `exec` 里 `recv().await` 收消息、处理、`send().await` 发消息。整张图由 **TOML** 描述拓扑，节点通过**编译期注册表**（`inventory`）按类型名查找构造。

### 2.2 要对齐的目标 API 面（源自真实源码）

**节点写法**（`flow-rs/src/lib.rs` 文档示例）：
```rust
use flow_rs::prelude::*;

#[inputs(a: i32, b: i32)]
#[outputs(c: i32)]
#[derive(Default, Node)]
struct BinaryOp { op: char }

#[methods]
impl BinaryOp {
    fn new(_: String, args: &Args) -> BinaryOp { /* 从 args 读配置 */ }
    async fn initialize(&mut self, _: &Context, _: ResourceCollection) {}
    async fn exec(&mut self) { /* recv -> 处理 -> send */ }
    async fn finalize(&mut self) {}
}
node_register!("BinaryOp", BinaryOp);
```

**`prelude` 导出**（`flow-rs/src/lib.rs:160`）：`broker / channel / envelope / graph / node / registry / resource / rt`、`Builder`、`Args = toml::value::Table`、`Arg = toml::value::Value`、`interlayer::{MsgType, MsgTypeId, PortInfo, PortType}`、`config::optimizer`、`flow_derive::*`。

**flow-derive 的 14 个宏**（`flow-derive/src/lib.rs`）：
- 属性宏：`#[inputs]`、`#[outputs]`、`#[methods]`、`#[atest]`、`#[amain]`、`#[add_cvt_func]`
- 派生宏：`#[derive(Node)]`、`#[derive(Actor)]`（含 `local` 属性）、`#[derive(Parser)]`
- 函数宏：`node_register!`、`opt_register!`、`resource_register!`、`submit!`、`feature!`

本书**核心实现**：`inputs / outputs / Node / methods / node_register! / amain / atest / add_cvt_func`；其余（`Actor` 手动派生、`Parser`、`opt_register!`/`resource_register!`/`submit!`/`feature!`）按需最小实现或指路。

**`Envelope<M>`**（`flow-rs/src/envelope/envelope.rs`）：`new(msg)`、`unpack(&mut) -> M`、`repack<T>(&self, T) -> Envelope<T>`、`repack_inplace(&mut, M)`；含 `EnvelopeInfo`（帧序号等元信息）。

**建图 API**：`Builder::default().template(TOML).build()?` → `graph.input(name)` / `graph.output(name)` / `graph.start()`（返回 handle）/ `graph.stop()`；顶层 `flow_rs::finalize().await`。

**TOML schema**：
```toml
main = "example"
[[graphs]]
name = "example"
nodes  = [ {name="add", ty="BinaryOp", op="+"} ]
inputs = [ {name="a", cap=16, ports=["add:a"]}, {name="b", cap=16, ports=["add:b"]} ]
outputs= [ {name="c", cap=16, ports=["add:c"]} ]
```
端口引用格式 `"节点名:端口名"`；`cap` 为 channel 容量；`[[graphs]]` 可多张（子图/多图）。

**节点测试**（`Sandbox`）：`Sandbox::with_args("BinaryOp", args)` → `add_data("a", |i| ...)` / `add_check("c", |v| assert...)` / `start().await`。

### 2.3 要教的 crate 清单（"讲解明白"的对象）

| crate | 用途 | 出现章节 |
|---|---|---|
| `anyhow` / `thiserror` | 错误处理（原版用 anyhow，我们引入 thiserror 做类型化错误） | Part 1 |
| `serde` / `serde_json` / `toml` | 配置反序列化、TOML 图解析 | Part 3 |
| `tokio` | 异步运行时、任务调度、`spawn` / `JoinHandle` | Part 1 / 3 |
| `futures-util` / `futures-lite` | 异步组合子（`join!` 等） | Part 1 |
| `async-channel` / `async-broadcast` | 多生产者多消费者 / 广播 channel | Part 1 / 4 |
| `proc-macro2` / `syn` / `quote` | 过程宏三件套 | Part 2 |
| `inventory` / `ctor` | 编译期分布式注册（节点表） | Part 2 |
| `dyn-clone` / `std::any::Any` | trait 对象克隆 / 类型擦除与 downcast | Part 1 / 2 |
| `dashmap` | 并发 HashMap（资源/共享） | Part 4 |
| `tracing`（可选） | 结构化日志与可观测性 | Part 5 |

---

## 3. 教学方法论（路线 A）

1. **忠实纵切**：按引擎真实模块顺序建（消息→节点/宏→注册表→配置→图→运行时→内置节点→子图→兼容优化），最终结构与真实 flow-rs 一一对应，概念可 1:1 迁移。
2. **即时补 Rust**：每进入一个模块前，用一个**小而可运行的练习**补齐该模块正好需要的 Rust 概念；讲完立刻用于实现引擎下一块。不做前置知识大灌输。
3. **TDD、每章能运行**：每章遵循「红→绿→重构」——先写测试/示例（红），实现到通过（绿），再优化（重构）。每章结束时 `cargo build` + `cargo test` 必须全绿。
4. **对书本身也 TDD**：作者（Claude）先把该章参考实现写出来、编译、跑测试通过，再据此落成书稿。**书里出现的每段关键代码都来自真实通过测试的 `code/` 工程**，杜绝「书上能跑、抄下来报错」。
5. **通俗讲解**：每个 Rust 新概念都用「为什么需要它 → 最小例子 → 在框架里怎么用 → 常见坑」四段式；每个 crate 讲清「解决什么问题 / 核心 API / 我们用它的哪部分」。

---

## 4. 交付物与项目布局

在**本项目父目录** `/data/algorithm_warehouse/bw100_dev/` 下新建独立项目 `megflow-rebuild/`（独立 git 仓库）：

```
megflow-rebuild/
├── README.md                       # 项目说明 + 如何读这本书 + 如何构建代码
├── book/                           # mdbook 源
│   ├── book.toml                   # mdbook 配置（含 mermaid、toc 预处理器）
│   └── src/
│       ├── SUMMARY.md              # 目录（章节树）
│       ├── part0-*/...             # 各部分/各章 .md
│       └── ...
├── code/                           # 读者一步步搭出的参考实现（cargo workspace）
│   ├── Cargo.toml                  # [workspace] members = flow-message/derive/rs
│   ├── flow-message/               # 消息层 crate
│   ├── flow-derive/                # 过程宏 crate（proc-macro = true）
│   ├── flow-rs/                    # 引擎 crate
│   └── examples/                   # 可运行示例图（BinaryOp、det-attr 风格图 …）
└── docs/
    └── specs/
        └── 2026-08-24-megflow-rebuild-book-design.md   # 本文件
```

- **书正文语言**：中文（通俗易懂）；代码与关键注释中英对照。
- **crate 命名**：沿用真实名字 `flow-rs` / `flow-message` / `flow-derive`（独立 workspace，不与线上冲突），API/宏/模块名对齐真实版 → 最大化「可替换」。
- **版本快照**：`code/` 维护为「当前/最终」状态；书稿承载每章增量 diff。可选：每章打 git tag（`ch03-end` 等）便于读者对照。

---

## 5. 详细章节大纲（6 部分 · 约 22 章）

> 每章标注：**目标** · **Rust 概念** · **crate** · **引擎产物** · **验收**

### Part 0 · 全景与环境（3 章）

**Ch0.1 什么是 dataflow / actor，MegFlow 全景**
- 目标：建立心智模型——节点/端口/channel/图/运行时；给出引擎架构全景图（mermaid）与最终成品演示。
- Rust 概念：无（概念章）。验收：读者能画出数据在图中的流动路径。

**Ch0.2 开发环境与项目骨架**
- 目标：装 rustup / mdbook / mdbook-mermaid；建 workspace 骨架；讲清 `cargo` 工作流。
- crate：cargo/mdbook 工具链。引擎产物：空的三 crate workspace 能 `cargo build`。验收：`cargo build` 通过、`mdbook serve` 能看到书。

**Ch0.3 跑通真实 flow-rs，钉死验收标准**
- 目标：把原版 BinaryOp 例子跑起来，作为我们重写的「对照参照系」。
- 验收：真实 flow-rs 的 BinaryOp `1 + 2 == 3` 端到端跑通，理解每一步。

### Part 1 · 消息与异步地基（4 章）

**Ch1.1 Rust 复习：并发下的所有权、借用、生命周期 + 错误处理**
- Rust 概念：ownership/borrow/lifetime 在并发场景的含义；`Result`、`?`、`anyhow` vs `thiserror`。
- crate：`anyhow`、`thiserror`。产物：定义引擎错误类型 `FlowError`（雏形）。验收：错误类型单测通过。

**Ch1.2 泛型、trait、trait 对象 `dyn`、`Any` 与 downcast**
- Rust 概念：泛型与单态化、trait 与默认方法、`Box<dyn Trait>`、对象安全、`std::any::Any` 与 `downcast_ref`。
- crate：`dyn-clone`。产物：类型擦除容器雏形。验收：擦除+还原类型的单测通过。

**Ch1.3 实现 `Envelope<M>` 与类型擦除消息层**
- 目标：实现 `Envelope<M>`（`new/unpack/repack/repack_inplace`）+ `EnvelopeInfo`；类型擦除的 `AnyEnvelope`。
- Rust 概念：泛型结构体、`Any`、所有权转移。产物：`flow-message`/`envelope` v0。验收：`repack` 后类型/数据正确的单测。

**Ch1.4 async/await、Future、tokio 入门 → channel 封装**
- 目标：讲透 `async fn`/`.await`/`Future`/`.await` 挂起点/tokio 运行时；封装 `Sender/Receiver`（`send/recv`，容量、关闭语义）。
- Rust 概念：`Future`、`Pin`（浅）、`.await`、`tokio::spawn`。crate：`tokio`、`async-channel`、`futures-util`。产物：`channel` v0。验收：多生产者/消费者、关闭后 `recv` 返回 `Err` 的异步测试（`#[tokio::test]`）。

### Part 2 · 节点与过程宏（4 章）

**Ch2.1 Node / Actor trait、端口、exec 循环（手写不用宏）**
- 目标：定义 `Node`/`Actor` trait、`Input<T>`/`Output<T>` 端口；**手写**一个节点跑通，暴露"样板代码很烦"的痛点（为下一章的宏铺垫）。
- Rust 概念：trait 继承、关联方法、`async-trait` 模式、`Send + 'static` 约束。产物：`node` v0 + 手写节点。验收：手写节点在最小驱动下跑通。

**Ch2.2 过程宏入门：proc-macro2 / syn / quote**
- 目标：讲透过程宏三件套；写出第一个 `derive` 宏（如给结构体加个方法）。
- Rust 概念：`TokenStream`、AST、`syn::parse`、`quote!`、span 与错误、`cargo expand` 调试。crate：`proc-macro2`/`syn`/`quote`。产物：`flow-derive` 骨架 + demo 派生宏。验收：demo 宏展开正确、单测通过。

**Ch2.3 实现 `#[inputs]`/`#[outputs]`/`#[derive(Node)]`/`#[methods]`**
- 目标：把 Ch2.1 的手写样板全部宏化，得到与真实版一致的节点写法。
- Rust 概念：属性宏改写 struct、派生生成 trait impl、方法包裹。产物：核心节点宏。验收：用宏重写 Ch2.1 节点，行为不变、测试仍绿。

**Ch2.4 `node_register!` + inventory 注册表**
- 目标：实现按类型名注册/查找节点构造器；`#[amain]`/`#[atest]` 宏。
- Rust 概念：编译期分布式收集、`ctor`、静态生命周期。crate：`inventory`、`ctor`。产物：`registry`。验收：注册两个节点、按名构造成功的测试。

### Part 3 · 图与运行时（4 章）

**Ch3.1 serde/toml 与图 TOML schema → 配置解析层**
- 目标：定义并反序列化 TOML 图（main/graphs/nodes/inputs/outputs/ports）到中间表示（`interlayer`）。
- Rust 概念：`serde::Deserialize`、`#[serde(...)]`、`toml`。crate：`serde`/`toml`。产物：`config` 解析。验收：解析 BinaryOp 图为结构体、端口字符串 `"n:p"` 正确切分的单测。

**Ch3.2 Graph Builder：装配节点与 channel**
- 目标：由配置创建节点实例、按 `ports` 连线创建 channel、做**构建期静态校验**（端口存在、类型一致、无悬空）。
- Rust 概念：`Arc`、`HashMap` 拓扑、所有权在装配期的流转。产物：`graph` builder。验收：错连端口/类型不匹配在 `build()` 报明确错误的测试。

**Ch3.3 tokio 调度：spawn actor、start/stop、优雅停机**
- 目标：把每个节点 spawn 成 tokio 任务，`exec` 循环调度；实现 `start()`/`stop()`/`JoinHandle`、全输入关闭即退出。
- Rust 概念：`tokio::spawn`、`JoinHandle`、取消/关闭传播、`select!`（浅）。产物：`rt` + 调度。验收：图能启动、处理、干净停机、无悬挂任务的测试。

**Ch3.4 端到端跑通 BinaryOp（大里程碑）+ Sandbox 测试框架**
- 目标：`graph.input/output`、`Builder::template().build()`；实现 `Sandbox`（`with_args/add_data/add_check/start`）。
- 产物：完整可跑的最小引擎。验收：**与原版等价的 `1 + 2 == 3` 端到端测试通过**；用 Sandbox 测 BinaryOp 通过。**（本书第一个"完整框架"里程碑）**

### Part 4 · 内置节点与高级特性（4 章）

**Ch4.1 transform / noop / bcast 广播 + `#[add_cvt_func]`**
- 目标：实现内置节点与类型转换函数注册；广播（一进多出）。
- Rust 概念：泛型转换、函数注册。crate：`async-broadcast`。产物：`node/transform`、`node/bcast`。验收：广播扇出、类型转换链路测试。

**Ch4.2 merge / demux / reorder（多路复用与重排序）**
- 目标：合流、按键分发、乱序重排——算法仓 SkipNode/多路流的基础。
- Rust 概念：状态机、缓冲与顺序保证。产物：`node/merge`、`node/demux`、`node/reorder`。验收：乱序输入→有序输出、按键分发正确的测试。

**Ch4.3 Resource 与 Context：共享模型/内存池**
- 目标：跨节点共享的资源集合（模型、内存池），节点 `initialize` 时获取。
- Rust 概念：`Arc<dyn Any + Send + Sync>` 共享、并发访问。crate：`dashmap`。产物：`resource`、`Context`。验收：多节点共享同一资源实例的测试。

**Ch4.4 子图 subgraph、多图 `[[graphs]]`、动态子图**
- 目标：图中引用子图、多图定义、按流动态实例化子图（算法仓每路视频一份子图）。
- Rust 概念：递归装配、动态生命周期管理。产物：subgraph 支持。验收：嵌套图与"每输入一份子图"的动态场景测试（对齐原版 `tests/01-03`）。

### Part 5 · 兼容 · 优化 · 收尾（3 章）

**Ch5.1 对齐真实 API，跑真实算法仓风格的图 + pplcore 边界**
- 目标：补齐 `prelude` 门面，写一个 detector→tracker→alarm 风格的多节点/子图 TOML 并跑通（用我们自己的 mock 节点）；**诚实讲清**能替换到什么程度、`flow-message::algo_base`/`blob-proxy`/`pplcore`/`mpp` 的边界在哪。
- 产物：真实风格集成示例。验收：多节点子图图跑通；文档明确边界。

**Ch5.2 优化与更少 bug：逐条对比原版**
- 目标：系统讲我们相对原版做的优化（见 §8），每条给出「原版怎样 / 我们怎样 / 为什么更好」。
- 产物：优化说明章 + 对应测试。验收：关键优化都有测试佐证。

**Ch5.3 全景回顾 + 进阶指路**
- 目标：回顾整条链路；指路后续方向——图优化器、调试器、Python/C 加载（给出真实源码入口与学习顺序）。
- 验收：读者能说清每个模块职责与扩展点。

---

## 6. 兼容性策略与诚实边界（§7 展开）

- **能对齐（本书做到）**：节点写法（全部核心宏）、`Envelope`、`prelude` 门面、TOML schema、`Builder`/`graph` API、`Sandbox`、注册表、内置节点、子图/多图/动态子图。→ **你用这套写的节点与图，语义与真实 flow-rs 一致。**
- **部分对齐（最小子集）**：`flow-message::algo_base`（Frame/Item/Image/Feature/Rect）——实现到"能跑真实风格图"的最小结构；张量用 `ndarray` 简化，不绑定 `blob-proxy` 的全部特性。
- **做不到 / 不承诺**：把我们的 `flow-rs` 直接链接进闭源 `pplcore-std`/`pplcore-rs`/`mpp` 全家桶（它们锁定了特定 flow-rs 私有 API 与版本）。这部分明确标注为边界，末章给出"若要真正替换需要补齐哪些私有面"的清单。

---

## 7. 「优化 / 更少 bug」具体清单（相对原版）

1. **异步模型简化**：原版为兼容 Python GIL 用了 `stackful` 有栈协程 + 自定义 `spawn_pinned` + 自定义 `rwlock`（`rt/*`）。纯核心引擎（无 Python）改用**原生 `async`/`await` + tokio**，去掉有栈协程复杂度与相关 `unsafe`，更简单、更易懂、更少并发坑。
2. **类型化错误**：原版大量 `anyhow::Result<()>`。引入 `thiserror` 定义领域错误（配置错误 / 端口未连接 / 类型不匹配 / 通道关闭 / 找不到节点类型…），可精确匹配、可测试、报错更准。
3. **校验前移到构建期**：端口存在性、连接类型一致性、悬空连接检查放到 `build()`，把运行期崩溃变成**构建期明确报错**。
4. **减少 unsafe**：loader/channel-storage 的 `unsafe` 在核心版尽量清零，用 `Arc`/`enum`/泛型表达。
5. **通道关闭语义显式化**：明确 send-on-closed / recv-on-closed / 全输入关闭即退出，写成显式状态 + 测试，替代隐式约定。
6. **宏的友好错误**：用 `syn::Error`（带 span）替代 `unwrap`/`panic`，编译期错误可读。
7. **测试覆盖**：每模块 TDD；集成测试对齐原版 `tests/01-08` 场景（subgraph / dyn-subgraph / share-subgraph / isolated / multi-graph / dispatcher / typeinfo / graph-features）。
8. **文档与可观测性**：每个公开项带文档注释与示例；可选 `tracing` 结构化日志。

---

## 8. 工具链与构建

- rustc/cargo 1.98（已装）、mdbook 0.4.40（已装）、mdbook-toc（已装）。
- 需装：`mdbook-mermaid`（画架构/数据流图，一条 `cargo install mdbook-mermaid`）。
- 构建：`cd book && mdbook build`（产出静态站点）；`cd code && cargo test`（参考实现全绿）。
- CI（可选，后续）：`mdbook test` 校验书中 Rust 代码块 + `cargo test`。

---

## 9. 成功标准 / 完成定义

1. `book/` 能 `mdbook build` 无错；章节树完整、图能渲染。
2. `code/` workspace `cargo build` + `cargo test` **全绿**。
3. 端到端：BinaryOp `1+2==3`（对齐原版）跑通；一个 detector→tracker→alarm 风格的多节点/子图 TOML 跑通。
4. 每章可独立验收（有明确的"验收"测试/示例）。
5. 读者按书走完，能**独立复现**该实现并解释每个模块的职责与取舍。

---

## 10. 风险与缓解

| 风险 | 缓解 |
|---|---|
| 篇幅大、需多会话 | spec + 实现计划分批推进；每章独立可验收，随时可停可续 |
| 「可替换」到 pplcore 的边界易被误解 | §7 明确标注，末章给"要真正替换需补齐什么"的清单，不承诺链接闭源 |
| 过程宏章节难度陡 | 给足脚手架、`cargo expand` 调试法、循序渐进（先 demo 宏再核心宏） |
| 原版某些细节（optimizer/debug/FFI）超范围 | 归入非目标，末章指路，不在正文展开 |
| 书稿与代码脱节 | 对书本身 TDD：先写通过测试的 `code/`，再落书稿；书中代码均来自真实工程 |

---

## 11. 下一步

本 spec 经用户评审通过后：
1. 用 `writing-plans` 技能产出**逐章实现计划**（把 §5 的 22 章拆成可执行的实现任务，每任务含"写测试→实现→落书稿"步骤）。
2. 用 `executing-plans` / TDD 逐章推进：先搭 `megflow-rebuild/` 骨架（Ch0.2），再按 Part 顺序实现。
