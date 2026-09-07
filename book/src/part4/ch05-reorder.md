# Ch4.5 Reorder：把乱序消息恢复为连续序列

本章前置是信封、单输入输出节点和 Graph。目标是复现原版
`flow-rs/src/node/reorder.rs` 的普通消息重排序行为。先读规则，再实现，再与原版对照。

## 1. 重排序需要序号

并行处理可能让 2 号结果早于 0 号完成。载荷本身不一定可比较，因此使用
`EnvelopeInfo.partial_id` 作为顺序依据。节点从 0 开始等待，收到 2 先缓存，
收到 0 就立即输出 0，收到 1 后输出 1 和缓存里的 2。

| 到达 | 下一个期待 seq_id | 缓存 | 本次输出 |
| --- | --- | --- | --- |
| 初始 | 0 | 空 | 无 |
| 2 | 0 | 2 | 无 |
| 0 | 1 | 2 | 0 |
| 1 | 3 | 空 | 1、2 |

它不等待输入关闭后才统一排序。若一直缺少 0，后面到达的消息会一直缓存；
当前原版算法没有为这个缓存设置上限。通道容量限制不能自动限制节点自己的缓存。

## 2. 按原版确定边界行为

每条普通消息必须带 partial_id。已输出的序号再次出现会 panic；尚未输出的未来序号
重复到达，后来的信封会覆盖缓存中的旧信封。输入关闭而缓存仍有缺口也会 panic。
这些是原版断言行为，不能擅自改成去重或忽略缺失序号。

有类型的空载荷信封仍可有 partial_id，因此同样参与排序。它不同于没有元信息的
DummyEnvelope，也不同于关闭通道。下游已关闭时原版忽略发送错误并继续消费输入。
本章沿用这些规则，而不是为测试方便改变业务语义。

## 3. 两个状态字段足够描述算法

在 `code/flow-rs/src/builtin.rs` 添加下面的实现。BTreeMap 用序号索引信封；
seq_id 保存下一条必须输出的序号。两个字段是内部状态，标为 state，通过 Default 构造。

```rust,ignore
{{#include ../../../code/flow-rs/src/builtin.rs:reorder}}
```

每轮先接收一条，检查序号，放入缓存。随后反复 remove(seq_id)：找到就发送并前进一步，
找不到就退出本轮，等待更多输入。remove 同时取走信封，所以输出后不会继续占用缓存。
发送的就是原信封，不进行 unpack/new，全部元信息与附带 Arc 都保留下来。

原版用有序遍历和 split_off 拆出连续区间；这里逐个 remove，算法写法不同，
需要通过对照证明可观察输出相同。不要仅因两者都用了 BTreeMap 就宣布等价。

## 4. 用最小容量的真实图验证

以下配置来自端到端测试：

```rust,ignore
{{#include ../../../code/flow-rs/tests/reorder_e2e.rs:graph_config}}
```

容量 1 会很快触发背压。输入生产和输出消费应并发进行；若主任务先等待发完大量数据，
再去读取输出，节点可能卡在发送，生产方卡在输入，形成测试自身的死锁。

下面的测试特别证明“输入仍打开时就输出”：

```rust,ignore
{{#include ../../../code/flow-rs/tests/reorder_e2e.rs:streaming_prefix}}
```

graph 持有一份输入发送端，因此仅 drop 外部 input 不足以关闭输入；还需要 graph.stop()
释放图持有的发送端。之后接收 ChannelClosed 并等待图任务结束。超时用于暴露挂起，
不能用固定 sleep 代替对消息和退出结果的断言。

## 5. 对照测试证明了哪些内容

```bash
cargo test --manifest-path code/Cargo.toml -p flow-rs --test reorder_e2e --locked
```

五个测试分别覆盖六个序号的全部 720 种排列、非法和重复序号边界、完整元信息、
下游关闭，以及流式输出。对照函数位于 `tests/reference/reorder_exec.rs`，保留原版
exec 方法与许可证；测试适配器只提供队列输入和输出收集，不依赖原版私有库。

这证明被覆盖场景中的普通消息输出与成功/失败类别。它没有运行原版完整调度器，
不证明空信号屏障、flush、动态子图或 Actor(local) 的语义相同。

## 6. Sandbox 为什么要能收发完整信封

add_data 适合只关心载荷的测试，但 Reorder 必须设置 partial_id。使用
`add_envelope("inp", move |index| ...)` 返回 `Option<Envelope<T>>`：Some 发送一条，
None 结束输入。闭包在 start 时才执行，index 从 0 开始，不是直接作为消息序号。
输出端用 add_envelope_check 检查元信息，并将数量存到 `Arc<Mutex<_>>`，start 后再核对。

`Some(Envelope::<T>::empty())` 仍发送一条消息；`None` 才结束。检查器类型不匹配
必须返回错误，不能被 `while let Ok(...)` 当成正常关闭而吞掉。

```bash
cargo test --manifest-path code/Cargo.toml -p flow-rs --test sandbox_envelopes --locked
```

这些测试检查延迟生成、空载荷、类型不匹配，以及检查器失败时仍等待节点收尾。
add_data 已采用原版的闭包参数；Vec 便捷输入使用本书额外提供的 add_items。
资源与动态端口等 Sandbox 能力仍需继续对齐。

## 练习与答案

1. 输入 1、1、0，两个 1 的载荷不同，输出谁？后到的 1，因为 BTreeMap::insert 覆盖未来缓存项。
2. 输入 0、2 后关闭为何失败？seq_id 是 1，缓存的 2 无法越过缺口；不能偷偷输出 2。
3. 为什么不能输出 Envelope::new(payload)？会丢掉 partial_id、寻址与附带数据。
4. 只断言收到的序号递增够不够？不够，零条输出也可能通过；要断言确切数量、内容、关闭和任务结果。
