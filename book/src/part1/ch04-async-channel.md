# Ch1.4 async/await、Future、tokio 入门 → channel 封装

上一章我们造好了「消息盒子」`Envelope<M>` 和它的类型擦除。可盒子只是静物——它得**流动**起来：从一个节点异步地发出、被另一个节点异步地收到。这一章给引擎装上**异步的心跳**：先补 `async`/`await`/`Future`/`tokio` 这几样引擎绕不开的异步地基，再把 tokio 的 channel **封装**成引擎自己的收发端，让上一章的 `SealedEnvelope` 真正在任务之间「跑」起来。

这也是 **Part 1 的收官章**。写完，我们就有了「消息层 + 异步通道」这套**地基**，Part 2 才能在上面盖「节点」。

<!-- toc -->

## 1. `async` / `await` / `Future`：不阻塞线程的等待

dataflow 引擎里，一个节点大部分时间在**等**——等上游发来消息、等下游腾出空位。如果用「一个节点一个线程 + 阻塞等待」，成百上千个节点就要成百上千个线程，绝大多数时间在睡觉，白白占着栈和调度开销。**异步**就是为解决这个而生：让**少量线程**驱动**大量**「在等待中让出、就绪后继续」的任务。

Rust 的异步三件套：

- **`Future`**：一个「**将来**才会算出值」的惰性状态机。它有个核心方法 `poll`：运行时问它「好了吗？」——要么答 `Ready(值)`，要么答 `Pending(还没，回头再问)`。关键：**`Future` 是惰性的**，不 `poll` 它就什么都不做。
- **`async fn` / `async {}`**：写异步代码的语法糖。`async fn foo() -> T` 实际返回的是一个 `Future<Output = T>`；函数体被编译器**改写成一个状态机**。
- **`.await`**：在一个 `Future` 上「等它出结果」。语义是「**让出**：如果没就绪，就把控制权交还运行时去跑别的任务，等就绪了再从这里继续」——注意是让出**任务**，不是阻塞**线程**。

```rust,ignore
// async fn 返回一个 Future；调用它不会立刻执行，要 .await（或交给运行时）才跑
async fn read_two(rx: &mut Receiver) -> (i32, i32) {
    let a = rx.recv::<i32>().await;   // 没消息就让出，别的任务趁机跑
    let b = rx.recv::<i32>().await;   // 就绪后从这里继续
    (a.unwrap().unpack(), b.unwrap().unpack())
}
```

一句话：**`.await` 是「协作式让出点」**。这正好贴合引擎——节点在 `recv().await` 处等消息时，线程被腾出来跑别的就绪节点，等消息到了再回来。

```mermaid
flowchart LR
    subgraph RT["tokio 运行时（少量线程）"]
        direction LR
        P["生产者任务<br/>send(Envelope).await"] -->|"seal → SealedEnvelope"| CH(["channel<br/>(mpsc 队列)"])
        CH -->|"recv().await 就绪时唤醒"| C["消费者任务<br/>recv::&lt;T&gt;().await → downcast"]
    end
    P -.->|"队列满则让出"| RT
    C -.->|"队列空则让出"| RT
```

## 2. 为什么用 tokio，替换原版的有栈协程

原版 flow-rs 自带一套**有栈协程（stackful coroutine）**运行时（`rt/{spawn_pinned, rwlock}` 等）：每个协程有自己的栈，靠手写的调度与 `unsafe` 切换上下文。它能工作，但代价是**大量 `unsafe`**、自维护调度器、以及和生态（tokio 那套异步库）割裂。

我们重写的关键决策之一（spec 里定的）：**换成原生 `async` + tokio**。

| | 原版：有栈协程 | 重写：tokio async |
|---|---|---|
| 机制 | 每协程独立栈，手写上下文切换 | 无栈状态机，编译器改写 `async fn` |
| unsafe | 多（栈切换、pin） | 极少（几乎交给 tokio/std） |
| 生态 | 自成一套 | 直接用 tokio 的 channel/锁/定时器 |
| 学习价值 | 偏门 | **主流 Rust 异步**，可迁移 |

