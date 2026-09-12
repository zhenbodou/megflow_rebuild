# Ch4.9b config 认识 dyn 连接：自动接线动态子图

[Ch4.9](ch09-dynports.md) 把运行期环路（create → publish → fetch → route）造了出来，但全靠**手搓**：手写 `DynPorts`、手挂 broker 订阅、example/测试里显式驱动。[Ch4.9a](ch09a-derive-dyn-ports.md) 让节点作者写 `#[outputs(out: dyn T0)]` 就长出 `DynPorts` 字段、派生宏还生成了 `Node::set_port_dynamic`——可它**没有调用者**，注册表里的 `INPUT_DYN`/`OUTPUT_DYN` 标记表也还没人读。

本章补上最后一块，把这两头接起来：**config 层认识 `dyn` 连接**。目标是——TOML 里写一个 dyn 输出/输入的触发节点 + 一张动态子图 + 一条连接，`Builder::build` 就**自动**：`flatten` 跳过 dyn 子图（保留成惰性 `GraphConfig`）、装配期识别 dyn 连接并**自动**为 dyn 端口 `set_port_dynamic` 注入 `DynPortsConfig`、让 `MainGraph` 自带 broker 生命周期。读者写的是**声明**，不再是驱动代码。对齐原版 `flow-rs/src/graph/mod.rs` 数 `dyn_rxn`/`dyn_txn` + 校验 + 建 `DynPortsConfig` 那段。

**恒等护栏先说死**：无 `dyn` 连接的配置，`flatten` 与 `assemble` 逐字节走老路径、`broker` 恒为 `None`——既有 subgraph / graph / demux / 资源 / dyn_ports 测试**一字不改继续绿**。新增只发生在「有 dyn 连接」这条新分支上。

<!-- toc -->

## 1. 契约：dyn 连接的可观察规则

先写出建图期看到什么、就自动做什么——这些都是可从测试观察到的行为：

| 建图期识别到 | 自动做什么 |
| --- | --- |
| 一条连接里**同时**有「注册节点的 dyn 端口」+「子图位点引用」 | 认成 **dyn 连接**：**不**建静态 channel；`broker.subscribe(topic=位点名)`、构造并 `set_port_dynamic` 注入 `DynPortsConfig` |
| 静态子图（没被任何 dyn 连接引用） | `flatten` **内联**（[Ch4.4](ch04-subgraph.md) 原样，前缀展开） |
| dyn 子图（被 dyn 连接引用） | `flatten` **不内联**，保留成惰性 `GraphConfig`，assemble 期现装实例 |
| 整图无 dyn 连接 | **恒等**：`broker = None`、走老 `assemble_graph`（逐字节等价） |

方向规则**反直觉但正确**（[Ch4.9a](ch09a-derive-dyn-ports.md) 点破过）：dyn **输出**端口持 `DynPorts<Sender>`，要往实例**入口**灌数据，故它的边界必须落在子图的 `inputs` 上；dyn **输入**端口持 `DynPorts<Receiver>`，从实例**出口**收结果，边界必须落在子图的 `outputs` 上。违反方向、或形态非法的，建图期直接报错（对齐原版 `graph/mod.rs` 的 dyn 连接校验）：

| 非法形态 | 报错 |
| --- | --- |
| dyn 输出接到子图**出口**（方向反）/ dyn 输入接到子图**入口** | `UnknownPort`（该边界不在期望的一侧） |
| 一条连接里有**两个** dyn 端口 | `BadConnection`（原版：一条连接恰一个 dyn 端口） |
| dyn 端口没接任何子图位点 / 子图位点没接任何 dyn 端口 | `BadConnection` |
| 保留了 dyn 子图、却没认出任何 dyn 连接 | `Unsupported` |

## 2. flatten 跳过 dyn 子图

第一步在压平期。[Ch4.4](ch04-subgraph.md) 的 `flatten` 把静态子图内联掉；现在它要先**预扫**每张图的连接，认出哪些子图引用参与了 dyn 端口连接——那些**不内联**，保留成惰性构造器。判定纯靠注册表（[Ch4.9a](ch09a-derive-dyn-ports.md) 加的 `output_is_dyn`/`input_is_dyn`），`code/flow-rs/src/subgraph.rs` 的 `dynamic_refs` 干这件事：

```rust,ignore
{{#include ../../../code/flow-rs/src/subgraph.rs:dynamic_refs_fn}}
```

关键的**恒等性**就藏在这里：既有所有节点 `output_is_dyn`/`input_is_dyn` 恒为 `false`（`INPUT_DYN`/`OUTPUT_DYN` 是空表、防御式回退 `false`），故无 dyn 端口的配置返回**空集**，`flatten` 退化成原来的静态压平——这是既有 subgraph 测试继续绿的根。

