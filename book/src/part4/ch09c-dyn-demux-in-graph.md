# Ch4.9c DynDemux 装进真实图 + Sandbox 动态端口

[Ch4.9](ch09-dynports.md) 造出运行期环路（create → publish → fetch → route）、[Ch4.9a](ch09a-derive-dyn-ports.md) 让 `#[outputs(out: dyn T0)]` 长出 `DynPorts` 字段 + 生成 `set_port_dynamic`、[Ch4.9b](ch09b-config-auto-wiring.md) 让 config 建图期**自动接线**动态子图——但触发方一直是**测试夹具**（Ch4.9 的裸 `route`、Ch4.9b 的 `AutoTrigger`）。本课把触发逻辑接入 `ty="DynDemux"` 的注册节点，这正是 [Ch4.7c](ch07c-demux-node.md) §5 结尾（「动态端口在原版 Sandbox 中还涉及 broker 与临时图……尚未实现」）与 [Ch4.9b](ch09b-config-auto-wiring.md) §8 明确 defer 的最后一块。

本章合上整条弧，做两件事：

1. 把那条环路封装成**注册内置节点** `DynDemux`（住进 `code/flow-rs/src/builtin.rs`）——真实 TOML 写 `ty="DynDemux"` 就能用，由 [Ch4.9b](ch09b-config-auto-wiring.md) 的 `Builder::build` 自动接线驱动，对齐原版 `node/demux.rs` 的 `DynDemux`。
2. 给 [Ch3.4](../part3/ch04-binaryop-e2e.md) 的单节点 `Sandbox` 补上**动态输出端口支持**（一个内部 broker + 一张平凡汇子图），让 `DynDemux` 这类 dyn 节点能被单独测试。

动态支持应保留已有静态图与沙箱的行为，已有测试用于回归验证。不以这些测试证明所有输入下逐字节等价；动态生命周期需单独检查。

<!-- toc -->

## 1. 契约：喂入侧单向的注册分流节点

`DynDemux` 与 [Ch4.7](ch07-demux.md) 的静态 `Demux` 是同一主题的两端——都按信封 `to_addr` 分流，区别只在**下游是谁**：

| | 静态 `Demux`（[Ch4.7c](ch07c-demux-node.md)） | `DynDemux`（本章） |
| --- | --- | --- |
| 输出端口 | `out: {T0}`（**字典**端口，`DynPorts` 之外） | `out: dyn T0`（**动态**端口，持 `DynPorts<Sender>`） |
| 下游 | **建图期就接好**的一组固定端点 | **运行期按 key 现装**的子图实例，一条流一份 |
| 生命周期 | 无——端点随图存亡 | 可创建、可拆除（空信封即拆） |
| 未命中 key | 静默丢弃 | `create` 一个新实例 |

它是**喂入侧单向**的（对齐原版 `node/demux.rs`）：只有一个 dyn 输出、只往实例入口**喂**，不从实例收结果。实例是**汇子图**——末端落进资源、没有对外出口。这与 [Ch4.9b](ch09b-config-auto-wiring.md) `AutoTrigger` 夹具的 feed+collect 双向不同；原版 `logical_test.toml` 的 `destination` 子图正是这种「只收不还」的汇。方向规则仍是那条[反直觉但正确](ch09b-config-auto-wiring.md)的：dyn **输出**持 `DynPorts<Sender>` → 往实例**入口**灌 → 边界落在子图 `inputs` 上。

运行期三条规则（`exec` / `finalize` 逐条落地，下一节即真实源码）：

- **有载荷信封**：`is_cached` 没命中 → `create` 一个实例（`JoinHandle` 收进 `tasks`），再 `fetch_with_cache` 取回该 key 的入口 `Sender`、`send_any` 灌进去。**每 key 只 create 一次**。
- **空信封**（`is_none`）：拆除信号——`evict` 撤掉该 key 的入口端点（实例失去唯一外部 `Sender` → 优雅停机），`await` 它的 `JoinHandle`。
- **`finalize`**：输入关闭后，把残留实例逐一 `evict` 再 `await`，在实例能够结束的正常路径等待它们退出。原版还依靠端点的显式 close；当前重构通过 evict 释放持有权，不能只比较 finalize 的几行代码就宣称更稳或等价。

