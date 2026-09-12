//! flow-rs · tests/dyn_demux_graph —— Ch4.9c：注册版 `DynDemux` 装进**真实 TOML 图**的端到端契约。
//!
//! 与 tests/dyn_wiring_e2e.rs 的区别在于「触发方是谁」：那边触发节点 `AutoTrigger` 是**测试
//! 夹具**、且 feed+collect 双向；这边触发方是 `builtin.rs` 里的**注册内置节点** `DynDemux`
//! ——真实 TOML 写 `ty="DynDemux"` 即可，全程走 config 自动接线（Ch4.9b）+ 注册表，无任何手写
//! `DynPorts`。且 `DynDemux` 是**喂入侧单向**的（对齐原版 `node/demux.rs`）：只往实例入口喂、
//! 不从实例收，实例是**汇子图**（末端落进资源、无对外出口）——对齐原版 `logical_test.toml`
//! 的 `destination` 汇子图。
//!
//! **怎么证「create-once + 路由」而实例又没有可观察输出？** 用一份共享资源 `Recorder`（实例日志）：
//! 汇子图里的 `Probe` 在 `initialize` 记一条 `"init"`（每实例一次 → 数它即得**实例数**）、每收
//! 一条消息记下**载荷**。图跑完后读 `Recorder`：`init` 数 == 不同 key 数（**create-once**），
//! 载荷齐全且路由到对齐 key（**routing**）。注入资源随 `DynDemux` 的 `create` 穿进实例——这也
//! 顺带验证了 Ch4.9 记过的「资源注入」（`ext_resource.chain` 的链接语义本弧仍 defer）。
//!
//! 另有一个 Sandbox 测试：`DynDemux` 在**单节点沙箱**里跑通——沙箱给 dyn 输出配了内部 broker +
//! NoopConsumer 汇子图（Ch4.9c 的 sandbox 支持），create→fetch→route→拆除→干净停机全程不挂。
//! 全部 `tokio::time::timeout` 包裹：teardown 一旦挂起即超时失败而非卡死 CI。
//!
//! 恒等护栏：本文件只**新增**测试，既有 subgraph/graph/demux/资源/dyn_ports/dyn_wiring 测试一字不改继续绿。

use flow_rs::prelude::*;
use std::sync::{Arc, Mutex};
use std::time::Duration;

// ANCHOR: fixtures
/// 共享资源「实例日志」：汇子图实例里的 `Probe` 往它记两类条目——每实例 `initialize` 记一条
/// `"init"`、每收一条消息记下**载荷**。图跑完后读它即可同时验证 **create-once**（`"init"` 条数
/// == 不同 key 数）与 **routing**（载荷齐全）。`Mutex<Vec<..>>` 即够：并发实例各记各的，读在图停后。
#[derive(Default)]
struct Recorder {
    log: Mutex<Vec<String>>,
}

impl Recorder {
    fn record(&self, entry: impl Into<String>) {
        self.log.lock().unwrap().push(entry.into());
    }
    fn snapshot(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }
}

impl BuildResource for Recorder {
    fn build(_args: &Args) -> Result<Self> {
        Ok(Recorder::default())
    }
}
resource_register!("Recorder", Recorder);

/// 汇子图里的探针**汇**节点（`#[outputs]` 为空 = 纯 sink，对齐原版 `TransGraph` 末端的 Printer）：
/// `initialize` 借出共享 `Recorder`、记一条 `"init"`（每实例只跑一次 → 数它即得实例数）；每 `exec`
/// 收一条消息、downcast 成 String、记下载荷后丢弃。子图无对外输出，故 `DynDemux` 只喂不收。
#[inputs(inp)]
#[outputs]
#[derive(Node, Actor, BuildFromPorts)]
struct Probe {
    /// 自有参数：要借用的资源名（TOML 里 `res="rec"`）。
    res: String,
    /// 运行期句柄：`#[state]` → 不从 args 反序列化，`initialize` 里按名从注入资源集借出。
    #[state]
    rec: Option<Arc<Recorder>>,
}

#[methods]
impl Probe {
    async fn initialize(&mut self, ctx: &Context) {
        // 资源随 DynDemux 的 create 注入进实例；每个实例的 Probe 恰 initialize 一次。
        self.rec = ctx.resource::<Recorder>(&self.res);
        if let Some(rec) = self.rec.as_ref() {
            rec.record("init");
        }
    }

    async fn exec(&mut self) -> Result<()> {
        let mut msg = self.inp.recv_any().await?;
        let payload = msg
            .downcast_mut::<Envelope<String>>()
            .expect("Probe expects a String payload")
            .unpack();
        if let Some(rec) = self.rec.as_ref() {
            rec.record(payload);
        }
        Ok(())
    }
}
node_register!("Probe", Probe);
// ANCHOR_END: fixtures

// ANCHOR: toml
// 一份 TOML 就够：汇子图 `worker`（一个 Probe，借资源 `rec`）+ 顶层 `top`（注册版 `DynDemux`
// + 动态子图站点 `site`）。关键是那条 **dyn 连接** `demux:out ↔ site:inp`——config 层据此自动
// 接线（Ch4.9b），无一行手写 `DynPorts`。资源 `rec` 声明在 `top`：装配期建一次，随 `DynDemux`
// 的 `create` 注入进每个实例，实例里的 `Probe` 按名借出。
const TOML: &str = r#"
main = "top"

