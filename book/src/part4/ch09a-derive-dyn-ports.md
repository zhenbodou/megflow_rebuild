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

拿到 `is_dyn: true` 的 PortSpec 后，字段展开在 expand_inputs / expand_outputs 中先判断动态端口：模板载荷生成无类型端点，具体载荷生成类型化端点。下面给出两个完整函数，避免要求读者自行寻找 if 分支插入位置。它们仍位于已有的 `code/flow-derive/src/node.rs` 模块中，使用前面端口课已实现的 PortSpec、port_field 和类型识别辅助函数。

```rust
{{#include ../../../code/flow-derive/src/node.rs:dynamic_inputs_full}}
```

```rust
{{#include ../../../code/flow-derive/src/node.rs:dynamic_outputs_full}}
```

按数据流阅读：先检查原项是否为具名结构体，再收集已有字段用于重名检测；遍历声明时判断动态、字典、列表和标量形态，构造对应字段类型；最后把修改后的整个 ItemStruct 输出。属性宏返回原结构体的替代项，因此不能只输出新字段。模板识别发生在生成字段之前：T0 是本项目的类型描述占位，不要求调用者定义一个叫 T0 的 Rust 类型。

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

生成的实现按 `port_info.name.as_str()` 匹配字段名字面量（如 `"out"`），命中就 `self.out.push(port_info.name.clone(), config)`——把 `DynPortsConfig` 塞进该字段的 `DynPorts.cfg`。这正是原版 `set_dyn_f` 的行为。无 dyn 字段的节点 `dyn_arms` 为空、不生成覆盖，沿用 trait 默认 no-op——既有那十来个 `#[derive(Node)]` 节点一个不受影响。（生成的 close 调用 `self.#id.close()`，只关闭 Broker 通知；缓存端点仍需 evict 或随字段销毁，不能据此保证实例退出。）

**本章的测试手工调用 `set_port_dynamic`，用来隔离验证派生代码。** 当前完整源码已有下一章的建图期调用者；不要把“本课尚未学习调用者”理解为“当前仓库没有调用者”。运行期实例的外部端点表与节点字段中的 DynPorts 是两个层次，下一章负责把配置接到后者。

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

第一条跑本章三个集成测试；第三个测试分别验证无类型和类型化的输入端口，至此四种端点都有真实收发验证。第二条同时运行工作区回归测试。注入测试通过行为观察配置：注入前 create 报错，注入后环路跑通，不直接读取私有 cfg 字段。

### 从零读懂本章的声明宏

`dyn_fetch_methods!` 的匹配器 `($endpoint:ty, $field:ident, $noun:literal)` 有三个参数。`ty` 接受 Rust 类型语法，例如 `SenderT<T>`；`ident` 接受一个字段名，例如 `inputs`；`literal` 接受用于错误信息的字面量，例如 `"input"`。宏不会在运行时判断这三个参数，它们在编译期间替换模板中的相应位置。

以 `dyn_fetch_methods!(Sender, inputs, "input")` 为例：`Result<$endpoint>` 变成 `Result<Sender>`，`conns.$field.remove(...)` 变成 `conns.inputs.remove(...)`。结果是三个普通方法定义，出现在 `impl DynPorts<Sender>` 内。这里没有 `$()*` 重复：一次调用直接生成三个方法，四处调用分别生成四组。先手写这两处替换，再读完整宏，会比把它当成字符串拼接容易理解。

本章说的“四种特化”是四个具体类型上的固有 impl，**不是 Rust nightly 的 specialization 功能**。`DynPorts<SenderT<T>>` 的 impl 仍是泛型实现，T 由使用处决定；稳定版 Rust 即可编译。

同样，端口声明中的 `dyn i32` 是本项目属性宏的输入语法，并不是合法的 Rust trait object 类型：i32 不是 trait。属性宏先消费 dyn 关键字，再解析后面的 Type，最后生成普通字段 `DynPorts<SenderT<i32>>`。因此这里的 dyn 不意味着产生 `Box<dyn Trait>` 或虚表。

### 借用与所有权如何贯穿生成的方法

`single_config()` 返回配置的借用，`broker.fetch().await` 可能挂起任务，但不会阻塞整个线程。`fetch_with_cache` 用局部块结束配置借用，随后才能修改同一个 self 的 cache。`HashMap::remove` 则把端点的所有权从通知中取出，余下的通知字段离开作用域时释放。

`cache.get(&key).cloned()` 克隆的是端点句柄，不是队列内容。尤其是 Receiver 的两个克隆共享一条队列并竞争消息，不会让两名消费者各收到一份。新增的输入测试为两种输入形态分别建立独立实例，避免以广播假设编写测试。

