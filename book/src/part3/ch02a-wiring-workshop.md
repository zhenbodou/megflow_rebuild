# Ch3.2a 接线实作：把名字变成真实端点

这一节的目标是亲手完成 Builder 最关键的中间步骤：根据配置创建队列，按端口名字收集端点，再按注册表顺序交给构造器。前置工程应已有 Ch3.1 的配置、Part 1 的 channel 和 Part 2 的节点注册表。这里开始把这些模块接起来，不再另造一个与框架无关的运行时。

## 1. 先在纸上接一张减法图

节点声明 `#[inputs(a, b)]`，业务是 `a - b`，输出叫 `c`。故意将图的输入配置倒着写：

```toml
inputs = [
    {name="right", cap=1, ports=["subtract:b"]},
    {name="left", cap=1, ports=["subtract:a"]}
]
outputs = [{name="answer", cap=1, ports=["subtract:c"]}]
```

必须区分三种名字：`left` 是调用者找图输入时使用的名字，`subtract` 是节点实例名，`a` 是节点字段名。调用 `graph.input("left")` 发送 10，应进入节点字段 `a`，与 TOML 中这行排第几无关。

| 新建的队列 | 发送端归谁 | 接收端归谁 |
| --- | --- | --- |
| left | 图的 inputs 表，调用者再克隆 | subtract 的 a 字段 |
| right | 图的 inputs 表，调用者再克隆 | subtract 的 b 字段 |
| answer | subtract 的 c 字段 | 图的 outputs 表，调用者取走 |

先写出这个表，再写 Rust。方向弄反时，程序可能仍然编译，因为多个字段的类型相同。

## 2. 创建中间表，暂时不要调用构造器

在 `graph.rs::MainGraph::assemble` 中准备：

```rust,ignore
let mut node_ins: HashMap<String, HashMap<String, Vec<Receiver>>> = HashMap::new();
let mut node_outs: HashMap<String, HashMap<String, Vec<Sender>>> = HashMap::new();
```

从外往内读：节点名字 → 端口名字 → 接到这个端口上的端点列表。最后一层之所以是 `Vec`，是因为后面的数组端口允许接多条队列。标量端口最多接一条；数组端口可以接零条或多条。不要用“一个端口名字”等同于“一个消息队列”。

遍历 `g.inputs`，每个图输入新建一次 `channel(pc.cap)`。把 `tx` 放进图的输入表；遍历该输入的 `ports`，拆开节点名与端口名，将 `rx.clone()` 挂到对应节点的中间表中。多个接收端克隆竞争同一队列，消息不会自动广播。

`attach_receiver` 里的链式操作可以拆成：

```rust,ignore
let ports = node_ins.entry(node.to_owned()).or_default();
let group = ports.entry(port.to_owned()).or_default();
```

第一行查节点，缺少时插入空端口表；第二行查端口，缺少时插入空端点列表。`entry` 提供“查找或插入”的入口，`or_default` 返回表内值的可变引用。此时检查标量端口的 `group` 是否已经非空，再 `push(rx)`，才能发现重复接线。

对输出重复同样过程，但收集的是发送端。对声明了 `ports=[]` 的图输入和图输出应返回 `BadConnection`：声明了一条边界连接，却没有任何内部端点。原版 `config/mod.rs::translate_conn` 也拒绝空连接。

## 3. 按注册表顺序移动端点

配置顺序可以变，注册条目的 `inputs = &["a", "b"]` 决定构造器顺序。不要遍历 `HashMap` 的值来构造节点：哈希表的迭代顺序不是字段声明顺序。

当前工程的输入收集核心是：

```rust,ignore
let mut ins = Vec::with_capacity(reg.inputs.len());
for &port in reg.inputs {
    let mut group = ins_map.remove(port).unwrap_or_default();
    if group.is_empty() && !reg.input_is_array(port) {
        group.push(Receiver::default());
    }
    ins.push(group);
}
```

`for &port` 将迭代产生的双层引用解开一层，得到端口名 `&str`。`remove` 把端点所有权从中间表转交出来；`get` 只借用，不能把端点直接搬进新节点。`ins.push(group)` 后，端点组归 `ins` 持有。

`Vec::with_capacity` 只预留空间，没有插入任何元素，`ins.len()` 此时仍从 0 开始。每处理一个注册端口，外层长度才增加 1。输出表同理，把缺省标量补成 `Sender::default()`。

