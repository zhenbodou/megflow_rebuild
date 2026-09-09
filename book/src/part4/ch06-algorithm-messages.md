# Ch4.6 算法消息：从矩形与跟踪状态开始

框架能传递任意 Rust 类型，不代表原版的业务消息已经实现。算法节点需要统一描述图像、
检测框、跟踪状态和分类结果。本章从无需外部存储依赖的 Rect、TrackState 开始；
Frame、Image、Item、特征及其他消息结构仍是后续必须补齐的内容。

## 1. 新建算法消息模块

在 `flow-message/src/lib.rs` 加入 `pub mod algo_base;`，创建 `algo_base/mod.rs`，
通过 `mod base; pub use base::{Rect, TrackState};` 导出基础类型。实现放入 base.rs。
这样调用者使用 `flow_message::algo_base::Rect`，不用知道内部文件如何拆分。

本节两种类型对照原版 `flow-message/src/algo_base/base.rs`，实现不需要新增 crate。
后续引入 BlobProxy、ndarray 等依赖时，必须分别讲清存储、视图与共享语义，不能直接
把它们替换成 Vec 就认为消息协议完整。

## 2. 先写结构，再写计算

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
    pub score: Option<f32>,
}
```

四个坐标表示两个端点，score 是可选评分。None 与 Some(0.0) 不同：前者没有评分，
后者有一个值为零的评分。Copy 可以逐值复制这份小结构，复制不会共享可变坐标。

按原版实现三个方法：width 返回 x2-x1，height 返回 y2-y1，area 返回两者乘积。
这里没有自动加一、取绝对值或限制为非负。比如 (1,2) 到 (5,8) 的宽高是 4、6，面积 24。
将 x1/x2 交换后宽度 -4、面积 -24，不能擅自改成正数。

再实现 `in_frame(frame_width, frame_height)`：如果 x2 超过宽度、x1 小于零、y2 超过高度
或 y1 小于零，返回 false，否则返回 true。端点恰好等于边界允许通过。
这个方法没有检查 x1≤x2、y1≤y2，也没有检查 NaN；所以它不是完整的“矩形是否合法”验证。

为什么单独测试 NaN？浮点比较遇到 NaN 不像正常数字，四个越界条件可能都不成立。
原版方法因此可能返回 true。保留行为不代表业务输入应接受 NaN；若业务节点需要额外校验，
应在明确的位置处理，不能偷偷改变基础类型的原版方法。

## 3. 用枚举表达状态，再实现字符串接口

TrackState 的八个变体按顺序是 No、New、Update、Miss、Die、Filtered、Select、Max。
No 显式从 0 开始，后面依次递增。保持变体顺序，因为调用者可能观察其数值。
Default 选择 No。

先实现 `name(&self) -> &'static str`，用 match 返回小写静态字符串。例如 New 对应 "new"。
字符串字面量存活于程序期间，方法不必分配新 String。再按原版实现 ToString，返回拥有
所有权的 String；它和 name 的内容相同，但返回类型与分配行为不同。

最后实现 `From<&str>`，对八个精确名字匹配。原版使用 unreachable! 处理未知字符串，
因此 "NEW"、" new" 和空串都会 panic，不能自动转小写、trim 或默认返回 No。
通常面向不可信文本可以另行设计 TryFrom 返回 Result，但那会是另一份接口契约。

## 4. 先验证边界，再接入节点

运行：

```sh
cargo test --manifest-path code/Cargo.toml -p flow-message --test algo_base --locked
```

测试覆盖普通矩形、贴边、越界、反向坐标与 NaN，并逐个检查状态名字、数值和默认值。
非法状态通过 catch_unwind 确认会 panic；这只用于验证原版失败行为，不是鼓励业务代码
依赖捕获 panic 解析日常输入。

独立练习：构造 x1=0、y1=0、x2=图像宽、y2=图像高的矩形，先预测面积；再把 x2 增加 1，
解释 area 与 in_frame 为何分别变化。随后创建 `Envelope<Rect>`，设置 partial_id 并用
repack 替换评分，验证消息上下文保留。这样才把业务数据与前面的框架消息层真正连接起来。

本节尚未实现跟踪算法或检测算法。TrackState 只是状态表示，Rect 只是几何数据与原版
辅助操作；其他算法消息和它们的 Rust 业务处理仍需继续开发，不能据两个结构可用就宣布
算法消息层完成。

## 5. RecordVec：实时长度与记录位置是两回事

原版 FrameResult 的 failed_tracks 和 binds 使用 RecordVec。它不是另一个会自动统计
长度的 Vec，而是在列表旁保存一个记录位置，供后续处理区分既有数据与新追加数据。

在 base.rs 中加入：

```rust,ignore
pub struct RecordVec<T>(pub(crate) Vec<T>, pub usize);
```

