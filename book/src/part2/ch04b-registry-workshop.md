# Ch2.4b 注册表实作：名字怎样变成对象

前置是函数、结构体、trait 对象和所有权。先不用 inventory、TOML 和节点宏，
把最核心的“字符串 → 构造函数 → 实例”写出来，再接回框架的注册表。
本课的小算子是用于隔离知识点的实验，不是 MegFlow 新增的内置节点。

## 第 1 步：先有两种具体对象

```rust
{{#include ../../../code/flow-rs/examples/registry_from_functions.rs:operation}}
```

Add 和 Multiply 保存不同状态，但都实现 Operation。调用 apply 时才执行业务，
创建对象时只是保存参数。一个类型可以创建多个状态不同的实例。

## 第 2 步：统一构造函数签名

```rust,ignore
{{#include ../../../code/flow-rs/examples/registry_from_functions.rs:constructors}}
```

先看两个 build 函数：参数都是 i32，返回都是 `Box<dyn Operation>`，因此可以用
同一种函数指针类型保存。Constructor 是类型别名，读成“接收 i32、返回装箱操作对象
的函数指针”。它不是执行函数，也不是保存函数返回值。

注册项中的 `constructor: build_add` 没有括号，保存函数地址；`build_add(2)` 有括号，
会执行构造。把两者写反，类型就对不上。静态表保存构造方法，不提前创建所有业务对象，
所以不同图可以用同一个注册类型生成自己的实例。

这里使用普通函数指针，不能直接放入捕获局部状态的闭包。如果构造需要参数，先像本例
一样把它作为函数参数传入，而不是把某次业务配置捕获进全局注册表。

`&'static str` 是注册名的字符串字面量引用；输入的查询名只是 &str，不需要活到程序
结束。比较字符串内容不会把查询名存进静态表，因此不要求查询名也为 'static。

## 第 3 步：查到条目后调用构造器

```rust,ignore
{{#include ../../../code/flow-rs/examples/registry_from_functions.rs:lookup}}
```

iter 遍历引用，find 返回 `Option<&Registration>`，找不到是 None。ok_or_else 把它
变成 Result，并在缺失时构造错误字符串；问号把错误返回给 build 的调用者。

`(entry.constructor)(argument)` 先取出函数指针，再传参数调用。这里没有把字符串
变成 Rust 标识符，也没有在运行时执行 Rust 源码；能构造什么由事先登记的函数决定。

## 第 4 步：验证实例与调用顺序

```rust,ignore
{{#include ../../../code/flow-rs/examples/registry_from_functions.rs:independent_instances}}
```

同一个 Add 名称构造出参数分别为 2 和 9 的实例，不会互相覆盖。装进同一个 Vec 后，
对象的具体类型不同，但都能通过 Operation 接口调用。fold 从 1 开始，依次应用操作，
最终得到 36。这是同步串行实验，还没有通道与任务调度。

完整源码在 `code/flow-rs/examples/registry_from_functions.rs`。仓库内运行：

```bash
cargo run --manifest-path code/Cargo.toml -p flow-rs --example registry_from_functions --locked
```

也可以把这个文件复制到空目录，直接运行 `rustc --edition=2021 main.rs`，然后执行
生成的程序。它只依赖标准库。预期输出为“注册原理通过……”的说明，所有断言成功。

## 第 5 步：将同一个原理接回 MegFlow

| 小实验 | 框架中的对应物 |
| --- | --- |
| Operation | Actor，统一启动不同节点 |
| Add、Multiply | 具体节点类型 |
| i32 构造参数 | Args 和输入/输出端口组 |
| Constructor | registry::NodeCtor |
| Registration | NodeRegistration |
| 手写静态 REGISTRY | inventory 分散登记后的条目集合 |
| build(name, argument) | 查找注册项，再按端口顺序调用 ctor |

不能只搬查表那一行：图配置按名字接线，构造器按位置消费端口组，因此 INPUTS/OUTPUTS
名表与字段填充顺序必须一致。举例：减法节点 a=10、b=3 应输出 7；交换 a/b 后仍能编译，
却会得到 -7。这说明位置错误属于业务错误，类型正确还不够。

在本书当前实现中，内层 Vec 表示一个端口名下的通道组，外层 Vec 按注册名表排序。
标量端口取一条，数组端口取整组。不要把 HashMap 的遍历顺序直接当构造顺序。

只有理解这张手写表后，再用 inventory 消除集中维护表的工作。inventory 不负责
解析 TOML、推断端口类型或处理业务参数，它只帮助收集条目。登记与对象构造是两件事，
模型文件等业务资源不应在宏展开时打开。

## 独立练习

1. 新增 Subtract，参数保存减数，输入 10、参数 3 输出 7。参考方向：增加结构体、trait
   实现、同签名构造函数和一个条目；查找函数不应修改。
2. 将注册名与结构体名设成不同名字，仍应按注册名查到。原因：查表比较的是 name 字段，
   不是编译器自动提供的类型名。
3. 删除某个条目，构造请求应返回错误。不要改成默认返回第一个类型，这会隐藏配置拼写错误。
4. 思考重名条目怎么办。本例 find 取静态表中第一个，但 inventory 不保证枚举顺序；
   正式注册表需要明确重名策略并验证，不能把本例的顺序当成框架公共协议。

完整框架仍需要原版的注册作用域、资源构造、动态节点和相关宏。本课解决的是基础
推导过程，不能用这个同步小实验替代它们的实现。
