# Ch1.1 Rust 复习：并发下的所有权、借用、生命周期 + 错误处理

Part 0 我们把**参照系**钉死了：真实 flow-rs 的 `1 + 2 == 3`。从这一章起动手施工。但在写第一行引擎代码前，先补一轮 Rust——**只补引擎真正要用的那几样**，不做语言全景。

这一章不是 Rust 教程的替代品。它是一张**过滤器**：把「写一个 async dataflow 引擎」会反复撞上的 Rust 概念挑出来，用引擎里的真实场景讲清楚。读完你要能回答四个问题：

1. 一条消息从上游流到下游，**所有权**是怎么交接的？
2. `'static` 到底在说什么——为什么 `Envelope<M>` 的 `M` 要 `'static`？
3. 一个类型凭什么能**跨任务/跨线程**跑（`Send` / `Sync`）？
4. 引擎出错时，我们用 `thiserror` 还是 `anyhow`——为什么？

<!-- toc -->

## 1. 所有权与移动：消息「交接」的本质

Rust 的**所有权（ownership）**一句话：每个值有且只有一个所有者；所有者离开作用域，值就被丢弃。把一个值赋给别人、或传进函数，默认是**移动（move）**——所有权交出去，原来的绑定就不能再用了。

这正是 dataflow 引擎里「一条消息从 A 流到 B」的本质：不是复制一份，而是**把所有权搬过去**。

```rust,ignore
struct Frame {
    data: Vec<u8>,   // 一帧图像，可能很大 / a (potentially large) image frame
}

fn producer() -> Frame {
    Frame { data: vec![0u8; 1920 * 1080 * 3] }
}

fn consume(frame: Frame) {           // 按值接收 = 拿走所有权 / takes ownership
    println!("got {} bytes", frame.data.len());
}                                    // frame 在此被丢弃 / dropped here

fn main() {
    let f = producer();
    consume(f);      // f 的所有权移动进 consume / moved into consume
    // consume(f);   // ❌ 编译错误：f 已被移动 / error: use of moved value
}
```

**为什么这对引擎是好事**：消息在节点之间「交接」而非「共享」，天然杜绝了两个节点同时改同一份数据的数据竞争。一帧图像发给下游，上游就不再持有它——不会有人在下游读的同时上游偷偷改。所有权系统在**编译期**就保证了这条纪律，不需要运行期加锁。

> 记住这条主线：**在我们的引擎里，「发送一条消息」= 移动它的所有权**。后面 Ch1.4 封装 channel 时，`send(msg)` 的签名就是**按值**接收 `msg`，把它搬进通道。

### 移动 vs 克隆

需要「两个都要」时，才显式 `.clone()` 复制一份。克隆是**明码标价**的——你写出 `.clone()`，就知道这里有一次拷贝开销。原版 `Envelope<M>` 要求 `M: Clone`，正是为了支持「广播」这类「一份消息发给多个下游」的场景（Ch4.1 的 `bcast`）：给每个下游 `clone` 一份。

```rust,ignore
let f = producer();
let f2 = f.clone();   // 显式复制一份，f 仍可用 / explicit copy; f still usable
consume(f);
consume(f2);
```

## 2. 借用与生命周期：`&T` / `&mut T` 与 `'static`

不想交出所有权、只想「看一眼」或「改一下」，就用**借用（borrow）**：

- `&T`：**共享借用**（只读），可以同时存在多个。
- `&mut T`：**独占借用**（可写），同一时刻只能有一个，且不能与任何 `&T` 并存。

这条「**要么多个只读、要么一个可写**」的规则（借用检查器 borrow checker 强制），是 Rust 在**单线程内**就杜绝数据竞争的核心。我们节点的心跳 `async fn exec(&mut self)` 用的就是 `&mut self`——每次 `exec` 独占地改自己的状态，框架保证不会有第二个 `exec` 同时在同一个节点上跑。

```rust,ignore
struct BinaryOp { op: char }

impl BinaryOp {
    // &mut self：这次调用独占地借用节点自身 / exclusive borrow of self
    async fn exec(&mut self) {
        // 能读能改 self.op、self.a、self.c …… 期间没有别人能碰 self
    }
}
```

### `'static` 不是「永远活着」

初学者最容易误解 `'static`。它**不**意味着「这个值活到程序结束」。作为**约束**（bound）时，`T: 'static` 的真实含义是：

