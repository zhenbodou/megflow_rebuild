# Ch1.4d 类型信息与转换表

先完成类型化端点。本课先表示两端的类型身份，再实现转换函数注册和收发适配；函数指针与锁的用途随实现步骤讲解。自动注册属性宏在宏专题第 10 课开发，此时可以先手动调用注册函数。

完成标准：能画出端口类型 → 队列类型 → 端口类型的转换方向，并用 repack 保留消息上下文。

<!-- toc -->

## 第十一步：先亲手创建类型描述模块

继续第十步的同一个工程。本步只是记录类型身份，不运行转换。我们现在才创建 config 模块；它暂时只有消息类型描述，不含 TOML、图节点或尚未学习的配置解析。不要复制最终仓库的 config/mod.rs，否则会把后续模块一起带进来。

**替换 src/lib.rs、src/channel.rs、src/channel/typed.rs；新增 src/config.rs 和 src/config/interlayer.rs**。Cargo.toml、error.rs 与 message 不变，没有新增依赖。

完整 **src/lib.rs**：

```rust
{{#include ../../labs/channel-steps/11/lib.rs}}
```

完整 **src/config.rs**：

```rust
{{#include ../../labs/channel-steps/11/config.rs}}
```

完整 **src/config/interlayer.rs**：

```rust
{{#include ../../labs/channel-steps/11/interlayer.rs}}
```

先理解为什么需要两个类型：MsgTypeId 用于比较身份，MsgType 同时保存给人看的名字。Rust 的 TypeId 来自标准库，不需要 serde；Template 只是待以后推导的编号；Python 只是原版标签兼容表示，不会启动 Python。Any 这个枚举分支也不是 std::any::Any trait。

derive 中 Copy 允许复制这个小枚举而不移走原值；Eq 与 PartialEq 使类型可比较；Hash 为下一步按类型对查表做准备。type_name 产生便于阅读的名字，不代替 TypeId 做业务身份比较。下面保留原版 Python 名称裁剪再哈希的规则，与信封地址不裁剪的规则分别处理。

完整 **src/channel.rs**：

```rust
{{#include ../../labs/channel-steps/11/channel.rs}}
```

完整 **src/channel/typed.rs**：

```rust
{{#include ../../labs/channel-steps/11/typed.rs}}
```

先核对新字段 channel_type，再看 channel 调用 channel_with_type 并传入 Any。创建两端时都保存同一个类型标识，克隆自然也保留它。最后读 TypeInfo：普通端点的 port_tid 是 Any，包装端点从 T 得到具体类型；两者 chan_tid 都来自队列创建时保存的值。此时仍没有 conversion 或 with_type 调用。

运行 `cargo test --offline`，预期 **11 项测试**通过。新增测试将队列标为 String，再包成 u32 端口，分别断言两个身份；它只查询描述，不向不匹配的通道发送消息。先能表达不匹配，下一步才讨论怎样转换。独立练习：克隆这两个端点，检查克隆不会改写队列类型。

维护脚本 `python3 scripts/check_basic_channel_course.py` 会从第一步累计构建到这里，确保这些类型确实由你已写出的模块提供，而不是从最终工程隐式取得。

## 类型信息：端口类型与通道类型为什么分开

到这里，我们已有 `SenderT<T>` 与 `ReceiverT<T>`——第十一步你已经在 `config/interlayer.rs` 定义了消息类型描述，并让两种端点实现了 `channel::TypeInfo`。这一节回头把这层设计讲透：它为后续类型推断与转换表准备数据，不会自动转换载荷。

先用一个问题理解需求：节点声明接收 `u32`，但接线系统为队列选择了 `String`。框架至少要记住两件事，才能判断是否存在 String → u32 的转换函数。只记录一个类型，就无法描述这个转换请求。

翻回你在第十一步 `src/channel.rs` 里写的 `TypeInfo` trait：它只声明 `port_tid` 与 `chan_tid` 两个方法，一个回答「端口声明成什么类型」，一个回答「底层队列是什么类型」——正好就是上面那两件必须记住的事。