`set_port_dynamic` 按字段名选择唯一 match 分支，把 config 移入该字段。`config` 不实现 Copy 也没关系，因为一次 match 只执行一条分支。未知名字走 `_ => {}`，配置随之释放；这不是自动报错。接线层必须验证名字，不能把这个 no-op 当成用户配置校验。

### 本章第三方 crate 的分工

| crate | 本章关键接口 | 它不负责的部分 |
| --- | --- | --- |
| syn | Token![dyn]、Type、LitStr，解析输入并定位错误 | 不解析类型别名的最终指向，不判断 T 是否满足运行时 trait |
| quote / proc-macro2 | quote! 插入类型、字段和 match 分支，产生 token | 不创建动态实例，也不执行消息路由 |
| Tokio | 异步任务、通道、time::timeout 的三秒测试界限 | 不自动回收仍被克隆句柄保持存活的实例 |
| inventory | 保存动态端口元数据随附的节点注册项 | 不调用 set_port_dynamic，调用由建图逻辑负责 |

本章不新增依赖，沿用上一章和宏专题的 manifest。syn 的 full feature 用于结构体、impl 等完整语法；Tokio 测试需要 rt、macros、time，通道需要 sync。宏生成代码中的 flow_rs 路径依赖运行库公开重导出，这属于两个 crate 间的接口约定。

### 完整测试文件

前面的片段未包含导入、SUB_TRANSFORM 常量和 subgraph 辅助函数。下面给出 `code/flow-rs/tests/dyn_port_derive.rs` 完整内容，保存整份文件后即可执行本节第一条命令：

```rust
{{#include ../../../code/flow-rs/tests/dyn_port_derive.rs}}
```

预期 `derive_dyn_output_injects_then_round_trips`、`derive_dyn_output_with_concrete_payload_is_typed`、`derive_dyn_inputs_receive_from_instance_outputs` 三项通过，退出码 0。每项均设三秒超时，并等待实例和 Broker 退出；不以 sleep 猜测队列是否处理完。

### 排错与独立练习

1. 把 typed 发送中的 `42i32` 改成 `42u32`，应出现类型不匹配的编译错误；恢复后通过。说明类型检查发生在生成后的 Rust 方法调用处。
2. 将注入名字 out 改为不存在的字段名，create 应失败。解释为什么这次不是宏解析错误，而是运行时没有取得配置。
3. 不看宏，手写 `DynPorts<ReceiverT<i32>>` 的 fetch 方法，指出读取 outputs 的原因，再与宏替换结果核对。
4. 画出发送端的三处所有者：通知、缓存、本地克隆。说明仅 drop 本地克隆为何不足以关闭实例，以及测试为何必须 evict 缓存；DynPorts::close 只关闭 Broker，不清空缓存。

## 7. 落差与后续

- **建图期注入在下一课讲解**：本章用手工调用隔离宏契约；当前仓库的自动识别、构造与注入见 [Ch4.9b](ch09b-config-auto-wiring.md)。
- **`DynPortsConfig` 的 `cap`/`args` 仍忽略**——`create` 接受 `cap` 字段但不用它（边界通道容量由子图配置自身决定），`args` 合并（`merge_table` 覆盖实例参数）本弧继续 defer。
- **DynDemux 与 Sandbox 的集成在后续课讲解**：当前实现见 [Ch4.9c](ch09c-dyn-demux-in-graph.md)，不能从本章的字段测试推断整个集成流程已经验证。

## 小结

- **`dyn T` 语法一路长到字段**：属性宏 `PortSpec::parse` 认 `Token![dyn]`（排在裸类型解析之前）→ `expand_inputs`/`expand_outputs` 按模板/具体载荷注入 `DynPorts<Sender|Receiver>` 或类型化特化 → 派生宏 `port_kind` 按结构认出 `OutputDyn`/`InputDyn`。
- **反直觉但正确**：dyn **输出**端口持 `DynPorts<Sender>`（抽实例入口、往里灌），dyn **输入**端口持 `DynPorts<Receiver>`（抽实例出口、往外拉）。
- **`set_port_dynamic` 是节点侧的运行期注入点**：trait 默认 no-op，派生宏只对含 dyn 字段的节点生成覆盖，按端口名 `push` 配置（对标 `set_dyn_f`）。本章生成它、测试调它，真正的建图期调用点在 [Ch4.9b](ch09b-config-auto-wiring.md)。
- **一个宏收四份 fetch**：`fetch` 系列在 4 个特化里体相同，仅差抽 `inputs`/`outputs` 与是否包壳类型化；`dyn_fetch_methods!` 按 `$field`/`$endpoint`/`$noun` 归一（对标 `port_impl!`）。**打标签只在 fetch 侧**，`create` 恒发无类型 `DynConns`。
- **注册表 `INPUT_DYN`/`OUTPUT_DYN`** 已就位，为 [Ch4.9b](ch09b-config-auto-wiring.md) 建图期识别 dyn 连接铺好路。