## 2. `DynDemux`：注册版内置节点

一切都由 [Ch4.9a](ch09a-derive-dyn-ports.md) 的 `dyn` 端口语法 + [Ch4.9b](ch09b-config-auto-wiring.md) 的自动接线撑着，节点本体因此极小——两个 `#[state]` 字段（每 key 的实例任务句柄 `tasks`、注入给实例的 `resources`），加上 `initialize` / `exec` / `finalize`：

```rust,ignore
{{#include ../../../code/flow-rs/src/builtin.rs:dyn_demux}}
```

三处值得停一下：

- **`out` 是 `dyn` 输出**，字段类型 `DynPorts<Sender>` 由派生宏生成（[Ch4.9a](ch09a-derive-dyn-ports.md)）；建图期 config 自动接线（[Ch4.9b](ch09b-config-auto-wiring.md)）把指向动态子图位点的 `DynPortsConfig` 经 `set_port_dynamic` 注入进它。节点代码里**看不到**任何手搓的接线——这正是整条弧的目的。
- **资源随 `create` 穿进实例**：`initialize` 从 `Context` 取一份 `resources`（`ResourceCollection` 是 `Arc` 共享的廉价克隆），`exec` 里 `create(key, resources)` 把它传给实例。于是实例与外层图**共享同一份资源集**——下一节的端到端测试正是靠这条把「实例侧发生了什么」观测出来的。
- **`send_any` 后 `.ok()`**：别把「实例通道已关」误判成「我的输入关」——实例可能已被拆除，灌不进去就丢，不影响 `DynDemux` 自己的收尾判定。

`node_register!("DynDemux", DynDemux)` 把它登进注册表——真实 TOML 写 `ty="DynDemux"` 即可。

## 3. 真实 TOML：全程走注册表 + 自动接线

到了兑现「装进真实图」的时候：一份 TOML，`ty="DynDemux"` + 一条 dyn 连接 + 一张动态子图，经 `Builder::build` 自动接线跑通。见 `code/flow-rs/tests/dyn_demux_graph.rs`。

**难点：`DynDemux` 喂入侧单向，实例没有可观察输出，怎么证「create-once + 路由」？** 用一份共享资源 `Recorder`（实例日志）：汇子图里的 `Probe` 在 `initialize` 记一条 `"init"`（每实例只 `initialize` 一次 → 数它即得**实例数**）、每收一条消息记下**载荷**。图跑完后读 `Recorder`：`"init"` 条数 == 不同 key 数（**create-once**），载荷齐全（**routing**）。`Probe` 的 `#[outputs]` 为空 = 纯 sink，对齐原版汇子图末端的 Printer：

```rust,ignore
{{#include ../../../code/flow-rs/tests/dyn_demux_graph.rs:fixtures}}
```

接线全在 TOML 里声明——汇子图 `worker`（一个 `Probe`，借资源 `rec`）+ 顶层 `top`（`DynDemux` + 子图位点 `site`），关键是那条 dyn 连接 `["demux:out", "site:inp"]`。资源 `rec` 声明在 `top`，装配期建一次、随 `create` 注入每个实例：

```rust,ignore
{{#include ../../../code/flow-rs/tests/dyn_demux_graph.rs:toml}}
```

主线测试经 `Builder::build`（`Config::from_toml` → `flatten` 跳过 dyn 子图 → `assemble` 自动接线，全在 [Ch4.9b](ch09b-config-auto-wiring.md)）跑通，断言 `"init"` 恰 2 条（key 7、42 各一个实例；key 7 的两帧复用同一实例）+ 载荷 `["a","b","c"]` 全部到达：

```rust,ignore
{{#include ../../../code/flow-rs/tests/dyn_demux_graph.rs:e2e}}
```

在仓库根目录复现：

