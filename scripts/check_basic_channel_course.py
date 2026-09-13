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
    for step in ("01", "02", "03", "04", "05", "06", "07", "08", "09", "10", "11", "12", "12a", "13", "14", "15", "16"):
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
            shutil.copyfile(ROOT / "code/flow-rs/tests/conversion.rs", target / "tests/conversion.rs")
        if step == "12a":
            shutil.copyfile(ROOT / "code/flow-rs/tests/channel_type_guess.rs", target / "tests/channel_type_guess.rs")
        subprocess.run([
            "cargo", "test", "--manifest-path", str(target / "Cargo.toml"), "--workspace", "--offline"
        ], check=True, timeout=180)
        print(f"通道第 {step} 步通过", flush=True)
print("通道到首个节点宏：同一工程十六步及 12a 补充构建通过。")