对本书三个目标都对味：**学 Rust**（学的是主流异步范式）、**更少 bug**（把调度/同步交给久经考验的 tokio，自己不写 `unsafe` 栈切换）、**功能一致**（tokio 的多线程运行时同样能跑成百上千个节点任务）。

> 有栈 vs 无栈不是「谁绝对好」——有栈协程在某些递归/深调用场景更省心。但对一个「节点在 `.await` 点让出」的 dataflow 引擎，无栈 async 恰好够用，且省下的 `unsafe` 和自维护调度器是实打实的复杂度削减。

## 3. tokio 入门：运行时、`spawn`、`mpsc`

tokio 是 Rust 事实标准的异步运行时。这一章只用到三样：

- **运行时（runtime）**：`Future` 不会自己跑，得交给运行时来 `poll` 驱动。测试里我们用 `#[tokio::test]` 自动起一个运行时；正式代码里 Ch3.3 会用 `#[tokio::main]` 或手动建 `Runtime`。
- **`tokio::spawn`**：把一个 `Future` 丢给运行时，作为一个**并发任务**跑（Ch3.3 每个节点 spawn 成一个任务）。被 spawn 的 future 必须 `Send + 'static`——正是 Ch1.1 讲的两张通行证。
- **`tokio::sync::mpsc`**：**多生产者、单消费者**的异步有界通道。`send(v).await` 在队列满时让出，`recv().await` 在队列空时让出。这就是我们要封装的底座。

引入依赖（workspace 集中锁版本，全走 crates.io 公共 crate）：

```toml
# code/Cargo.toml
[workspace.dependencies]
tokio = { version = "1", features = ["sync", "rt", "macros"] }
thiserror = "2"
```

`sync` 给我们 `mpsc`，`rt` + `macros` 给我们 `#[tokio::test]`/`#[tokio::main]`。**只开用得到的 feature**，不把整个 tokio 拉进来。

## 4. 红：先写通道的收发测试

老规矩，先红。我们要封装的通道，契约照着 Ch0.3 读到的原版形状：一条**承载 `SealedEnvelope`** 的通道，有**未类型化**的 `send_any`/`recv_any`，和**类型化**的 `send::<T>`/`recv::<T>`（内部 seal / downcast）。测试写进 `code/flow-rs/src/channel.rs`：

```rust,ignore
#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use flow_message::Envelope;

    #[tokio::test]
    async fn typed_send_recv_roundtrip() {
        let (tx, mut rx) = channel(4);
        tx.send(Envelope::new(42i32)).await.unwrap();
        let mut e = rx.recv::<i32>().await.unwrap();
        assert_eq!(e.unpack(), 42);      // 发 i32、收回 i32
    }

    #[tokio::test]
    async fn recv_wrong_type_is_type_mismatch() {
        let (tx, mut rx) = channel(4);
        tx.send(Envelope::new(1i32)).await.unwrap();
        // 发的是 i32，却想收 String → 类型不符
        assert!(matches!(rx.recv::<String>().await, Err(Error::TypeMismatch)));
    }

    #[tokio::test]
    async fn send_after_receiver_dropped_is_closed() {
        let (tx, rx) = channel(1);
        drop(rx);                        // 下游没了
        let err = tx.send(Envelope::new(1i32)).await.unwrap_err();
        assert!(matches!(err, Error::ChannelClosed));
    }

    #[tokio::test]
    async fn recv_after_senders_dropped_is_closed() {
        let (tx, mut rx) = channel(1);
        drop(tx);                        // 上游全没了
        assert!(matches!(rx.recv::<i32>().await, Err(Error::ChannelClosed)));
    }
}
```

`cargo test -p flow-rs` → **红**：`cannot find function channel`、`cannot find type Error`……契约立好了，下面实现。

> 一个小插曲：这几个 `recv` 测试最初写成 `.unwrap_err()`，编译**不过**——因为 `unwrap_err()` 要打印 `Ok` 里的值，于是要求 `Envelope<T>: Debug`，而我们的 `Envelope` 没实现 `Debug`（`extra_data` 里的 `dyn Any` 无从 `Debug`）。与其为测试便利去给 `Envelope` 强加一个 `Debug`，不如换成 `matches!(..., Err(...))`——只做模式匹配、不打印，表达的是同一个断言。**不为测试的方便而扩大生产类型的约束**，这也是「更少 bug」的一种自律。