> **`T` 类型里不含任何「借来的、寿命短于整个程序」的引用**——它要么是拥有所有权的数据（`String`、`Vec<u8>`、`i32`），要么内部只含 `'static` 引用（如 `&'static str`）。

为什么引擎处处要求 `'static`？因为消息要被**移动进一个异步任务**（tokio task），而任务可能比创建它的那个函数活得久——如果消息里藏着一个指向局部变量的短命引用，任务跑起来时那个局部早没了，就悬垂了。要求 `M: 'static` 就是编译期拒绝这种情况：**能跨任务搬运的，必须是自包含、不借别人东西的数据**。

```rust,ignore
// ✅ i32 是 'static：纯拥有的数据 / owned data, satisfies 'static
fn ok(x: i32) { spawn_task(x); }

// ✅ String 是 'static：自己拥有堆上的字节 / owns its bytes
fn ok2(s: String) { spawn_task(s); }

// ❌ &str 借用了别处的数据，一般不是 'static（除非 &'static str）
// fn bad(s: &str) { spawn_task(s); }   // 任务可能比 s 借用的源活得久
```

这就解释了 §1 里 `Envelope<M>` 的那行约束 `M: 'static + Send + Clone`——`'static` 保证消息能安全地被搬进任意任务。

## 3. `Send` + `Sync`：跨任务/线程的通行证

这是写并发引擎**最关键**的一对 trait，也是很多人没真正弄懂的地方。它们是**自动 trait（auto trait）**：编译器按字段自动推导，你几乎从不手写实现。

- **`Send`**：类型的值可以**被移动到另一个线程**。绝大多数类型都是 `Send`。典型的**非 `Send`**：`Rc<T>`（非原子引用计数，跨线程改计数会出错）、裸指针。
- **`Sync`**：类型可以被**多个线程同时共享 `&T` 引用**。等价定义：`T: Sync` ⟺ `&T: Send`。典型非 `Sync`：`Cell` / `RefCell`（内部可变但无同步）。

**和引擎的关系**：tokio 的多线程运行时会把任务调度到不同工作线程上跑。要把一个 future 交给运行时 `spawn`，这个 future（连同它捕获的所有数据）必须是 `Send`。顺着推导下去：

- 我们的消息 `M` 要能跟着任务走 → `M: Send`。
- channel 的发送端/接收端要能分给不同任务持有 → 它们要 `Send`。
- 跨节点共享的资源（Ch4.3）要能被多个任务同时只读访问 → `Sync`。

所以原版 `Envelope<M>` 的类型擦除句柄写的是 `Box<dyn AnyEnvelope + Send>`——那个 `+ Send` 不是装饰，是「这个装箱后的消息**允许跨线程搬运**」的硬性通行证。

```rust,ignore
use std::rc::Rc;
use std::sync::Arc;

fn needs_send<T: Send>(_: T) {}

fn main() {
    needs_send(Arc::new(1));  // ✅ Arc 是 Send + Sync（原子计数）
    // needs_send(Rc::new(1)); // ❌ Rc 不是 Send：编译期就被拦下
}
```

> 一句话记住：**`Send` = 能搬去别的线程；`Sync` = 能被多线程同时只读。** 引擎里凡是要跨任务流动或共享的东西，编译器都会在背后默默检查这两张通行证——不合格，`spawn` 那一行就编译不过。这是 Rust 相比原版有栈协程方案「更少 bug」的底气之一：数据竞争在编译期就没了。

## 4. `Arc`：共享所有权

移动是「交接」，借用是「看一眼」。但有时确实需要**多个所有者共享同一份数据**且都能长期持有——比如一个内存池、一份只读的模型配置，要被图里所有节点共用。这时用 **`Arc<T>`**（Atomically Reference Counted，原子引用计数的共享指针）：

- `Arc::clone` 只增加引用计数、**不复制底层数据**，开销极小。
- 最后一个 `Arc` 被丢弃时，底层数据才释放。
- `Arc<T>` 是 `Send + Sync`（前提 `T: Send + Sync`），所以能安全地分发给多个任务。

```rust,ignore
use std::sync::Arc;

let config = Arc::new(vec![1, 2, 3]);      // 一份共享配置 / shared config
let for_node_a = Arc::clone(&config);      // 计数 +1，不拷贝数据 / bumps refcount
let for_node_b = Arc::clone(&config);      // 计数 +1
// 三个 Arc 指向同一份 Vec；各自的任务都能只读它
```

