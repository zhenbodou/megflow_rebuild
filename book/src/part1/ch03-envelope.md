# Ch1.3 实现 `Envelope` 消息信封与类型擦除消息层

前两章是「原理课」：Ch1.1 讲清了消息交接靠**移动所有权**、跨任务靠 `Send`、共享只读靠 `Arc`；Ch1.2 讲透了**泛型→trait→`dyn`→`Any`** 这条从「编译期定死类型」到「运行期擦掉类型」的路。这一章开始是「施工课」——把原理**落成能编译、能测试的真实代码**，写进 `code/flow-message`。

从这一章起，凡是引擎的真实代码，一律走**红-绿 TDD**：先写一个会失败的测试（红），再写最小实现让它通过（绿）。这不是形式主义——它逼我们在动手前先把「对外契约」用可执行的形式钉死。本章要交付的契约，就是 Ch0.3 从真实 flow-rs 里读出来的那几行：`new` / `unpack` / `repack<T>` / `repack_inplace`，外加类型擦除的 `seal` 与安全 `downcast`。

<!-- toc -->

## 1. 先立契约：这一章要造什么

一句话：**造一个能被一条 channel 搬运的「消息盒子」**。它有三层：

- `Envelope<M>`——带类型的信封，装着载荷 `M` 和一份元信息 `EnvelopeInfo`。这是节点代码里**看得见类型**的那一面。
- `AnyEnvelope`——类型擦除的 trait，把泛型 `M` 藏起来，只暴露不带泛型的方法（Ch1.2 §4 的对象安全约束）。
- `SealedEnvelope = Box<dyn AnyEnvelope + Send>`——装箱擦除后的盒子，是 channel 真正搬运的东西；下游用 `downcast` 把类型「认领」回来。

对应到真实 flow-rs（Ch0.3 读过），原版把这套拆在 `envelope/` 下三个文件里（`envelope.rs` / `any_envelope.rs` / `mod.rs`）。我们重写**合成一个 `envelope.rs`**——这一层总共数百行，拆三个文件反而割裂阅读；文件该按「一起变的东西放一起」来分，而不是按类型数量分。

## 2. 红：先写一个跑不起来的测试

TDD 的第一步永远是**红**——写出测试，运行，看它**因为「东西还不存在」而失败**。这一步的价值在于：它先帮你站在**调用方**视角，把 API 长什么样定下来。

先在空练习目录创建 `red.rs`，只写以下完整文件。此时故意不定义 `Envelope`：

```rust,compile_fail
{{#include ../../labs/envelope/red.rs}}
```

运行：

```bash
rustc --edition=2021 --test red.rs -o red-test
```

预期编译失败，诊断包含 E0433 和未声明的 `Envelope`。行号、措辞和错误总数可能随工具链变化，不作为契约。维护脚本会实际验证这个失败，避免把环境故障误认为正确的“红”。

这个测试先约定 `new`、`unpack` 与空载荷状态。接着按下文理解实现，建立章末的完整 Cargo 工程，再运行成功测试；`red.rs` 是独立的故障实验，不放入完成工程的 `src` 或 `tests`。后续测试继续约定元信息保留、类型擦除、失败路径和资源释放。

```mermaid
flowchart LR
    R["🔴 红<br/>写测试 → 编译失败<br/>（类型不存在）"] -->|"钉死对外契约"| G["🟢 绿<br/>写最小实现 → 测试通过"]
    G -->|"下一个特性"| R
```

## 3. 绿：`EnvelopeInfo` 与 `Envelope<M>`

### 3.1 `EnvelopeInfo`——保留原版七项元信息

信封除了载荷，还携带调度和路由需要的元信息。完整重构应保留原版七个字段：
即使当前节点不解释某个字段，也不能在转发或重打包时将它丢掉。

```rust,ignore
use std::any::Any;
use std::sync::Arc;
{{#include ../../../code/flow-message/src/envelope.rs:envelope_info}}
```

Default 令 skipped 为 false，其余六项为 None。None 表示未指定，不能自动当作
Some(0)：例如序号 0 是重排序的第一条消息，未指定序号则是协议错误。
地址字段只储存地址，不会自动路由；skipped 也不会自动过滤消息。

`extra_data` 的 `Arc<dyn Any + Send + Sync>` 允许共享任意可跨线程的数据。
克隆信封时克隆的是 Arc 引用，两个信封仍指向同一份附带对象，不是深拷贝。
转换载荷应使用 repack 保留整份元信息；重新 Envelope::new 会恢复默认元信息。

字符串地址的转换也与原版一致：先尝试解析 u64，失败再对完整字符串做哈希。