```sh
cargo test --manifest-path code/Cargo.toml -p flow-rs --test dyn_demux_graph --locked
```

预期两个测试全过：

```text
running 2 tests
test dyn_demux_runs_in_sandbox ... ok
test dyn_demux_routes_streams_in_a_real_graph ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

关键观察：`inits == 2`——key 7 连送两帧只 `create` 一次实例（若每帧新建会数到 3 个 `"init"`），key 42 是另一份独立实例，这就是 **create-once**；三条载荷 `a`/`b`/`c` 全部落进对应 key 的实例日志，这就是 **routing**。全程**没有一行手搓 `DynPorts`**，也没有 `AutoTrigger` 那样的测试夹具触发器——真实注册节点 + 真实 TOML。

## 4. Sandbox 动态端口支持

[Ch3.4](../part3/ch04-binaryop-e2e.md) 的 `Sandbox` 能「按类型名建一个节点、喂输入、收输出」，但它一直**不支持动态端口**（[Ch4.7c](ch07c-demux-node.md) §5 结尾明说了）。问题在于：dyn 输出节点运行期要 `create` 出子图实例、把数据灌进去；图里这实例由 config 自动接线指向某张真实子图，**沙箱里没有那张图**。

本章补上最小支持：给 dyn 输出端口配一张**平凡汇子图**当惰性构造器——一个 `NoopConsumer`（收下即弃）、边界入口叫 `inp`。被测节点的 `create` 便造出这个「黑洞」实例、`fetch` 回入口 `Sender`、`send_any` 灌进去，数据被安静吸收。用 TOML 解析（与真实图同一条 `Config::from_toml` 路径），不手搓 `GraphConfig`：

```rust,ignore
{{#include ../../../code/flow-rs/src/sandbox.rs:sandbox_sink}}
```

`with_args` 里，dyn 输出端口**不开** channel（它的字段是 `DynPorts`，构造器走 `Default`、不从 outs 取），只记下端口名；节点造好后，为每个 dyn 端口建一个内部 broker 订阅 + 构造 `DynPortsConfig` + `set_port_dynamic` 注入——这与 [Ch4.9b](ch09b-config-auto-wiring.md) 建图期的注入是同一套动作，只是这里 topic 和汇子图是沙箱现造的：

```rust,ignore
{{#include ../../../code/flow-rs/src/sandbox.rs:sandbox_dyn_wiring}}
```

`start()`（见 `code/flow-rs/src/sandbox.rs`）里的次序沿用 [Ch4.8](ch08-broker.md) 的硬纪律：**先** `broker.run()`（订阅早在 `with_args` 就为每个 dyn 端口做完了——订阅先于 run），**再** spawn 节点；节点收尾后才 `await` broker 句柄（节点 close 关闭 Broker 通知 → topic fan-out 任务结束 → broker 句柄解析）。无 dyn 端口时 `broker` 为 `None`、这两步都是空操作——**恒等护栏**就落在这里。

于是 `DynDemux` 能在单节点沙箱里跑通——create → fetch → route → 拆除 → 干净停机全程不挂（`tokio::time::timeout` 守住任何挂起）：

```rust,ignore
{{#include ../../../code/flow-rs/tests/dyn_demux_graph.rs:sandbox}}
```

复现：

```sh
cargo test --manifest-path code/Cargo.toml -p flow-rs --test dyn_demux_graph --locked
```

## 5. 落差与后续：这一章**没做**什么

本章完成一条注册节点驱动动态汇子图的基础链，但尚未证明整条 DynPorts 协议与原版等价。下面的资源继承、实例参数和 Sandbox 动态输入**仍在纯 Rust 目标范围内，必须继续实现与讲解**；只有最后一项 Python/C 绑定路径被排除。

- **资源只注入、不链接**——原版 `ext_resource.chain(in_resource)` 把外层注入资源与子图自带资源链起来、按 instance-id 分尺度；本弧多个实例共享同一注入集合，子图暂不声明自带 `resources`。这是 [Ch4.9](ch09-dynports.md) 起就记着的 defer，本章的注册节点不改变它。
- **`DynPortsConfig.args` 未合并**——原版按 instance 用 `merge_table` 覆盖实例参数；本弧 `create` 接受 `cap` 但不做 args 合并。
- **Sandbox 只支持 dyn 输出、不支持 dyn 输入**——dyn 输入端口要一张**产出侧**临时子图喂数据，而没有哪个内置节点是纯产出者（`with_args` 对 dyn 输入直接返回 `Error::Unsupported`，见 `code/flow-rs/src/sandbox.rs`）。真正的 dyn 输入验证靠 [Ch4.9b](ch09b-config-auto-wiring.md) 的 `AutoTrigger`（feed+collect）图端到端覆盖，沙箱不替代它。
- **Python / C-FFI loader 的动态端口路径**——原版有一套 FFI 的 dyn 端口 no-op 分支，属本书 Rust-only 范围外。

## 小结

- **`DynDemux` = 把整条弧封装成一个注册节点**：`#[inputs(inp: T0)] #[outputs(out: dyn T0)]` + `#[state]` 存 `tasks`/`resources`，`exec` 就是 create-once + route + 空信封拆除那条环路，`node_register!("DynDemux", ..)` 让真实 TOML 用得上。
- **喂入侧单向**：单个 dyn 输出、只喂不收，实例是汇子图——对齐原版 `node/demux.rs` 与 `logical_test.toml` 的 `destination`。
- **真实 TOML 端到端**：`ty="DynDemux"` + 一条 dyn 连接经 `Builder::build`（[Ch4.9b](ch09b-config-auto-wiring.md)）自动接线，用 `Recorder` 资源 + `Probe` 汇节点把不可观察的实例行为观测出来——`"init"` 数证 create-once、载荷证 routing。全程无手搓 `DynPorts`。
- **Sandbox 动态端口支持**：dyn 输出端口配内部 broker + 平凡 `NoopConsumer` 汇子图当惰性构造器，`set_port_dynamic` 注入——与建图期同一套动作；`start()` 先 run 后 spawn、收尾后 await broker；无 dyn 端口恒为 `None`、逐字节恒等。
- **落差诚实标注**：资源只注入不链接、`args` 未合并、Sandbox 无 dyn 输入、Python/C-FFI 路径——都在原版分层或 Rust-only 范围外，边界写清。
- flow-rs 测试从 151 抬到 **153**（新增 `dyn_demux_graph` 的 2 个测试）；既有 151 个一字不改继续绿——恒等护栏成立。

## 完整测试文件与证据边界

下面是 `code/flow-rs/tests/dyn_demux_graph.rs` 的完整内容，包含资源、Probe、配置、全部导入以及两个测试。按前文讲解逐段填写后，用这份完整文件检查是否漏掉注册或辅助代码。

```rust
{{#include ../../../code/flow-rs/tests/dyn_demux_graph.rs}}
```

本章测试的观察量要分别解释：初始化次数证明创建数量；载荷日志证明消息到达；等待图句柄与超时证明这个正常场景能结束。若日志没有记录实例 key，就不能仅凭载荷集合证明每一条都进入了正确 key 的实例；应另用 Ch4.9b 的有状态分 key 场景验证，进一步增强本测试时也应记录归属，而不是只增加消息条数。

Sandbox 的 NoopConsumer 会丢弃数据。因此“沙箱能结束”不证明业务输出正确，更不证明动态输入支持。真实图测试和沙箱测试回答不同问题，不能互相替代。

独立练习：画出 tasks 和 out.cache 两张表的关系。前者保存运行任务句柄，后者保存通信端点；删除其中一张的项为什么不会自动删除另一张？再分析空信封到达一个尚未创建的 key 时应发生什么，先读当前 exec，再对照原版，分别记录两者的行为和待补测试。

关闭路径的已知差异见 [Ch4.9a](ch09a-derive-dyn-ports.md)：原版 close 还会关闭缓存通道，当前重构只关闭 Broker。这个差异与资源继承、参数覆盖一样属于验收缺口，不能在本章小结中改写为目标外工作。
