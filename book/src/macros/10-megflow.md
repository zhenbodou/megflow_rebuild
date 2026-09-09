# 第 10 课：crate 协作与 MegFlow 毕业实战

本课收拢前面九课：先用可运行的 inventory 实验理解注册，再把所有相关 crate
放回它们的编译和运行阶段，最后逐项核对原版宏。前面的学习实验不等于引擎功能已经对齐。

## 1. inventory 中登记的是什么

项目的 `NodeRegistration` 保存名称、端口名表、数组标记和构造函数指针。
登记的是“如何构造节点”，不是已经启动的节点对象。真正调用构造器发生在图装配时。
否则宏展开期间就去打开模型文件或启动异步任务，会混淆编译环境与业务运行环境。

先忽略 Actor 和 channel，运行这个完整例子：

```rust,ignore
{{#include ../../../code/flow-rs/examples/registry_basics.rs}}
```

项目根目录命令：

```bash
cargo run --manifest-path code/Cargo.toml -p flow-rs --example registry_basics --locked
```

预期结果为 `[("double", 6), ("increment", 4)]`。
`run: fn(i32) -> i32` 是函数指针，不是调用结果。`submit!` 里保存它，
之后 `(p.run)(3)` 才执行计算。两个模块无需互相导入，却登记到同一个 Plugin 表。