```rust
{{#include ../../../code/flow-message/src/envelope.rs:str2addr}}
```

不要预先 trim 字符串或把哈希值当成跨版本的永久 ID。实现使用的 DefaultHasher
没有承诺跨 Rust 版本保持同一种算法。测试应比较同一算法产生的结果。

```bash
cargo test --manifest-path code/Cargo.toml -p flow-message --test envelope_contract --locked
```

这组测试检查默认值、所有字段的克隆/重打包/类型擦除、Arc 身份、空载荷与地址转换。
它证明元信息存储与传递，不证明后续寻址和批处理消费者已经完整实现。

### 3.2 `Envelope<M>`——载荷为什么是 `Option<M>`

载荷用 `Option<M>` 而非裸 `M`，有两个刚需：① `unpack` 要能把载荷**拿走**（`Option::take` 留下 `None`），这对应「消息被下游取走消费」的语义；② 允许存在**空信封**（`empty()`、或载荷已被取走）。

方法实现直接照契约写。注意 `new`/`unpack`/`repack`/`info` 这些**不涉及类型擦除**的方法放在**无约束**的 `impl<M>` 块里——载荷装取、换包本身不需要 `M: 'static/Send`（真实源码取自 `code/flow-message/src/envelope.rs`）：

```rust
{{#include ../../../code/flow-message/src/envelope.rs:envelope_struct}}
```

`repack<T>` 是引擎里节点做「输入信封 → 输出信封」映射的关键：**载荷类型从 `M` 变成 `T`，但元信息（序号等）克隆保留、随消息一路流下去**。这就是为什么它带一个新的类型参数 `T` 而不是固定 `M`。

### 3.3 `Clone` 约束：为什么单独一个 `impl` 块

原版 `Envelope<M>` 的方法块整体要求 `M: 'static + Send + Clone`。我们**把 `Clone` 拆出来、单独条件实现**——只有当载荷本身可克隆时，信封才可克隆（真实源码取自 `code/flow-message/src/envelope.rs`）：

```rust
{{#include ../../../code/flow-message/src/envelope.rs:envelope_clone}}
```

为什么单独拆一个块？因为「信封可克隆」应当**跟着载荷的能力走**：载荷 `M: Clone`，信封才 `Clone`；载荷不能克隆，信封也不能——这比原版把 `Clone` 硬绑进主方法块更精确。

不过要**诚实地记一笔落差**：把信封**送进引擎**（`seal` 成 `SealedEnvelope` 跨节点搬运）这条路，最终仍然要求 `M: Clone`。原因是下一步的类型擦除接口 `AnyEnvelope` 带了一个 `clone_box` 方法（§4.1 细说），而 Ch4.2 的广播 `Bcast`「一份消息发给多个下游」正是靠它在**擦除之后**还能复制信封。所以真实源码里 `impl<M: 'static + Send + Clone> AnyEnvelope for Envelope<M>`——`Clone` 是硬门槛。这里的 `impl<M: Clone> Clone` 只是把「信封克隆」这件事本身写成条件实现，它服务于 `clone_box`，而不是说「不可克隆的载荷也能流经引擎」。若你此刻还没读到 Ch4.2，把 `Clone` 读作「凡是要进引擎搬运的消息都得可克隆」即可，来龙去脉在广播那章讲透。

## 4. 绿（续）：类型擦除三件套

现在是这一章的技术核心——把 Ch1.2 的原理写成代码。

### 4.1 `AnyEnvelope`：对象安全的擦除接口

trait 里**每个方法都不带泛型参数**（守住 Ch1.2 §4 的对象安全红线），关键是 `as_any` 把自己「降级」成 `&dyn Any`。除此之外还有一个**关键方法** `clone_box`——它是「类型擦除之后还能克隆信封」的本项目采用的方案（真实源码取自 `code/flow-message/src/envelope.rs`）：

```rust
{{#include ../../../code/flow-message/src/envelope.rs:any_envelope_trait}}
```

实现块把泛型 `M` 的约束定死为 `M: 'static + Send + Clone`：

```rust
{{#include ../../../code/flow-message/src/envelope.rs:any_envelope_impl}}
```

三个约束逐一对应：`'static` 来自超 trait `AnyEnvelope: Any`（`Any` 的前提）；`Send` 因为擦除后的盒子恒为 `+ Send`（要跨任务搬运）；**`Clone` 则是 `clone_box` 逼出来的**——擦掉 `M` 后标准库的 `Clone` 已无从谈起（trait 对象不是 `Sized`、也不知道具体类型怎么复制），只能把克隆能力**烙进 trait**：每个具体实现者自己 `self.clone()` 一份、再重新封箱。这正是 `dyn-clone` crate 在背后生成的东西，我们手写它。

