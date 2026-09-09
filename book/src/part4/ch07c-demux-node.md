# Ch4.7c 把 Demux 装进真实图

前面分别完成了路由函数、字典宏、标签装配和模板推导。现在将这些部件组合成可在
TOML 中使用的内置 Demux。节点业务代码很短，但每个声明都依赖前面完成的机制。

## 1. 从函数到节点

在 `flow-rs/src/builtin.rs` 加入以下实现：

```rust,ignore
{{#include ../../../code/flow-rs/src/builtin.rs:static_demux}}
```

`inp:T0` 和 `out:{T0}` 声明同一模板关系，不能改成两个独立 Any 来绕过推导。
inputs/outputs 生成字段，Node 生成输出清理，Actor 生成执行循环，BuildFromPorts
生成构造器和元数据；node_register! 才使 Builder 能按字符串 Demux 找到该节点。
Default 让空端点和空字典也有可构造状态。

exec 中的 recv_any 得到 SealedEnvelope。它读取信封地址，随后移动整份消息到选中
输出，不拆载荷、不重建元信息。HashMap::get 返回借用，因此不需要取走路由表中的
Sender；同一地址可以接收多条消息。

没有地址时 expect 会 panic；未知地址时 if let 的分支不执行，消息随作用域结束
被释放；下游关闭时 `.await.ok()` 丢弃发送错误。这些选择对照父目录原版静态
`flow-rs/src/node/demux.rs::Demux::exec`，并非通用节点的推荐错误策略。

## 2. 配置声明地址，消息选择地址

图中 `route:out:7` 把输出端点放在键 7 下；`route:out:camera:42` 把端点放在
str2addr("camera:42") 对应的键下。发送者为信封设置相同的 to_addr，Demux 才能
查到它。冒号之后的地址不会成为新的端口名，端口名仍是 out。

这一节点只有静态路由表。收到一个新的未知地址不会创建输出，也不会创建子图。
空的类型化信封仍是一条可转发消息，不会删除路由；DynDemux 的动态创建与删除
属于另一套协议，尚需后续开发。

## 3. 用真实图逐项验收

完整测试如下，可以直接运行。测试通过 Builder 注册查找和构造 Demux，而不是
在测试里手写 HashMap 后调用 route。

```rust,ignore
{{#include ../../../code/flow-rs/tests/demux_e2e.rs}}
```

在仓库根目录运行：

```sh
cargo test --manifest-path code/Cargo.toml -p flow-rs --test demux_e2e --locked
```

主测试按这个顺序验证：未知地址丢弃；字符串地址收到 42 且元信息保持；数字地址
收到空信封；关闭字符串地址的接收端后，再发送到该地址并继续向数字地址发送。
最后数字地址必须收到 7，证明节点没有因为前一次发送失败退出。

所有发送都处于有限容量通道中，接收与节点执行并发进行。超时把错误接线造成的
无限等待变成测试失败。结束时 drop 输入并释放图持有的端点，随后等待任务、检查
下游关闭，避免仅测试消息值而遗漏关闭行为。

缺地址测试期待图任务返回 TaskJoin 错误，因为当前图包装器将节点 panic 转换成
这个错误。它没有要求普通 exec 返回业务错误，也没有要求 panic 后执行 finalize。
本书当前调度与原版完整控制/异常协议尚未全部一致，应分别看待路由规则与运行时协议。

## 4. 不看答案独立复现

关闭上面的完整代码，只保留三个信息：输入名 inp、字典输出名 out、模板编号 0。
自己写出节点、注册与两路 TOML，发送一条带地址的消息。然后逐项加入未知地址、
空载荷、目标关闭和缺地址的检查。每次先预测结果再运行。

如果失败，按链路定位：找不到类型检查注册名；找不到端口检查 inp/out；缺标签
检查第三段；类型不匹配检查模板推导及转换；能收到但不结束检查输出清理和持有的
Sender 克隆。这些位置对应不同阶段，不能全部归结为“宏有问题”。

本节已完成静态 Demux 在当前 Builder 的路由接入。Sandbox 的静态字典标签接口在下节补齐；
DynDemux、原版完整关闭/flush/资源作用域等仍属于整本书后续必须完成的部分。

## 5. 在 Sandbox 中单独测试字典节点

原版 sandbox.rs 的静态端口装配对每个输入和输出调用 set_port，并统一传入 Some(0)。
因此 Sandbox::pure("Demux") 的 out 字典只有键 0。检查回调仍写 `"out"`，不能写
`"out:0"`；消息的 to_addr 才是决定是否选中这个端点的地址。

当前实现按这个规则调整 with_args：每个端口创建一条通道，将节点侧端点包装成
`TaggedEndpoint::new(endpoint, Some(0))`，调用注册表的 tagged_ctor。沙箱侧的
HashMap 仍用普通端口名保存句柄，已有回调 API 不变。输入字典同样得到键 0。

通道容量也对齐原版静态 Sandbox 的 0，即无界队列。它不代表容量为零的同步握手。
如果要验证有限容量背压或多个地址路由，应使用 Builder 图测试；本节的单端口沙箱
不能替代那些验证。

完整测试如下：第一个测试发送目标 0、99、0，检查只收到前后两条；第二个测试定义
类型化字典输入，验证可以从键 0 收到数据。两者均通过回调和节点运行验证接线。

```rust,ignore
{{#include ../../../code/flow-rs/tests/sandbox_dictionary.rs}}
```

运行：

```sh
cargo test --manifest-path code/Cargo.toml -p flow-rs --test sandbox_dictionary --locked
```

独立练习：将第一项所有消息地址改为 99，预测收集结果为空；再恢复为 0，确认消息
全部到达。随后比较图配置可以声明多个标签，而 Sandbox 固定为每个端口一条、标签
为 0。这是两种测试工具的具体装配规则，不能自行用数组序号扩展 Sandbox 标签协议。

动态端口在原版 Sandbox 中还涉及 broker 与临时图。本次仅接入静态字典，不包含
这些尚未实现的动态机制。
