# Ch0.4 完整重构的验收账本

目标是让初学者逐步实现父目录 `megflow/` 的完整 Rust 功能与业务逻辑。
目前的 `code/` 是起步实现，不能因为它的测试通过就称为完整替代品。
本文记录源码对照发现的差距；未完成项仍是本书必须补齐的工作。

## 参照系与范围

在项目根目录执行 `git -C ../megflow rev-parse HEAD`，记录原版版本。本次检查版本为 `95f870bfefd48fa31f9cf88320de4cc177985c72`。
参照路径均相对于 `../megflow/`，实现路径均相对于本项目 `code/`。
纯 Rust 目标不要求 Python 调用接口，但 Rust 实现的运行时、消息、宏、
插件服务、配置处理和调试能力不能因为依赖私有库就直接删除。
私有依赖需要逐项查清其可观察行为并替换；源码或协议不足时明确标为待验证。
C/Python 绑定的业务逻辑如果同时为 Rust 路径服务，也要保留对应能力。

## 源码对照发现

| 能力 | 原版证据 | 当前实现与缺口 | 证明完成需要的验证 |
| --- | --- | --- | --- |
| 四则运算示例 | `flow-rs/src/lib.rs` | 已补除法及左信封元信息传递 | 四种运算、不同左右元信息的端到端测试 |
| 完整信封 | `flow-rs/src/envelope/` | 已补齐七项元信息及 str2addr；消费者协议仍需逐项验收 | 字段默认值、寻址、权重、重打包及空信封语义逐项对照 |
| 通道 | `flow-rs/src/channel/` | 已补 cap=0 无界分支、限时接收与按权重批量接收；仍缺 flush、多消费者，底层为 Tokio MPSC，不等于原版完整协议 | 多消费者、关闭、背压、空信号及批处理路径 |
| 节点与宏 | `flow-derive/src/` | 简化端口语法和生命周期接口 | 原版合法 Rust 节点编译、运行与非法输入诊断 |
| 配置 | `flow-rs/src/config/` | 直接 serde 解析与静态展开 | 模板、预处理、类型推断、连接校验及原版配置夹具 |
| 动态子图 | `flow-rs/tests/02-dyn-subgraph.rs` | 仅静态子图内联 | 运行期创建、逐消息寻址、关闭和回收 |
| 共享子图 | `flow-rs/tests/03-share-subgraph.rs` | 资源 Arc 共享不能证明子图共享 | 多调用方隔离、共享节点、响应回流与空信号 |
| 隔离、多图、分派 | `flow-rs/tests/04-isolated.rs`、`05-multi-graph.rs`、`06-dispatcher.rs` | 尚未逐项迁移 | 对应原版测试场景和消息归属断言 |
| 类型信息与特性 | `flow-rs/tests/07-typeinfo.rs`、`08-graph-features.rs` | 尚未对齐 | 错配端口建图失败、特性选择行为 |
| 优化器 | `flow-rs/tests/09-graph-optimizer.rs` | 无对应模块 | 每个 pass 的拓扑变换与优化前后业务结果 |
| 内置节点 | `flow-rs/src/node/` | 有 Bcast、Merge、Transform 等，已补 Reorder 普通消息路径，缺少完整动态转换、Demux、shared 和空信号协议 | 每类节点的数据、元信息、关闭和边界行为 |
| 资源与上下文 | `flow-rs/src/resource/`、`graph/context.rs` | 简化按名获取 | 作用域、共享、构造次数及释放顺序 |
| 消息业务类型 | `flow-message/src/algo_base/` | 当前同名 crate 主要承载引擎信封 | Frame、Image、Item、Feature 等模型及转换 |
| Rust 服务插件 | `flow-plugins/src/` | 工作区没有对应 crate | bytes/image/video 服务和 glider/limbo 协议行为 |
| 调试与性能观测 | `flow-rs/src/debug/`、`profile/` | 无对应模块 | 协议、图信息、指标采集与关闭 |
| 初学者独立复现 | 各章代码与 `code/` | 多数片段为 `rust,ignore`，没有证明按章从空目录构建 | 逐章起点、文件改动、完整代码、命令、预期结果和排错 |

这里“有对应源码”不等于“已证明语义相同”。例如原版和当前 Merge 都用
`select_ok`，不能因此宣称保证公平，也不能单凭方法同名宣称兼容。

## 宏专题必须达到什么程度