最后检查中间表是否有剩余键：配置引用了注册表不存在的端口时，不能默默丢弃它。然后调用 `(reg.ctor)(&nd.args, ins, outs)?`。其中 `reg.ctor` 是函数指针；括号表示调用存储在字段中的函数，而不是调用一个名叫 ctor 的方法。

## 4. 为什么未接线可以构造成功

原版 `config/postprocess/conn_check.rs` 对未接线端口记录警告，继续返回成功。此前本书将它一律改成 `PortNotConnected`，改变了原版接受的配置范围；当前 Builder 已纠正。

默认端点不是容量为 0 的 channel。`channel(0)` 是真正的无界队列，`Receiver::default()` 则没有底层队列。

| 状态 | 收消息 | 发消息 |
| --- | --- | --- |
| 默认未接线端点 | 立即报告关闭 | 丢弃消息并成功返回 |
| 真正的队列 | 等待消息，发送端全部释放且队列耗尽后关闭 | 正常入队；接收端关闭后失败 |

因此“节点缺少输入 b”和“配置引用不存在的端口 z”不同：前者保留已声明字段的默认值，后者是无效引用。当前重构还没有复刻原版的未接线警告日志；行为测试只覆盖默认端点及建图结果。

构造宏为何仍能执行 `ins.remove(0).remove(0)`？因为 Builder 给每个未接线标量填了**一个默认端点**，不是交给它空列表。数组端口则保留空列表，宏直接搬走整组。直接调用构造器时也要满足这个分组约定。

## 5. 用一个能发现接反的测试验收

把下面测试放在 `code/flow-rs/tests/graph_builder.rs`，沿用该文件前面已注册的 `TestBinaryOp`。如果你在自己的逐章工程操作，先确保这个节点的发送使用 `ea.repack(r)`，否则会丢失左输入的元信息。

```rust,ignore
{{#include ../../../code/flow-rs/tests/graph_builder.rs:wiring_by_name}}
```

在仓库根目录运行：

```sh
cargo test --manifest-path code/Cargo.toml -p flow-rs --test graph_builder --locked
```

验收的两条证据各有目的：结果必须是 `10 - 3 = 7`，接反会得到 `-7`；`partial_id` 必须来自左输入的 42，不能是右输入的 99。原先只验证 `1 + 2 = 3` 无法识别两个输入交换。

`timeout` 用来使错误接线造成的等待有明确失败上限，不用于证明性能。最后释放调用者的发送端，再 `graph.stop()` 释放图持有的发送端，等任务结束。只释放其中一处还可能让节点一直等下一条消息。

## 6. 独立修改与下一步

先不看答案做两项修改：将 `left/right` 的 TOML 顺序交换，结果仍应为 7；将两处 `ports` 的目标交换，结果应变成 -7，元信息应来自新接到 a 的输入。这能证明你理解的是名字映射，而不是碰巧记住一段顺序。

再读取 `subgraph::flatten`，追踪 `branch:input` 如何变成 `branch/leaf:inp`。这一步发生在上述接线之前，只重写配置，没有创建队列。当前静态压平仍不能覆盖原版子图运行时、资源作用域和动态实例协议，这些是后续必须补齐的开发内容。

## 7. 将类型选择真正接入 Builder

前面的按名接线解决“接到哪个字段”，类型选择解决“队列用什么类型，哪里执行转换”。
先完成 Ch1.4d 的选择算法，以及宏专题第 10 课的 input_types/output_types，再修改 graph.rs。

新增 `connection_type(g, tx, rx)`。它接收已经按方向拆开的端口引用，不要重新猜测方向：

1. 用节点实例名查注册项。
2. 发送端查 outputs，接收端查 inputs，找到端口名对应的下标。
3. 调用同方向的类型信息函数，按下标取 MsgTypeId。
4. 将发送与接收类型分别收集成 HashSet，交给 guess_channel_type。

这里 `Result<HashSet<_>>` 的收集有两个作用：相同类型自动去重，任意一次查找失败则返回
错误。若手写注册项的类型列表比名字列表短，也应明确报元信息长度不匹配，而不是越界 panic。

接着修改三处创建队列的位置：

