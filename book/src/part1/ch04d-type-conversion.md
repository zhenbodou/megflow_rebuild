# Ch1.4d 类型信息与转换表

先完成类型化端点。本课先表示两端的类型身份，再实现转换函数注册和收发适配；函数指针与锁的用途随实现步骤讲解。自动注册属性宏在宏专题第 10 课开发，此时可以先手动调用注册函数。

完成标准：能画出端口类型 → 队列类型 → 端口类型的转换方向，并用 repack 保留消息上下文。

<!-- toc -->

## 类型信息：端口类型与通道类型为什么分开

到这里，我们已有 `SenderT<T>` 与 `ReceiverT<T>`。接下来在 `config/interlayer.rs` 定义消息类型描述，并让端点实现 `channel::TypeInfo`。这一层为后续类型推断与转换表准备数据，不会自动转换载荷。

先用一个问题理解需求：节点声明接收 `u32`，但接线系统为队列选择了 `String`。框架至少要记住两件事，才能判断是否存在 String → u32 的转换函数。只记录一个类型，就无法描述这个转换请求。

```rust
{{#include ../../../code/flow-rs/src/channel.rs:type_info_trait}}
```

`port_tid` 描述端口声明：普通端点是 `Any`，`ReceiverT<u32>` 是 Rust 的 u32 类型。`chan_tid` 描述底层通道：类型包装和克隆不会改变它。`Any` 在这里是 MegFlow 枚举的一种值，不是 `std::any::Any` trait，也不代表队列会检查每条消息并改变自己的声明。

### 第一步：定义可以比较的类型身份

`MsgTypeId` 有四个分支：`Any`、`Rust(TypeId)`、`Python(u64)`、`Template(usize)`。Rust 部分用 `TypeId::of::<T>()` 获得类型身份；模板分支存放待后续推断的编号。保留 Python 标签是保留原版描述结构，不会引入 Python 执行器依赖。

`TypeId` 适合判断当前程序中两个类型是否相同，需要 `T: 'static`。这里的 `'static` 约束不表示值必须一直存活到程序结束；例如一个局部创建并及时释放的 String 也满足这个类型约束。不要把 TypeId 的哈希值作为跨版本存档或网络协议编号，其哈希与排序不保证跨 Rust 版本稳定。[标准库 TypeId 文档](https://doc.rust-lang.org/std/any/struct.TypeId.html)

`MsgType` 再把可读名字和类型身份放在一起：`name: String` 用于描述，`id: MsgTypeId` 用于判断。原版辅助函数 `any()` 生成 Any，`template(3)` 生成名字 T3 与编号 3，`of::<u32>()` 生成 Rust 类型描述。Python 名字按原版规则先 trim 再哈希；它与消息地址 `str2addr` 不裁剪空白的规则不同。

### 第二步：让队列端点保留类型描述

在 Sender、Receiver 中各增加 `channel_type: MsgTypeId` 字段。在 `channel_with_type(capacity, channel_type)` 创建两端时复制同一个标识；这个枚举实现 Copy，复制它不会复制消息队列。已有 `channel(capacity)` 调用该函数并传入 Any，因此原来的无类型调用保持原行为。

默认未接线端点也采用 Any。随后给普通端点实现 TypeInfo：`port_tid` 返回 Any，`chan_tid` 返回保存的字段。给类型包装实现同一 trait：`port_tid` 返回 `MsgTypeId::of::<T>()`，`chan_tid` 委托内部端点。

这正是 `tests/type_info.rs` 里的回归测试 `wrapper_type_does_not_relabel_channel`（对容量 0、1 各跑一遍，发送、接收两端都验）：

```rust
{{#include ../../../code/flow-rs/tests/type_info.rs:wrapper_test}}
```

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

上一节先完成类型描述，现在接入实际转换。创建 `channel/conversion.rs`，定义函数指针：

```rust
{{#include ../../../code/flow-rs/src/channel/conversion.rs:cvt_f_type}}
```

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