## 5. 绿：错误类型 + 通道封装

### 5.1 `Error`：Ch1.1 的 thiserror 承诺兑现

Ch1.1 说过，引擎是库，错误要能被调用方 `match`，所以用 `thiserror` 定义**类型化枚举**。现在它第一次真正写进 `code/`（`code/flow-rs/src/error.rs`）：

```rust,ignore
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("channel closed")]                 // 通道关闭：对端全 drop
    ChannelClosed,
    #[error("message type mismatch on recv")]  // recv::<T> 的 T 与真实载荷不符
    TypeMismatch,
}

pub type Result<T> = std::result::Result<T, Error>;
```

注意这里**只放当前用得到的两个变体**。Ch1.1 预告过 `UnknownNode` / `Config`，但那要 Part 2/3 才真正构造——现在加进来只会得到「从未被构造」的死代码告警。**枚举按需生长**，每个变体在被真正 `Err(...)` 出来时才加入。

### 5.2 `channel`：tokio mpsc 的薄封装

先实现下面这个单消费者教学阶段，再按本章后面的容量、批量、限时和多消费者小节扩展。最终 `code/flow-rs/src/channel.rs` 已支持接收端 Clone 与 `&self` 接收；不要把这里的中间代码覆盖到最终版本：

```rust,ignore
use crate::error::{Error, Result};
use flow_message::{Envelope, SealedEnvelope};
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct Sender {
    inner: mpsc::Sender<SealedEnvelope>,   // 通道里跑的正是上一章的「封箱信封」
}
pub struct Receiver {
    inner: mpsc::Receiver<SealedEnvelope>,
}

pub fn channel(capacity: usize) -> (Sender, Receiver) {
    let (tx, rx) = mpsc::channel(capacity);
    (Sender { inner: tx }, Receiver { inner: rx })
}
```

发送端：`send_any` 直发封箱信封；`send::<T>` 先 `seal` 再走 `send_any`——**类型化只是薄薄一层，落到通道里全是 `SealedEnvelope`**：

```rust,ignore
impl Sender {
    pub async fn send_any(&self, msg: SealedEnvelope) -> Result<()> {
        self.inner.send(msg).await.map_err(|_| Error::ChannelClosed)
    }
    pub async fn send<T>(&self, msg: Envelope<T>) -> Result<()>
    where T: 'static + Send {
        self.send_any(msg.seal()).await         // ← Ch1.3 的 seal
    }
    pub fn is_closed(&self) -> bool { self.inner.is_closed() }
}
```

接收端对称：`recv_any` 收封箱信封；`recv::<T>` 收完把类型 `downcast` 回来——用的正是 Ch1.3 那套**零 unsafe** 的安全 downcast，猜错类型返回 `TypeMismatch` 而非崩溃：

```rust,ignore
impl Receiver {
    pub async fn recv_any(&mut self) -> Result<SealedEnvelope> {
        self.inner.recv().await.ok_or(Error::ChannelClosed)   // None = 关闭
    }
    pub async fn recv<T>(&mut self) -> Result<Envelope<T>>
    where T: 'static + Send {
        let mut sealed = self.recv_any().await?;
        match sealed.downcast_mut::<Envelope<T>>() {          // ← Ch1.3 的安全 downcast
            Some(e) => Ok(e.take()),                          // ← Ch1.3 的 take()：取出拥有所有权的信封
            None => Err(Error::TypeMismatch),
        }
    }
}
```

`recv::<T>` 这里把 Ch1.3 的三个零件串起来了：`downcast_mut` 认领类型、`take()` 取出一个拥有所有权的 `Envelope<T>`（元信息克隆保留、载荷移动出来）。再跑：

```bash
cargo test -p flow-rs
```

```text
running 5 tests
test channel::tests::recv_after_senders_dropped_is_closed ... ok
test channel::tests::send_after_receiver_dropped_is_closed ... ok
test channel::tests::recv_wrong_type_is_type_mismatch ... ok
test channel::tests::typed_send_recv_roundtrip ... ok
test channel::tests::untyped_send_any_recv_any ... ok

test result: ok. 5 passed; 0 failed
```

