# Ch4.4a 子图实作：亲手追踪递归展开

读本节前，你应能解释 Ch3.2a 的“节点名字 → 端口名字 → 端点组”。本节在接线之前工作：把对子图边界的引用改写成对内部叶子节点的引用。输入和输出都是 `Config`，没有创建 channel，也没有运行节点。

## 1. 区分定义与实例

一份 Branch 定义可以创建 first、second 两个实例。它们内部的节点都叫 leaf，但不能共享实例名字，否则装配器会把两份节点当成同一个。

```rust,ignore
{{#include ../../../code/flow-rs/examples/subgraph_steps.rs:definitions}}
```

将这段配置写在 `code/flow-rs/examples/subgraph_steps.rs` 中，顶部引入：

```rust,ignore
use flow_rs::{config::Config, error::Error, subgraph::flatten};
```

`Branch` 是图定义的名字，`first` 是使用这份定义的节点实例名，`leaf` 是定义内部的局部名字。展开后需要得到 `first/leaf` 和 `second/leaf`。前缀来自实例名，不是图定义名；否则两份实例都会变成 `Branch/leaf`。

## 2. 先手算，再运行

在文件末尾添加 `fn main() { ... }`，将下段放入函数体：

```rust,ignore
{{#include ../../../code/flow-rs/examples/subgraph_steps.rs:inspect}}
```

在仓库根目录运行：

```sh
cargo run --manifest-path code/Cargo.toml -p flow-rs --example subgraph_steps --locked
```

输出中应有：

```text
节点：["first/leaf", "second/leaf"]
输入：["first/leaf:inp"]
内部连接：["first/leaf:out", "second/leaf:inp"]
输出：["second/leaf:out"]
```

注意 `first:i` 的两次查找：先在 top 的节点列表中找到 first，知道它使用 Branch；再到 Branch.inputs 找 i，得到 `leaf:inp`，最终补上 first 的实例前缀。这里的 i 是图边界名字，inp 是叶子节点端口名字，不需要同名。

## 3. 怎样自己写递归函数

在 `subgraph.rs` 中，先写顶层 `flatten` 的数据准备：按图名建立 `HashMap<&str, &GraphConfig>`，找到 main，准备输出节点列表、连接列表和祖先栈。这张索引表借用原配置，不复制所有图；输出列表则拥有改写后的节点与字符串。

接着写 `expand`。每次调用需要知道当前图、当前实例前缀、图索引、祖先栈和两个输出列表。不要让一个全局变量同时代表这六种状态。

一次调用按下面顺序完成：

1. 如果当前图名已在祖先栈上，返回 `SubgraphCycle`；否则压栈。
2. 检查本图声明的边界没有空 `ports`。不能等到父图引用它时才检查，否则未引用的坏边界会消失。
3. 遍历节点。节点类型是图名则递归调用，并把 `实例名/` 追加到前缀；否则复制叶子的类型与参数，生成带前缀的节点名。
4. 遍历内部连接，将每个引用解析到叶子后追加到输出连接列表。
5. 当前调用完成，弹出祖先栈。

这里用 `&mut Vec<NodeConfig>` 收集结果：每一层都追加到同一个列表，避免先创建很多小列表再合并。可变借用保证同一时刻不会有另一个地方随意改它；一次递归调用返回后，外层继续使用列表。

再写 `resolve_ref`：查当前节点，若它是叶子就输出带前缀的引用；若它是子图就查边界，再对边界中的每一个引用递归。边界可能连接多个叶子，所以函数向结果 `Vec<String>` 追加，而不是只返回一个字符串。

原实现先展开子节点再处理本图连接，使引用解析所需的可达子树已经经过环检测。随意交换这两步可能让 `resolve_ref` 在递归图中无限下钻。

## 4. 为什么两个 Branch 不算环

先手写祖先栈的变化：

```text
进入 top：       [top]
进入 first：     [top, Branch]
完成 first：     [top]
进入 second：    [top, Branch]
完成 second：    [top]
完成 top：       []
```

祖先栈记录的是**当前调用路径**，不是所有曾经见过的图。用一个永不删除的全局 visited 集合，会把第二次合法复用 Branch 误判为环。

把下面代码继续放入 main：

```rust,ignore
{{#include ../../../code/flow-rs/examples/subgraph_steps.rs:cycle}}
```

这次把 Branch 的叶子改成对 top 的引用。路径变为 `top → Branch → top`，第二次进入 top 时它仍在栈上，必须报错。错误发生时整个展开返回失败，输出中的部分结果不会被交给 Builder；当前栈也不再继续使用。

## 5. 不能从这个实验推导出什么

实验验证的是配置改写和递归终止，不是完整子图运行时。当前主图边界与内部连接分别保留自己的容量 1、2、3，而 Branch 的边界容量 4 没有成为独立队列。是否应该保留、传播或协调边界容量，必须按照原版通道注入和运行时规则进一步实现，不能看最终整数相同就宣称等价。

同样，当前实现只复制主图资源，子图声明的资源和子图实例参数没有完成对应处理；资源共享示例成功并不能证明资源作用域完整。原版的动态实例、共享子图和生命周期仍属于必做内容。

已补的空边界检查对应原版 `config/mod.rs::translate_graph` 对每个输入、输出调用 `translate_conn` 的行为。当前检查覆盖展开路径可达的图；它尚不能证明未使用图定义也完成了原版所有配置检查。

## 6. 独立练习与验收

- 将 first 改名成 camera，并更新引用。验收：叶子变成 camera/leaf，second/leaf 不变。
- 添加第三份 Branch，先不接线。验收：得到第三个独立叶子，不能因为复用定义被判为环。
- 将 Branch 的边界 i 改成 `ports=[]`。验收：展开返回 `BadConnection`；即使父图不引用 i，也应拒绝。
- 手画三级定义 top → middle → Branch 的调用栈与前缀，再实现配置。验收：前缀按实例路径累积，返回上一层后兄弟实例不继承前一个兄弟的前缀。

运行相关回归测试：

```sh
cargo test --manifest-path code/Cargo.toml -p flow-rs subgraph --locked
```

掌握这节后，你应该能够解释递归每次调用的输入、输出和终止条件，再继续开发完整子图运行时；“能读懂递归代码”只是这个过程中的一个检查点。
