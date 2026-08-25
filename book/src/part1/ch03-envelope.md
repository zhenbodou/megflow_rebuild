# Ch1.3 实现 `Envelope` 消息信封与类型擦除消息层

前两章是「原理课」：Ch1.1 讲清了消息交接靠**移动所有权**、跨任务靠 `Send`、共享只读靠 `Arc`；Ch1.2 讲透了**泛型→trait→`dyn`→`Any`** 这条从「编译期定死类型」到「运行期擦掉类型」的路。这一章开始是「施工课」——把原理**落成能编译、能测试的真实代码**，写进 `code/flow-message`。

从这一章起，凡是引擎的真实代码，一律走**红-绿 TDD**：先写一个会失败的测试（红），再写最小实现让它通过（绿）。这不是形式主义——它逼我们在动手前先把「对外契约」用可执行的形式钉死。本章要交付的契约，就是 Ch0.3 从真实 flow-rs 里读出来的那几行：`new` / `unpack` / `repack<T>` / `repack_inplace`，外加类型擦除的 `seal` 与安全 `downcast`。

<!-- toc -->

## 1. 先立契约：这一章要造什么

一句话：**造一个能被一条 channel 搬运的「消息盒子」**。它有三层：

- `Envelope<M>`——带类型的信封，装着载荷 `M` 和一份元信息 `EnvelopeInfo`。这是节点代码里**看得见类型**的那一面。
- `AnyEnvelope`——类型擦除的 trait，把泛型 `M` 藏起来，只暴露不带泛型的方法（Ch1.2 §4 的对象安全约束）。
- `SealedEnvelope = Box<dyn AnyEnvelope + Send>`——装箱擦除后的盒子，是 channel 真正搬运的东西；下游用 `downcast` 把类型「认领」回来。

对应到真实 flow-rs（Ch0.3 读过），原版把这套拆在 `envelope/` 下三个文件里（`envelope.rs` / `any_envelope.rs` / `mod.rs`）。我们重写**合成一个 `envelope.rs`**——这一层总共一百多行，拆三个文件反而割裂阅读；文件该按「一起变的东西放一起」来分，而不是按类型数量分。

## 2. 红：先写一个跑不起来的测试

TDD 的第一步永远是**红**——写出测试，运行，看它**因为「东西还不存在」而失败**。这一步的价值在于：它先帮你站在**调用方**视角，把 API 长什么样定下来。

我们在 `code/flow-message/src/envelope.rs` 里先只写测试模块（此刻 `Envelope` 等类型都还不存在）：

```rust,ignore
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_then_unpack() {
        let mut e = Envelope::new(1i32);
        assert!(e.is_some());
        assert_eq!(e.unpack(), 1i32);
        assert!(e.is_none()); // unpack 取走后信封为空
    }

    #[test]
    fn repack_carries_info_and_changes_type() {
        let mut src = Envelope::new(7i32);
        src.info_mut().partial_id = Some(42);
        // 换载荷类型：i32 → String，元信息随行
        let mut dst = src.repack(String::from("hi"));
        assert_eq!(dst.info().partial_id, Some(42));
        assert_eq!(dst.unpack(), "hi");
    }

    #[test]
    fn type_erasure_seal_then_downcast() {
        let sealed: SealedEnvelope = Envelope::new(3i32).seal();
        assert!(sealed.is::<Envelope<i32>>());
        assert!(!sealed.is::<Envelope<String>>());
        // 认领回具体类型：猜错得 None（安全），猜对拿到 &mut
        assert!(sealed.downcast_ref::<Envelope<String>>().is_none());
        let mut sealed = sealed;
        let inner = sealed.downcast_mut::<Envelope<i32>>().unwrap();
        assert_eq!(inner.unpack(), 3i32);
    }
}
```

在 `lib.rs` 里挂上 `pub mod envelope;`，然后运行：

```bash
cargo test -p flow-message
```

结果如预期——**红**：

```text
error[E0433]: cannot find type `Envelope` in this scope
  --> flow-message/src/envelope.rs:17:21
   |
17 |         let mut e = Envelope::new(1i32);
   |                     ^^^^^^^^ use of undeclared type `Envelope`
...
error: could not compile `flow-message` (lib test) due to 15 previous errors
```

这三个测试就是**可执行的契约**：`new`/`unpack`/`is_some`/`is_none`、`repack` 换类型且**元信息随行**、`seal`→`downcast` 的类型擦除与安全认领。下面写实现让它们变绿。

```mermaid
flowchart LR
    R["🔴 红<br/>写测试 → 编译失败<br/>（类型不存在）"] -->|"钉死对外契约"| G["🟢 绿<br/>写最小实现 → 测试通过"]
    G -->|"下一个特性"| R
```

