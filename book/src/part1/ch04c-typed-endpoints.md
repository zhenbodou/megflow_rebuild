# Ch1.4c 类型化与默认端点

前置知识是泛型、PhantomData、Deref，以及上一课的共享队列。本课为已有端点添加类型包装，再明确未接线端点的行为。

完成标准：能独立实现 SenderT/ReceiverT，并区分默认端点与实际创建的队列。

<!-- toc -->

## 类型化端点：把类型写在端口上

前面每次接收都写 `receiver.recv::<u32>()`。原版还提供 `ReceiverT<T>`、`SenderT<T>`，
让端口本身固定类型。例如：

```rust,ignore
let (sender, receiver) = channel(0);
let sender: SenderT<u32> = sender.into();
let receiver: ReceiverT<u32> = receiver.into();
sender.send(Envelope::new(7)).await.unwrap();
let mut envelope = receiver.recv().await.unwrap();
assert_eq!(envelope.unpack(), 7);
```

变量上的 u32 决定 send/recv 的载荷类型。这是类型推导，不是宏替你补字符串。
当前完整包装实现如下：

```rust,ignore
{{#include ../../../code/flow-rs/src/channel/typed.rs}}
```

按三层理解：

1. 元组结构体保存原 Sender/Receiver 与 `PhantomData<T>`。PhantomData 不存储真实载荷，
   但告诉编译器此包装与 T 有类型关系，参与 trait 和自动 Send/Sync 的检查。
2. From 接收原端点的所有权，into 根据目标变量类型选择 From 实现。它不新建通道，
   不复制队列，也不读取队列中的消息。
3. 固有 send/recv 方法固定 T；Deref 则让包装继续使用底层 send_any/recv_any 等接口。
   因此它不是禁止所有异类型输入的封闭容器。未类型化发送仍可能把其他类型送入队列。

类型化 recv 沿用原版下转型失败 panic 的行为。它不等于当前未类型化 `recv::<T>` 的
TypeMismatch 返回；这处错误模型差异仍应在完整 API 对照中跟踪。批量和限时接口
复用前面实现，不重复编写权重循环或计时逻辑。

```bash
cargo test --manifest-path code/Cargo.toml -p flow-rs --test typed_endpoints --locked
```

三个测试验证类型推导、限时与部分批次、克隆后的共享队列、通过 Deref 调用未类型化
操作，以及错误载荷的失败行为。

原版 From 还查询 CVT_VTABLE，以端口类型和通道类型查找转换函数。当前尚未实现
该转换表，TypeInfo 在 [下一课](ch04d-type-conversion.md) 补齐，完整类型推断仍缺失；默认未接线端点见下一节。因此这些包装只是
类型化端口宏的基础，不能把 From 可用说成跨类型转换已完成。

练习：为什么 `SenderT<u32>` 能调用 send_any？方法查找可通过 Deref 找到 Sender 的
方法；要设计严格禁止绕过类型检查的独立 API，应慎重决定是否提供 Deref，但这里
迁移的是原版公开接口，不能擅自删去这一能力。

## 默认未接线端点：Default 不等于新建通道

原版 Sender/Receiver 支持 Default，节点可以先默认构造字段，再由图装配接线。
默认端点没有实际队列。当前 Sender 使用 Unconnected 枚举分支，Receiver 使用
Option 的 None 表示这一状态；调用 is_none() 可以与已接线后关闭区分。

原版 sender.rs 的 send_any 在没有底层实现时直接返回 Ok(())。因此未接线输出的
消息被丢弃，但发送不报错；接收端没有队列时则返回错误。这个行为不能根据直觉改成
“只要 is_closed 为真，send 一定失败”。

| 端点状态 | is_none | 发送结果 |
| --- | --- | --- |
| 默认未接线 Sender | true | 丢弃消息，Ok(()) |
| 已接线，但接收者全释放 | false | Err(ChannelClosed) |
| 已接线且接收者仍在 | false | 入队，可能等待有界容量 |

SenderT/ReceiverT 的 Default 手工委托给底层端点，不构造 T，所以不需要 T: Default。
这是泛型约束设计的一个实际例子：包装类型能默认构造，不代表它标记的载荷类型也必须
默认构造。测试用未实现 Default 的空类型确认没有误加约束。

```bash
cargo test --manifest-path code/Cargo.toml -p flow-rs --test typed_endpoints --locked
```

注意：具备默认端点并不自动让 Node/Actor 宏支持所有原版节点写法；字段识别、接线、
TypeInfo 和生命周期仍需一起迁移。当前图装配仍校验必需端口，不会因为 Default 存在
就自动忽略缺少接线的配置错误。