`expand` 拿到这个「dyn 子图名集合」后，节点递归就多了一条分支：dyn 子图引用原样保留（`ty` 仍是子图名，assemble 据此识别）、子图定义去重收进 `retained`；静态子图照旧递归内联：

```rust,ignore
{{#include ../../../code/flow-rs/src/subgraph.rs:expand_fn}}
```

产出的 `Config` 里，扁平主图在前、被保留的 dyn 子图跟在后。`tests/dyn_wiring_e2e.rs` 的 `flatten_retains_dyn_subgraph_as_lazy_constructor` 把这条钉死：站点节点 `worker` **没被内联**（`ty` 仍是 `"sub"`、没有 `worker/s` 这种带前缀的内部叶子），子图 `sub` 作为第二张图保留。

## 3. assemble 分流：静态一张图 vs 动态自动接线

`MainGraph::assemble` 据「除 `main` 外还有没有保留的子图」分流：没有 → 老路径 `assemble_graph`（恒等）；有 → 新路径 `assemble_dynamic`。

```rust,ignore
{{#include ../../../code/flow-rs/src/graph.rs:assemble}}
```

## 4. 认 dyn 连接：`DynWiring::from_conn`

自动接线的**心脏**：把一条主图连接解析成一条 `DynWiring`（或判定它是普通连接、或报错）。这段直接对标原版 `graph/mod.rs` 里数 `dyn_rxn`/`dyn_txn` + 校验的逻辑：遍历连接的端口引用，认出两端——「注册节点的 dyn 端口」与「动态子图位点」——再按 §1 的规则校验、组装：

```rust,ignore
{{#include ../../../code/flow-rs/src/graph.rs:dyn_wiring}}
```

三个细节值得停一下：

- **topic = 位点节点名**。同一个子图站点（如 `worker`）上的多条 dyn 连接（feed 接 `worker:inp`、collect 接 `worker:out`）**共用同一 topic**。于是触发方一次 `create` 广播出的那一份 `DynConns`，feed 端（抽入口 `Sender`）和 collect 端（抽出口 `Receiver`）**两边都收得到**——这正是 [Ch4.8](ch08-broker.md) broker「每订阅者一份独立克隆」语义的直接用法。
- **target = 边界端口名**，配 `is_out` 决定从 `DynConns` 抽哪一端（§1 的反直觉规则就落在 `(is_out && !in_inputs) || (!is_out && !in_outputs)` 这行校验上）。
- **返回 `Ok(None)` 是恒等出口**：两端都不 dyn 的普通连接原样交回 `assemble_graph` 建 channel，老路径丝毫不变。

## 5. 自动接线 + `set_port_dynamic` 注入

`assemble_dynamic` 把上面这些拼起来：扫连接收 `DynWiring`、造一张**过滤图**（剔掉 dyn 子图位点节点 + dyn 连接，其余交恒等的 `assemble_graph`）、再对每条 wiring 建 broker 订阅 + 构造 `DynPortsConfig` + 在节点**构造后** `set_port_dynamic` 注入：

```rust,ignore
{{#include ../../../code/flow-rs/src/graph.rs:assemble_dynamic}}
```

注入这步正是 [Ch4.9a](ch09a-derive-dyn-ports.md) 生成的 `set_port_dynamic` 的**第一个真实调用者**：`port_info.name` 取 dyn 端口的字段名（如 `feed`/`collect`），派生宏生成的覆盖据此 `match` 到对应字段、把 `DynPortsConfig` `push` 进那张空 `DynPorts`。至此「派生宏生成注入派发」（4.9a）与「建图期谁来调它」（本章）接上了。

## 6. broker 进图：`MainGraph` 的生命周期

要让 dyn 端口在运行期能 `create`/`fetch`，broker 必须活到图跑完。`MainGraph` 因此多一个 `broker` 字段：

```rust,ignore
{{#include ../../../code/flow-rs/src/graph.rs:main_graph_struct}}
```

它只在装配期识别出 dyn 连接时才 `Some`。`start()`（见 `code/flow-rs/src/graph.rs`）里的次序是硬纪律：**先** `broker.run()`（订阅早在 `assemble_dynamic` 接线时就为每个 dyn 端点做完了——**订阅先于 run** 是 [Ch4.8](ch08-broker.md) 反复强调的），**再** spawn 各节点任务；节点任务全部收尾后才 `await` broker 句柄（节点 drop 掉持有的 `BrokerClient` → 每 topic 的 fan-out 任务收到「发布端全关」而结束 → broker 句柄解析）。无 dyn 连接时 `broker` 为 `None`、这两步都是空操作——这就是「无动态子图即恒等」的直接落点。

## 7. 端到端：一份 TOML 自动接线

夹具只有两个节点：有状态的子图节点 `Seq`（每收一条 `count += 1`、把计数拼进出口，用来从**可观察输出**证明 create-once），和顶层触发节点 `AutoTrigger`（手写的 `DynDemux` 前身——`exec` 就是那条环路）。注意 `feed` 是 **dyn 输出**、`collect` 是 **dyn 输入**：