**绿**。上一章的静态信封，现在能在异步任务间流动了。

## 6. 几个设计决断

第一版用 Tokio MPSC 学习所有权和异步等待，但原版要求多消费者，不能以简化为理由删除。
本章后续通过共享接收端补上竞争接收。广播则不同：每个下游都得到一份消息，
需要 Bcast 节点显式克隆；竞争接收只把每条消息交给其中一个消费者。

原版 Demux 使用地址路由，不能将它等同于任意消费者抢单。后续迁移要结合 to_addr、
端口映射与原版节点实现验证，不应仅因名字像“分流”就自行设计另一种行为。

## 7. 对照原版后的剩余边界

当前代码支持普通消息的有界/无界队列、共享接收、限时和批量接收。
原版还包含 flush epoch、类型转换表、ChannelStorage、统计与运行时协作。
这些仍需要继续迁移。当前错误模型也不等同于原版，尤其不能把所有接收错误都解释为
永久关闭。真实代码见 `code/flow-rs/src/channel.rs` 与 `error.rs`，测试范围见各小节。

## 小结 · Part 1 收官

这一章给引擎装上了异步心跳，也补齐了 Part 1 的地基：

- **异步三件套**：`Future` 是惰性状态机（`poll` 问「好了吗」）；`async fn` 返回 Future；`.await` 是**协作式让出点**——让出任务而非阻塞线程。
- **tokio 替换有栈协程**：主流无栈 async，少 unsafe、接生态；用到运行时、`spawn`、`mpsc` 三样。
- **通道封装**：`tokio::sync::mpsc` 薄封装成承载 `SealedEnvelope` 的 `Sender`/`Receiver`；`send_any`/`recv_any` + 类型化 `send::<T>`/`recv::<T>`（seal / 安全 downcast 各薄薄一层）。
- **错误落地**：`thiserror` 的 `Error` 枚举第一次写进 `code/`，两变体、按需生长。
- **化简**：广播/demux 上移到节点层，通道保持极简；对比原版削掉大量 unsafe 与自制机制。

**Part 1 完成**。我们现在有了：能装任意载荷、能类型擦除的**消息层**（`flow-message`），和能在异步任务间搬运它的**通道**（`flow-rs::channel`）。

下一部分 **Part 2 · 节点与过程宏**：让消息真正「被处理」。先手写一个 `Node`/`Actor` trait 和它的 `exec` 循环（不用宏，看清本质），再一头扎进 **过程宏**——`proc-macro2` / `syn` / `quote`，亲手实现 `#[derive(Node)]`、`inputs!`/`outputs!` 和编译期注册表 `node_register!`。那是本书「学 Rust」含金量最高的一段，也是 MegFlow 最有辨识度的设计。

## 原版协议补充：容量 0 与无界通道

原版 `flow-rs/src/channel/storage.rs` 的 ChannelStorage::new 明确规定：cap 大于 0
创建有界队列，否则创建无界队列。因此 TOML 中 `cap = 0` 不是非法容量，也不是
发送方必须等接收方接手的会合通道。

当前 `code/flow-rs/src/channel.rs` 使用两个内部枚举保存 Tokio 的有界和无界端点。
channel(0) 调用 unbounded_channel，正容量调用 channel(capacity)。外部 Sender/Receiver
接口保持一致，send_any 和 recv_any 在内部 match 对应分支。

为什么无界发送还保留 async 方法？这是引擎的统一接口；无界分支本身直接入队，
不等待容量腾出。有界分支仍可能暂停直到容量可用。async 并不意味着每次调用都暂停。
无界队列没有容量背压，生产长期快于消费时，待处理消息会持续占用内存。

```bash
cargo test --manifest-path code/Cargo.toml -p flow-rs --test unbounded_channel --locked
```

测试先完成 1000 条发送，再接收，证明发送不依赖消费者腾出容量；随后验证最后一个
发送端释放后仍能排空队列，以及接收端释放后的发送错误。测试设置超时，避免错误地
实现成有界队列后无限挂起。

### 还不能把这一层称为完整原版通道

