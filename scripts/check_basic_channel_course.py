#!/usr/bin/env python3
"""Compile the introductory channel, without the later runtime or macros."""
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix="megflow-basic-channel-") as directory:
    target = Path(directory) / "channel-basic"
    (target / "src").mkdir(parents=True)
    subprocess.run([
        "python3", str(ROOT / "scripts/message_checkpoint.py"), "--stage", "envelope", "--out", str(target / "message")
    ], check=True, timeout=60)
    shutil.copyfile(ROOT / "code/Cargo.lock", target / "Cargo.lock")
    for step in ("01", "02", "03", "04", "05", "06", "07", "08", "09", "10", "11", "12", "12a", "13", "14", "15", "16", "17", "18", "19", "20", "21", "22"):
        if step in ("01", "02", "03"):
            files = {"Cargo.toml": f"channel-steps/{step}/Cargo.toml",
                     "src/lib.rs": f"channel-steps/{step}/lib.rs"}
            if step == "03":
                files["src/error.rs"] = "channel-steps/03/error.rs"
        elif step == "04":
            files = {"src/lib.rs": "channel-steps/04/lib.rs",
                     "src/channel.rs": "channel-steps/04/channel.rs"}
        elif step == "05":
            files = {name: f"channel-basic/{name}" for name in
                     ("Cargo.toml", "src/lib.rs", "src/error.rs", "src/channel.rs")}
        elif step in ("14", "15"):
            files = {"src/lib.rs": "node-steps/lib.rs",
                     "src/node.rs": f"node-steps/{step}/node.rs"}
        elif step == "16":
            files = {"Cargo.toml": "node-steps/16/Cargo.toml",
                     "src/lib.rs": "node-steps/16/lib.rs",
                     "src/node.rs": "node-steps/16/node.rs",
                     "derive/Cargo.toml": "node-steps/16/derive-Cargo.toml",
                     "derive/src/lib.rs": "node-steps/16/derive-lib.rs"}
        elif step in ("17", "18", "19", "20"):
            # 沿用 step16 的 Cargo.toml / lib.rs / derive/Cargo.toml（累积保留），
            # 每步只覆盖 node.rs（Doubler 用上新宏）与 derive/src/lib.rs（宏本身长出新能力）。
            files = {"src/node.rs": f"node-steps/{step}/node.rs",
                     "derive/src/lib.rs": f"node-steps/{step}/derive-lib.rs"}
        elif step == "21":
            # Ch2.4 注册表第一步：给 flow-rs 加 inventory 依赖 + registry 模块（表 + BuildFromPorts
            # 契约）+ derive(BuildFromPorts) 宏。node.rs 沿用第二十步（累积保留、不覆盖）；
            # 新增独立集成测试 tests/register.rs，先直接 build() 跑通。
            files = {"Cargo.toml": "node-steps/21/Cargo.toml",
                     "src/lib.rs": "node-steps/21/lib.rs",
                     "src/registry.rs": "node-steps/21/registry.rs",
                     "derive/src/lib.rs": "node-steps/21/derive-lib.rs",
                     "tests/register.rs": "node-steps/21/register.rs"}
        elif step == "22":
            # Ch2.4 注册表第二步：加函数式宏 node_register!（第三种宏形态）。Cargo.toml / lib.rs /
            # registry.rs / node.rs 全沿用第二十一步；只覆盖 derive/src/lib.rs 与 tests/register.rs
            # （后者补上 node_register! + 按名字查找/构造/运行的端到端测试）。
            files = {"derive/src/lib.rs": "node-steps/22/derive-lib.rs",
                     "tests/register.rs": "node-steps/22/register.rs"}
        else:
            files = {"src/channel.rs": f"channel-steps/{step}/channel.rs"}
            if step == "08":
                files["Cargo.toml"] = "channel-steps/08/Cargo.toml"
            if step in ("09", "10", "11", "12", "13"):
                files["src/channel/typed.rs"] = f"channel-steps/{step}/typed.rs"
            if step == "11":
                files.update({"src/lib.rs": "channel-steps/11/lib.rs",
                              "src/config.rs": "channel-steps/11/config.rs",
                              "src/config/interlayer.rs": "channel-steps/11/interlayer.rs"})
            if step == "12":
                files["src/channel/conversion.rs"] = "channel-steps/12/conversion.rs"
            if step == "12a":
                files["src/channel/conversion.rs"] = "channel-steps/12a/conversion.rs"
                files["src/error.rs"] = "channel-steps/12a/error.rs"
            if step == "13":
                files["Cargo.toml"] = "channel-steps/13/Cargo.toml"
                files["tests/channel_close.rs"] = "channel-steps/13/channel_close.rs"
        for destination, source in files.items():
            (target / destination).parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / "book/labs" / source, target / destination)
        if step == "12":
            (target / "tests").mkdir(exist_ok=True)
            shutil.copyfile(ROOT / "book/labs/channel-steps/12/conversion_test.rs", target / "tests/conversion.rs")
        if step == "12a":
            shutil.copyfile(ROOT / "book/labs/channel-steps/12a/channel_type_guess.rs", target / "tests/channel_type_guess.rs")
        subprocess.run([
            "cargo", "test", "--manifest-path", str(target / "Cargo.toml"), "--workspace", "--offline"
        ], check=True, timeout=180)
        print(f"通道第 {step} 步通过", flush=True)
print("通道到过程宏五件套 + 编译期注册表：同一工程二十二步及 12a 补充构建通过。")
