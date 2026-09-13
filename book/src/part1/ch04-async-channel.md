# Ch1.4 五步写出第一条消息通道

这一章沿着上一章的工程继续写。目标很具体：上游放入一个带序号的信封，下游拿到同一条消息。你不需要提前写 Node、config、TypeInfo、conversion 或 Graph，也不要从最终仓库复制 channel.rs。

我们先做一个小而能运行的版本，再在后续章节逐项增加协议。**每一步跑通再往下写**；遇到错误先对照当前步骤的完整文件，不要直接跳到最后一份代码。

<!-- toc -->

## 开始前：摆好已经写出的文件

Ch1.3 的消息库就是本章唯一的本地依赖。新建一个 `flow-rs` 目录，将上一章完成的消息工程整个放入它的 `message/` 子目录；保留消息库的全部文件。在 flow-rs 下创建 src 目录。现在应是：

```text
flow-rs/
  src/                     # 现在是空目录
  message/                 # 上一章完成的工程
    Cargo.toml
    src/lib.rs
    src/envelope.rs
    tests/envelope_contract.rs
    examples/first_principles.rs
```

接下来所有命令都在 **flow-rs 目录**执行。message 只是路径名称；它的 Cargo.toml 中 package 名仍为 flow-message。先运行 `cargo test --manifest-path message/Cargo.toml --offline`，确认上一章的 8 个单元测试和 7 个契约测试通过。不要在未完成上一章时使用本章的代码。

## 第一步：只让数字 7 穿过队列

### 为什么先引入 Tokio

队列是一个暂存消息的地方。这里约定最多存一条：发送方遇到满队列，需要等接收方腾出位置；接收方遇到空队列，需要等发送方放入消息。

标准库的同步通道会阻塞当前线程。我们将来要让很多节点共享少量线程，所以先用 Tokio 的异步队列：等待消息时，任务可以把线程让给其他就绪任务。Tokio 是第三方 crate，Rust 标准库没有自动帮你安装它。

创建 **Cargo.toml**，完整内容：

```toml
{{#include ../../labs/channel-steps/01/Cargo.toml}}
```

逐行读依赖配置：

- package 定义当前库的名字、版本和 Rust edition。名字 flow-rs 在 Rust 路径中写作 flow_rs。
- 空的 workspace 表使这个练习成为自己的工作区；exclude 让已有消息工程保留它自己的工作区声明。
- dependencies 告诉 Cargo 下载哪些库。`version = "1"` 是兼容版本范围，不是精确版本；Cargo.lock 记录实际选择，生成后保留它。
- Tokio 的 sync feature 提供队列；rt 提供运行 Future 的执行器；macros 提供下面的测试属性宏。现在不需要网络、定时器和多线程运行时。

创建 **src/lib.rs**，完整内容：

```rust
{{#include ../../labs/channel-steps/01/lib.rs}}
```

先认识文件外壳。lib.rs 是库入口；`#[cfg(test)]` 表示 tests 模块只在测试构建时加入。`use tokio::sync::mpsc` 引入模块路径，mpsc 表示多发送者、单接收者。`#[tokio::test]` 是属性过程宏，它替这个测试创建当前线程运行时并驱动 async 函数；普通 `#[test]` 不会自动运行 async 测试。

再读函数里的五行：`mpsc::channel::<u32>(1)` 创建容量 1、元素为 u32 的队列；元组拆分得到 sender 和 receiver。receiver 要声明 mut，因为出队需要可变借用。send 成功返回 Ok，测试用 unwrap 检查成功；recv 返回 Option，有消息是 Some(7)。最后主动 drop 发送端，队列已空且不会再有发送者，recv 返回 None。

`async fn` 的调用产生 Future；Future 表示一个可继续推进的计算。运行时通过 poll 推进它，完成时得到 Ready，尚未完成时得到 Pending，并依靠唤醒通知再次推进。`.await` 等待 Future 完成；如果它立刻就绪，就直接继续，并不保证每次都让出线程。本例先发一条、再收一条，不会填满队列后卡住。

运行：

```bash
cargo test
```

首次执行可能下载依赖。预期 `a_number_crosses_the_queue ... ok`、`1 passed; 0 failed`；后续步骤可用 `cargo test --offline`。如果提示找不到 tokio，先检查 dependencies；如果找不到 tokio::test，检查 macros 和 rt feature。不要添加不明白的 feature 来试错。

小练习：把 7 改成 9，同时修改对应期待值。不要先连续发送两条再接收——容量只有 1，同一任务会在第二次发送处等待，无法走到接收行。

## 第二步：用上一章的信封替换数字