| 连接来源 | 发送类型取自 | 接收类型取自 |
| --- | --- | --- |
| 图输入 | 空集合，调用者还没有端点声明 | ports 指向的节点输入 |
| 图输出 | ports 指向的节点输出 | 空集合 |
| 内部连接 | 已分类的发送端引用 | 已分类的接收端引用 |

得到类型后，将 `channel(cap)` 改为 `channel_with_type(cap, ty)`。必须先选类型再创建队列，
随后构造器生成的 `.into()` 才能按正确方向查转换表。只修改注册宏却仍调用旧 channel，
队列类型一直是 Any，普通具体类型对的转换便不会按预期被选中。

图边界仍允许调用者拿到无类型 Sender/Receiver。它们不会因为调用了泛型 send/recv 就
自动重新查转换表；需要异类型转换时应使用类型化端点包装。这不等于任意载荷都可以安全发送。

### 用完整 TOML 图验收

`tests/graph_conversion.rs` 定义两个节点：Source 接收并输出 Number，Sink 接收并输出 Text。
转换属性宏登记 Number → Text，TOML 仅声明 source:out 与 sink:inp 相连，不手动指定队列类型。

运行：

```sh
cargo test --manifest-path code/Cargo.toml -p flow-rs --test graph_conversion --locked
```

测试发送 Number(42)，应收到 Text("42")，partial_id 仍为 7。若队列类型选 Number，转换在
接收侧执行；若选 Text，转换在发送侧执行。原版候选遍历不保证这两个合法类型的固定优先级，
测试因此检查可观察消息结果，不硬编码转换必须发生在哪一侧。

再将两个节点类型对调，内部连接变成 Text → Number。因为没有登记反向转换，build 应返回
ChannelTypeMismatch；不应等 start 后再通过 downcast panic 才发现。这个检查只针对当前已有
端口描述与直接转换关系，不能替代原版跨连接模板变量传播、动态端口和共享子图类型协议。

独立练习：手写反向转换并登记，解释它对非法文本如何处理，再观察反向图是否能建图。
不要将字符串解析的业务错误与建图的类型兼容问题混为一谈：登记函数存在，只证明可调用，
不能证明每个实际输入值都能成功解析。

### 混合类型实作：一条边界连接不等于一个节点

再用一个稍复杂的图检验你是否理解类型约束。number 节点原样传递 Number，text 节点
原样传递 Text；只登记 Number → Text。让它们共同竞争一个图输入，并汇入一个图输出：

```toml
inputs = [{name="in", cap=1, ports=["number:inp", "text:inp"]}]
outputs = [{name="out", cap=1, ports=["number:out", "text:out"]}]
```

先不要运行，手算两条队列的类型：

- 输入队列只能选 Number。Number 消费者直接接收，Text 消费者用 Number → Text 转换；
  若选择 Text，Number 消费者就需要尚未登记的反向转换。
- 输出队列只能选 Text。Number 生产者转换后入队，Text 生产者直接入队；
  若选择 Number，就要求 Text 生产者反向转换。

这里候选是唯一的，因此测试可以明确断言 `input.chan_tid()` 为 Number，
`output.chan_tid()` 为 Text。和上面的单生产者、单消费者例子相比，多端点增加了约束，
缩小了可行类型集合。

接下来从图输入发送 0 到 99，每条消息的 partial_id 等于数值。每条消息只被一个节点
取走：可能直接走 number，也可能转换后走 text。最终输出统一是 Text，结果排序后必须
恰好是 0 到 99，partial_id 与解析出的数字相等。不能要求两条支路分别收到 50 条，
队列竞争不提供这种分配保证，也不能要求输出严格保持输入顺序。

现有测试 `mixed_boundary_types_compete_without_losing_or_duplicating_messages` 已实现
这些断言，和前面两项测试一起运行。容量设为 1，生产和消费并行执行；若先等发送 100 条
结束再开始读取输出，背压可能让测试永远无法走到读取阶段。

注意测试在取走输入、输出句柄后调用 graph.stop()，但生产者仍持有输入 Sender，
所以图不会立即停止。生产者完成并释放最后一个 Sender 后，关闭才沿节点输出传播。
“调用了 stop”与“全部节点已停止”不是同一个事件，需要等待聚合任务确认结束。

这项实验覆盖当前类型转换与多消费者共享队列的组合；没有证明负载公平、全局顺序或
动态子图协议。验收时应该分别写出这些要求，不能用“100 条都收到了”替代所有并发语义。