这是元组结构体：`.0` 是实际 Vec，仅本 crate 内可直接访问；`.1` 是公开的记录位置。
两个字段的可见性不同，不能因为结构体是 pub 就以为所有字段都公开。

### 按原版顺序实现五个操作

1. `From<Vec<T>>`：先取得 value.len()，再把 value 与长度放进结构体。这里必须先取长度，
   因为移动 value 之后不能继续使用它。
2. `push`：只调用内部 Vec 的 push，不改变 `.1`。
3. `From<RecordVec<T>> for Vec<T>`：取出内部 Vec，丢弃记录位置；这是所有权移动，不复制元素。
4. `Default`：创建空 Vec 与记录位置 0，不要求 T 实现 Default。
5. `Deref<Target=[T]>` 与 DerefMut：返回内部 Vec 的切片引用，让调用者可索引、遍历和修改元素。

为什么解引用目标是 `[T]` 而不是 `Vec<T>`？切片提供读取与元素修改能力，不直接暴露
clear、truncate 等改变容器长度的方法。追加由显式 push 提供。可变切片并不意味着
可任意增加或删除元素，这体现了类型接口对允许操作的约束。

例如：

```rust,ignore
use flow_message::algo_base::RecordVec;
let mut values: RecordVec<u32> = vec![10, 20].into();
assert_eq!(values.1, 2);
values.push(30);
assert_eq!(values.len(), 3);
assert_eq!(values.1, 2);
assert_eq!(values.iter().skip(values.1).copied().collect::<Vec<_>>(), vec![30]);
values.1 = values.len(); // 调用者明确推进记录位置
```

原版 `algo_base/python/collections.rs::const_vec_build` 先读取 `.1`，处理记录位置之后
追加的元素，最后更新 `.1`。这段原版使用场景解释了为什么 push 不能自动更新它；
本次保留 Rust 容器行为，并未实现 Python 同步执行器。

公开记录位置也意味着调用者可以手动写入不合理的值。原版容器没有自动维护“记录位置
永远不超过长度”这样的不变量，不能在迁移时暗中增加会改变既有行为的校验。

### 用不能 Clone 的类型验证移动

`tests/algo_base.rs` 使用未实现 Clone/Default 的 OnlyMove 元素，验证从 Vec 构造、追加、
切片修改以及转换回 Vec 都能工作。若实现时误用 `.clone()`，这个测试会在编译期失败。
同时检查 push 后 len 增加而 `.1` 不变，才能发现把记录位置误当实时长度的业务错误。

运行 `cargo test --manifest-path code/Cargo.toml -p flow-message --test algo_base --locked`。
独立练习：从两个元素开始，追加两次、推进记录位置、再追加一次；每一步先写出 len、`.1`
和 `iter().skip(.1)` 的结果，再执行程序。你应该能解释“数据内容”和“处理进度”如何分别保存。

RecordVec 没有因这一迁移自动获得原版 FrameResult 的共享锁、图像存储或 ItemResult 语义。
后续仍需把这些依赖逐一实现，再组装完整的帧结果。

## 6. Dr：什么时候需要重新同步载荷

原版 ItemResult、FrameResult 的包装还依赖 `dr::Dr<T>`。它不是共享指针，不会自动把
数据放进 Arc；它管理的是“这份载荷是否需要同步”和附着的私有数据。

新建 `flow-message/src/dr.rs`，在 lib.rs 中公开模块。先写接口：

```rust,ignore
pub trait SyncWith<T> {
    fn sync_with(&mut self, dest: T);
}
```

目标 T 可以是可变引用，也可以是其他接收对象，由具体实现决定。trait 本身不规定网络、
磁盘或 Python 目标。然后定义 Dr，包含载荷 inner、布尔 dirty 和私有数据 HashMap。

### 从状态变化推导方法

创建 Dr 时 dirty=true，首次同步必须执行。实现 `SyncWith<U> for Dr<T>` 时要求
`T: SyncWith<U>`，只有 dirty 为 true 才调用 inner.sync_with(dest)，成功返回后清除 dirty。
因此连续同步两次，中间没有取得可变载荷访问，只执行一次内部同步。

`Deref` 返回 `&T`，读取不改变标记；`DerefMut` 在返回 `&mut T` 之前将 dirty 设为 true。
注意它观察到的是“获得可变访问”，不是具体发生了什么赋值。即使拿到引用后没有修改值，
下一次也会同步。这样不需要代理每一个可能修改 T 的方法。

这也有明确限制：如果 T 自己通过 Cell、Mutex 等内部可变性在共享引用下改变数据，
Dr 的 Deref 不会自动察觉。不要把这个标记理解成能够检测任意内存变化的系统。

### 私有数据为什么不参与克隆

