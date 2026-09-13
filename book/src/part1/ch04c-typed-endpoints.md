# Ch1.4c 第九、十步：类型包装与默认端点

继续上一章第八步的 flow-rs 工程。本章不增加第三方依赖，也不创建 config 或转换表。先把“每次调用都指定类型”变成“端口本身记住类型”，再处理尚未接线的端口。

<!-- toc -->

## 第九步：把类型参数搬到端口上

现在你会写 `receiver.recv::<u32>()`。如果一个节点永远只收 u32，每次重复指定容易写错；我们希望先声明 `ReceiverT<u32>`，以后只写 recv()。

PhantomData 是标准库提供的零大小标记。包装中并不保存一个 T 值，但这个类型参数仍影响方法签名和 Send/Sync 等自动 trait 推导。先记住它负责“这个类型与 T 有关系”，不负责实际校验队列内容。

From 则定义怎样把已有端点移动进包装；into 由目标变量类型选择 From 实现。这个过程不创建队列，也不接收消息。Deref 和 DerefMut 允许借用内部端点，复用它已经写好的方法；它们不会复制端点。

### 本步修改清单

Cargo.toml、src/lib.rs、src/error.rs 和 message 保持第八步不变。**替换 src/channel.rs，新增 src/channel/typed.rs**。Rust 允许 channel.rs 与 channel/ 目录同时存在；channel.rs 中的 `mod typed;` 正是读取子目录下的 typed.rs。

完整 **src/channel.rs**：

```rust
{{#include ../../labs/channel-steps/09/channel.rs}}
```

这一文件的原有收发代码保持第八步逻辑，只增加模块声明、公开重导出及一项测试。不要把最终仓库的 channel.rs 放进来，否则会出现尚未实现的类型信息与转换依赖。

完整 **src/channel/typed.rs**：

```rust
{{#include ../../labs/channel-steps/09/typed.rs}}
```

### 沿调用顺序读实现

先看 `SenderT<T>(Sender, PhantomData<T>)`：这是元组结构体，用 `.0` 访问底层端点，`.1` 是标记。From 的参数按值接收 Sender，所以包装后旧绑定不能继续使用；若调用方确实需要两份句柄，应先显式 clone。

再看 send：类型参数 T 现在来自 impl，而不是每次调用重新选择。它仍委托上一课实现的 Sender::send。接收端也委托 recv_any，但按原版类型化入口的约定，认领错误时 panic；它与普通 `recv::<T>` 返回 TypeMismatch 的接口不同，不要把两者混为一个错误模型。

为什么仍写 Send、Clone、'static？封箱需要这些能力；引入包装不会消除底层约束。Deref 的 Target 指定被借用的类型，返回 `&self.0`；DerefMut 返回独占借用 `&mut self.0`。

最后读限时和批量方法：Duration 来自标准库；BatchRecvError 是上一章已写的枚举；这些方法继续调用底层已实现的方法，不需要重新写计时循环。本步 From 没有 with_type 调用，因为类型身份和转换表还没实现，下一章才加入。

### 运行与排错

在当前 flow-rs 目录运行 `cargo test --offline`。预期 **9 项测试**通过：原来八项不丢失，新测试验证类型包装后得到 7，partial_id 仍为 Some(42)。

把新测试发送的 u32 改成 String，编译应在 send 调用处指出参数类型不匹配。恢复后再尝试通过 Deref 暴露的 send_any 发送不同类型：这条底层接口仍可绕过包装的泛型限制，接收时才发现错误。因此类型包装方便业务调用，并不是一个禁止所有异类型消息的密封容器。

独立练习：将一对端口包装为 String 类型，发送一个字符串，并断言序号保留。不要新建第二条队列来实现 From，否则原来的两端会失去联系。

## 第十步：没有接线也能构造端点

后面创建节点结构体时，图还没有给它连接真实队列。因此需要 Default 构造“尚未接线”的端点。默认端点与真实创建但随后关闭的队列不是同一状态。

原版协议规定：未接线发送端丢弃消息并返回成功；未接线接收端立即返回关闭错误。不要凭直觉把发送改成报错，默认输出本来就允许没有下游。

### 本步修改清单

**替换 src/channel.rs 和 src/channel/typed.rs**，其他所有文件不变。没有新依赖。

完整 **src/channel.rs**：

```rust
{{#include ../../labs/channel-steps/10/channel.rs}}
```

完整 **src/channel/typed.rs**：

```rust
{{#include ../../labs/channel-steps/10/typed.rs}}
```

先读枚举新增的 Unconnected 分支与 default 属性。Sender 的默认字段得到这个分支；recv 枚举也有一个不包含真实队列的默认分支。Receiver 增加 connected 标志，构造真实队列时为 true，Default 时为 false，因此 is_none 不需要异步拿锁。字段是私有的，只由默认构造和 channel 构造器设置，不让调用者随意改出矛盾状态。

默认 Receiver 的 Arc 和 Mutex 只是保护一个 Unconnected 枚举，并没有底层 mpsc 队列。recv_any 对该分支得到 None，再复用之前的 ok_or 返回错误。默认 send_any 的分支返回 Ok，按值传入的消息在函数结束时释放。

最后给类型包装实现 Default：把底层默认端点与 PhantomData 组合起来即可。不需要 T: Default，因为没有真的构造一个 T。

### 运行与独立练习

运行 `cargo test --offline`，预期 **10 项测试**通过。新测试同时检查默认两端的 is_none、默认发送成功、默认接收报错、真实队列的 is_none 为 false，以及类型化默认发送端。

练习：使用一个没有实现 Default 的业务类型，构造 `SenderT<该类型>`::default()，解释为什么应当能编译。然后比较 drop 一个真实发送克隆和使用默认发送端：前者涉及共享队列的存活，后者根本没有接线。

维护脚本 `python3 scripts/check_basic_channel_course.py` 从 Ch1.4 第一步一直构建到本步，连续检查十个版本。下一章才加入 MsgTypeId、TypeInfo 和转换函数注册；当前阶段任何出现这些名字却没定义它们的代码，都不应复制到这里。