## 编译诊断实验：缺少类型与非法载荷

运行测试不能替代宏输入诊断。为本章增加三个完整下游程序，它们应在宏展开阶段失败，而不是因为缺少运行库而失败。文件放在 `code/flow-derive/tests/ui/`，由已有 trybuild 入口逐个编译。

`dynamic_missing_payload.rs`：

```rust,compile_fail
{{#include ../../../code/flow-derive/tests/ui/dynamic_missing_payload.rs}}
```

对应 `dynamic_missing_payload.stderr`：

```text
{{#include ../../../code/flow-derive/tests/ui/dynamic_missing_payload.stderr}}
```

解析器先保存消费到的 `Token![dyn]`；如果后面已经没有 token，或立即遇到列表分隔逗号，就用 `syn::Error::new_spanned(keyword, ...)` 返回专用错误。这样下划线落在用户写的 dyn 上，错误还给出两种合法写法。不要等解析 Type 失败后再要求初学者理解一整串语法候选。

`dynamic_slice_payload.rs`：

```rust,compile_fail
{{#include ../../../code/flow-derive/tests/ui/dynamic_slice_payload.rs}}
```

对应 `dynamic_slice_payload.stderr`：

```text
{{#include ../../../code/flow-derive/tests/ui/dynamic_slice_payload.stderr}}
```

`dynamic_trait_object_payload.rs`：

```rust,compile_fail
{{#include ../../../code/flow-derive/tests/ui/dynamic_trait_object_payload.rs}}
```

对应 `dynamic_trait_object_payload.stderr`：

```text
{{#include ../../../code/flow-derive/tests/ui/dynamic_trait_object_payload.stderr}}
```

后两种输入能解析成 syn::Type，但不满足当前端口 DSL 的规则。因此错误由我们检查 Type 变体后产生；第一种则连载荷类型都没有。这是“语法不完整”和“语法完整但接口不接受”的区别。这里记录重构宏的拒绝规则，不据此推断原版全部合法声明已覆盖。

运行命令：

```bash
cargo test --manifest-path code/Cargo.toml -p flow-derive --test ui --locked
cargo test --manifest-path code/Cargo.toml -p flow-derive dynamic_port_ --locked
```

第一条运行整个负例集合，第二条验证动态语法与四种字段分类。首次新增用例没有 stderr 时，trybuild 会生成 wip 并使测试失败；先确认诊断确实针对用户输入，再保存预期。不能直接批量覆盖旧快照来消除失败。

练习：把缺类型写法改为 `#[outputs(out: dyn, other: i32)]`，预期仍指出 dyn 后缺少类型，不能把 other 当成它的载荷。再恢复为 `dyn Vec<u8>`：解析层接受它，因为载荷是拥有型 Vec，而不是被明确拒绝的裸切片 Type::Slice。

## 生命周期排错：为什么 close 后仍可能等不到实例退出

新增输入测试最初只执行 drop(sender)、creator.close()，随后等待 instance，三秒超时触发。这不是调度太慢：creator.cache 里仍持有一个入口 Sender，子图接收端不能得到“所有发送者已消失”的条件。增大超时不会解决所有权问题。

正确顺序是释放本地 Sender、`drop(creator.evict(8))`、关闭 Broker，再等待实例和 Broker 任务。evict 返回 `Option<Sender>`；显式 drop 确认返回值也已释放。如果把它赋给仍存活的变量，发送端依然存在。

**这是明确的原版兼容缺口**：固定提交的 `flow-rs/src/node/port.rs` 中，close 不仅遍历 cfg 关闭 Broker，还遍历 cache 调用每个 `chan.close()`。当前重构的 DynPorts::close 只执行前一部分。原版通过端点的显式关闭影响其他克隆，重构测试中的 evict + drop 仅在没有其他发送端所有者时导致队列关闭，两者不能混为一谈。

因此，上面的释放顺序是当前实现的可运行收尾方式，不是已经对齐原版 close 的证明。完整兼容需要先补齐通道显式关闭协议，再让动态缓存逐端点关闭，并新增“仍持有端点克隆时 close 也能唤醒接收者”的回归测试。这里补充的是四种端点的基本注入/收发与回收测试，不据此宣称多个主题、容量覆盖、动态参数或共享实例协议均已对齐。