[[graphs]]
name = "worker"
nodes = [{name="p", ty="Probe", res="rec"}]
inputs = [{name="inp", cap=1, ports=["p:inp"]}]

[[graphs]]
name = "top"
nodes = [{name="demux", ty="DynDemux"}, {name="site", ty="worker"}]
inputs = [{name="in", cap=1, ports=["demux:inp"]}]
connections = [{cap=1, ports=["demux:out", "site:inp"]}]
resources = [{name="rec", ty="Recorder"}]
"#;
// ANCHOR_END: toml

/// 造一个带目标地址 key 的有载荷信封（sealed，喂给图对外输入）。
fn addressed(payload: &str, key: u64) -> SealedEnvelope {
    Envelope::with_info(
        payload.to_owned(),
        EnvelopeInfo {
            to_addr: Some(key),
            ..Default::default()
        },
    )
    .seal()
}

/// 造一个带目标地址的**空**信封（`is_none` 为真）——生命周期拆除信号（sealed）。
fn teardown(key: u64) -> SealedEnvelope {
    let mut empty = Envelope::<String>::empty();
    empty.info_mut().to_addr = Some(key);
    empty.seal()
}

// ANCHOR: e2e
/// 主线：注册版 `DynDemux` 在一份真实 TOML 里经 `Builder::build` 自动接线，按 key 现装汇子图实例。
/// 断言两件事：**create-once**（不同 key 数 == `"init"` 条数）与 **routing**（载荷全部到达实例）。
#[tokio::test]
async fn dyn_demux_routes_streams_in_a_real_graph() {
    tokio::time::timeout(Duration::from_secs(5), async {
        // 只写 TOML：ty="DynDemux" + 一条 dyn 连接，全由 config 自动接线（Ch4.9b）。
        let mut graph = Builder::default().template(TOML).build().unwrap();
        // 图跑完后读回这份注入资源，验证实例侧记录（build 后即持有同一个 Arc）。
        let recorder = graph.resource::<Recorder>("rec").expect("resource 'rec'");
        let input = graph.input("in").expect("external input 'in'");
        let task = graph.start();

        // key 7 两帧、key 42 一帧：key 7 复用同一实例（不重建），key 42 是另一份独立实例。
        input.send_any(addressed("a", 7)).await.unwrap();
        input.send_any(addressed("b", 7)).await.unwrap();
        input.send_any(addressed("c", 42)).await.unwrap();

        // 空信封拆除两个 key 的实例（DynDemux 撤入口端点 + await 实例任务——await 保证实例把
        // 缓冲里的载荷全 drain 完、记录落地后才收尾）。
        input.send_any(teardown(7)).await.unwrap();
        input.send_any(teardown(42)).await.unwrap();

        // 停机：撤掉对外输入的两份 Sender → 关闭涟漪传导、聚合句柄解析（含 DynDemux.finalize）。
        drop(input);
        graph.stop();
        task.await.unwrap().unwrap();

        // 读实例日志断言。task.await 已确保所有实例（经 DynDemux 拆除时 await）收尾、记录落地。
        let log = recorder.snapshot();
        let inits = log.iter().filter(|e| e.as_str() == "init").count();
        let mut payloads: Vec<&str> = log
            .iter()
            .map(String::as_str)
            .filter(|e| *e != "init")
            .collect();
        payloads.sort_unstable();

        assert_eq!(
            inits, 2,
            "create-once：2 个不同 key（7、42）→ 恰 2 个实例；key 7 的两帧复用同一实例。实际日志：{log:?}"
        );
        assert_eq!(
            payloads,
            vec!["a", "b", "c"],
            "routing：三条载荷都路由进了对应 key 的实例。实际日志：{log:?}"
        );
    })
    .await
    .expect("dyn demux graph e2e timed out");
}
// ANCHOR_END: e2e

/// 造一个带目标地址 key 的**未封箱** `Envelope<String>`（喂给 Sandbox 的 `add_envelope`）。
fn addressed_env(payload: &str, key: u64) -> Envelope<String> {
    Envelope::with_info(
        payload.to_owned(),
        EnvelopeInfo {
            to_addr: Some(key),
            ..Default::default()
        },
    )
}

/// 造一个带目标地址的**空** `Envelope<String>`（拆除信号，未封箱）。
fn teardown_env(key: u64) -> Envelope<String> {
    let mut empty = Envelope::<String>::empty();
    empty.info_mut().to_addr = Some(key);
    empty
}

// ANCHOR: sandbox
/// `DynDemux` 在**单节点沙箱**里跑通：沙箱给它的 dyn 输出 `out` 配了内部 broker + NoopConsumer
/// 汇子图（Ch4.9c 的 sandbox 支持）。只喂静态输入 `inp`——create→fetch→route→拆除→干净停机
/// 全程不挂即为通过（timeout 守住任何挂起）。这是原版 sandbox 动态端口支持的忠实（教学）重写。
#[tokio::test]
async fn dyn_demux_runs_in_sandbox() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut sb = Sandbox::pure("DynDemux").unwrap();
        // 喂 inp：key 1 两帧（create + 复用），再一条空信封拆除 key 1；随后数据源结束、输入关闭。
        sb.add_envelope("inp", |i: usize| match i {
            0 => Some(addressed_env("x", 1)),
            1 => Some(addressed_env("y", 1)),
            2 => Some(teardown_env(1)),
            _ => None,
        });
        sb.start().await.unwrap();
    })
    .await
    .expect("sandbox dyn demux timed out");
}
// ANCHOR_END: sandbox
