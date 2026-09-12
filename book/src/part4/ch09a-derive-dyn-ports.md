# Ch4.9a 派生宏 dyn 端口：set_port_dynamic 与类型化特化

[Ch4.9](ch09-dynports.md) 造好了运行期 **create → publish → fetch → route** 环路，但触发方要**手搓** `DynPorts::<Sender>::new(cfg)` 才能装配动态端口。它在 §7 记了三块欠账：派生宏 `dyn` 端口关键字、config 自动接线、注册版 `DynDemux`。本章还第一块——让**派生宏**替节点作者干这件事：写 `#[outputs(out: dyn T0)]` 就长出 `DynPorts<Sender>` 字段，并生成 `Node::set_port_dynamic` 把建图期送来的 `DynPortsConfig` 按端口名塞进对应字段。顺带补齐类型化 4 特化、用一个本地宏消掉 fetch 的重复。

这条契约直接对标原版：节点签名 `#[inputs(inp:T0)] #[outputs(out:dyn T0)]`（`node/demux.rs` 的 `DynDemux`）、派生宏的 `set_dyn_f`、以及 `node/port.rs` 的 `port_impl!` 宏。

**本章不碰 config 自动接线**——「TOML 里写条 dyn 连接、建图期自动发 `DynPortsConfig`」是 [Ch4.9b](ch09b-config-auto-wiring.md) 的活。这里的 `set_port_dynamic` 仍由测试**手工调用**，钉死的契约是「派生宏生成了正确的注入派发」，而非「谁来调它」。

<!-- toc -->

## 1. 契约：`dyn T` 语法 → 生成什么

先把这一步要长出来的东西列清楚。端口声明里的 `dyn T` 按**载荷是模板还是具体类型**分两路，按**输入还是输出**决定句柄装 `Sender` 还是 `Receiver`：

| 声明 | 注入字段 | 从 `DynConns` 抽哪张表 |
| --- | --- | --- |
| `#[outputs(out: dyn T0)]` | `out: DynPorts<Sender>` | `inputs[target]`（实例**入口** Sender） |
| `#[outputs(out: dyn i32)]` | `out: DynPorts<SenderT<i32>>` | 同上，fetch 侧贴回 `i32` 标签 |
| `#[inputs(inp: dyn T0)]` | `inp: DynPorts<Receiver>` | `outputs[target]`（实例**出口** Receiver） |
| `#[inputs(inp: dyn i32)]` | `inp: DynPorts<ReceiverT<i32>>` | 同上，fetch 侧贴回 `i32` 标签 |

有一处**反直觉、必须点破**：dyn **输出**端口持的是 `DynPorts<Sender>`——因为触发方要拿到实例**入口**的 `Sender`、往实例里灌数据。对偶地，dyn **输入**端口持 `DynPorts<Receiver>`，抽实例出口。这正是 `DynDemux` 的形态：它的 dyn 输出握着通往每份新建实例的发送端。

除了字段，派生宏还要生成两样：`Node::set_port_dynamic` 的**覆盖实现**（把 `DynPortsConfig` 按端口名 `push` 进字段），以及注册表里的 `INPUT_DYN`/`OUTPUT_DYN` 标记表（[Ch4.9b](ch09b-config-auto-wiring.md) 建图期靠它识别 dyn 端口）。

## 2. 解析接缝：属性宏认识 `dyn` 关键字

`#[inputs]`/`#[outputs]` 的 `PortSpec::parse` 在读到冒号后，依次试探 `[]`（数组端口，[Ch4.2](ch02-array-ports-bcast-merge.md)）、`{}`（字典端口，[Ch4.7b](ch07b-dictionary-ports.md)）。本章加第三支——`dyn`。`code/flow-derive/src/node.rs` 里 `PortSpec` 先加一个 `is_dyn: bool` 字段，`parse` 插入这一分支：

```rust,ignore
{{#include ../../../code/flow-derive/src/node.rs:dyn_parse}}
```

`dyn` 是 Rust 关键字，syn 提供 `Token![dyn]`，`input.peek(Token![dyn])` 即可判定。随后 `parse::<Type>()` 取载荷（模板 `T0` 或具体类型），并**拒绝** `Slice`/`Array`/`TraitObject`——`dyn T0` / `dyn String` 合法，`dyn [u8]` / `dyn dyn Trait` 报错。

**这一支的位置很关键**：它必须排在解析裸类型载荷（`dyn` 分支之后那段 `let payload: Type = input.parse()?`）**之前**。否则 `dyn String` 会被 syn 当成 trait object 语法（`dyn Trait`）解析成 `Type::TraitObject`、撞上后面那个「仅支持标量」的拒绝分支——本章正是要把这个语法从「非法」翻转成「动态端口」。

## 3. 字段展开与 port_kind：`DynPorts` 字段从哪来

