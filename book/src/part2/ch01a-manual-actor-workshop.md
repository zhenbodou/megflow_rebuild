# Ch2.1a 在已有节点上做排错实验

起点是 Ch2.1 第十五步的同一工程。本课不创建另一个使用 Tokio 裸队列的节点例子，也不引入另一套 Node/Actor 接口。先运行 `cargo test --offline --lib node::tests`，确认三个测试通过，再开始修改副本。

## 实验一：一行问号为什么改变收尾

在 src/node.rs 的 start 中找到内层 async。它的作用不是并发，而是建立一个 Result 提前返回的边界。

先画出两种路径：成功时循环结束后保存 Ok；失败时问号结束内层 async 并保存 Err。两条路径都会继续 close、finalize。

在副本中故意去掉内层 async，让 exec 的问号直接位于最外层任务。只运行：

```bash
cargo test --offline --lib node::tests::send_error_still_closes_and_finalizes
```

预期事件列表断言失败：你会得到 initialize、exec，却缺少 close 和 finalize。即使任务销毁时释放了输出，也不能因此说 finalize 已执行。恢复上一课完整文件后，这项测试应重新通过。

## 实验二：看懂 JoinHandle 中的两层结果

上一课 Actor::start 返回 `JoinHandle<Result<()>>`。第一次 await 处理任务是否成功结束，得到 Tokio 的 `Result<Result<()>, JoinError>`。

| 结果 | 含义 |
|---|---|
| Ok(Ok(())) | 任务结束，节点业务也成功 |
| Ok(Err(error)) | 任务正常返回一个业务错误 |
| Err(join_error) | 任务 panic 或被取消，未正常返回业务结果 |

错误测试只 unwrap 外层，再 match 内层 ChannelClosed；如果写两次 unwrap，正常的业务错误会令测试直接 panic，根本走不到后面的生命周期断言。

独立练习：把 exec 的业务计算暂时改为 panic，观察 start 返回的句柄结果；不要把这个结果改写成普通 ChannelClosed。恢复后再运行全部测试。原版运行时的错误包装仍需在调度器章节对照，本课只解释当前明确的两层类型。

## 实验三：元信息是业务结果的一部分

第一项测试不只检查 14，还检查 partial_id。将 repack 换成 Envelope::new 后再运行这项测试，数值正确而序号错误，仍应判定失败。

独立练习：给输入再设置 to_addr，补上输出地址不变的断言。你需要修改测试的构造与断言，不应修改 exec 的转发代码；repack 已经负责整份元信息。

## 实验四：背压与退出

成功测试为什么把生产循环放入 tokio::spawn？容量只有 1，如果同一个任务先发完三条再接收，节点可能被输出队列阻塞，而主任务又被输入队列阻塞，形成互相等待。

先画出三者：生产者 → 节点 → 消费者。然后在不修改节点接口的前提下，将容量改为 2，输出集合和事件列表应保持不变。不能依据任务调度顺序要求某个线程先执行。

合上书后独立实现一个加一节点：复制 Doubler 的整体结构，修改业务计算和预期值，保留输入关闭、输出关闭与普通错误收尾。指出哪些部分需要重复生成，这才是下一章引入宏的实际需求。

本课不增加文件或依赖；恢复后仍以第十五步完整文件为基准。此前独立的 manual_actor_steps 示例只作为旧实验留在仓库，不再承担本书主线的阶段验收。