原版 Receiver 可克隆并由多个消费者竞争接收；当前已补普通消息的共享接收，见本章末尾。原版 sender.rs
还对 DummyEnvelope 按发送端 epoch 做汇合：同一轮参与发送端的信号收齐后才向队列
发送一个信号。receiver.rs 则把 flush 事件转成接收错误，由上层生命周期协议处理。
这与普通 `Envelope::<T>::empty()` 不同，不能简单用“载荷为空”判断 flush。

原版还有批量接收、超时、类型转换表、计数和统计。移植时必须联合验证 Actor 与
通道的控制流；只新增同名方法或把 flush 当永久关闭，会使动态子图提前退出。
本章的普通消息与容量测试不覆盖这些能力，完整迁移仍见验收账本。

练习：为什么容量 1 的通道不能用“先发 1000 条再收”的测试？第二条发送可能等待
容量，而主任务还没进入接收阶段，测试自己造成死锁。有界场景需并发生产和消费。

## 批量接收：按权重凑一批

原版 receiver.rs 的 batch_recv_any 使用 `weight.unwrap_or(1)` 累加权重。
所以 n=3 不一定返回三条消息：权重依次为 0、1、4 时，返回三条，累计权重为 5；
如果第一条权重就是 4，则一条便足够。None 默认算 1，Some(0) 算 0，不能混为一谈。

从单条接收扩展为批量接收，新增三个状态：已经收到的 batch、累计 weight、整批共享的
计时器。计时器在循环外创建，不能每收一条就重新计时，否则持续有消息时可能永不超时。

```rust,ignore
{{#include ../../../code/flow-rs/src/channel.rs:batch_receive}}
```

`tokio::pin!` 让计时器在循环的多次 select 中保持同一个 Future。select 等待超时或
下一条消息，哪一项就绪就处理哪一项；两项同时就绪时不承诺固定优先级。

返回规则要分别记住：

| 条件 | 返回值 | 已收到的消息 |
| --- | --- | --- |
| n=0 | Ok(空 Vec) | 不消费队列 |
| 累计权重达到 n | Ok(batch) | 包括让权重越过阈值的整条消息 |
| 整批超时 | Ok(batch) | 可以为空或不足阈值 |
| 输入提前关闭 | Err(BatchRecvError::Closed(batch)) | 放在错误里交还调用者 |

若收到关闭错误就直接丢弃错误值，部分批次也会丢失。业务节点需要显式 match
Closed(batch)，决定处理剩余数据还是报告不完整。类型化 batch_recv 在接收后转换
每个信封；与原版一样，类型错误会 panic。它不同于本书单条 recv 的 TypeMismatch 返回，
调用者不能假定当前所有入口有统一错误模型，完整 API 对照仍需继续完成。

```bash
cargo test --manifest-path code/Cargo.toml -p flow-rs --test batch_receive --locked
```

三个测试验证零权重与默认权重、超过阈值后的剩余消息、n=0 不消费、关闭时携带部分批次，
以及超时后接收端仍可继续使用。生产依赖也必须启用 Tokio 的 time feature，不能只在
dev-dependencies 中打开；否则测试通过而下游正常构建失败。

本次实现尚未接入原版 flush epoch 协议。原版遇到 flush 也会以 Closed(batch) 返回，
但底层通道未必永久关闭；后续必须由 Actor 区分轮次结束与真正关闭。

练习：若 batch_recv(2, ...) 返回长度为 1 的 Ok，是否错误？不一定，可能是权重
已经达到 2，也可能是超时。该接口只返回批次，不额外标识这两个成功原因。

## 限时接收：三种结果不能混淆

原版 `try_recv_any(dur)` 会等待最多给定时长，名称中的 try 不表示立即返回。
它不同于许多队列的同步 try_recv。当前补充的实现如下：

```rust,ignore
{{#include ../../../code/flow-rs/src/channel.rs:timed_receive}}
```

外层 Result 回答“接收是否遇到错误”，内层 Option 回答“本次等待有没有收到消息”：

- `Ok(Some(envelope))`：收到一条信封，其载荷本身仍可能为空。
- `Ok(None)`：等待超时，通道依旧可用，可以再次接收。
- `Err(ChannelClosed)`：当前实现已关闭且没有剩余消息。

