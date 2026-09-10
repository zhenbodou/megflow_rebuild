//! 集成测试：Ch5.1 的 `prelude` 门面。
//!
//! 通篇**只有一行** `use flow_rs::prelude::*;`（外加 std 的 `Arc`）——却足以①定义并注册一个
//! **节点**、②定义并注册一份**资源**、③用 `Builder` 搭图、跑图、图外读回资源。这钉死了门面
//! 的**完备性**：换成真实下游算法仓，node / resource 作者与 app 作者都只需这一行 use，不必再
//! 逐个 `use flow_rs::channel::…` / `flow_rs::node::…` / `flow_derive::…` 铺一屏导入。
//!
//! An integration test proving the `prelude` facade is complete: a single
//! `use flow_rs::prelude::*;` suffices to author + register a node, author +
//! register a resource, and build + run a graph.

// ANCHOR: prelude_authoring
use flow_rs::prelude::*;
use std::sync::Arc; // 唯一的额外导入是标准库的 Arc（节点持有 `Arc<资源>` 句柄）——它不属于本引擎。

// ── 只靠 prelude 定义一份资源 ────────────────────────────────────────────────
// 证明 `BuildResource` trait、`resource_register!` 宏、`Args` 类型都从门面里来。

/// 一个最小共享资源：原子计数器（`Counter` 的同款替身，但独立命名以免与内置注册项撞名）。
#[derive(Default)]
struct Bag {
    seen: std::sync::atomic::AtomicU64,
}

impl Bag {
    fn bump(&self) {
        self.seen.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    fn get(&self) -> u64 {
        self.seen.load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl BuildResource for Bag {
    fn build(_args: &Args) -> Result<Self> {
        Ok(Bag::default())
    }
}

resource_register!("PreludeBag", Bag);

// ── 只靠 prelude 定义一个节点 ────────────────────────────────────────────────
// 证明 `#[inputs]`/`#[outputs]`/`#[methods]`、派生宏 `Node`/`Actor`/`BuildFromPorts`、
// `#[state]`、`node_register!`、`Envelope`、`Context`、`Result` 以及 channel 的 recv/send
// 全从门面里来。它把 i32 收进、+1 发出，并在共享资源上 bump 一次。

#[inputs(inp)]
#[outputs(out)]
#[derive(Node, Actor, BuildFromPorts)]
struct PreludeTally {
    /// 自有参数：要借用的资源名（TOML 里 `res="bag"`）。
    res: String,
    /// 运行期句柄：`#[state]` → 不从 args 反序列化，`initialize` 里按名借出。
    #[state]
    bag: Option<Arc<Bag>>,
}

#[methods]
impl PreludeTally {
    async fn initialize(&mut self, ctx: &Context) {
        self.bag = ctx.resource::<Bag>(&self.res);
    }

    async fn exec(&mut self) -> Result<()> {
        let mut env = self.inp.recv::<i32>().await?;
        let v = env.unpack();
        if let Some(b) = self.bag.as_ref() {
            b.bump();
        }
        if let Some(out) = self.out.as_ref() {
            out.send(Envelope::new(v + 1)).await?;
        }
        Ok(())
    }
}

node_register!("PreludeTally", PreludeTally);

// 一段最小图：单节点 t 借用资源 bag，对外 in→t:inp、t:out→out。
const PRELUDE_GRAPH: &str = r#"
main = "g"

[[graphs]]
name = "g"
resources = [{name = "bag", ty = "PreludeBag"}]
nodes = [{name = "t", ty = "PreludeTally", res = "bag"}]
inputs = [{name = "in", cap = 8, ports = ["t:inp"]}]
outputs = [{name = "out", cap = 8, ports = ["t:out"]}]
"#;

#[tokio::test]
async fn prelude_only_imports_suffice_end_to_end() {
    // 搭图、跑图——全用门面里的 `Builder`。
    let mut g = Builder::default().template(PRELUDE_GRAPH).build().unwrap();
    let handle = g.start();
    let tx = g.input("in").unwrap();
    let out = g.take_output("out").unwrap();

    for v in [10i32, 20, 30] {
        tx.send(Envelope::new(v)).await.unwrap();
    }
    // 定量收 3 条（守 Ch4.2 硬教训：图保留对外输入 Sender 直到 stop，不能 drain-到-close）。
    let mut got = Vec::new();
    for _ in 0..3 {
        got.push(out.recv::<i32>().await.unwrap().unpack());
    }
    assert_eq!(got, vec![11, 21, 31], "门面定义的节点端到端 +1");

    // 门面定义的资源被门面定义的节点正常共享：收满 3 条时（先 bump 再 send）计数必为 3。
    let bag = g.resource::<Bag>("bag").unwrap();
    assert_eq!(bag.get(), 3, "prelude 门面定义的资源被正常构造、共享、读回");

    drop(tx);
    g.stop();
    handle.await.unwrap().unwrap();
}
// ANCHOR_END: prelude_authoring

// ANCHOR: prelude_compile_checks
// ── 编译期完备性检查：其余门面名字也都从这一行 glob 里解析得到 ──────────────
// 下面这些 item 永不被调用，只要它们**能编译**，就证明对应名字在作用域里。

/// `TypeName` 派生宏在作用域（它生成固有 `type_name()`，自带绝对路径，无需额外导入）。
#[derive(TypeName)]
struct _PreludeName {}

/// 同名的 **trait**（类型命名空间）与**派生宏**（宏命名空间）在 prelude 里并存——这正是上面
/// `#[derive(Node, Actor, BuildFromPorts)]` 用其宏形态、而这里用其 trait 形态做约束、二者互不
/// 打架的「serde 套路」。`BuildResource` 也一并钉住。
#[allow(dead_code)]
fn _traits_in_scope<N: Node, A: Actor, B: BuildFromPorts, R: BuildResource>() {}

/// `Error` / `Sandbox` / `MainGraph` / `Context` 四个类型名在作用域（作参数类型引用即可，
/// 无需真的构造）。
#[allow(dead_code)]
fn _types_in_scope(_a: Error, _b: Sandbox, _c: MainGraph, _d: Context) {}

/// `channel` 函数与 `Sender` / `Receiver` 类型在作用域。
#[allow(dead_code)]
fn _channel_in_scope() -> (Sender, Receiver) {
    channel(1)
}
// ANCHOR_END: prelude_compile_checks
