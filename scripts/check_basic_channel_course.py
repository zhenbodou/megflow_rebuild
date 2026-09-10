#!/usr/bin/env python3
"""Compile the introductory channel, without the later runtime or macros."""
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix="megflow-basic-channel-") as directory:
    target = Path(directory) / "channel-basic"
    shutil.copytree(ROOT / "book/labs/channel-basic", target)
    subprocess.run([
        "python3", str(ROOT / "scripts/message_checkpoint.py"), "--out", str(target / "message")
    ], check=True, timeout=60)
    shutil.copyfile(ROOT / "code/Cargo.lock", target / "Cargo.lock")
    subprocess.run([
        "cargo", "test", "--manifest-path", str(target / "Cargo.toml"), "--offline"
    ], check=True, timeout=180)
print("基础通道章：独立工程五项行为测试通过。")