> **与 §3.3 呼应的诚实记账**：正因为 `clone_box` 要求实现者 `M: Clone`，「进引擎搬运」的信封才统统需要可克隆。这不是把 `Clone` 强加给一个小众特性——Ch4.2 的广播 `Bcast` 把一份**已封箱**的 `SealedEnvelope` 扇给多个下游，靠的就是 `clone_box`（见 §4.2 的 `impl Clone for SealedEnvelope`）。所以最精确的说法是：**基本信封操作（`impl<M>` 块）不要求 `Clone`，但一旦要 `seal` 进引擎，`Clone` 就是硬门槛**。

### 4.2 `SealedEnvelope` 与 `seal`

`SealedEnvelope` 是装箱擦除后的盒子；`seal` 把 `Envelope<M>` 装进去。多出的 `impl Clone for SealedEnvelope` 是让**封箱后的**盒子也能克隆——标准库对 `Box<dyn Trait>` 不白给 `Clone`，只能手写一条转调 `clone_box`（真实源码取自 `code/flow-message/src/envelope.rs`）：

```rust
{{#include ../../../code/flow-message/src/envelope.rs:sealed_envelope}}
```

`seal` 要求 `M: 'static + Send + Clone`——`Send` 因为 `Box::new(self)` 要能 unsize 强制转换成 `Box<dyn AnyEnvelope + Send>`（前提 `Envelope<M>: Send`，即 `M: Send`，Ch1.1 §3 的跨线程通行证）；`Clone` 则是承接 §4.1 `AnyEnvelope` 实现块的门槛（`clone_box` 要它）。而 `impl Clone for SealedEnvelope { self.clone_box() }` 这一条，正是 Ch4.2 广播 `Bcast` 能对已封箱消息「复制一份发给下一个下游」的底层支撑——它把 §4.1 烙进 trait 的克隆能力，兑现成了对 `SealedEnvelope` 直接 `.clone()`。

### 4.3 安全 downcast：抹掉原版那段 `unsafe`

这是本章相对原版最实在的一处改进。回顾 Ch1.2 §5：原版在 `impl dyn AnyEnvelope` 上**手写** `downcast_ref`，内部用 `unsafe` 把裸指针转换后解引用得到 `&T`。我们不必自己转换裸指针——既然 `as_any()` 能给出 `&dyn Any`，就把「变回具体类型」全权交给**标准库那套久经考验的安全实现**（真实源码取自 `code/flow-message/src/envelope.rs`）：

```rust
{{#include ../../../code/flow-message/src/envelope.rs:safe_downcast}}
```

整段代码**零 `unsafe`**。`downcast` 带泛型参数 `T`，因此这里把它放在 trait 对象的 `impl` 块中，作为**固有方法**，不参与 vtable 分发（Ch1.2 §4 的对象安全约束依然生效）——这一点和原版一致；不同的是内部实现从「手写 unsafe 裸指针转换」换成了「std 安全路径」。测试 `type_erasure_seal_then_downcast` 里「猜错类型得 `None` 而不崩」正是这份安全性的体现。

> **一处细节**：这三个便捷方法定义在 `dyn AnyEnvelope + Send` 上（正是 `SealedEnvelope` 的内层类型），所以对一个 `SealedEnvelope` 直接 `sealed.downcast_ref::<...>()` 就能用。原版为了同时支持 `+ Send`、`+ Send + Sync` 等多种标记组合写了三个几乎重复的 `impl` 块；我们当前只有 `+ Send` 一种擦除盒子在用，就只写这一个——需要别的组合时再加，不预先铺开。

### 4.4 `DummyEnvelope`：一个不带载荷的占位信封

某些控制路径（比如后面的停机信号）需要「一个空信封」而根本不携带任何 `M`。原版用 `DummyEnvelope` 承担。它同时是 `AnyEnvelope` 的**第二个实现者**，正好证明这个 trait 不是 `Envelope<M>` 专属（真实源码取自 `code/flow-message/src/envelope.rs`）：

```rust
{{#include ../../../code/flow-message/src/envelope.rs:dummy_envelope}}
```

`clone_box` 对它同样必须实现（trait 的一部分）——克隆一个 `DummyEnvelope` 再封箱即可。`info()` 用 `unimplemented!()`——这是**刻意保留**原版行为：占位信封不带元信息，引擎的控制路径也从不对它调 `info()`。真到有人调用，`unimplemented!` 会立刻 panic 指出「这里逻辑错了」，比返回一份假数据更早暴露 bug。