私有数据保存为 `HashMap<String, Box<dyn Any + Send + Sync>>`，可附加不同类型的缓存。
add_private_data 按名字插入，private_data/private_data_mut 通过 downcast 取回具体类型。
键不存在或请求类型不匹配都返回 None。

原版 Clone 只要求 T: Clone，并复制 inner；私有数据表重新置空，dirty 重新设为 true。
原因不能仅凭代码猜测成某个固定业务规则，但这个可观察行为必须保持：新副本需要独立同步，
不能假定源对象的私有缓存也跟随过去。Box 中的 Any 并不要求 Clone，机械地派生 Clone
也无法实现这份契约。

修改私有数据不会设置 dirty；dirty 跟踪的是通过 DerefMut 取得的载荷访问。
into_inner 消费 Dr 并取出 T，其他私有数据随包装释放。

### 使用一个同步记录器验证

测试定义 Value(u32)，为它实现 `SyncWith<&mut Vec<u32>>`：每次同步把当前值追加到 Vec。
于是输出列表就是实际同步次数的证据：

| 操作 | 记录器结果 |
| --- | --- |
| 创建 Value(1)，连续同步两次 | [1] |
| 只读取载荷、修改私有缓存后同步 | [1] |
| 取得可变载荷引用但不写值，再同步 | [1, 1] |
| 改成 Value(2) 并同步 | [1, 1, 2] |

另一个测试验证克隆丢弃私有数据并重新同步。运行：

```sh
cargo test --manifest-path code/Cargo.toml -p flow-message --test dirty_record --locked
```

实现与原版 `flow-message/src/dr.rs` 对照，未添加额外 crate。独立练习：先同步一个 Dr，
克隆后分别修改源对象和副本，预测两个记录器的内容，并检查源对象的私有缓存仍然存在。
这能帮助你区分“复制载荷”“共享对象”和“复制同步状态”，它们是三种不同的行为。

完整帧结果还需要 With、锁、图像存储和 ItemResult。Dr 的实现只是其中一层，不能
直接用它宣称帧结果支持并发访问；并发访问规则由外部锁和共享指针共同决定。

## 7. 组合实作：更新检测框、记录变化、发送面积

现在把三个类型放在同一个程序里，检验它们是否只是孤立的知识点。在消息 crate 的
examples 目录新建 algorithm_records.rs，先引入 Rect、RecordVec、Dr、SyncWith 和 Envelope。

任务分成四步：

1. 用 Detection(Rect) 包装检测框，为它实现向 `Vec<f32>` 同步面积的 SyncWith。
2. 连续同步两次，只记录一次面积；再通过 Dr 的可变访问修改 x2，下一次同步记录新面积。
3. 将面积列表移动到 RecordVec，追加一条记录，检查记录位置之后只有新增元素。
4. 将检测框装进带 partial_id 的信封，取出载荷并 repack 成面积，检查序号保留。

完整可运行代码如下。先按四步自己写一遍，再对照：

```rust,ignore
{{#include ../../../code/flow-message/examples/algorithm_records.rs}}
```

在仓库根目录执行：

```sh
cargo run --manifest-path code/Cargo.toml -p flow-message --example algorithm_records --locked
```

预期输出：

```text
同步面积：[12, 24]；新增记录：[30]；消息面积：24，序号：42
```

`detection.0.x2` 中 `.0` 是 Detection 的元组字段，不是 Dr 的私有字段。
Rust 通过 DerefMut 访问内部 Detection，因此在拿到可变字段之前，Dr 已经置脏。
`areas.into()` 则把整个 Vec 移走；之后不能再使用原变量 areas，数据已由 records 持有。

最后 `into_inner()` 取回 Detection，随后将 Rect 放进消息。这里没有强行要求 Dr 实现
所有信封载荷的接口，而是根据阶段需要转移对象。容器的处理进度与消息的 partial_id
承担不同职责，不应该相互覆盖。

为了验证它不依赖最终引擎，`scripts/check_message_course.py` 也在临时独立消息工程中
运行该示例。这个检查使用标准库及本消息 crate，不会因项目里已有 flow-rs 就掩盖依赖。

独立练习：将 y2 从 4 改成 5，先预测两次同步面积和最终消息面积；再删除第二次同步前的
坐标修改，确认记录器不会多出重复项。每次只改变一个条件，才能知道结果由哪层行为造成。

### 下一层依赖的证据边界

原版 `With<T, U>` 使用私有 crate `stackful-any`，Feature、Image 等使用私有 crate
`blob-proxy`。当前父目录原版只有调用代码，本地缓存也未找到这两项实现。因此不能把
With 的附加头直接换成 HashMap、把 BlobProxy 直接换成 Vec 后宣称等价；布局、克隆、
共享、视图及外部内存所有权都需要继续核对。它们仍是完整目标中的待完成依赖，不是
从教学范围删除这些类型的理由。
