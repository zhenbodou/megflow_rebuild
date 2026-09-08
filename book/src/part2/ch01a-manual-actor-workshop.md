# Ch2.1a 手写节点实作：从收发循环到生命周期

起点是上一章的异步三步实验和已经实现的 flow-message。本课不使用任何 MegFlow
节点生成宏；Tokio 的 main 宏仍用于启动运行时。终点是一个手写节点：将正整数翻倍，
保留消息序号，遇到负数返回业务错误，并在普通成功/错误路径都执行关闭和 finalize。
负数错误是本课故意设置的练习规则，不是原版 BinaryOp 的业务规则。

## 第 1 步：先列出节点必须记住什么

```rust,ignore
{{#include ../../../code/flow-rs/examples/manual_actor_steps.rs:state}}
```

inp 负责接收，out 负责发送。out 使用 Option，是为了 close 时取代为 None，释放原
发送端。input_closed 记录输入已经结束；events 仅用于观察生命周期，不参与业务。
它是 `Arc<Mutex<Vec<&'static str>>>`：共享一个事件列表，持锁期间只 push，不跨 await。
这里用标准库 Mutex，因为锁内没有异步等待，不能因此推断所有异步场景都应使用普通锁。

## 第 2 步：把“一次处理”和“反复处理”分开

```rust,ignore
{{#include ../../../code/flow-rs/examples/manual_actor_steps.rs:business}}
```

exec 每次最多处理一条。收到 None 时只记录结束，不产生业务输出；收到值时先记录
事件，取出载荷，检查练习规定的错误，再用 repack 保留元信息并发送。
`map_err` 把 Tokio 发送错误转为本课的静态字符串错误，`?` 将它返回给 exec 的调用者。

initialize/finalize 现在只记录事件。先把调用次数与顺序做对，再在资源章节加入真正
的模型、计数器或其他资源。finalize 中断言 out 为 None，明确要求先关闭输出。

## 第 3 步：定义调度器需要的最小接口

```rust,ignore
{{#include ../../../code/flow-rs/examples/manual_actor_steps.rs:contracts}}
```

Node 回答“能否继续”和“如何关闭”。Actor 的 start 接受 `Box<Self>`，把节点所有权
交给任务，避免任务还没完成，外部却销毁节点。Send 是可在线程间移动的要求；'static
表示任务不借用寿命不够长的外部数据，不是要求节点永远不释放。

返回值是 `JoinHandle<Result<()>>`。等待句柄先检查任务是否 panic/取消，得到的内层
Result 才是业务成败。同步的 start 可以放进 dyn Actor 接口；真正异步的循环放入任务。
本阶段接口不带 Context，后续资源章节再扩展，不能与最终代码签名混用。

## 第 4 步：手写生命周期，特别检查问号的返回边界

```rust,ignore
{{#include ../../../code/flow-rs/examples/manual_actor_steps.rs:lifecycle}}
```

最容易写错的是直接在最外层 async 块中写 `self.exec().await?`。业务出错时，问号
会立刻返回，后面的 close/finalize 根本不会执行。

这里把循环包进内层 async，得到 result 后先存起来。错误只提前退出内层循环；
外层继续 close、finalize，最后把原业务结果交给等待句柄的调用者。这保证普通 Result
错误收尾，不保证 panic 或任务强制取消时还能异步 finalize；后两者需要另外的机制。

练习预测：输入 1、-1、3 会记录几个 exec？两个。负数那次被调用并返回错误，3 不再
进入业务处理。不要把“输入已经送入队列”误认为“业务一定已经执行”。

## 第 5 步：手工装配，暂时不用注册表和 TOML

```rust,ignore
{{#include ../../../code/flow-rs/examples/manual_actor_steps.rs:assemble}}
```

两条容量 1 的队列分别连接生产方→节点、节点→消费方。端点的所有权分配完后，
把具体 Doubler 装成 `Box<dyn Actor>`，模拟未来图中存放不同节点的方式。

生产任务与消费循环并发推进，避免背压死锁。生产结束释放 source；节点因此收到
None，关闭 out；消费循环随后结束。最后等待两个任务并核对结果，不能只核对收到的
每个值而忘记确切数量。

## 完整文件与运行

在自己的工程中创建 `examples/manual_actor_steps.rs`，完整内容如下：

```rust,ignore
{{#include ../../../code/flow-rs/examples/manual_actor_steps.rs}}
```

它只导入标准库、已实现的 flow-message 和 Tokio，不导入 flow-rs 的 Node/Actor。
所需依赖为 flow-message 的本地路径，以及开启 rt、macros、sync、time 的 Tokio。
在本仓库根目录运行参考终点：

```bash
cargo run --manifest-path code/Cargo.toml -p flow-rs --example manual_actor_steps --locked
```

预期输出 `手写节点通过：正常处理、元信息、错误传播、关闭与 finalize`。
程序检查成功和失败两条路径，并设置三秒超时。对应测试不是要求机器必须三秒内完成
业务，而是为这个有限输入的小实验暴露未关闭端点造成的挂起。

## 合上书后独立完成

1. 增加输入 0，它应输出 0，序号保持不变。理由：本课只拒绝负数。
2. 将错误规则改为只拒绝 2，先写新的期待输出与事件，再修改 exec；不要修改生命周期。
3. 故意把问号移到最外层，失败路径中会缺少 close/finalize 事件。任务销毁可能仍使输出
   关闭，因此只检查接收端退出无法证明 finalize 执行过。
4. 将 Doubler 改成整数转字符串节点：修改输出端点类型与处理函数，保留生命周期。
   若你必须重写 start 循环，说明业务和调度边界还没分清。

进入宏章节前，请能指出哪些代码因业务变化而变化，哪些在不同节点之间重复。
宏的目标是生成后者，并通过相同的输入、元信息和生命周期测试证明没有改变行为。

维护检查命令为 `python3 scripts/check_manual_node_course.py`。它创建新工程，只依赖
前面已经实现的消息层和 Tokio，不依赖最终 flow-rs。嵌套导出的消息工程有自己的
workspace，父工程用 exclude 排除它，避免 Cargo 报多个工作区根；offline 模式要求
依赖已经下载。这个工作区问题与节点逻辑无关，排错时要先区分构建配置和 Rust 代码。