## 3. 绿：`EnvelopeInfo` 与 `Envelope<M>`

### 3.1 `EnvelopeInfo`——只留现在用得上的字段

原版 `EnvelopeInfo` 有 **7 个字段**（`skipped` / `weight` / `partial_id` / `from_addr` / `to_addr` / `transfer_addr` / `extra_data`），服务于重排序、寻址、转发等**还没实现**的特性。我们重写秉持 **YAGNI**（You Aren't Gonna Need It）：先只放当下真正要用的两个，其余等对应特性（Ch4.2 重排序等）落地时再按需补——**避免提前引入一堆用不上、却要一直维护的状态**。

```rust,ignore
use std::any::Any;
use std::sync::Arc;

#[derive(Default, Clone)]
pub struct EnvelopeInfo {
    /// 序号（可重复），用于重排序等场景。
    pub partial_id: Option<u64>,
    /// 任意类型、可跨线程共享的附带数据。
    pub extra_data: Option<Arc<dyn Any + Send + Sync>>,
}
```

`extra_data` 的类型 `Option<Arc<dyn Any + Send + Sync>>` 把前两章的原理**一次用全**：`Arc` 负责共享 + 跨线程（Ch1.1 §4），`dyn Any` 负责「类型任意 + 可安全认领」（Ch1.2 §5），`Send + Sync` 是跨任务通行证（Ch1.1 §3）。`#[derive(Default, Clone)]` 让它能作为字段默认构造、随信封一起克隆。

### 3.2 `Envelope<M>`——载荷为什么是 `Option<M>`

```rust,ignore
pub struct Envelope<M> {
    info: EnvelopeInfo,
    msg: Option<M>,
}
```

载荷用 `Option<M>` 而非裸 `M`，有两个刚需：① `unpack` 要能把载荷**拿走**（`Option::take` 留下 `None`），这对应「消息被下游取走消费」的语义；② 允许存在**空信封**（`empty()`、或载荷已被取走）。

方法实现直接照契约写。注意 `new`/`unpack`/`repack`/`info` 这些**不涉及类型擦除**的方法放在**无约束**的 `impl<M>` 块里——载荷装取、换包本身不需要 `M: 'static/Send`：

```rust,ignore
impl<M> Envelope<M> {
    pub fn new(msg: M) -> Self {
        Self { info: EnvelopeInfo::default(), msg: Some(msg) }
    }
    pub fn empty() -> Self {
        Self { info: EnvelopeInfo::default(), msg: None }
    }
    pub fn info(&self) -> &EnvelopeInfo { &self.info }
    pub fn info_mut(&mut self) -> &mut EnvelopeInfo { &mut self.info }

    /// 取出载荷（留下空信封）。空信封会 panic。
    pub fn unpack(&mut self) -> M {
        self.msg.take().expect("envelope has no message")
    }

    /// 复用同一份元信息，换一个新载荷 T，产出「新类型」的信封。
    pub fn repack<T>(&self, msg: T) -> Envelope<T> {
        Envelope { info: self.info.clone(), msg: Some(msg) }
    }

    /// 原地替换载荷（类型不变）。
    pub fn repack_inplace(&mut self, msg: M) {
        self.msg = Some(msg);
    }

    pub fn is_some(&self) -> bool { self.msg.is_some() }
    pub fn is_none(&self) -> bool { self.msg.is_none() }
    // …… with_info / take / get_ref / get_mut 同理，见真实代码
}
```

`repack<T>` 是引擎里节点做「输入信封 → 输出信封」映射的关键：**载荷类型从 `M` 变成 `T`，但元信息（序号等）克隆保留、随消息一路流下去**。这就是为什么它带一个新的类型参数 `T` 而不是固定 `M`。

### 3.3 `Clone` 约束：为什么单独一个 `impl` 块

原版 `Envelope<M>` 的方法块整体要求 `M: 'static + Send + Clone`。我们**把 `Clone` 拆出来、单独条件实现**——只有当载荷本身可克隆时，信封才可克隆：

```rust,ignore
impl<M: Clone> Clone for Envelope<M> {
    fn clone(&self) -> Self {
        Envelope { info: self.info.clone(), msg: self.msg.clone() }
    }
}
```

为什么要拆？因为「信封可克隆」这个能力**只有广播（Ch4.1 的 `bcast`：一份消息发给多个下游）才需要**。把 `M: Clone` 从「所有信封的基本门槛」降级成「按需才要」，意味着**载荷不可克隆的消息类型也能正常流经引擎**（只要不走广播）。这是一处「约束更精确 → 适用面更宽」的重写改进：不为一个小众特性向所有消息强加 `Clone`。

## 4. 绿（续）：类型擦除三件套

现在是这一章的技术核心——把 Ch1.2 的原理写成代码。