## 5. 再跑测试：绿

```bash
cargo test -p flow-message
```

```text
running 8 tests
test envelope::tests::dummy_is_empty ... ok
test envelope::tests::extra_data_shared_via_arc ... ok
test envelope::tests::new_then_unpack ... ok
test envelope::tests::repack_carries_info_and_changes_type ... ok
test envelope::tests::repack_inplace_keeps_type ... ok
test envelope::tests::type_erasure_seal_then_downcast ... ok
test envelope::tests::sealed_envelope_is_cloneable ... ok
test envelope::tests::cloned_sealed_envelope_carries_info ... ok

test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

**绿**。红-绿走完一轮，消息层的地基就位了。多出的 `sealed_envelope_is_cloneable` / `cloned_sealed_envelope_carries_info` 两条正是钉死 §4 那套 `clone_box`——**封箱后仍能克隆、且元信息随克隆一起复制**（Ch4.2 广播的前提）。完整代码在本仓库 `code/flow-message/src/envelope.rs`。本章手写片段用于分阶段解释，最终实现以该文件与契约测试为准。

> **依赖账**：当前信封实现使用标准库，通过对象安全的 clone_box 方法克隆类型擦除信封；原版使用 dyn_clone::DynClone。当前工程没有引入 dyn-clone，后续广播复用 clone_box。

## 6. 逐条对比：我们相对原版改了什么

| 维度 | 原版 flow-rs | 本书重写 | 为什么 |
|---|---|---|---|
| `downcast` 实现 | 先比较类型，再在 `unsafe` 中转换裸指针并解引用 | `as_any() -> &dyn Any` + std 安全 downcast | 把类型恢复交给标准库安全接口 |
| `EnvelopeInfo` 字段 | 7 个 | 同样保留 7 个 | 转发与重打包不丢失协议元信息 |
| `M: Clone` 约束 | 主方法块有 Clone 约束 | 基本信封操作（`impl<M>`）不要求 Clone；`AnyEnvelope`/`seal` 仍要求 Clone（`clone_box` 逼出，Ch4.2 广播用） | 可构造不可克隆载荷的信封做纯本地运算，但送进引擎搬运（seal）仍需 Clone |
| `Send` 约束 | 混在方法块 | 只加在真正要跨线程的 `seal` 上 | 约束跟着需求走 |
| `dyn_clone` 依赖 | 继承 `DynClone` | 自行提供对象安全的 clone_box | 用标准库实现类型擦除后的克隆 |
| 文件组织 | 拆 3 个文件 | 合成 1 个 `envelope.rs` | 一起变的东西放一起 |

这些都不是「为改而改」——每一条都对应「学 Rust」或「更少 bug / 更精确约束」这两个目标里的一个，且都有测试兜底。

## 小结

这一章把消息层从原理**落成了真实、通过测试的代码**：

- **红-绿 TDD 开张**：先写会失败的测试钉死契约（`new`/`unpack`/`repack`/`seal`/`downcast`），再写最小实现变绿。从此引擎代码皆如此。
- **`Envelope<M>`**：载荷 `Option<M>`（支持取走 + 空信封）；`repack<T>` 换类型且元信息随行；`Clone` 拆成按需条件实现。
- **类型擦除三件套**：对象安全的 `AnyEnvelope`（`as_any` 降级）→ `SealedEnvelope = Box<dyn AnyEnvelope + Send>`（`seal` 装箱，`+ Send` 通行证）→ **零 unsafe** 的安全 `downcast`（认领回具体类型，猜错得 `None`）。
- **约束跟着需求走**：`'static`/`Send`/`Clone` 各自只加在真正需要的方法/impl 上，而非一刀切绑死。
- **零外部依赖**：纯 std 完成；当前无需添加 `dyn-clone`。

下一章 **Ch1.4**：给引擎装上**异步的心跳**——`async`/`await`、`Future`、`tokio` 入门，然后把 tokio 的 channel **封装**成引擎自己的收发端，让 `SealedEnvelope` 真正在任务之间「流动」起来。同样红-绿，代码写进 `code/flow-rs`。

## 独立检查点：不用完整引擎也能完成本章

本章之前不应要求你已经写好 Node、Graph 或过程宏。下面的工程只含消息层，
不继承本仓库 workspace 的配置，也没有第三方依赖。

在仓库根目录导出到一个尚不存在的目录：

```bash
python3 scripts/message_checkpoint.py --stage envelope --out /tmp/megflow-message-chapter
cd /tmp/megflow-message-chapter
cargo test --offline
```

