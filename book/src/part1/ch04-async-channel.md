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

`code/flow-rs/src/channel.rs`——发送端可 `Clone`（多生产者扇入），接收端 `recv` 取 `&mut self`（单消费者）：

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

**为什么是 MPSC（多生产者单消费者）？** 一条通道就是引擎里的一条**边**。多个上游发往同一下游（**扇入**）很常见，所以 `Sender` 可 `Clone`；而一条边的**下游只有一个**消费者，所以 `Receiver` 单持有、`recv` 取 `&mut self`——这正好是 tokio `mpsc` 的形状，天然契合。

**那广播（一份发多路）和 demux（多消费者抢单）呢？** 不在这一层。**广播**是 Ch4.1 `bcast` **节点**的职责：它持有多个输出端口，对每个下游 `clone` 一份消息分别 `send`——是节点层用「多条 mpsc 边」拼出来的，通道本身不需要多消费者。**demux/work-stealing** 留到 Ch4.2。把通道保持成「一条极简的管子」，复杂语义交给节点组合——这比原版把 stats / storage / 容量策略 / 广播全塞进 `channel/` 要清爽得多。

**错误为什么只有两种？** 通道层能出的错，本质只有「对端没了」（`ChannelClosed`）和「类型认领失败」（`TypeMismatch`）。不臆造更多变体。

## 7. 逐条对比：相对原版的化简

| 维度 | 原版 flow-rs `channel/` | 本书重写 | 为什么 |
|---|---|---|---|
| 底层 | 有栈协程感知的自制通道（`inner`/`storage`） | `tokio::sync::mpsc` 薄封装 | 少写 unsafe 与自维护调度，接主流生态 |
| 文件量 | 6 文件、~50KB（含 stats/storage） | 1 个 `channel.rs`、~百行 | 只做「异步收发信封」这一件事 |
| 广播/demux | 揉在通道层 | 上移到节点层（Ch4.1/4.2） | 通道极简，语义靠节点组合 |
| 错误 | `anyhow` + 多处自定义 | `thiserror` 两变体，按需生长 | 类型化、可 `match`、无死代码 |
| 运行时 | 自制 `rt/spawn_pinned` | tokio `spawn` / `#[tokio::main]` | 主流、久经考验 |

同样，每条化简都对齐「学 Rust / 更少 bug / 功能一致」，且有 5 个测试兜底。真实代码见 `code/flow-rs/src/{channel,error}.rs`。

## 小结 · Part 1 收官

这一章给引擎装上了异步心跳，也补齐了 Part 1 的地基：

- **异步三件套**：`Future` 是惰性状态机（`poll` 问「好了吗」）；`async fn` 返回 Future；`.await` 是**协作式让出点**——让出任务而非阻塞线程。
- **tokio 替换有栈协程**：主流无栈 async，少 unsafe、接生态；用到运行时、`spawn`、`mpsc` 三样。
- **通道封装**：`tokio::sync::mpsc` 薄封装成承载 `SealedEnvelope` 的 `Sender`/`Receiver`；`send_any`/`recv_any` + 类型化 `send::<T>`/`recv::<T>`（seal / 安全 downcast 各薄薄一层）。
- **错误落地**：`thiserror` 的 `Error` 枚举第一次写进 `code/`，两变体、按需生长。
- **化简**：广播/demux 上移到节点层，通道保持极简；对比原版削掉大量 unsafe 与自制机制。

**Part 1 完成**。我们现在有了：能装任意载荷、能类型擦除的**消息层**（`flow-message`），和能在异步任务间搬运它的**通道**（`flow-rs::channel`）。

下一部分 **Part 2 · 节点与过程宏**：让消息真正「被处理」。先手写一个 `Node`/`Actor` trait 和它的 `exec` 循环（不用宏，看清本质），再一头扎进 **过程宏**——`proc-macro2` / `syn` / `quote`，亲手实现 `#[derive(Node)]`、`inputs!`/`outputs!` 和编译期注册表 `node_register!`。那是本书「学 Rust」含金量最高的一段，也是 MegFlow 最有辨识度的设计。
