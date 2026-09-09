# Ch4.7b 实现字典端口宏与按地址装配

现在构造器能收到标签，可以把 `#[outputs(out:{})]` 真正接进框架了。本节完成字典
字段、类型识别、构造、注册和关闭路径，并在 TOML 图里运行一个路由节点。

先限定这一开发步骤的输入：`{}` 表示任意消息，`{u32}` 等表示具体 Rust 类型。
原版 `{T0}` 还需要模板元数据和图内模板约束传播；本节不会把 T0 偷换成 Any。
因此测试节点叫 Router，尚不能将它宣布为完整迁移的原版 Demux。

## 1. 给属性宏增加花括号分支

打开 `flow-derive/src/node.rs`。给 PortSpec 增加 dict 标记。读取冒号后，先用
`input.peek(syn::token::Brace)` 判断是不是字典声明，再用 braced! 取得内部游标。

内部为空就记录 payload=None，否则解析一个 syn::Type。最后检查内部游标为空。
这一步让 `{Result<u32, String>}` 合法，让 `{u32, String}` 报错：前者是一个完整
Rust 类型，后者是两个并列类型，不符合端口语言规则。

字段生成仍分两步：具体类型使用 SenderT/ReceiverT；无类型使用 Sender/Receiver。
随后把端点放入 `std::collections::HashMap<u64, Endpoint>`。字典输入和输出都没有
外层 Option，空字典就是尚未接线的状态。

这里暂时在原有 array 标记旁增加 dict。动态形态和模板接入时还需要进一步统一
PortInfo 元数据，不能把多个布尔值当作最终完整端口模型。

## 2. 派生宏怎样知道字段是端口

属性宏完成后，派生宏看到的是普通结构体字段。增加 dict_value，逐层检查：

1. 字段必须是 Type::Path，末段是 HashMap。
2. 泛型实参必须恰好两个，第一个是 u64 类型。
3. 第二个是 Receiver、`ReceiverT<T>`、Sender 或 `SenderT<T>`，才认作端口。

不能用 `to_string().contains("Sender")`，否则业务字段 HistorySender 会被误判。
`HashMap<String, Sender>` 也不能按地址端口处理：框架接收到的是 u64 地址键。
当前按路径末段识别，类型别名尚不自动展开；过程宏看到的是语法，不能直接要求编译器
替它完成类型解析。

新增 InputDict、OutputDict 两种 PortKind。生成消息类型描述时，先剥开 HashMap，
再读内部 ReceiverT/SenderT 的载荷类型。这样 Builder 对 `{u32}` 看到的是 u32，
不是 HashMap 的 TypeId；`{}` 则是 Any。

## 3. 两套构造入口怎样生成

含字典的节点覆盖 build_tagged。对一个字典输出字段，核心代码是：

```rust,ignore
out: outs.remove(0).into_iter().map(|p| {
    (p.tag.expect("dict port need a tag"), p.endpoint.into())
}).collect()
```

逐步读这段表达式：remove(0) 移出一个端口组；into_iter 消费组；map 把每个元素
变成“键、值”二元组；collect 根据字段要求收集成 HashMap。endpoint.into() 将
未类型化端点转换成需要的 SenderT，保留现有通道转换机制。

标签缺失时 expect 会 panic，与原版 set_f 的字典分支一致。同键再次出现时后写
覆盖前写，旧端点被释放。这里不是多值字典，也不会向同键的两个下游广播。

在同一个 build_tagged 中，普通输入取 `.endpoint.into()`，普通输出包 Some，
列表把元素的 endpoint 收集成 Vec，业务参数与 state 字段保持原来的初始化规则。
不能只实现字典字段：一个 Router 同时有普通输入和字典输出。

没有标签的旧 build 入口对含字典节点会包装 None 后调用 build_tagged，因此已接线
字典不会凭空获得地址；缺标签就失败。没有字典的旧节点保留原来生成的 build。

## 4. 注册表、Builder 和 close 一起修改

BuildFromPorts 新增 INPUT_DICT、OUTPUT_DICT，默认空切片方便已有手写实现继续
使用。派生实现为每个端口生成相应标记，node_register! 将标记交给 NodeRegistration。
查询函数使用 get，手写实现没有字典标记时返回 false。

Builder 的两个判断都要更新：字典允许积累多个已接线端点；未接线字典保持空组，
不应像标量一样补一个无标签默认端点。漏掉第二项会让没有连接的字典在构造时 panic。
列表标记与字典标记分别保存，不把字典伪装成 Vec。

最后在 derive(Node) 的 close 中对字典输出调用 clear。HashMap 被清空后，其中的
Sender 都被释放，下游才会结束。只补接线而忘记关闭，正常数据能收到，图却可能无法
结束；所以验收必须包含关闭，而不能只检查输出值。

## 5. 编写并运行完整验证

下面的集成测试包含普通 Router 和 TypedDictionary 两种节点。Router 原样移动封箱
消息，按 to_addr 查找字典；TypedDictionary 从地址 7 收 u32，在地址 42 发 String。

```rust,ignore
{{#include ../../../code/flow-rs/tests/dict_ports.rs}}
```

运行：

```sh
cargo test --manifest-path code/Cargo.toml -p flow-rs --test dict_ports --locked
cargo test --manifest-path code/Cargo.toml --workspace --locked
```

第一项验证四件独立的事：按 7/42 路由并丢弃未知地址 99、类型化输入输出、同键覆盖
释放旧端点、已连接字典缺标签时失败。关闭断言同时约束宏生成的 close。第二项确认
标量、数组、注册、类型转换及前面章节仍能协作。

同键测试为什么先观察旧通道关闭，再观察新通道超时？旧通道关闭说明旧 Sender
已经释放；新通道超时但没有关闭，说明它仍由节点持有；最后 drop 节点，新通道才
关闭。这比仅仅断言“构造没有报错”更能证明所有权行为。

## 6. 独立重做与下一步

把 Router 的地址从 7/42 改为不连续的大整数，确认不用扩容到那个下标。再把输出
标签换成字符串，并用同一个 str2addr 给消息设置 to_addr，验证字符串标签路由。
接着临时删除 close 的字典分支，观察关闭检查能否发现问题，最后恢复它。

本节之后字典装配已在 Builder 可用。后续必须继续完成 T0 模板关联、原版 Demux
注册及生命周期协议、Sandbox 的标签输入接口、带标签的子图边界和动态端口。
这些都属于最终框架要求；本节的 Router 是验证已完成链路的具体节点，不是终点。

模板编号的宏阶段已在[宏第 11 课第 7 节](../macros/11-port-grammar.md#7-把模板编号带过两个宏阶段)
接入；静态跨连接推导见 [Ch3.2b](../part3/ch02b-template-inference.md)，该节也已接入端点转换装配；完整作用域协议仍待补齐。

现在继续 [Ch4.7c](ch07c-demux-node.md)，用同一套模板和字典机制注册真正的静态 Demux。