若目标已存在，脚本会拒绝覆盖。请换一个新目录，保留你已经写过的代码。
这里的 offline 用于证明消息层不需要下载第三方 crate；首次安装 Rust 工具链仍需
按环境章节完成。预期是 8 个源码单元测试和 7 个信封契约测试通过。

生成的文件结构：

```text
megflow-message-chapter/
  Cargo.toml
  src/
    lib.rs
    envelope.rs
  tests/
    envelope_contract.rs
  examples/
    first_principles.rs
```

### 手工搭建时逐个文件做什么

1. 创建 Cargo.toml：package 名为 flow-message，version 为 0.1.0，edition 为 2021。
   增加空的 `[workspace]`，让它即使放在其他 workspace 内也保持独立；无需 dependencies。
2. 创建 src/lib.rs，写 `pub mod envelope;`，再公开重导出本章信封类型与 str2addr。
3. 在 src/envelope.rs 按本章顺序实现元信息、基本信封操作、类型擦除、克隆与占位信封。
   完整参考文件随检查点导出，遇到报错时比较当前步骤涉及的部分，不必一次复制全部。
4. 将契约测试放入 tests/envelope_contract.rs。它以外部调用者身份使用 flow_message，
   能发现“模块内部能用，但库没有公开导出”的问题。
5. 执行 cargo test --offline，检查测试数量，不能把零测试通过当作本章完成。

### 本阶段的完整文件

前文分段解释每项机制；下面给出可直接核对的完整文件。这里的 lib.rs 只声明 envelope，不引入后面的 algo_base 或 Dr。默认导出模式 full 用于后续消息业务章节；本章必须使用上面的 `--stage envelope`。

`Cargo.toml`：

```toml
{{#include ../../labs/envelope/Cargo.toml}}
```

`src/lib.rs`：

```rust
{{#include ../../labs/envelope/src/lib.rs}}
```

`src/envelope.rs`（包含八个单元测试）：

```rust
{{#include ../../../code/flow-message/src/envelope.rs}}
```

`tests/envelope_contract.rs`（独立调用者的契约测试）：

```rust
{{#include ../../../code/flow-message/tests/envelope_contract.rs}}
```

`examples/first_principles.rs`（逐步从普通值走到类型擦除）：

```rust
{{#include ../../../code/flow-message/examples/first_principles.rs}}
```

保存这些文件后，在新工程目录执行 `cargo test --offline` 和 `cargo run --offline --example first_principles`。库入口负责公开类型；单元测试可以访问模块内部，集成测试只能访问公开 API；example 则是使用这个库的独立可执行目标。三者用途不同，不能只确认 lib.rs 编译成功就省略后两种检查。

### 练习：用测试发现元信息丢失

先读完整契约测试中的三个边界实验：

- `empty_access_panics_and_repeated_take_remains_empty` 区分 `unpack` 和 `take`：前者要求载荷存在，后者允许返回空信封。`catch_unwind` 仅在测试中捕获预期 panic，`AssertUnwindSafe` 是测试对捕获环境的显式声明，不是运行时恢复策略；测试不约束普通 panic 文案。
- `wrong_downcast_preserves_payload_and_dummy_rejects_metadata` 检查类型认领失败后原消息仍可读取，并检查占位信封不能访问元信息。空的 `Envelope<T>` 有元信息，`DummyEnvelope` 没有，两者不可随意替换。
- `payload_moves_and_replacements_release_exactly_once` 用实现了 `Drop` 的载荷记录释放次数。`take` 和 `seal` 只移动，不销毁载荷；克隆产生第二个载荷；原地替换释放旧值；最终丢弃两个盒子分别释放剩余载荷。计数器使用 `Arc<AtomicUsize>` 便于所有副本报告到同一位置，这个同步测试没有后台任务，也不需要 sleep。

这些断言来自固定参照版本 `flow-rs/src/envelope/envelope.rs` 中的 `Option::take`、赋值替换、条件克隆和占位信封实现。它们验证本地可观察行为，不意味着已经运行了原版整个工作区。

在导出的副本里，故意将 repack 的新信封元信息改为 Default::default()，再运行测试。
应当看到元信息保留相关测试失败；修复后恢复通过。不要修改测试期待值来迁就这个错误。
这个练习帮助你理解：测试约束的是业务契约，不是对现有代码的机械描述。

仓库维护者可以执行 `python3 scripts/check_message_course.py`：脚本每次创建全新
临时目录、导出工程、离线测试，最后自动清理。CI 也执行同一个命令，防止本章以后
意外依赖尚未讲到的引擎模块。这个检查点证明本章终点可独立复现，不代表其他章节
已经全部具备逐步检查点。
