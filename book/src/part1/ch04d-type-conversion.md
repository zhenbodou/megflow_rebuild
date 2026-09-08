# Ch1.4d 类型信息与转换表

先完成类型化端点。本课先表示两端的类型身份，再实现转换函数注册和收发适配；函数指针与锁的用途随实现步骤讲解。自动注册属性宏在宏专题第 10 课开发，此时可以先手动调用注册函数。

完成标准：能画出端口类型 → 队列类型 → 端口类型的转换方向，并用 repack 保留消息上下文。

<!-- toc -->

## 类型信息：端口类型与通道类型为什么分开

到这里，我们已有 `SenderT<T>` 与 `ReceiverT<T>`。接下来在 `config/interlayer.rs` 定义消息类型描述，并让端点实现 `channel::TypeInfo`。这一层为后续类型推断与转换表准备数据，不会自动转换载荷。

先用一个问题理解需求：节点声明接收 `u32`，但接线系统为队列选择了 `String`。框架至少要记住两件事，才能判断是否存在 String → u32 的转换函数。只记录一个类型，就无法描述这个转换请求。

```rust,ignore
pub trait TypeInfo {
    fn port_tid(&self) -> MsgTypeId;
    fn chan_tid(&self) -> MsgTypeId;
}
```

`port_tid` 描述端口声明：普通端点是 `Any`，`ReceiverT<u32>` 是 Rust 的 u32 类型。`chan_tid` 描述底层通道：类型包装和克隆不会改变它。`Any` 在这里是 MegFlow 枚举的一种值，不是 `std::any::Any` trait，也不代表队列会检查每条消息并改变自己的声明。

### 第一步：定义可以比较的类型身份

`MsgTypeId` 有四个分支：`Any`、`Rust(TypeId)`、`Python(u64)`、`Template(usize)`。Rust 部分用 `TypeId::of::<T>()` 获得类型身份；模板分支存放待后续推断的编号。保留 Python 标签是保留原版描述结构，不会引入 Python 执行器依赖。

`TypeId` 适合判断当前程序中两个类型是否相同，需要 `T: 'static`。这里的 `'static` 约束不表示值必须一直存活到程序结束；例如一个局部创建并及时释放的 String 也满足这个类型约束。不要把 TypeId 的哈希值作为跨版本存档或网络协议编号，其哈希与排序不保证跨 Rust 版本稳定。[标准库 TypeId 文档](https://doc.rust-lang.org/std/any/struct.TypeId.html)

`MsgType` 再把可读名字和类型身份放在一起：`name: String` 用于描述，`id: MsgTypeId` 用于判断。原版辅助函数 `any()` 生成 Any，`template(3)` 生成名字 T3 与编号 3，`of::<u32>()` 生成 Rust 类型描述。Python 名字按原版规则先 trim 再哈希；它与消息地址 `str2addr` 不裁剪空白的规则不同。

### 第二步：让队列端点保留类型描述

在 Sender、Receiver 中各增加 `channel_type: MsgTypeId` 字段。在 `channel_with_type(capacity, channel_type)` 创建两端时复制同一个标识；这个枚举实现 Copy，复制它不会复制消息队列。已有 `channel(capacity)` 调用该函数并传入 Any，因此原来的无类型调用保持原行为。

默认未接线端点也采用 Any。随后给普通端点实现 TypeInfo：`port_tid` 返回 Any，`chan_tid` 返回保存的字段。给类型包装实现同一 trait：`port_tid` 返回 `MsgTypeId::of::<T>()`，`chan_tid` 委托内部端点。

可以在测试里运行如下片段：

```rust,ignore
use flow_rs::channel::{channel_with_type, ReceiverT, TypeInfo};
use flow_rs::config::interlayer::MsgTypeId;

let (_sender, receiver) = channel_with_type(1, MsgTypeId::of::<String>());
let receiver: ReceiverT<u32> = receiver.into();
assert_eq!(receiver.port_tid(), MsgTypeId::of::<u32>());
assert_eq!(receiver.chan_tid(), MsgTypeId::of::<String>());
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

```rust,ignore
pub type CvtF = fn(SealedEnvelope) -> SealedEnvelope;
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