### 4.1 `AnyEnvelope`：对象安全的擦除接口

trait 里**每个方法都不带泛型参数**（守住 Ch1.2 §4 的对象安全红线），关键是 `as_any` 把自己「降级」成 `&dyn Any`：

```rust,ignore
pub trait AnyEnvelope: Any {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn is_some(&self) -> bool;
    fn is_none(&self) -> bool;
    fn info(&self) -> &EnvelopeInfo;
    fn info_mut(&mut self) -> &mut EnvelopeInfo;
}

impl<M: 'static> AnyEnvelope for Envelope<M> {
    fn as_any(&self) -> &dyn Any { self }          // Envelope<M> → &dyn Any
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
    fn is_some(&self) -> bool { self.msg.is_some() }
    fn is_none(&self) -> bool { self.msg.is_none() }
    fn info(&self) -> &EnvelopeInfo { &self.info }
    fn info_mut(&mut self) -> &mut EnvelopeInfo { &mut self.info }
}
```

`AnyEnvelope: Any` 这个**超 trait**约束要求实现者 `'static`（`Any` 的前提），所以 `impl` 块写 `M: 'static`。注意这里**不要求 `M: Clone`**——擦除、认领、读写元信息都跟能否克隆无关，又一次把约束收窄到刚好够用。

### 4.2 `SealedEnvelope` 与 `seal`

```rust,ignore
pub type SealedEnvelope = Box<dyn AnyEnvelope + Send>;

impl<M: 'static + Send> Envelope<M> {
    pub fn seal(self) -> SealedEnvelope {
        Box::new(self)   // Envelope<M> 被 unsize 强制转换成 dyn AnyEnvelope + Send
    }
}
```

`seal` 这里才要求 `M: Send`——因为 `Box::new(self)` 要能强制转换（unsize coercion）成 `Box<dyn AnyEnvelope + Send>`，前提是 `Envelope<M>: Send`，而这需要 `M: Send`（Ch1.1 §3：擦除后的盒子要跨线程搬运，`+ Send` 是硬通行证）。把 `Send` 只加在 `seal` 这个真正需要它的方法上，而不是全类型——**约束跟着需求走**，这条原则本章反复出现。

### 4.3 安全 downcast：抹掉原版那段 `unsafe`

这是本章相对原版最实在的一处改进。回顾 Ch1.2 §5：原版在 `impl dyn AnyEnvelope` 上**手写** `downcast_ref`，内部用 `unsafe` 把裸指针 `transmute` 成 `&T`。我们不必自己 transmute——既然 `as_any()` 能给出 `&dyn Any`，就把「变回具体类型」全权交给**标准库那套久经考验的安全实现**：

```rust,ignore
// 定义在 `dyn AnyEnvelope + Send` 上（即 SealedEnvelope 的内层类型）
impl dyn AnyEnvelope + Send {
    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        self.as_any().downcast_ref::<T>()      // ← std 的安全 downcast，无 unsafe
    }
    pub fn downcast_mut<T: Any>(&mut self) -> Option<&mut T> {
        self.as_any_mut().downcast_mut::<T>()
    }
    pub fn is<T: Any>(&self) -> bool {
        self.as_any().is::<T>()
    }
}
```

整段代码**零 `unsafe`**。`downcast` 带泛型参数 `T`，所以它只能是 `impl` 块上的**关联函数**、进不了 vtable（Ch1.2 §4 的对象安全约束依然生效）——这一点和原版一致；不同的是内部实现从「手写 unsafe transmute」换成了「std 安全路径」。测试 `type_erasure_seal_then_downcast` 里「猜错类型得 `None` 而不崩」正是这份安全性的体现。

> **一处细节**：这三个便捷方法定义在 `dyn AnyEnvelope + Send` 上（正是 `SealedEnvelope` 的内层类型），所以对一个 `SealedEnvelope` 直接 `sealed.downcast_ref::<...>()` 就能用。原版为了同时支持 `+ Send`、`+ Send + Sync` 等多种标记组合写了三个几乎重复的 `impl` 块；我们当前只有 `+ Send` 一种擦除盒子在用，就只写这一个——需要别的组合时再加，不预先铺开。

### 4.4 `DummyEnvelope`：一个不带载荷的占位信封

某些控制路径（比如后面的停机信号）需要「一个空信封」而根本不携带任何 `M`。原版用 `DummyEnvelope` 承担。它同时是 `AnyEnvelope` 的**第二个实现者**，正好证明这个 trait 不是 `Envelope<M>` 专属：

