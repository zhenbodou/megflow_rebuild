# Ch4.7a 让地址标签真正到达构造器

上一节的 HashMap 是手写的。要让 TOML 驱动 Demux，必须先解决一个数据丢失问题：
`Vec<Vec<Sender>>` 保存了分组和端点，却没有保存字典的键。仅改宏生成 HashMap 不够，
因为构造节点时已经不知道哪个 Sender 对应地址 7。

这一节直接改框架的配置解析、Builder、注册表和子图展开，并通过真实消息验证。
暂时仍使用标量端口探针来观察标签；字典宏及按键装配是后续必须接上的工作。

## 1. 用一个结构体把标签和端点绑在一起

在 `flow-rs/src/registry.rs` 增加：

```rust,ignore
pub struct TaggedEndpoint<T> {
    pub tag: Option<u64>,
    pub endpoint: T,
}
```

这里的 T 是普通 Rust 泛型，与宏语言里的 T0 模板编号不是同一个概念。
`TaggedEndpoint<Sender>` 装发送端，`TaggedEndpoint<Receiver>` 装接收端。Option
让“没有标签”和“标签是 0”保持区别。地址不是消息载荷，也不属于 Sender 本身；同一
条队列接到不同节点时，各端口的标签可以不同。

外层分组依旧按端口声明顺序排列，内层元素改成 TaggedEndpoint。不要额外维护一份
平行的标签 Vec：移动、过滤或重排端点时，很容易忘记同步标签。

## 2. 新旧构造入口怎样共存

给 BuildFromPorts 增加 `build_tagged`，参数是两份带标签的分组。默认实现逐层
into_iter，把每个元素的 endpoint 移出来，再调用已有 build。

这段默认转换的含义是“当前标量和列表不使用地址键”。它不能用于字典端口，因为
字典需要标签。字典实现必须覆盖 build_tagged，读取 tag 后插入 HashMap。原版
`flow-derive/src/node.rs::set_f` 的字典分支使用 `tag.expect("dict port need a tag")`
和 insert；标签缺失会失败，相同键再次插入会替换旧值。后续不能擅自按端点下标生成键。

在 NodeRegistration 中同时保存 ctor 和 tagged_ctor。node_register! 的展开增加：

```rust,ignore
tagged_ctor: <NodeType as flow_rs::registry::BuildFromPorts>::build_tagged,
```

这是函数指针，并没有在注册时构造节点。Builder 改为调用 tagged_ctor；现有 Sandbox
仍使用旧 ctor，所以本节不能证明 Sandbox 已支持字典。使用默认实现的旧节点仍能构造。

## 3. 标签从文本走到端点

沿下面四步阅读并修改代码：

1. `config.rs::PortRef::parse` 通过 interlayer::Port::parse 拆成节点、端口和可选地址。
2. `graph.rs::attach_sender/attach_receiver` 收到 tag，将它与端点一起放入组。
3. 对外输入、对外输出、内部连接三条装配路径都传入各自引用的 tag。
4. 节点构造阶段按注册表的名字顺序取组，调用 tagged_ctor。

`router:out:camera:42` 的端口名是 out，标签原文是 camera:42。splitn(3) 只拆两次，
第三段中的冒号保留。str2addr 先尝试解析整数，否则哈希整段文本；不会裁剪空白。
PortRef 仍拒绝空节点名和空端口名，这是当前配置入口比原版底层解析更严格的诊断边界。

子图展开也必须保留标签。在叶子分支使用 `format!("{}{}", prefix, r)` 给完整引用
加节点前缀。例如 `a:out:camera:42` 变为 `branch/a:out:camera:42`。不能只用解析后的
node 和 port 重建字符串，否则第三段消失；也不要把已经哈希的数字转回文本再处理。

注意两种位置不同：子图边界映射到带标签的内部叶子，本节已经支持；直接给子图边界
引用添加标签，还需要图端口元数据及原版运行时语义，当前会明确返回 Unsupported，
不会默默丢弃。这也是必须继续补齐的功能。

## 4. 用探针证明端到端传递

下面测试定义一个最小转发节点。它的旧 build 故意 panic：如果 Builder 仍调用旧入口，
测试会直接失败。新的构造入口断言输入标签为 7、输出标签为 camera:42 的地址，然后
将端点移动到节点。普通 exec 将消息原样转发。

```rust,ignore
{{#include ../../../code/flow-rs/tests/tagged_constructor.rs}}
```

运行：

```sh
cargo test --manifest-path code/Cargo.toml -p flow-rs --test tagged_constructor --locked
```

这个测试覆盖子图的内部叶子引用、主图的对外输入和输出，以及两个节点之间的连接。
收到 23 才证明端点仍能工作；构造器中的断言证明地址也到达了，而不只是配置成功解析。
超时用于让接线错误导致的等待变成测试失败。

## 5. 独立复现与验收

先暂时把 Builder 的调用改回 ctor，确认测试在探针的 build 中失败；恢复后再把叶子
重写改成只拼 node 和 port，确认标签断言失败。最后恢复正确实现并重新运行。
这两次故障分别验证了“选择正确入口”和“保留正确数据”，不是同一个检查。

你现在应能回答：为什么 tag 不放进消息？为什么字典不能沿用默认 build_tagged？
为什么附加子图前缀时需要保留完整引用？能解释并独立重做后，再进入字典宏与 Demux
装配；本节只是使它们必需的数据真正穿过现有框架，并未完成整个节点协议。

下一节：[实现字典端口宏与按地址装配](ch07b-dictionary-ports.md)。
