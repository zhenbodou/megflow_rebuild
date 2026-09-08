# Ch1.4a 异步三步实验：先看见等待，再封装通道

前置知识只有变量、函数、循环、所有权和 Option。先用 Tokio 原生队列观察机制，
再回到 Ch1.4 的信封通道。这样遇到问题时，你能判断是异步基础还是引擎封装出了错。

## 准备：把命令和代码位置说清楚

完整程序是 `code/flow-rs/examples/async_steps.rs`。从仓库根目录执行：

```bash
cargo run --manifest-path code/Cargo.toml -p flow-rs --example async_steps --locked
```

如果你要在自己的空项目中手写，创建下面的 Cargo.toml，再把三步代码依次写入 src/main.rs：

```toml
[package]
name = "async-steps"
version = "0.1.0"
edition = "2021"

[dependencies]
tokio = { version = "1", features = ["rt", "macros", "sync"] }
futures-util = "0.3"
```

Tokio 的 rt 提供运行时，macros 提供 main 属性宏，sync 提供队列。futures-util 在这里
只用于把一个 Future 显式检查一次，观察 Pending，不负责业务计算。根目录命令使用
仓库 Cargo.lock；自己的空工程第一次构建会生成自己的锁文件。

main 的外壳写成 `#[tokio::main(flavor = "current_thread")] async fn main() { ... }`。
current_thread 让这个示例使用单线程运行时，更容易理解：即使只有一个线程，也可以
在等待时交替推进生产和消费任务。并发不一定需要多个线程并行执行。

## 第一步：创建 Future 不等于执行业务

```rust,ignore
{{#include ../../../code/flow-rs/examples/async_steps.rs:lazy_future}}
```

Cell 在这个单线程小实验里记录调用次数，允许通过共享引用修改数字。不要将它当成
可跨线程共享的计数器；多线程共享计数通常需要原子类型或锁。

async 块创建 work 时，块内的加一尚未执行，所以第一次断言是 0。await 驱动这个
Future 后得到 7，调用次数变成 1。这里没有等待外部事件，Future 可以直接就绪；
所以不能理解成“每个 await 都一定切换任务”。

预测题：如果去掉 work.await，只保留 let work，会得到 7 吗？不会，业务未被驱动。
如果把创建 Future 当成一次函数业务调用，你之后就容易写出“节点创建了但没有启动”的错误。

## 第二步：用 Pending 看见背压

```rust,ignore
{{#include ../../../code/flow-rs/examples/async_steps.rs:backpressure}}
```

容量 1 的队列先装入 10。第二次发送 20 时，我们还没有直接 await 到完成：
先用 pin! 固定 Future，再用 poll! 检查一次，结果应是 Pending，因为队列满了。
接收者取走 10 之后，第二次发送就可以完成，再次接收得到 20。

这里的 pin! 是宏；它为 Future 提供满足 poll 要求的固定位置引用。对可能不实现 Unpin 的 Future，固定后不能再随意移动其本体，Pin 的详细约束要在学习自定义 Future 时继续展开。
poll! 是本实验观察状态的工具，业务节点通常直接 await，不需要手工轮询。

为什么不直接先 await 两次 send，再执行 recv？第二次 send 会等容量，当前任务却要
等它结束才进入 recv，双方永远等不到进展。这是控制流程问题，不是 Rust 类型错误。

预测题：把容量改为 2，Pending 断言还成立吗？不成立，第二条可以入队。删掉这条
断言只适用于你有意改变了实验条件，不能用来掩盖生产代码的背压错误。

## 第三步：并发收发和关闭

```rust,ignore
{{#include ../../../code/flow-rs/examples/async_steps.rs:shutdown}}
```

spawn 将生产 Future 登记为一个任务，运行时可以在主任务等待 recv 时推进它。
async move 把 producer_sender 移入该任务；主任务还有另一份 sender，所以必须主动 drop。
当生产任务结束、最后一个发送端释放并且队列排空时，原生 Tokio recv 返回 None。
引擎封装再把这一状态映射为 ChannelClosed。

producer.await 等待的是任务结果 JoinHandle，不是重新执行生产循环。这里循环返回
单位值，因此只有任务层的 Result；如果任务体自身返回 Result，就会有两层结果，
分别表示任务是否崩溃、业务是否成功，调度章节会继续讲。

把 drop(sender) 注释掉会怎样？收齐三条以后仍有发送端存活，recv 会继续等待，不会
凭“目前没消息”判定结束。在练习副本中验证时可加 timeout，或在终端用 Ctrl+C 结束。

## 输出与自测

程序应依次输出步骤 1、步骤 2、步骤 3 的说明，最后正常退出。完整实现：

```rust,ignore
{{#include ../../../code/flow-rs/examples/async_steps.rs}}
```

合上代码，自己完成以下变化：

1. 输入改为 1 到 5，消费方乘以 2，断言输出 2、4、6、8、10。参考思路：业务处理放在
   recv 之后，生产任务只负责发送，收尾逻辑保持不变。
2. 增加第二个生产任务，各发送不同的一组序号，克隆发送端后释放主任务那份。最终
   排序核对完整集合；不要断言两个生产任务之间必定按某一种顺序交错。
3. 把 u32 替换成上一章的 Envelope，在接收后验证 partial_id。参考思路：只改变队列
   元素类型，背压与关闭仍由端点和队列状态决定，不能由载荷是否为空决定。

能解释这三步后，再读引擎的 send_any/recv_any，就能把“类型擦除”“等待”“关闭”
分开理解，而不是把所有代码都归结为“Tokio 自动处理”。

维护者运行 `python3 scripts/check_async_course.py` 可在临时空工程验证本实验；该检查
使用 offline，需要先完成主工程依赖下载。它不依赖 flow-rs、flow-message 或任何节点宏。