```rust,ignore
{{#include ../../../code/flow-rs/tests/dyn_wiring_e2e.rs:fixtures}}
```

接线全在这份 TOML 里声明——**没有一行手搓 `DynPorts`**：

```rust,ignore
{{#include ../../../code/flow-rs/tests/dyn_wiring_e2e.rs:toml}}
```

主线测试经 `Builder::build` 自动接线，断言按 key 路由、create-once（有状态子图计数累加为证）、空信封拆除：

```rust,ignore
{{#include ../../../code/flow-rs/tests/dyn_wiring_e2e.rs:e2e}}
```

`Builder::build` = `Config::from_toml` → `subgraph::flatten`（§2 跳过 dyn 子图）→ `MainGraph::assemble`（§3 分流到 §5 自动接线）。跑起来的关键观察：key 7 连送两帧，`frame-1#1` → `frame-2#2`——**同一实例、状态累加**，证明 create-once（若每帧新建实例会是 `frame-2#1`）；key 42 是另一张独立图，从 `frame-42#1` 重新起。

flatten 跳过 dyn 子图、以及 §1 校验表里两条错误路径，也各有单元测试钉住：

```rust,ignore
{{#include ../../../code/flow-rs/tests/dyn_wiring_e2e.rs:flatten_skip}}
```

```rust,ignore
{{#include ../../../code/flow-rs/tests/dyn_wiring_e2e.rs:validation}}
```

在仓库根目录复现：

```sh
cargo test --manifest-path code/Cargo.toml -p flow-rs --test dyn_wiring_e2e --locked
```

预期 4 个测试全过：`auto_wires_dyn_subgraph_from_toml`、`flatten_retains_dyn_subgraph_as_lazy_constructor`、`reversed_boundary_is_rejected`、`two_dyn_ports_in_one_connection_is_rejected`。再跑一遍全量确认恒等重构没破既有测试：

```sh
cargo test --manifest-path code/Cargo.toml -p flow-rs --locked
```

本章把 flow-rs 测试总数从 147 抬到 **151**（新增 4 个 `dyn_wiring_e2e` 测试）；既有 147 个一字不改继续绿——恒等护栏成立。

## 8. 落差与后续：这一章**没做**什么

- **`DynDemux` 还不是注册 builtin**——本章触发节点 `AutoTrigger` 是**测试夹具**，不在 `builtin.rs`、真实 TOML 里 `ty="DynDemux"` 还用不了。把这套环路封装成注册节点 + Sandbox 动态端口支持（内部 broker + 临时图），已在 [Ch4.9c](ch09c-dyn-demux-in-graph.md) 实现，也正是 [Ch4.7c](ch07c-demux-node.md) §5 明确 defer 的那块。
- **资源只注入、不链接**——原版 `ext_resource.chain(in_resource)` 把外层注入资源与子图自带资源链起来、按 instance-id 分尺度；本章多个实例共享同一注入集合（`AutoTrigger` 甚至只传 `ResourceCollection::default()`），子图暂不声明自带 `resources`。这是 [Ch4.9](ch09-dynports.md) 记过的 defer，本章的自动接线不改变它。
- **`DynPortsConfig.cap` 接受但用法最小**——建图期从连接的 `cap` 取值填进去，`create` 里 `assemble_graph` 用子图自身的边界容量装配；原版按 instance 合并 `args`（`merge_table`）本弧继续 defer。

## 小结

- **config 认 dyn 连接 = 把 4.9a 的两头接上**：注册表的 `INPUT_DYN`/`OUTPUT_DYN`（谁是 dyn 端口）+ 派生宏生成的 `set_port_dynamic`（怎么注入），本章补上「建图期谁来识别、谁来调用」。
- **flatten 跳过 dyn 子图**：`dynamic_refs` 注册表驱动地认出 dyn 子图引用，`expand` 不内联、保留成惰性 `GraphConfig`；无 dyn 连接时返回空集、退化为恒等静态压平。
- **`DynWiring::from_conn` 是心脏**：一条连接解析成 (触发节点, dyn 端口, topic=位点名, target=边界端口)，按反直觉方向规则校验；`Ok(None)` 是普通连接的恒等出口。
- **topic = 位点节点名**：同站点的 feed/collect 共用 topic，一次 `create` 广播两端都收得到——broker「每订阅者一份克隆」的直接用法。
- **broker 进图**：`MainGraph.broker: Option<Broker>`，`start()` 先 `run()` 后 spawn（订阅先于 run）、节点收尾后再 await broker；无 dyn 连接恒为 `None`、逐字节恒等。
- **落差诚实标注**：`DynDemux` 成注册 builtin、Sandbox dyn 支持已在 [Ch4.9c](ch09c-dyn-demux-in-graph.md) 实现；资源只注入不链接、`args` 未合并仍是整条弧的既定 defer。
