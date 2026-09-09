# Ch3.2b 从端口关系推导通道类型

前面的类型选择只考虑一条连接。现在加入 `inp:T0 → out:T0` 的转发节点：如果 out
连接到 u32 接收端，那么 inp 也应关联到同一个模板结果。否则框架只看到了两条互不
相关的边，丢失了节点声明表达的关系。

本节实现当前静态压平图的推导阶段，替换上一阶段 Builder 对所有模板节点的临时
拒绝。原版全局节点、图边界模板、动态运行时仍需继续
对齐，不把本节解释成整个原版类型系统已经完成。

## 1. 区分三种身份

节点类型是可复用的定义，例如 Relay。节点实例是配置里的 a 或 b。模板编号是这个
实例内部端口之间的关系标记，例如 T0。真正的关联条件是“同一实例、同一编号”，
不是仅仅编号相同。

例如 a 的 T0 可以从邻居推导为 u32，b 的 T0 可以推导为 String。注册表仍保留
Template(0)，不能修改全局注册表来保存某个实例的结果，否则下一次建图也会受影响。
因此推导器复制本次图的端口描述，在自己的 Vec 中更新类型。

## 2. 把图变成可遍历的数据

新建 `flow-rs/src/config/type_infer.rs`，数据分成三张表：

- ports：端口的节点实例名、输入输出方向、当前消息类型。
- edges：每条连接的发送端和接收端索引。
- state：每条连接的 Pending、Visiting 或 Done(类型)。

usize 索引是 Vec 中的位置，不是业务地址标签。字典地址 7、42 不参与模板身份；
同一个字典端口上的所有接线都引用相同端口描述。

解析阶段先建立 `(节点名, 端口名, 方向) → 索引` 的 HashMap。对外输入只能指向
节点输入，对外输出只能指向节点输出；内部连接根据注册表判定方向。空连接、未知
节点、未知端口、类型列表长度不匹配都必须在创建队列前报错。

## 3. 为什么需要 Visiting

推导一条连接时，模板可能要求先看另一端口的连接；那条连接又可能回来看当前连接。
这是推导依赖的环，并不自动说明业务数据流非法。如果没有访问标记，递归永远结束不了。

visit 的第一步是：已处理或正在处理就返回；否则把当前连接标为 Visiting。递归返回
后再收集当前端口类型，调用已有 guess_channel_type。成功后记为 Done，并把选出的
类型写入同一实例、同一编号的模板端口。

这里“正在处理就返回”只是避免重复深入，不代表已经得到具体类型。真正的结果只
存在 Done 中。这对应原版 infer_conn 先写入临时标记的做法。

## 4. 按顺序写 visit

建议按以下顺序独立实现，再对照源文件：

1. 检查状态并设置 Visiting。
2. 收集本连接的端口索引，避免在递归修改 self 时仍借用它的 edges。
3. 对模板端口查找同一节点的同编号关联端口，尝试递归推导其连接。
4. 清除当前连接的临时状态，收集两侧类型 HashSet，调用 guess_channel_type。
5. 记录成功结果，并传播到仍为该模板的端口。

先收集索引再递归是 Rust 借用规则在算法设计中的实际用途。我们拥有整个推导器的
`&mut self`，不需要照抄原版通过裸指针同时修改配置的方式。复制的是少量索引，
不是消息或通道队列。

完整实现如下：

```rust,ignore
{{#include ../../../code/flow-rs/src/config/type_infer.rs}}
```

## 5. 底层错误与图层回退

guess_channel_type 不凭空选择 Rust 类型。所有端点都是模板时，它返回
TemplateInferFault。原版 infer_graph 捕获这一类错误，将该连接记录为 Any；当前
静态推导层保留这个处理。其他错误继续返回，不能把 ChannelTypeMismatch 也吞掉。

这一回退只记录当前连接，并不把节点的所有模板统一改成 Any。这一点会影响之后
处理相关连接时还能否看到模板关系。

当前按配置中的稳定顺序遍历连接，原版内部使用 HashMap，存在多个有效转换候选时
不保证相同的遍历顺序。本节用唯一具体类型验证传播，不据此宣称多候选选择顺序已
与原版逐项相同。

## 6. Builder 使用结果，而不是重新猜

infer 返回 InferredGraph，其中 connections 依次对应图输入、图输出、内部连接，
ports 按实例名、端口名、方向保存推导后的端口类型。Builder 在创建任何队列前
先完成 infer，然后按相同顺序取类型创建通道。不能在每条边装配时重新查询未解析的
注册表，否则推导结果又被丢掉。