```rust,ignore
#[derive(Default, Clone)]
pub struct DummyEnvelope;

impl DummyEnvelope {
    pub fn seal(self) -> SealedEnvelope { Box::new(self) }
}

impl AnyEnvelope for DummyEnvelope {
    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
    fn is_some(&self) -> bool { false }
    fn is_none(&self) -> bool { true }
    fn info(&self) -> &EnvelopeInfo {
        unimplemented!("DummyEnvelope has no EnvelopeInfo")  // 与原版一致：占位信封不携带元信息
    }
    fn info_mut(&mut self) -> &mut EnvelopeInfo {
        unimplemented!("DummyEnvelope has no EnvelopeInfo")
    }
}
```

`info()` 用 `unimplemented!()`——这是**刻意保留**原版行为：占位信封不带元信息，引擎的控制路径也从不对它调 `info()`。真到有人调用，`unimplemented!` 会立刻 panic 指出「这里逻辑错了」，比返回一份假数据更早暴露 bug。

## 5. 再跑测试：绿

```bash
cargo test -p flow-message
```

```text
running 6 tests
test envelope::tests::dummy_is_empty ... ok
test envelope::tests::extra_data_shared_via_arc ... ok
test envelope::tests::new_then_unpack ... ok
test envelope::tests::repack_carries_info_and_changes_type ... ok
test envelope::tests::repack_inplace_keeps_type ... ok
test envelope::tests::type_erasure_seal_then_downcast ... ok

test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

**绿**。红-绿走完一轮，消息层的地基就位了。完整代码在 [`code/flow-message/src/envelope.rs`](https://github.com/) 里（本仓库 `code/flow-message/src/envelope.rs`），本章所有片段都从那里节选，逐字对应。

> **依赖账**：这一章**没有引入任何外部依赖**——纯标准库（`std::any::Any` + `std::sync::Arc`）就够了。原版 `AnyEnvelope` 还继承了 `dyn_clone::DynClone` 来支持「克隆一个 `Box<dyn AnyEnvelope>`」；那个能力同样**只有广播（Ch4.1）才需要**，所以推迟到那时再引入 `dyn-clone`（crates.io 公共 crate）。地基阶段不背它。

## 6. 逐条对比：我们相对原版改了什么

| 维度 | 原版 flow-rs | 本书重写 | 为什么 |
|---|---|---|---|
| `downcast` 实现 | `impl dyn AnyEnvelope` 手写，含 `unsafe` transmute | `as_any() -> &dyn Any` + std 安全 downcast，**零 unsafe** | 更少 unsafe = 更少潜在 UB，把正确性交给标准库 |
| `EnvelopeInfo` 字段 | 7 个（多数服务未实现特性） | 2 个（`partial_id` / `extra_data`），按需再加 | YAGNI：不提前背未使用的状态 |
| `M: Clone` 约束 | 绑在主方法块，几乎所有信封都要求 | 拆成 `impl<M: Clone> Clone`，仅广播场景需要 | 约束更精确 → 不可克隆的消息也能流经引擎 |
| `Send` 约束 | 混在方法块 | 只加在真正要跨线程的 `seal` 上 | 约束跟着需求走 |
| `dyn_clone` 依赖 | 一开始就继承 `DynClone` | 推迟到 Ch4.1 广播时才引入 | 地基阶段零外部依赖 |
| 文件组织 | 拆 3 个文件 | 合成 1 个 `envelope.rs` | 一起变的东西放一起 |

这些都不是「为改而改」——每一条都对应「学 Rust」或「更少 bug / 更精确约束」这两个目标里的一个，且都有测试兜底。

## 小结

这一章把消息层从原理**落成了真实、通过测试的代码**：

- **红-绿 TDD 开张**：先写会失败的测试钉死契约（`new`/`unpack`/`repack`/`seal`/`downcast`），再写最小实现变绿。从此引擎代码皆如此。
- **`Envelope<M>`**：载荷 `Option<M>`（支持取走 + 空信封）；`repack<T>` 换类型且元信息随行；`Clone` 拆成按需条件实现。
- **类型擦除三件套**：对象安全的 `AnyEnvelope`（`as_any` 降级）→ `SealedEnvelope = Box<dyn AnyEnvelope + Send>`（`seal` 装箱，`+ Send` 通行证）→ **零 unsafe** 的安全 `downcast`（认领回具体类型，猜错得 `None`）。
- **约束跟着需求走**：`'static`/`Send`/`Clone` 各自只加在真正需要的方法/impl 上，而非一刀切绑死。
- **零外部依赖**：纯 std 完成；`dyn-clone` 等推迟到用得上的章节。

下一章 **Ch1.4**：给引擎装上**异步的心跳**——`async`/`await`、`Future`、`tokio` 入门，然后把 tokio 的 channel **封装**成引擎自己的收发端，让 `SealedEnvelope` 真正在任务之间「流动」起来。同样红-绿，代码写进 `code/flow-rs`。