数字队列只能装 u32；框架要让同一类队列承载不同消息，所以使用上一章已经实现的 SealedEnvelope。**替换 Cargo.toml 和 src/lib.rs**，其他文件保持不变。

完整 **Cargo.toml**：

```toml
{{#include ../../labs/channel-steps/02/Cargo.toml}}
```

新增的 `flow-message = { path = "message" }` 指向相对于当前 Cargo.toml 的目录，不去网上下载我们的消息库。Cargo 读取该目录下的 Cargo.toml，并编译它的 src/lib.rs；`flow_message` 是我们在 Rust 中使用的 crate 路径。它与 `crate::...` 不同，后者始终指当前 flow-rs 库。

完整 **src/lib.rs**：

```rust
{{#include ../../labs/channel-steps/02/lib.rs}}
```

按消息走向读：new 创建 u32 信封 → info_mut 写入序号 → seal 装箱擦除类型 → Tokio 队列移动盒子 → downcast_mut 恢复具体信封的可变借用 → unpack 取出载荷。队列不解释 partial_id，也不重新创建 Envelope，所以元信息不会在运输途中恢复默认值。

这里 send 使用 `assert!(...is_ok())`，因为 Tokio 的发送错误会携带原消息；直接 unwrap 还会要求错误可以 Debug 格式化，而我们的擦除信封没有实现 Debug。这个限制来自 unwrap 的签名，不是消息不能发送。

运行 `cargo test --offline`，预期 `an_envelope_crosses_the_queue ... ok`，共 1 项测试。错误的消息库路径会在编译前失败；类型认领错误会在测试的 unwrap 处 panic。故意把认领的 u32 改成 i32，观察失败，再恢复。Rust 的这两个整数类型不会因数值都为 7 就变成同一类型。

## 第三步：先手写错误，再学习错误派生宏

None 对 Tokio 表示没有下一条消息。我们的框架需要一个可供调用方 match 的错误，因此写自己的 Error。现在先不引入 thiserror，让它以后生成的代码有可理解的来源。

**替换 Cargo.toml、src/lib.rs，新增 src/error.rs**。完整 **Cargo.toml**：

```toml
{{#include ../../labs/channel-steps/03/Cargo.toml}}
```

完整 **src/error.rs**：

```rust
{{#include ../../labs/channel-steps/03/error.rs}}
```

enum 的两个变体是关闭和类型不匹配。`derive(Debug)` 提供开发时的格式化；Display 定义给人看的文本，fmt 接收格式化器，把变体对应的字符串写进去。`Formatter<'_>` 中的占位生命周期交给编译器推断，本函数不保存这个借用。标准 Error trait 以 Debug 和 Display 为前提，这里没有底层错误链，所以实现体为空。

最后一行 Result 别名固定错误为本章 Error，成功类型 T 仍可以变化。不要把它与 fmt::Result 混淆，后者用于报告格式化失败。

完整 **src/lib.rs**：

```rust
{{#include ../../labs/channel-steps/03/lib.rs}}
```

`pub mod error;` 把刚创建的文件加入当前库；仅创建文件不会自动编译它。`crate::error` 因而已经存在，无需借用后面的图运行时。辅助 receive 函数用 match 将 Some 转为 Ok、None 转为 Err。这段 match 是后面 `ok_or` 简写的来源。

运行 `cargo test --offline`，预期 `distinguish_message_from_closed_queue ... ok`，共 1 项测试，同时验证错误显示文本。练习：解释为什么 Ok 分支携带消息，而 Err 分支没有消息；不要把两种分支都改为成功来让测试通过。

## 第四步：把重复的队列操作封装成自己的端点

每次使用队列都手写类型擦除、关闭处理，很容易不一致。因此用结构体包住 Tokio 端点。**替换 src/lib.rs，新增 src/channel.rs**；Cargo.toml 和 error.rs 沿用第三步。

完整 **src/lib.rs**：

```rust
{{#include ../../labs/channel-steps/04/lib.rs}}
```

完整 **src/channel.rs**：

```rust
{{#include ../../labs/channel-steps/04/channel.rs}}
```

读到一个新名字就找到它的来源：`crate::error` 是上一步写的模块，`flow_message` 是上一章的库，mpsc 是第一步引入的 Tokio 模块。此时没有 config、conversion 或任何尚未写出的模块。

Sender 和 Receiver 的 inner 字段保存真正的端点。构造函数把同一队列的两端分别包起来，并不创建两条队列。Sender 派生 Clone，只复制发送句柄；Receiver 没有 Clone，本阶段仍只有一个消费者。