1. 入门：宏与函数、展开阶段、token tree、`macro_rules!` 匹配和重复、作用域与 `$crate`。
2. 基础工程：普通库、过程宏库和调用者的依赖方向；解释每个 Cargo 配置项。
3. 三种过程宏：函数式、derive、attribute，分别写完整可运行案例。
4. crate 实操：`proc_macro`、`proc-macro2`、`syn`、`quote` 的边界、常用类型、features、输入输出和错误处理。
5. 进阶：自定义 `Parse`、`Punctuated`、泛型/生命周期/where、span、辅助属性、展开顺序与依赖改名。
6. 项目实战：端口、Node、Actor、methods、构造器、节点/资源注册，以及原版额外宏逐个迁移。
7. crate 协作：`inventory` 的 collect/submit/iter 与链接后的运行时枚举；Tokio、Serde、thiserror 的宏如何进入生成代码。
8. 工程验收：语法树单测、下游成功编译测试、失败诊断测试、展开检查、行为测试；不能只检查输出字符串含某个词。

Ch2.0 已增加声明宏入门；Ch2.2a/b 提供 token 实验、三种过程宏、泛型与独立检查点；
Ch2.3a 增加 trybuild、span、错误路径收尾和语法树端口分类；Ch2.4a 增加
inventory 实验、crate 协作和原版宏逐项差距。完整原版宏协议仍需随着引擎功能继续迁移。
“从入门到精通”以能独立实现、解释、调试并维护这些宏为验收标准。

## 每完成一项，如何记录

记录原版文件和版本、输入场景、预期输出/错误/副作用、重写文件、测试命令、
实际结果以及差异。涉及异步的测试应设置超时，并验证输出数量与任务退出。
只断言每个收到的结果正确不够：没有任何结果时也可能虚假通过。

当前基线测试命令为 `cargo test --manifest-path code/Cargo.toml --workspace --locked`。
它证明现有测试覆盖的行为，不代表上表未实现项通过。mdBook 构建和逐章复现
还需要独立执行，`rust,ignore` 片段不会自动得到编译验证。


## 本轮已经取得的验证证据

- 宏课已补充 Ch2.0、Ch2.2a/b、Ch2.3a、Ch2.4a；独立宏实验从空工作区分三步运行，
  用 `python3 scripts/check_macro_course.py` 统一复验。
- TypeName 已用真实下游测试验证泛型、生命周期、默认类型参数、const 参数和 where。
- 端口诊断有五个 trybuild 用例；端口分类改用语法树，防止 HistorySender 被误关。
- Actor 的普通 exec 错误路径会 close 后 finalize，再保留原错误返回；有事件顺序和超时测试。
- BinaryOp 四则运算和左信封元信息已补回归测试。
- mdBook 已在本机使用配置对应的 0.5.4 / toc 0.15.4 / mermaid 0.17.1 完成构建。

这些证据只对应上述条目。尤其 methods 的原版参数适配、配置更新回调、动态端口等
仍未完成；详见 Ch2.4a 的完整原版宏清单。


## 独立宏课程的增补记录

宏教学已整理为 [10 课连续专题](../macros/00-roadmap.md)，覆盖声明宏递归与片段转发、
三种过程宏、syn/quote 实操、辅助属性与泛型约束、路径与属性组合、诊断测试、
构建性能和发布维护。新增 Describe 实验在独立下游验证生命周期、const 泛型、
已有 where、默认类型参数及跳过字段不施加多余约束；课程检查脚本覆盖这些实验。

这份记录只更新教学覆盖，不将原版尚未实现的宏和运行时能力标为完成。
依赖改名、no_std、feature 矩阵等已解释实现和验收方法，但未验证的能力在课程中明确标注。

## Reorder 与信封的验证记录

七项元信息与寻址转换由 envelope_contract 验证。Reorder 使用原版 exec 原文加测试 I/O 适配器作为对照，比较 720 种排列以及重复、缺口、缺失序号、空载荷场景；另验证容量 1、元信息与 Arc 身份、下游关闭、输入未关闭时输出连续前缀。详见 [Reorder 教程](../part4/ch05-reorder.md)。此对照没有运行原版完整通道和 Actor，不能证明 flush 协议或完整运行时已对齐。

## Sandbox 数据源接口记录

add_data 已改为原版 `FnMut(usize) -> Option<T>` 形式，Vec 便利入口另命名 add_items；
发送失败仍继续调用源直到 None，匹配原版有限数据源的副作用次数。sandbox_envelopes
测试验证延迟执行、索引、数量和提前关闭。同名回调替换已改为原版的按名字覆盖，并用旧回调 panic、新回调输出的测试验证；完整资源与动态端口仍待补齐。

## 初学者复现记录

消息层新增独立检查点：message_checkpoint.py 导出不继承主 workspace 的完整工程，
check_message_course.py 在全新临时目录离线构建并运行 12 项测试。CI 已增加该检查。
这证明 Ch1.3 的终点可以独立复现；逐章中间步骤和其余运行时章节仍需继续建立检查点。