例如视频节点周期性检查新帧时，Ok(None) 可以触发一次空闲处理，不能直接作为退出条件。
`Some(Envelope::empty())` 则确实消耗了一条队列消息，其 partial_id 等元信息仍须保留。
限时接收的类型转换沿用原版的类型不匹配 panic 行为。

```bash
cargo test --manifest-path code/Cargo.toml -p flow-rs --test timed_receive --locked
```

测试对有界和无界通道验证：先超时、再发送、再接收成功、最后关闭；另验证空载荷和
元信息。超时分支会取消本次等待，但不能取走后来到达的消息。零时长与已就绪消息同时
出现时，不承诺哪个分支胜出，因此不要用它代替精确的同步非阻塞队列操作。

原版 flush 也能令本次接收返回错误，但通道不一定永久关闭。当前 flush 尚未接入，
上述关闭判断仅描述已经实现的普通消息路径。


## 多消费者：克隆的是队列入口，不是消息

原版 Receiver::clone 共享底层队列，每个消费者调用 recv 都会取走一条消息。
本书现在使用 `Arc<tokio::sync::Mutex<RecvImpl>>` 包装底层 Tokio 接收端：Arc 让多个
Receiver 指向同一队列；Mutex 确保同一时刻只有一个任务修改队列的接收状态。
Receiver 因此可以实现 Clone，recv 系列方法改为接收 `&self`。

接收流程是：异步等待获得锁 → 等待底层消息 → 取出消息 → 释放锁 → 返回信封。
锁只保护队列操作，不保护业务处理；收到消息后各消费者可以并行计算。与原版底层
实现相比，这个版本串行化了接收等待，不宣称吞吐量或任务调度次序相同。

不要改用 std::sync::Mutex 并跨 await 持有普通锁，否则等待消息的任务可能占住执行
线程，阻止其他任务推进。这里的异步锁允许等待期间让出执行机会。

克隆接收端不克隆载荷。4 个消费者处理 1000 条输入，总共应当得到 1000 条，不是
4000 条；也不要求每个消费者恰好得到 250 条。调度和处理速度可以影响分配。

```bash
cargo test --manifest-path code/Cargo.toml -p flow-rs --test competing_receivers --locked
```

测试覆盖容量 0、1、16 下 4 个消费者竞争 1000 条消息，核对完整集合、数量和退出；
还检查释放一个 Receiver 后其他克隆仍可接收，最后一个释放后发送端观察到关闭。
取消测试先将 recv Future 显式 poll 到 Pending，再取消，证明它释放了已持有的锁，
后续消费者没有永久卡住。只创建 Future 然后立即 drop 无法证明实际等待的取消行为。

限时接收的计时包括等待这把锁的时间。批量接收每次只锁定一条消息的接收，因此不同
消费者的批次可以交错取得消息，不能把 batch_recv 当成对队列的事务锁。

图装配已支持内部连接多下游竞争接收；对外输入也支持多目标竞争接收；共享子图仍需独立对齐。
原版每个接收者还维护 flush 轮次；本次共享的是普通消息队列，尚未补齐这一控制协议。

### 用原版算法做独立对照

仅用自己写的期待值测试，可能把对原版的误解同时写进实现和测试。因此
`tests/reference/batch_recv_any.rs` 保留了指定原版提交中 batch_recv_any 的方法原文
和许可证。外围测试适配器提供队列接收与同一 Tokio 时间源，没有改写方法内的权重逻辑。

`weighted_batches_match_original_method_exhaustively` 为 6 条消息分别选择 None、
Some(0)、Some(3)，形成 729 种序列；每种使用 0 至 7 的 8 个阈值，共 5,832 次对照。
每次检查成功/关闭类别、载荷、权重和队列剩余数量。这样不仅检查“输出看起来正确”，
也检查阈值达到后是否多消费了一条消息。

对照队列预先装好数据并关闭，避免把并发调度的偶然顺序当成固定答案。限时行为仍由
前面的独立测试验证。这层证据不覆盖原版通道的 flush、多消费者轮次或统计；适配器
只隔离验证批量算法，不能宣称已经运行完整原版运行时。

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
该转换表，尚未提供原版 TypeInfo；默认未接线端点见下一节。因此这些包装只是
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
