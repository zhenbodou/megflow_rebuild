# Ch1.4e 取消、超时与任务错误

先运行上一课的双端转换测试。本课不增加新的业务节点，而是验证已有转换代码在取消和 panic 时的真实行为。

完成标准：能根据消息的所有权所在阶段解释取消结果，并区分业务 Err 与任务 panic。

<!-- toc -->

## 取消实作：停止等待不等于回滚已经发生的事

接入类型转换后，一次 `recv()` 不再只有“等队列”这一个阶段：它先等待并取出消息，
然后把消息所有权交给阻塞转换任务，再等待转换结果。取消发生在哪个阶段，会改变结论。

| 取消时刻 | 消息归属 | 再次接收的结果 |
| --- | --- | --- |
| 队列仍为空、尚未出队 | 没有取走消息 | 可以接收后来发送的消息 |
| 已出队且转换任务已开始 | 转换任务拥有消息 | 不会自动从原队列再收到这一条 |

Tokio 已启动的 `spawn_blocking` 任务不能像普通 async 任务那样被 abort 停止。
取消等待它的外层接收任务，不会让已经执行的业务转换回滚。
[Tokio spawn_blocking 文档](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html)

本仓 `tests/conversion.rs::cancelling_receive_does_not_restore_message_already_in_conversion`
使用三个信号把时序变成可重复的实验：

1. 转换函数发出 started，证明它确实已经拿到消息；测试不能只凭启动了接收任务就假定消息已出队。
2. 转换函数等待 release；测试收到 started 后取消外层任务，并等待取消完成。
3. 测试发出 release，转换继续执行并发出 finished。再次接收只能看到队列关闭，不能重取那条消息。

started 与 finished 用 Tokio oneshot，适合只通知一次的事件。release 用标准库同步通道，
因为等待它的是阻塞线程；不能把这个同步等待直接放进 async 执行线程。信号句柄用 Mutex
和 Option 包装，是为了从共享测试状态中安全地取出只能发送一次的 oneshot Sender。

测试用有限超时防止失败时永久等待，但先后顺序来自信号，不来自 `sleep(100ms)`。
随意睡一会儿无法证明阻塞任务已经进入函数，在负载变化时容易产生偶发失败。

运行 `cargo test --manifest-path code/Cargo.toml -p flow-rs --test conversion --locked`。
先预测：取消之后转换里的计数副作用会不会继续发生？再查看 finished 的断言。
这有助于理解为什么不能把 `try_recv` 超时解释成“这段时间什么也没有发生”。

原版 rt 的 spawn_blocking 也委托给 Tokio，但它使用自己的全局运行时与句柄包装。
本实验只证明当前实现的已启动转换取消行为，尚未覆盖原版运行时关闭、异常传播和完整 flush。
完整业务等价仍需沿这些边界继续验证；不能为了表面上“不丢消息”就擅自加入原版没有的重试。


## 错误分层实作：业务 Err 和任务 panic 不是同一个出口

看见一个异步函数返回 `Result`，不能推断它永远不会 panic。转换函数本身的签名是
`fn(SealedEnvelope) -> SealedEnvelope`，没有业务错误返回值；如果内部 downcast 或解析
使用 unwrap 失败，就会 panic。

原版 `flow-rs/src/rt/join_handle.rs` 的 `Future::poll` 将 Tokio 任务结果展开为：

```rust,ignore
match ret {
    Poll::Pending => Poll::Pending,
    Poll::Ready(t) => Poll::Ready(t.unwrap()),
}
```

先把变量层次说清楚：`ret` 是轮询的结果；Ready 中的 `t` 是 Tokio 的
`Result<T, JoinError>`。转换任务正常返回时得到 T；转换任务 panic 时得到 JoinError，
随后这次 unwrap 又让等待它的任务 panic。这段规则来自原版源码，不应擅自改成业务 Err。

当前 `channel::convert` 采用对应处理：等待 `spawn_blocking`，对 JoinError 使用 unwrap，
成功时才把转换结果作为正常消息返回。这里的 unwrap 是刻意保留原版契约；如果设计一个
新框架，可以选择不同错误策略，但那需要明确变更，而不是声称行为完全相同。

### 用两层结果读懂测试

测试另起一个 Tokio 任务执行 send 或 recv，因此观察结果时有两层：

| 表达式结果 | 意义 |
| --- | --- |
| `Ok(Ok(message))` | 任务正常结束，接收也成功 |
| `Ok(Err(ChannelClosed))` | 任务正常结束，业务接收报告关闭 |
| `Err(join_error)` 且 `is_panic()` | 整个接收任务发生 panic |

发送函数的成功值是 `()`，分层规则相同。不能把外层与内层都叫“出错”然后一并忽略，
否则会误以为 Actor 的普通 Err 收尾逻辑也覆盖 panic。当前 Actor 在普通 Result 失败后
安排的 finalize，不是 unwind 清理保障；完整异常生命周期仍需继续核对原版。

`tests/conversion.rs::converter_panic_propagates_as_task_panic_on_send_and_receive`
故意登记一个 panic 的转换函数。发送侧断言任务 panic，队列中没有消息；接收侧先入队，
再在转换中 panic，断言消息已出队且不能重取。测试通过不表示运行期间没有 panic，
而是证明它发生在约定的出口。

运行：

```sh
cargo test --manifest-path code/Cargo.toml -p flow-rs --test conversion --locked
```

独立练习：暂时把转换任务的 unwrap 改成 `map_err` 返回普通错误，预测哪条断言失败。
你应能解释：外层从 Err 变成 Ok，内层才携带 Err，节点的后续错误处理路线随之改变。
完成实验后恢复原版传播规则。

这次仅对齐收发转换的任务失败出口；原版全局运行时、线程固定、关闭等待和完整节点
清理仍有独立验收项，不能由这一项测试推导它们全部完成。