send_any 按值接收消息，把它移入队列；底层发送失败时返回的原消息随错误分支被丢弃，我们只留下 ChannelClosed。recv_any 用 match 把底层 Option 转成自己的 Result。发送和接收的方法参数分别是 &self 和 &mut self，来源于 Tokio 两端所需的借用方式。

运行 `cargo test --offline`，预期 `wrapper_moves_an_envelope ... ok`，共 1 项测试。到这里，你已经亲手写出了第一版 channel 模块。容量 0 暂时明确拒绝；下一课再按原版语义把它扩展为无界队列，而不是把这个 assert 当成最终框架契约。

## 第五步：让业务代码直接发送和接收 `Envelope<T>`

目前调用者还要手动 seal 和 downcast；最后添加泛型便捷方法，并用 thiserror 替换已经理解的错误格式化样板。**替换下面四个文件**，message 工程仍保持不变。

完整 **Cargo.toml**：

```toml
{{#include ../../labs/channel-basic/Cargo.toml}}
```

thiserror 是编译期的派生宏库，不负责捕获错误或调度任务。它读取枚举及辅助属性，生成 Display 和标准 Error 实现；功能就是第三步手写的那两段。`version = "2"` 表示兼容版本范围，具体版本仍由锁文件记录。第一次增加它后运行 cargo test 允许下载，之后再用 offline。

完整 **src/error.rs**：

```rust
{{#include ../../labs/channel-basic/src/error.rs}}
```

逐个对照第三步：Debug 仍由标准派生生成；thiserror::Error 生成另两个 trait；`#[error("...")]` 定义对应的显示文本。这是派生过程宏与辅助属性，不是运行时注解扫描。宏专题会进一步讲它们背后的 syn、quote 和 proc-macro2，现在不需要自己实现这些宏。

完整 **src/lib.rs**：

```rust
{{#include ../../labs/channel-basic/src/lib.rs}}
```

完整 **src/channel.rs**：

```rust
{{#include ../../labs/channel-basic/src/channel.rs}}
```

先与第四步比较旧方法：map_err 只转换 Err 分支，ok_or 把 None 换成指定错误，与前面的 match 表达同一件事。然后只读两个新方法：

- `send<T>` 消耗 `Envelope<T>`，seal 后调用 send_any。Clone、Send、'static 是上一章 seal 的要求，本次 send 本身不会主动克隆消息。
- `recv<T>` 先等待 recv_any。`?` 在错误时提前返回；成功才继续 downcast_mut。认领成功时 map 调用 Envelope::take，把载荷移进一个可以返回的信封；失败时 ok_or 返回 TypeMismatch。不能返回对局部 message 的借用，因为局部盒子随后会销毁。

类型不匹配发生在出队之后，因此错误消息被消费并丢弃，不会回到队列。测试故意放入两条不同类型消息，验证报错后仍能取到第二条，避免只检查错误变体而漏掉消息归属。

运行 `cargo test`，预期五项测试通过：

| 测试 | 结果 |
|---|---|
| typed_roundtrip_preserves_metadata | 得到 7，地址仍为 Some(42) |
| untyped_roundtrip | 封箱后仍能认领出 9 |
| wrong_type_consumes_only_that_message | 第一条报错，第二条得到 2 |
| last_sender_drop_drains_before_close | 先取出已有的 3，再收到关闭 |
| receiver_drop_rejects_send | 接收者已释放时发送报关闭 |

输出顺序、编译时间可能不同；检查 `5 passed; 0 failed` 和退出码 0。排错练习：删除 `send<T>` 的 Clone 约束，确认 seal 处编译失败，再恢复。独立练习：不看第五步，给第四步补出 `send<T>` 和 `recv<T>`，再用这五项测试验收。

## 本章结束时，你写出了什么

你有了名为 flow-rs 的库，包含 error 和 channel 两个模块，并通过路径依赖使用上一章的 flow-message。不要用最终仓库源码覆盖它。后续章节必须在这份工程上扩展；无界队列、共享接收、类型包装、转换、批量、关闭等能力还没有凭空出现。

维护者运行 `python3 scripts/check_basic_channel_course.py`。它从空目录开始，按本章顺序覆盖指定文件，连续验证五个步骤；第四步保留第三步的 error，直到第五步才加入 thiserror。这个检查不是只编译最终文件。

继续 [异步三步实验](ch04a-async-workshop.md)，亲手观察任务和背压，再进入 [通道协议](ch04b-channel-protocols.md)。基础版是工程的第一个可运行版本，完整 MegFlow 协议仍须在后续开发中逐项完成。