拿到 `is_dyn: true` 的 `PortSpec` 后，字段展开在 `code/flow-derive/src/node.rs` 的 `expand_inputs` / `expand_outputs` 各加一支 `if spec.is_dyn`（排在 array/dict 之前）：模板载荷 → 无类型 `DynPorts<Receiver>` / `DynPorts<Sender>`；具体载荷 → 类型化 `DynPorts<ReceiverT<T>>` / `DynPorts<SenderT<T>>`。这几行是 `if`-链中的片段（带悬挂大括号），故不单独 include，读 `code/flow-derive/src/node.rs` 的 `expand_inputs`/`expand_outputs` 即可对照。

属性宏跑完后，派生宏看到的只是普通结构体字段，得靠 `port_kind` **按语法结构**认出它们（`code/flow-derive/src/node.rs`）：`DynPorts<..>` 内层是 `Sender`/`SenderT` → `OutputDyn`（触发方，推数据进实例入口）；`Receiver`/`ReceiverT` → `InputDyn`（消费方，从实例出口拉）。这排在 dict/标量识别之前，避免把 `DynPorts` 误判成别的端口。

`expand_build_from_ports` 里，dyn 字段**登记进** `INPUTS`/`OUTPUTS` 名表 + 并行的 `INPUT_DYN`/`OUTPUT_DYN` 布尔表，但初始化走 `Default::default()`——**不**从 `ins`/`outs` 消费端口，和 `#[state]` 字段同路：字段先是一张**空** `DynPorts`，配置留到运行期由 `set_port_dynamic` 注入。注册表 `NodeRegistration` 相应承载 `input_dyn`/`output_dyn` 表 + `input_is_dyn`/`output_is_dyn` 访问器（镜像既有的 `input_is_array`/`input_is_dict`，手写 `BuildFromPorts` 靠默认 `&[]` 安全回退）——[Ch4.9b](ch09b-config-auto-wiring.md) 建图期就靠 `output_is_dyn` 把一条连接认成「指向动态子图」。

## 4. set_port_dynamic：节点侧的运行期注入点

`Node` trait 先加一个**默认 no-op** 的 `set_port_dynamic`（`code/flow-rs/src/node.rs`）——没有动态端口的节点原样不受影响：

```rust,ignore
{{#include ../../../code/flow-rs/src/node.rs:set_port_dynamic}}
```

派生宏则**只对含 dyn 字段的节点**生成覆盖（`code/flow-derive/src/node.rs`）：

```rust,ignore
{{#include ../../../code/flow-derive/src/node.rs:set_port_dynamic_gen}}
```

生成的实现按 `port_info.name.as_str()` 匹配字段名字面量（如 `"out"`），命中就 `self.out.push(port_info.name.clone(), config)`——把 `DynPortsConfig` 塞进该字段的 `DynPorts.cfg`。这正是原版 `set_dyn_f` 的行为。无 dyn 字段的节点 `dyn_arms` 为空、不生成覆盖，沿用 trait 默认 no-op——既有那十来个 `#[derive(Node)]` 节点一个不受影响。（`close()` 里 dyn 字段走 `self.#id.close()`，evict 掉缓存里每份实例的端点，见 §3 的 `expand_derive_node`。）

**注意本章的 `set_port_dynamic` 还没有调用者。** 运行期实例（[Ch4.9](ch09-dynports.md) 那条路径）靠装配期接线 + 端点留在 `MainGraph.inputs/outputs` 就位，根本不经过「构造后再 set_port」。派生 / config 路径（节点字段持 `DynPorts`）才需要它——本章生成它、测试手工调它验证派发正确，真正的建图期调用点在 [Ch4.9b](ch09b-config-auto-wiring.md)。

## 5. 类型化特化：一个宏收四份 fetch

[Ch4.9](ch09-dynports.md) §4 已把整份 `dyn_ports.rs` include 进来（含本节要讲的宏与特化）；这里把它讲透。`fetch` 三方法（`fetch` / `try_fetch` / `fetch_with_cache`）在**四个特化**里方法体完全相同，只差两点：从 `DynConns` 的哪张表抽端点（入口抽 `inputs`、出口抽 `outputs`），以及抽出的无类型端点要不要**包壳成类型化端点**。于是收进一个本地 `macro_rules!`：

```rust,ignore
{{#include ../../../code/flow-rs/src/dyn_ports.rs:dyn_fetch_macro}}
```

两个**无类型**特化直接实例化这个宏（`.into()` 走 `From<Sender> for Sender` 的恒等转换）：

```rust,ignore
{{#include ../../../code/flow-rs/src/dyn_ports.rs:dyn_ports_sender}}
{{#include ../../../code/flow-rs/src/dyn_ports.rs:dyn_ports_receiver}}
```

两个**类型化**特化只多一步——抽出的无类型 `Sender`/`Receiver` 经 `From<Sender> for SenderT<T>` 打上 `MsgTypeId::of::<T>()` 标签：

```rust,ignore
{{#include ../../../code/flow-rs/src/dyn_ports.rs:dyn_ports_typed}}
```