这种“位置相同就对应”的接口是当前阶段的实现方式。增加连接类别或重排连接时，
必须同时调整生产和消费顺序；后续完整中间层可用连接标识保存结果。

## 7. 用运行结果验证传播

下面测试创建两个独立 Relay 实例，分别接到 u32 和 String 节点。入口的 chan_tid
必须不同，实际消息也必须分别到达。另一个测试让一个模板节点两侧受到不兼容的
具体类型约束，要求建图失败。

```rust,ignore
{{#include ../../../code/flow-rs/tests/template_inference.rs}}
```

运行：

```sh
cargo test --manifest-path code/Cargo.toml -p flow-rs --test template_inference --test template_metadata --locked
```

独立练习：在数字链路中再插入一个 Relay，预测图入口类型后运行；再将两个实例名
改成不同的名字，确认编号相同仍不会关联。最后构造只有模板的连接，检查回退 Any，
不要期待编译器替框架选一个整数类型。

## 8. 把推导结果交给端点的 with_type

只保存连接类型还不够：队列搬运 Raw，不代表节点也应该收到 Raw。模板可能已经被
下游约束推导为 Stored，这时接收端必须在交给 exec 之前转换。

原版 Sender::with_type 查询“端口类型 → 通道类型”，Receiver::with_type 查询
“通道类型 → 端口类型”。现在 channel.rs 也提供这两个入口；SenderT/ReceiverT 的
From 实现复用它们，避免维护两套方向不同的查询逻辑。

```rust,ignore
// Sender 内部
self.conversion = conversion::lookup(*port_type, self.chan_tid());
// Receiver 内部
self.conversion = conversion::lookup(self.chan_tid(), *port_type);
```

with_type 缓存转换函数，不马上转换消息。真正的转换仍在 send/recv 时执行；这保留了
原有转换线程、元信息和取消行为。重新调用 with_type 会重新查询并替换缓存。它也
不会验证用户之后发送的每个动态载荷，不能把类型描述当作 Rust 编译器的静态保证。

推导器现在返回两份结果：每条连接的类型，以及每个端口最终的类型。Builder 克隆
某个端点后、放入 TaggedEndpoint 前调用 with_type，传入该节点实例的端口结果。
对外输入、对外输出、内部连接两端都要做；不能只修改内部连接。

模板编号依旧保留在注册表里。InferredGraph 中保存的是本次图的结果，下一次构造
另一个图时会重新计算。未类型化端点的 port_tid 仍按原版返回 Any；with_type 的
职责是设置转换，不是把端点变成 SenderT 或 ReceiverT。

## 9. 设计一个确实需要接收端转换的测试

如果队列偶然选择了 Stored，测试即使漏掉模板接收端的 with_type，也可能通过。
因此需要强制选择 Raw，不能依赖 HashSet 恰好选中哪一个候选。

下面在同一个图输入上添加一个 Raw 类型约束端口。转换表只有 Raw→Stored，没有
Stored→Raw，因此通道唯一候选是 Raw。模板节点的另一个端口连接 Stored 接收端，
让这个实例的 T0 推导为 Stored。模板节点的 exec 明确接收 Stored；遗漏 with_type
就会在这里得到类型不匹配。

RawConstraint 是测试探针：声明 Raw 输入后立即结束，不消费共享队列。它不会与
模板节点竞争数据，因此测试不依赖调度公平性。不能把这种探针当成生产业务节点。

```rust,ignore
{{#include ../../../code/flow-rs/tests/template_conversion.rs}}
```

运行：

```sh
cargo test --manifest-path code/Cargo.toml -p flow-rs --test template_conversion --locked
```

Raw(41) 转成 Stored(42)，partial_id 保留为 9。另一个测试直接检查未类型化 Sender
设置 with_type 后按发送方向转换。两项分别检查 Builder 的真实接收链路与发送接口。

独立练习：临时删去 Builder 的接收端 with_type，确认第一项失败，再恢复；然后交换
Sender 查询的两个类型，确认发送方向测试失败。这样你能解释为什么代码里的参数
顺序会改变业务行为。

## 10. 剩余的完整框架要求

当前处理的是静态压平后的节点实例。原版 archives、全局节点、图端口模板与动态
子图实例不在本节实现内，仍属于最终框架必须完成的要求。模板转换的多候选路径、
完整存储/控制协议也不能只靠本节这两个测试就宣布全部对齐。