`collect!` 声明哪种类型可收集，必须与该类型在同一 crate；`submit!`
在模块级生成登记项；`iter::<T>` 在运行时枚举。枚举次序没有保证，因此示例先排序。
这些接口约定见 [inventory 官方文档](https://docs.rs/inventory/latest/inventory/)。

### 不要把 inventory 讲成完全没有运行时机制

静态条目与初始化入口由宏生成并随程序链接，但当前 inventory 0.3.24 的实现
通过平台初始化机制登记条目；它不是简单在链接时生成一个数组、随后遍历 section。
可以在本地该 crate 的 `src/lib.rs` 查 `__do_submit`、`__ctor` 和 `Registry`。
因此不能未经测量就宣称比 lazy_static“零运行时开销”。
同样，模块中 submit 的先后顺序不是“同名后注册覆盖前注册”的约定。
引擎需要明确处理重名注册；当前 find 取第一个匹配项，还没有给出完整的重名校验。

## 2. 把实验接回 node_register!

按以下顺序读当前源文件，每次只跟一个数据流：

1. `flow-derive/src/node.rs` 的 `NodeRegisterArgs`：将 `"Doubler", Doubler` 解析为 LitStr 和 Path。
2. `expand_node_register`：把名字插进 `NodeRegistration.name`，把类型插进 `<Doubler as BuildFromPorts>::build`。
3. `flow-rs/src/registry.rs` 的 `NodeRegistration` 与 `inventory::collect!`：定义条目数据。
4. `find("Doubler")`：枚举匹配；返回的是构造说明，而不是节点。
5. `flow-rs/tests/register.rs`：给构造器传 Args 和端口组，启动节点，检查三个输出及退出。

`<#ty as Trait>::method` 是完全限定语法，明确选择某个 trait 的关联项。
生成时插入类型路径，运行时通过函数指针调用；这两步不要混为一个“宏会创建节点”。

运行真实节点注册测试：

```bash
cargo test --manifest-path code/Cargo.toml -p flow-rs --test register --locked
```

resource_register 采用同一个参数解析器，但登记 ResourceRegistration，
构造器是 `build_arc::<T>`，没有输入/输出表。对应业务测试在 Part 4 的 resource_e2e。

## 3. 涉及的 crate，在哪个阶段起作用

| crate/工具 | 当前采用的用途 | 必须掌握的接口或约束 |
| --- | --- | --- |
| Rust 自带 `proc_macro` | 编译器宏入口 | TokenStream 输入/输出，宏执行上下文 |
| `proc-macro2` | 可测试的 token 表示 | TokenTree、Span、TokenStream、边界转换 |
| `syn` 2 | 解析和改写 Rust 语法 | Parse、Punctuated、DeriveInput、ItemStruct、ItemImpl、Error |
| `quote` | 生成 token | # 插值、重复、ToTokens、format_ident、quote_spanned |
| `trybuild` | 编译诊断回归测试 | compile_fail、.stderr、审核后更新预期 |
| `inventory` | 分散提交的类型化注册 | collect、submit、iter，无顺序保证 |
| `tokio` | 生成代码的任务执行 | spawn、JoinHandle；#[tokio::test] 建测试运行时 |
| `serde` | 构造器参数及配置反序列化 | Deserialize、derive feature、flatten/default 辅助属性 |
| `toml` | 配置格式与 Args 数据 | TOML 值转为类型；错误必须传回装配调用者 |
| `thiserror` | 引擎错误类型派生 | Error derive、error/from 属性；它不负责捕获 panic |
| `futures-util` | 节点中的异步组合 | join! 等生成/组合 future，不会自行创建线程 |
| `cargo-expand`（可选） | 看真实调用处展开 | 工具安装与 crate 依赖不同 |

不是每个 crate 都要加入过程宏库。比如生成结果里有 `tokio::spawn`，
要能在**调用者编译环境**找到 Tokio；只给 flow-derive 加 Tokio 没用。
本项目流向为 flow-rs 依赖 flow-derive，并在 prelude 重导出宏；
下游若直接使用 Tokio 的属性宏，也需在自己的 Cargo.toml 中声明依赖。

Serde 的 `Deserialize` 与同名 derive 分属类型/宏命名空间；
`#[serde(default)]` 是 derive 读取的配置，不会自己执行反序列化。
同理，Node trait 规定运行时方法，Node derive 只负责生成 impl，两者不能互相替代。

`ctor`、`lazy_static`、`proc-macro-crate` 不属于当前 flow-derive 的直接依赖；
需要讨论原版机制或完善改名支持时再引入具体实现与测试，不能在依赖表里写了名字就算教过。
原版还有 anyhow 和开启 `span-locations` 的 proc-macro2；它们服务于原版错误表示
及位置处理，不意味着重写必须照搬所有依赖，也不意味着可以删除相关行为。

## 4. 原版宏清单：不能遗漏哪些能力

下表来自本地 `../megflow/flow-derive/src/lib.rs` 的入口，固定参照版本见 Ch0.4。
“部分”表示名字相同但能力、语法或生命周期有差距，不是已完成兼容。

| 原版宏 | 原版源码模块 | 当前重写状态及必须补齐的内容 |
| --- | --- | --- |
| inputs / outputs | ports、lib | 部分：普通标量/数组及具体类型标量；类型化数组、字典、动态端口与信息表待补 |
| Node derive | node | 部分：关闭与标志；动态接线、状态、空消息转发和统计待补 |
| Actor derive（含 local） | actor | 部分：spawn 与普通错误收尾；local、性能统计及空信号协议待补 |
| Parser derive | internal | 未实现原版内部声明解析派生 |
| node_register! | node/internal | 部分：inventory 构造器注册；原版注册接口仍需对照 |
| methods | methods | 部分：当前包装 exec；原版还处理参数适配、同步/异步、更新回调、validator/filter |
| opt_register! | pass | 未实现优化 pass 注册 |
| resource_register! | resource | 部分：资源构造注册，生命周期和原版接口待补 |
| submit! | internal | 未实现对应公共入口 |
| feature! | lib/internal | 未实现原版图特性功能 |
| atest / amain | lib | 未实现原版入口；当前测试用 Tokio 属性宏，不能说已兼容 |
| add_cvt_func | cvt_func | 未实现转换函数登记及调用链 |

本书自己的 TypeName、BuildFromPorts 是教学/实现辅助宏，不是用来填平上述缺项的替代名称。
原版 `methods` 会生成 Actor 实现，当前需额外 derive Actor；因此原版
`#[derive(Default, Node)] + #[methods]` 不能仅靠复制进当前项目就视为成功。

## 5. 从熟练使用到能独立维护的验收

完成本专题后，先独立做五个任务：

1. 不看模板，从空目录写出第 5 课三种宏的实验，说明每个 crate 为什么在这个 Cargo.toml 中。
2. 对一种带生命周期、where 与 const 泛型的类型生成方法，并让下游真正编译通过。
3. 给一个非法端口语法添加带 span 的错误，再写 trybuild 用例证明不是其他编译错误。
4. 修改 Actor 生成代码时，说明每个 `?` 退出哪个 async 块，并验证错误路径 finalize 恰一次。
5. 从上表挑一种原版宏，列出所有可观察行为，再写成功、失败和业务测试；不能只移植宏入口函数名。

本专题已提供前四项的参考实现、命令和测试。第五项要随着动态端口、配置更新、
资源和优化器继续完成，仍在完整重构验收范围内。“精通”不是记住几个 API，
而是能用这些方法独立迁移并维护完整宏系统。


---

[课程首页](00-roadmap.md) · [上一课](09-maintenance.md)

## 标量类型化端口：把解析器接到实际节点

当前已支持原版风格 `#[inputs(inp: u32)]` 和 `#[outputs(out: String)]`。
新增语法不是只让 syn “读懂冒号”，还需要贯通四处代码：

1. PortSpec 在名字后遇到冒号时解析 syn::Type，保存 payload；无冒号保留既有语法。
2. 输入属性生成 `ReceiverT<payload>`，输出属性生成 `SenderT<payload>`。
3. BuildFromPorts 将图提供的未类型化端点通过 into 转成指定类型，不从 TOML 读取端口字段。
4. Node::close 用 Default 替换类型化输出，释放已接线发送端。旧的 `Option<Sender>`
   输出仍置 None；数组输出仍 clear，三者不能套同一种赋值。

第 6 课的泛型约束和第 7 课的 AST 分类在这里实际用到：必须识别 SenderT 的泛型类型
结构，不能按名称包含 Sender 就把业务字段当端口。当前识别仍不解析类型别名，依赖改名
也仍需后续处理。

完整下游节点及测试：

```rust,ignore
{{#include ../../../code/flow-rs/tests/typed_node.rs}}
```

```bash
cargo test --manifest-path code/Cargo.toml -p flow-rs --test typed_node --locked
```

注意 exec 直接调用 `self.out.send(...)`，不再写 Option 的 as_ref；recv 的类型来自
字段，不再写 `recv::<u32>()`。载荷改变使用 repack，保留 partial_id。测试在 start 后
检查确切输出和元信息，并设置超时，防止 close 漏掉类型化输出导致永不收尾。

这只是具体 Rust 类型的标量端口。原版 T0 模板变量、`name:[T]` 数组、字典与 dyn
端口的信息表和生命周期尚未完整移植。当前对数组/动态形式明确报未支持诊断，不把
它们误认为标量载荷。旧 `name[]` 教学数组语法暂时保留。

## 毕业实作补充：`#[add_cvt_func]` 转换注册宏

前面已实现节点宏，现在用一个真实框架需求检验你能否自己设计属性宏：用户只想写 `Count → Label` 的业务函数，引擎却需要 `SealedEnvelope → SealedEnvelope` 的函数指针，并要知道源、目标类型。

### 1. 先写用户希望使用的代码

```rust,ignore
#[derive(Clone)]
struct Count(u32);
#[derive(Clone)]
struct Label(String);

#[add_cvt_func]
fn label(Count(value): Count) -> Label {
    Label(format!("frame-{value}"))
}
```

这里的 `Count(value)` 是函数参数模式，不是类型。解析函数时应从 `FnArg::Typed` 的 `ty` 取 Count，不能把整个参数转换成字符串再截取。原函数仍应可以通过 `label(Count(3))` 直接调用。

### 2. 手写宏应生成的适配器

输入是一个已封箱的信封。按顺序做四件事：

```rust,ignore
let envelope = envelope.downcast_mut::<Envelope<Count>>()
    .expect("type error in convert function label");
let input = envelope.unpack();
let result = label(input);
envelope.repack(result).seal()
```

`unpack()` 取走载荷，信封对象还保留元信息。`repack` 用目标载荷构造新信封，并保留这些元信息。若改用 `Envelope::new(result)`，结果文字可能正确，但序号、权重和地址会丢失。

适配器没有捕获环境，因此它的闭包可以转换为普通函数指针 `CvtF`。输入和输出在外部看起来都是 SealedEnvelope，转换逻辑内部才知道具体载荷类型。

### 3. 再把类型写进登记项

运行时增加 `ConversionRegistration`：保存取得源类型标识的函数、取得目标类型标识的函数，以及上面的信封适配器。

为什么类型标识也用函数？静态登记发生时只保存函数指针，等到初始化转换表时再调用 `MsgTypeId::of::<Count>`。这样将静态登记数据与运行时类型标识取得分开，无需在宏执行时尝试查询用户类型的 TypeId。

在 `flow-derive/src/conversion.rs`，使用 `syn::ItemFn` 解析函数，读取 `sig.inputs`、`sig.output` 和 `sig.ident`；`quote!` 输出原来的完整 ItemFn，以及一个匿名 const 作用域内的 inventory 登记。匿名作用域避免为每个转换器额外生成可能冲突的固定函数名。生成路径使用 `::flow_rs::...`，由引擎重导出 inventory，用户不需要自己声明这个依赖。

当前实现不支持依赖改名后的路径自动发现；这种工程化要求仍需按宏专题第 7、9 课继续验证和实现。

### 4. 检查错误签名，而不是等生成代码碰巧报错

当前宏要求一个参数、显式返回类型、安全同步 Rust 函数，不接受泛型、async 或外部 ABI。这是因为登记项需要一份已经确定输入/输出类型的同步函数。生成器返回带 span 的 `syn::Error`，入口把它转换成 `compile_error!`。

Rust 类型从签名取得；属性参数解析现已支持原版的空参数、下划线占位及字符串提示写法。字符串提示在 Rust 分支不改变类型标识。Python 类型的特殊识别与执行适配尚未实现，不能据此宣称 Python 转换已支持。

传入参数是否满足 `'static`、输出是否满足 Clone/Send 等要求仍由下游编译约束检查。当前单元测试覆盖几种非法签名，尚不替代完整 trybuild 诊断快照和 cfg 属性组合测试。

### 5. 运行真正的下游验证

```sh
cargo test --manifest-path code/Cargo.toml -p flow-rs --test conversion_macro --locked
```

测试没有手动调用初始化或注册函数。它用属性登记 Count → Label，发送 Count(7)，收到 Label("frame-7")，并断言 `partial_id=42`、`weight=3` 保留。测试还直接调用原函数，证明宏没有把用户函数变成只能由框架使用的特殊入口。

本重构先用 inventory 收集静态登记，在转换表首次访问时通过 LazyLock 建表；原版使用 ctor 在初始化期间调用注册函数。当前普通静态转换可以使用，但动态库加载时机和重复静态登记的执行顺序尚未证明等价。不要注册多个相同类型对并依赖哪个属性函数最后生效；显式 `add_cvt_func_impl` 的后写覆盖规则另有测试。

完成后独立添加反向函数 Label → Count。先决定字符串解析失败如何表达：当前 CvtF 返回信封而不是 Result，不能直接在业务函数返回 Err 后仍假装目标是 Count。返回类型改变，登记的目标类型也随之改变。这道练习检验你是否理解“函数签名就是转换契约”。

### 属性组合进阶：函数不存在时，登记也必须不存在

属性宏往往生成多个 item。用户给函数写 `#[cfg(...)]`，实际想控制的是整个功能，而不仅是展开结果里的第一项。因此转换宏必须让函数和登记代码共享存在条件。

考虑这个输入：

```rust,ignore
#[add_cvt_func]
#[cfg_attr(feature = "disabled", cfg(any()))]
fn convert(value: Input) -> Output { /* ... */ }
```

`cfg(any())` 是永不满足的条件；当 disabled 条件成立时，函数不存在。如果生成的登记仍引用 convert，便出现“函数找不到”的错误。条件成立或不成立可能改变编译器交给宏的属性集合，因此只运行一份正常示例不足以验证属性组合。

也不能简单复制所有属性。`#[inline]` 对函数有意义，贴在登记用的 const 上则不合适。当前生成器增加 `gate(meta)`，按 AST 递归筛选：

1. 遇到 `cfg` 原样保留。
2. 遇到 `cfg_attr`，先取第一个 Meta 作为条件，再递归处理后面的属性。
3. 后面的属性若没有留下任何存在条件，整个 cfg_attr 不复制。
4. 其他属性只留在原函数上。

例如：

```rust,ignore
#[cfg_attr(feature = "x", inline, cfg_attr(unix, cfg(any()), allow(dead_code)))]
```

登记代码仅继承：

```rust,ignore
#[cfg_attr(feature = "x", cfg_attr(unix, cfg(any())))]
```

这不是字符串替换。`syn::Meta` 表示属性内容，`Punctuated<Meta, Token![,]>` 处理逗号分隔的内容，`parse_quote!` 重建筛选后的语法树。函数上的原属性完全保留，只有新生成的登记需要这份筛选副本。

`gate` 返回 `Result<Option<Meta>>`：Err 表示属性语法不正确，None 表示这个属性不需要复制，Some 表示得到一个存在条件。收集时的 `transpose()` 将 `Result<Option<T>>` 变成 `Option<Result<T>>`，从而可以先通过 `filter_map` 忽略 None，再由 `collect::<Result<Vec<_>>>()` 保留错误。这行组合代码的意义是“忽略无需复制的属性，但不能吞掉解析错误”。

测试分两层：生成器单测检查嵌套 cfg_attr 筛选后确实只剩存在条件；下游测试使用不存在的载荷类型定义禁用函数，确认禁用代码不泄漏，并验证带 inline 的启用函数仍能完成消息转换。下游测试无法独自证明筛选函数运行过，因为编译器可能先移除条件不成立的函数，所以两层都要保留。

### 给初学者看的错误也要验收

新增 `tests/ui/conversion_async.rs` 和 `conversion_missing_return.rs`，分别故意使用 async 和遗漏返回类型。对应 stderr 快照要求错误指向函数声明，说明需要同步函数或显式返回类型。执行：

```sh
cargo test --manifest-path code/Cargo.toml -p flow-derive --test ui --locked
```

维护者修改诊断后可以在审核输出的前提下更新快照；学习者日常运行不用设置 `TRYBUILD=overwrite`。不能为了让测试通过而直接接受一份看不懂的编译器内部错误。

练习：给同一个转换函数加上 `#[cfg_attr(all(), inline)]` 与一个真实存在条件，画出应该作用于函数、应该作用于登记的两份属性列表。然后对照展开结果。这是从“会写 quote”走向“能维护供别人使用的宏”的必要步骤。


### 参数解析实作：为什么 `(_, _)` 也要支持

原版 `flow-derive/examples/cvt_func.rs` 既使用 `#[add_cvt_func]`，也使用
`#[add_cvt_func(_, _)]`。重构若只接受无参数写法，就会拒绝原版合法 Rust 代码。
兼容性要检查用户写下的 token，不能只比较宏最后生成的函数。

解析器分两个槽位，分别对应源类型提示和目标类型提示。每个槽位可以为空、是 `_`，
或是字符串字面量。`_` 的意思是此槽位不提供提示，并不是 Rust 类型通配符。
在当前 Rust 函数转换中，实际身份始终由函数参数和返回类型决定：

```rust,ignore
#[add_cvt_func(_, _)]
fn widen(value: u32) -> u64 { value as u64 }
```

展开仍登记 u32 → u64。即使提供字符串提示，也不能用它把 u32 重命名成别的 Rust 类型。
这保留了原版 `type_id` 的 Rust 分支规则；Python 分支对提示有其他用途，不能混淆。

实现位置是 `flow-derive/src/conversion.rs::CvtFnOption`，它实现 `syn::parse::Parse`。
`ParseStream` 是一条可消费的 token 输入流，`peek(Token![_])` 只看下一个 token 而不取走；
`parse::<Token![_]>()` 才将它消费。若不是下划线，就尝试读取 `LitStr`，数字等非法提示
会在这里返回语法错误。读完两个槽位后，外层 `syn::parse2` 还要求没有剩余 token，
因此第三个槽位不会被悄悄忽略。

为保持原版边界，解析器也沿用它的可选逗号行为；本教程推荐明确写 `(_, _)`，
不鼓励依赖省略分隔符的宽松形式。原版不接受两个槽位之后额外追加逗号，当前测试也保留这一点。
如果将来希望扩展语法，应该先明确这是兼容性扩展，而不是无意中让解析结果变了。

本次验证包含两类证据：生成器测试比较不同合法 Rust 参数写法的展开结果相同；
下游转换测试实际使用 `(_, _)` 和字符串提示，确认登记、收发和元信息保留仍然有效。
独立练习：增加非法第三槽位，观察错误是在参数解析阶段出现，而不是等消息发送才发现。

## 节点宏与类型推断衔接：注册表必须保留载荷类型

转换函数已经能登记，但 Builder 还需要知道每个节点端口声明了什么类型。
不能先随便创建节点再问：构造器需要端点，端点的通道类型又取决于节点声明，会形成循环依赖。
因此类型信息应与端口名一样，在注册阶段可查询。

### 1. 从手写契约开始

在 `BuildFromPorts` 中加入：

```rust,ignore
fn input_types() -> Vec<MsgTypeId>;
fn output_types() -> Vec<MsgTypeId>;
```

当前 trait 为旧的无类型手写实现提供默认方法：按 INPUTS/OUTPUTS 的长度生成 Any 列表。
若手写类型化节点，就必须覆盖这两个方法，不能继续依赖默认值。

`NodeRegistration` 保存这两个方法的函数指针。和转换注册项一样，静态登记只保存
“怎样取得信息”，真正需要 TypeId 时再调用函数，不是在过程宏执行时计算用户类型。

### 2. 在同一次字段遍历中生成三份对应数据

已有宏遍历字段生成端口名和数组标记。现在同时追加消息类型：

| 字段 | 端口名字 | 数组标记 | 消息类型 |
| --- | --- | --- | --- |
| `a: ReceiverT<u32>` | a | false | `MsgTypeId::of::<u32>()` |
| `b: Receiver` | b | false | Any |
| `out: SenderT<String>` | out | false | `MsgTypeId::of::<String>()` |
| `inps: Vec<Receiver>` | inps | true | Any |

同一次遍历的目的不仅是少写循环，还要保持对应关系：类型列表的第 i 项必须属于名字列表
的第 i 项。不要对其中一张列表排序，也不要分别遍历两个 HashMap 来生成它们。

`message_type` 复用 `wrapped_type` 从 `Type::Path` 最后一个路径段提取唯一类型实参。
它处理明确支持的 ReceiverT/SenderT，不猜测字符串中包含 Sender 的任意业务类型。
当前对类型别名和完整模板/字典端口的识别仍需补齐，不能声称支持所有 Rust 类型表达式。

生成的是函数体中的代码：

```rust,ignore
fn input_types() -> Vec<flow_rs::config::interlayer::MsgTypeId> {
    vec![flow_rs::config::interlayer::MsgTypeId::of::<u32>()]
}
```

这里 quote 的 `#payload` 插入 syn::Type，不是将类型名转换成字符串。字段类型为 T 时，
泛型约束仍需让生成代码满足 TypeId 的 `'static` 要求；过程宏不是类型检查器，不能替
编译器推断任意别名或缺失约束。

### 3. 不创建节点也能验证声明

`tests/typed_node.rs::registration_exposes_payload_types_without_constructing_node`
直接查 TypedPortNode 的登记，验证 inp 对应 u32、out 对应 String，没有调用节点构造器。
执行：

```sh
cargo test --manifest-path code/Cargo.toml -p flow-rs --test typed_node --locked
```

独立练习：声明 a、b 两个不同载荷类型的输入，将字段顺序交换，检查端口名和类型列表
是否一起交换。这个实验用于发现宏生成的“名字对了、类型下标错了”。

现在 Builder 可以读取具体类型，但尚需把同一条连接的端口按发送/接收方向收集成集合，
调用上一课的 guess_channel_type，再把选出的类型用于创建队列。当前 Builder 已接入具体类型信息的收集与队列创建（Ch3.2a），跨连接模板等完整建图类型推断仍在验收账本中，不应将测试通过误写成全图转换已经自动完成。