一条业务规则要记牢：**打标签只发生在 fetch 侧**。`create` 恒发**无类型** `DynConns`（端点擦除入队），类型信息由订阅方按声明的载荷 `T` 在 fetch 时现场贴回。这与原版 `port_impl!` 的 typed 臂一致，也是「类型化 `send` 送进无类型 `Transform` 边界能原样透传」的原因——转换表查不到 `i32→Any` 便走恒等。四份实现收进一个宏、按 `$field`/`$endpoint`/`$noun` 实例化，正是消掉了 Ch4.9 §7 记的那对重复。

## 6. 测试与复现

新建 `code/flow-rs/tests/dyn_port_derive.rs`。两个测试节点直接用本章的 `dyn` 语法声明（对标 `DynDemux` 的触发签名），只 `#[derive(Node)]`——测试手工驱动 `self.out`、不把它跑成 actor：

```rust,ignore
{{#include ../../../code/flow-rs/tests/dyn_port_derive.rs:dyn_nodes}}
```

第一个测试用无类型 `Trigger` 钉死 (a)(b)(c) 三件事——字段类型是 `DynPorts<Sender>`、`set_port_dynamic` 前后的空/注入状态、注入后 `create → fetch → route` 跑通（复用 [Ch4.9](ch09-dynports.md) 的 broker + 子图夹具）：

```rust,ignore
{{#include ../../../code/flow-rs/tests/dyn_port_derive.rs:injects_test}}
```

第二个测试用 `TypedTrigger`（`dyn i32`）钉死 (d)：字段升成类型化 `DynPorts<SenderT<i32>>`，typed `fetch` 拿到 `SenderT<i32>`、typed `send` 只收 `Envelope<i32>`：

```rust,ignore
{{#include ../../../code/flow-rs/tests/dyn_port_derive.rs:typed_test}}
```

在仓库根目录运行：

```sh
cargo test --manifest-path code/Cargo.toml -p flow-rs --test dyn_port_derive --locked
cargo test --manifest-path code/Cargo.toml --workspace --locked
```

第一条跑本章两个集成测试；第二条捎带 `flow-derive` 的派生宏单元测试（`PortSpec` 加 `is_dyn` 字段后，那批 `scalar`/`array_port` 夹具与端口分类断言仍需继续绿）。(b) 用行为断言观察 `set_port_dynamic`——注入前 `create` 报「无配置」、注入后整条环路跑通——因为 `DynPorts.cfg` 是私有字段，不能直接读。

## 7. 落差与后续

- **config 还不认 `dyn` 连接**——本章 `set_port_dynamic` 仍由测试手工调用。建图期自动识别 dyn 连接、跳过 dyn 子图的静态压平、自动构造并注入 `DynPortsConfig`、让图自带 broker 生命周期，已在 [Ch4.9b](ch09b-config-auto-wiring.md) 实现（对齐原版 `graph/mod.rs`）。
- **`DynPortsConfig` 的 `cap`/`args` 仍忽略**——`create` 接受 `cap` 字段但不用它（边界通道容量由子图配置自身决定），`args` 合并（`merge_table` 覆盖实例参数）本弧继续 defer。
- **`DynDemux` 还不是注册 builtin**——把机制封装成能由真实 TOML 端到端驱动的注册节点 + Sandbox 动态端口支持，已在 [Ch4.9c](ch09c-dyn-demux-in-graph.md) 实现，也正是 [Ch4.7c](ch07c-demux-node.md) §5 明确 defer 的那块。

## 小结

- **`dyn T` 语法一路长到字段**：属性宏 `PortSpec::parse` 认 `Token![dyn]`（排在裸类型解析之前）→ `expand_inputs`/`expand_outputs` 按模板/具体载荷注入 `DynPorts<Sender|Receiver>` 或类型化特化 → 派生宏 `port_kind` 按结构认出 `OutputDyn`/`InputDyn`。
- **反直觉但正确**：dyn **输出**端口持 `DynPorts<Sender>`（抽实例入口、往里灌），dyn **输入**端口持 `DynPorts<Receiver>`（抽实例出口、往外拉）。
- **`set_port_dynamic` 是节点侧的运行期注入点**：trait 默认 no-op，派生宏只对含 dyn 字段的节点生成覆盖，按端口名 `push` 配置（对标 `set_dyn_f`）。本章生成它、测试调它，真正的建图期调用点在 [Ch4.9b](ch09b-config-auto-wiring.md)。
- **一个宏收四份 fetch**：`fetch` 系列在 4 个特化里体相同，仅差抽 `inputs`/`outputs` 与是否包壳类型化；`dyn_fetch_methods!` 按 `$field`/`$endpoint`/`$noun` 归一（对标 `port_impl!`）。**打标签只在 fetch 侧**，`create` 恒发无类型 `DynConns`。
- **注册表 `INPUT_DYN`/`OUTPUT_DYN`** 已就位，为 [Ch4.9b](ch09b-config-auto-wiring.md) 建图期识别 dyn 连接铺好路。