原版 `EnvelopeInfo` 里那个 `extra_data: Option<Arc<dyn Any + Send + Sync>>` 就是这个用法：给消息挂一份**类型任意、可跨线程共享**的附带数据——`Arc` 负责共享与线程安全，`dyn Any` 负责「类型任意」（下一章 Ch1.2 专讲 `dyn` 与 `Any`）。

> **`Arc<T>` 只给你共享的只读访问**。想在共享的同时还能改，得配一把锁：`Arc<Mutex<T>>` / `Arc<RwLock<T>>`。原版为了性能自造了协程感知的 `rwlock`；我们重写直接用 tokio 的 `Mutex`/`RwLock`（Ch4.3 再展开），少一坨 unsafe。

## 5. 错误处理：`Result` / `?` 与 `thiserror` vs `anyhow`

Rust 没有异常。可恢复的错误一律用 `Result<T, E>` 表达，`?` 运算符负责「出错就提前返回」的样板：

```rust,ignore
fn parse_op(s: &str) -> Result<char, MyError> {
    let c = s.trim().chars().next().ok_or(MyError::Empty)?;  // None → 提前返回 Err
    Ok(c)
}
```

真正要**决策**的是错误类型 `E` 怎么定。两条主流路线，也是原版与我们重写的分野：

| | `anyhow`（原版用） | `thiserror`（本书重写用） |
|---|---|---|
| 定位 | 「一个能装下任何错误」的动态错误类型 | 为你的库**定义**结构化错误枚举 |
| 典型 | `anyhow::Result<T>`，随手 `?` 上抛 | `#[derive(Error)] enum Error { ... }` |
| 优点 | 写应用飞快、不用起名 | 调用方能 `match` 分支、类型即文档 |
| 代价 | 调用方拿到的是「黑盒」，难精确处理 | 要花几行定义枚举 |

**我们的选择：`thiserror`。** 理由和「学 Rust + 更少 bug」的目标一致：引擎是一个**库**，它的错误会被上层代码 `match` 和分别处理（channel 关了？TOML 配置错了？节点没注册？这几种错误调用方要区别对待）。`thiserror` 让我们把这些情形写成一个**类型化枚举**，每个变体自带一句人读的消息，调用方既能精确匹配、又能直接打印。这比原版 `anyhow` 的「一个黑盒装所有」更适合做被别人依赖的引擎。

预告一下我们的错误类型会长这样（Ch1.4 起真正写进 `code/`）：

```rust,ignore
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("channel closed")]           // 通道已关闭
    ChannelClosed,
    #[error("node type not registered: {0}")]  // 节点类型未注册
    UnknownNode(String),
    #[error("config parse error: {0}")]  // 配置解析失败
    Config(String),
}

pub type Result<T> = std::result::Result<T, Error>;
```

`#[derive(Error)]` 是 `thiserror` 提供的派生宏，`#[error("...")]` 里的字符串会成为该变体的 `Display` 输出，`{0}` 引用变体里的字段。**注意这和过程宏是两回事**——`thiserror` 是我们从 crates.io 引入的现成库，Part 2 我们要亲手写的 `#[derive(Node)]` 才是自造的过程宏。

## 小结

这一章挑出了写引擎绕不开的 Rust 地基，全部锚在真实场景上：

- **所有权与移动**：发送一条消息 = 移动它的所有权，编译期杜绝共享写竞争；需要「都要」时显式 `.clone()`。
- **借用与 `'static`**：`&mut self` 让 `exec` 独占改自身状态；`T: 'static` = 「不含短命借用、可安全跨任务搬运」，不是「永远活着」。
- **`Send` / `Sync`**：跨任务/线程的两张编译期通行证——`Send` 能搬、`Sync` 能共享只读；这是 `Box<dyn AnyEnvelope + Send>` 里 `+ Send` 的由来，也是「更少 bug」的底气。
- **`Arc`**：多所有者共享只读数据（`extra_data`、共享资源），克隆只加计数不拷数据；要改再加锁。
- **错误处理**：重写用 `thiserror` 定义类型化错误枚举（引擎是库，调用方要 `match`），而非原版的 `anyhow` 黑盒。

下一章 **Ch1.2**：把「类型任意」这件事讲透——**泛型、trait、trait 对象 `dyn`、`Any` 与 `downcast`**。它是 Ch1.3 实现 `Envelope` **类型擦除**（让一条 channel 能搬运「装了任意 `M` 的信封」）的直接前置。