`port_tid` 描述端口声明：普通端点是 `Any`，`ReceiverT<u32>` 是 Rust 的 u32 类型。`chan_tid` 描述底层通道：类型包装和克隆不会改变它。`Any` 在这里是 MegFlow 枚举的一种值，不是 `std::any::Any` trait，也不代表队列会检查每条消息并改变自己的声明。

### 第一步：定义可以比较的类型身份

`MsgTypeId` 有四个分支：`Any`、`Rust(TypeId)`、`Python(u64)`、`Template(usize)`。Rust 部分用 `TypeId::of::<T>()` 获得类型身份；模板分支存放待后续推断的编号。保留 Python 标签是保留原版描述结构，不会引入 Python 执行器依赖。

`TypeId` 适合判断当前程序中两个类型是否相同，需要 `T: 'static`。这里的 `'static` 约束不表示值必须一直存活到程序结束；例如一个局部创建并及时释放的 String 也满足这个类型约束。不要把 TypeId 的哈希值作为跨版本存档或网络协议编号，其哈希与排序不保证跨 Rust 版本稳定。[标准库 TypeId 文档](https://doc.rust-lang.org/std/any/struct.TypeId.html)

`MsgType` 再把可读名字和类型身份放在一起：`name: String` 用于描述，`id: MsgTypeId` 用于判断。原版辅助函数 `any()` 生成 Any，`template(3)` 生成名字 T3 与编号 3，`of::<u32>()` 生成 Rust 类型描述。Python 名字按原版规则先 trim 再哈希；它与消息地址 `str2addr` 不裁剪空白的规则不同。

### 第二步：让队列端点保留类型描述

在 Sender、Receiver 中各增加 `channel_type: MsgTypeId` 字段。在 `channel_with_type(capacity, channel_type)` 创建两端时复制同一个标识；这个枚举实现 Copy，复制它不会复制消息队列。已有 `channel(capacity)` 调用该函数并传入 Any，因此原来的无类型调用保持原行为。

默认未接线端点也采用 Any。随后给普通端点实现 TypeInfo：`port_tid` 返回 Any，`chan_tid` 返回保存的字段。给类型包装实现同一 trait：`port_tid` 返回 `MsgTypeId::of::<T>()`，`chan_tid` 委托内部端点。

你在第十一步 `src/channel.rs` 的单元测试里已经亲手写过对应的回归测试 `wrapper_does_not_relabel_queue`（容量 1，发送、接收两端都验）：把队列标成 `String`、两端包成 `u32` 端口后，断言 `port_tid()` 是 `u32`、`chan_tid()` 仍是 `String`。（仓库终点另有一份 `tests/type_info.rs::wrapper_type_does_not_relabel_channel`，把容量 0、1 都跑一遍，作用相同。）

`receiver.into()` 只是移动端点到类型包装中，不能把 String 队列重新标成 u32。此时两种类型不同是允许被描述的状态，并不意味着接收已经安全可用。下一节接入转换表；如果没有登记匹配转换且实际载荷不是 u32，类型化接收的 downcast 仍会失败。

### 第三步：区分描述、验证与转换

| 工作 | 当前状态 |
| --- | --- |
| 描述端口与队列类型 | 已实现 TypeInfo |
| 保留克隆、包装、默认端点的类型信息 | 有回归测试 |
| 根据全图约束推断队列类型 | 尚未完整迁移 |
| 按源/目标类型查转换函数并转换消息 | 下一节实现直接注册转换；完整推断尚缺 |

`channel_with_type` 是本重构当前的装配辅助入口，原版通过 ChannelStorage 及状态对象携带通道类型。它不扫描消息、不强制类型，也不能替代完整 ChannelStorage 协议。

运行 `cargo test --manifest-path code/Cargo.toml -p flow-rs --test type_info --locked`。然后独立把示例的队列改成 Any、端口改成 i32，写出两个查询的预期值；再克隆接收端，检查队列标识没有变化。验收重点是能解释两个类型的来源，而不是只背下两个函数名。

## 转换表实作：把类型对映射到函数

### 第十二步：先手动登记函数，再接入收发

继续第十一步工程。**新增 src/channel/conversion.rs，替换 src/channel.rs 和 src/channel/typed.rs**；Cargo.toml、lib.rs、error.rs、config 两个文件和 message 全部保留。此步不使用 inventory，静态自动注册留到注册宏开发时再加入。

完整 **src/channel/conversion.rs**：

```rust
{{#include ../../labs/channel-steps/12/conversion.rs}}
```

先写函数指针别名 CvtF：参数和返回都是 SealedEnvelope，因此一张表能保存不同业务类型的转换。函数签名无法替你检查它注册的源类型是否正确，后面的测试负责验证登记和函数实现一致。

HashMap 是标准库的键值表，键是有顺序的二元组 `(源类型, 目标类型)`。LazyLock 在第一次使用时创建表；RwLock 允许多次查询共享读锁，插入需要独占写锁。它们都来自 std，不增加依赖。lookup 通过 copied 复制一个函数指针后释放读锁，绝不拿着注册表锁执行用户函数。unwrap 表示锁中毒时 panic，本步没有实现锁中毒恢复策略。

完整 **src/channel.rs**：

```rust
{{#include ../../labs/channel-steps/12/channel.rs}}
```

完整 **src/channel/typed.rs**：

```rust
{{#include ../../labs/channel-steps/12/typed.rs}}
```

这次才出现 with_type：它不是上一课凭空可用的方法。Sender 查询端口类型到队列类型，Receiver 查询队列类型到端口类型，各自保存 `Option<CvtF>`。From 在构造包装时调用它；复制端点也会复制缓存，注册表后来变化不会追溯修改旧端点。

发送先运行转换再入队；接收先取出信封并退出接收锁作用域，再运行转换。convert 使用 Tokio 已有 rt feature 提供的 spawn_blocking，把同步函数放到阻塞任务线程池执行。它返回 JoinHandle，await 等待结果；unwrap 遇到任务 panic 时继续 panic。这不是启动新进程，也不会自动取消已经运行的转换函数。

运行 `cargo test --offline`，预期 **12 项测试**通过。新测试使用互不混淆的 Input、Wire、Output 类型，手动登记加 1 和乘 2 两个函数，输入 10 输出 22，序号保持 42。测试类型定义在测试内部，避免共享注册表与别的测试碰巧使用同一类型对。

排错实验：把 encode 中的 repack 改成 Envelope::new，确认序号断言失败；恢复后通过。独立练习：把第二个转换改成减 3，先写下期望输出 8，再修改代码验证。不要改变序号断言。

维护脚本现可连续构建第一至第十二步。以下文字进一步解释直接转换和后续类型选择的规则；自动选择队列类型尚不是第十二步已经写出的功能。

上一节先完成类型描述，现在回头看你在第十二步 `channel/conversion.rs` 里写下的函数指针别名：`pub type CvtF = fn(SealedEnvelope) -> SealedEnvelope;`。

输入和输出都被类型擦除，因为一张注册表需要容纳不同类型的转换函数。函数内部负责 downcast 到已约定的输入类型，取出业务数据，构造目标数据，再封箱返回。转换表无法从这个签名判断函数是否遵守注册的类型约定；错误的注册仍会造成运行错误。

### 1. 用有方向的键保存转换函数

当前实现用 `HashMap<(MsgTypeId, MsgTypeId), CvtF>`。键是 `(源类型, 目标类型)`，因此 A → B 与 B → A 是不同条目。原版使用两层 HashMap，查询规则相同：只查直接登记的边；登记 A → B 和 B → C 不会自动获得 A → C；同一类型对后登记覆盖先登记。

表由 `LazyLock` 延迟初始化，`RwLock` 保护并发查询和修改。注册时持写锁插入，查找时持读锁并 `.copied()` 取出函数指针。锁在查询结束后释放，**不能持有注册表锁运行转换函数**，否则转换函数内部再次注册时可能相互等待。

公开入口 `channel::add_cvt_func_impl(from, to, function)` 对齐原版同名注册 API。普通函数可作为函数指针；捕获外部环境的闭包不能直接填进这个函数指针类型。

### 2. 包装端点时选定转换函数

`SenderT<T>::from(sender)` 查询 `T → sender.chan_tid()`；`ReceiverT<T>::from(receiver)` 查询 `receiver.chan_tid() → T`。找到的函数指针保存在该端点中，之后收发使用这份缓存。后来覆盖注册表不会自动更新已经包装好的端点；克隆端点也会复制这份缓存。这与原版 From 实现先查询再保存的方式一致。

普通 `Sender::send<T>` 的泛型参数不会替你重新查表。需要在完成注册后构造类型化端点；没有找到转换函数时，端点沿用无转换路径，而不是直接报“转换不存在”。完整建图类型推断还没有迁移，不能依赖它事先挡住所有不匹配。

### 3. 运行一次端到端转换

阅读并运行 `code/flow-rs/tests/conversion.rs`：

```sh
cargo test --manifest-path code/Cargo.toml -p flow-rs --test conversion --locked
```

程序定义三种不同的载荷 Input、Wire、Output，并登记两个函数：Input → Wire 将数字加 1，Wire → Output 将数字乘 2。队列声明存 Wire，发送端接受 Input，接收端返回 Output。因此输入 10 最终得到 22。两次转换都用 `repack` 保留上下文，测试还检查 `partial_id` 仍为 42。

发送路径在入队前转换，接收路径先取出消息、释放队列接收锁，再转换。当前使用 Tokio 的 `spawn_blocking` 执行同步转换函数，以免转换计算阻塞执行异步任务的线程。任务返回后再继续收发；该安排不保证多个竞争接收者完成转换的顺序等于出队顺序。

DummyEnvelope 是控制消息，不送入业务转换函数；完整 flush 协议仍未迁移。转换任务 panic 的传播已按原版句柄行为修正：等待它的收发任务继续 panic；具体过程见 [错误分层实验](ch04e-cancellation-errors.md)。

### 4. 独立验收

将第二个转换改为减 3，输入 10 应得到 8；保留 `partial_id` 断言。再临时将一个转换中的 `repack` 改成 `Envelope::new`，确认元信息断言失败。最后解释为什么注册 A → B 不应该自动允许 B → A：很多转换会丢失数据，根本无法逆转。

转换表已能驱动已注册的业务转换，但尚不能替代 ChannelStorage、完整模板类型推断、转换函数注册宏及控制协议。这些仍属于完整实现的必做部分。


## 选择队列类型：把所有端口的要求放在一起

### 第十二步补充：将选择规则写成可运行代码

在第十二步工程上，**替换 src/error.rs、src/channel/conversion.rs 和 src/channel.rs，新增 tests/channel_type_guess.rs**。其余文件包括 typed.rs 不变。本步仍不增加依赖，不接入图构造器；调用方直接提供发送和接收类型集合。

先阅读下面候选选择的五段解释，再保存以下完整文件。

完整 **src/error.rs**：

```rust
{{#include ../../labs/channel-steps/12a/error.rs}}
```

新增两个错误变体分别表示缺少具体类型与找不到兼容类型。它们需要在写算法前定义，不能等后续节点错误模块来提供。

完整 **src/channel/conversion.rs**：

```rust
{{#include ../../labs/channel-steps/12a/conversion.rs}}
```

完整 **src/channel.rs**：

```rust
{{#include ../../labs/channel-steps/12a/channel.rs}}
```

这个文件只比第十二步增加 guess_channel_type 的公开重导出；重复展示是为了让你能核对完整文件，并不要求重写其余方法。

完整 **tests/channel_type_guess.rs**：

```rust
{{#include ../../labs/channel-steps/12a/channel_type_guess.rs}}
```

运行 `cargo test --offline --test channel_type_guess`；全部通过后运行 `cargo test --offline`，保留前十二步的全部测试。维护脚本把这个步骤标为 12a，在第十三步前构建。不要为了选定某个类型而把 HashSet 改成固定遍历顺序：只要候选合法即可，下面会解释原版为何不承诺唯一结果。

手动指定队列类型只是前一步。连接可能挂多个发送端和多个接收端，装配器需要选择一种
所有端点都能衔接的类型。本节迁移原版 `channel/storage.rs::ChannelStorage::guess`
的选择规则，当前入口叫 `channel::guess_channel_type`；完整 ChannelStorage 仍需继续开发。

### 1. 先定义输入和约束

函数接收两个 `HashSet<MsgTypeId>`：tx 是发送端声明的类型集合，rx 是接收端声明的类型集合。
使用集合是因为相同类型出现两次不会增加新的类型约束。假设候选队列类型是 C：

- 每个具体发送类型 S 必须与 C 相同，或登记了直接转换 S → C。
- 每个具体接收类型 R 必须与 C 相同，或登记了直接转换 C → R。

方向不能交换：发送端先转换再入队，接收端先出队再转换。这两条要求是把转换表接到
接线系统上的条件，不是在计算某个值。

### 2. 先处理没有具体类型的情况

原版先检查是否所有类型都是 Template。若是，返回模板推断失败。两个集合都为空时
也会失败：迭代器 `all` 对空输入返回 true，因为没有反例；不要把它误改成成功返回 Any。

随后检查是否全部是 Any 或 Template。这个分支返回 Any。结合上一步可知：
只有模板时失败，包含 Any 且没有具体类型时成功得到 Any。顺序是算法的一部分。
当前分别用 `Error::TemplateInferFault` 与 `Error::ChannelTypeMismatch` 表达模板失败和
不存在兼容候选的情况；原版使用自己的错误包装，错误表示不完全相同。

### 3. 从端口已有类型中逐个尝试

候选只来自 tx 和 rx 中实际出现的具体类型。过滤掉 Any 与 Template，然后对每个候选
检查上面的两条条件。不是枚举所有注册转换的类型，也不是寻找最短转换路径。

例如已有 A → B、B → C 两个转换：

| 发送类型集合 | 接收类型集合 | 结果 |
| --- | --- | --- |
| A | C | 失败：候选只有 A、C，没有直接 A → C |
| A、B | B、C | 选择 B：A 能到 B，B 能到 C |
| C | A | 失败：没有反向转换 |

若有多个合法非 Python 候选，原版会返回遍历中先遇到的那个。HashSet 不保证固定的
类型遍历顺序，不能在测试里凭想象断言某种稳定优先级。原版会优先采用可行的非 Python
类型，Python 候选留作后备；保留标签选择规则并不表示已经实现 Python 执行支持。

### 4. 写实现时怎样读迭代器链

先用 `tx.iter().chain(rx)` 查看两侧类型，再用 `filter` 筛除抽象类型。对每个候选，
分别对 tx、rx 使用 `all` 检查兼容性；类型相同时无需查转换表。

整个候选检查期间持有转换表读锁，看到的是同一份注册状态。不能每查一条边就松锁、
再重新取锁，否则并发注册可能让同一次推断混用不同时刻的规则。此处只检查键是否存在，
不执行用户转换函数；完成选择后释放锁。

实现保存在 `channel/conversion.rs`，可以与前面的表查询放在一起复用读锁。
增加公开导出后运行：

```sh
cargo test --manifest-path code/Cargo.toml -p flow-rs --test channel_type_guess --locked
```

测试先验证空集合、模板、Any、直接转换方向，再将选出的类型传给 `channel_with_type`。
实际消息从 Raw(9) 转成 Stored(10)，出队后得到 Rendered("10")，证明选择结果可以驱动
前面已经实现的两侧转换。测试所用的候选是唯一合法者，不依赖 HashSet 顺序。

### 5. 这一步如何进入最终建图流程

接下来的装配需要先从节点宏和注册信息取得各端口类型，把同一连接的类型分别汇总到
发送集合、接收集合，再调用选择函数创建通道。当前 Builder 已接入具体类型的收集与选择，详见 Ch3.2a 第 7 节；
本节的独立集成测试仍由调用者显式提供集合；不能把这项通过写成“全图类型推断已完成”。
模板变量在不同连接间的约束传播也不是本函数负责的工作，原版 postprocess/type_infer
还有更完整的过程。后续必须将这些步骤实现并串联起来。

独立练习：在表格第一行补登记 A → C，重新列出合法候选；再加入一个既不能转换到候选、
也不等于候选的新发送类型 D。解释为什么多一个发送端可能使原先成功的连接变成失败，
然后用测试验证，而不是仅凭错误字符串猜测算法。
