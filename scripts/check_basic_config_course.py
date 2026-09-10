#!/usr/bin/env python3
"""Verify the four-structure configuration stage without the graph runtime."""
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix="megflow-basic-config-") as directory:
    target = Path(directory) / "config-basic"
    shutil.copytree(ROOT / "book/labs/config-basic", target)
    shutil.copyfile(ROOT / "code/Cargo.lock", target / "Cargo.lock")
    subprocess.run([
        "cargo", "test", "--manifest-path", str(target / "Cargo.toml"), "--offline"
    ], check=True, timeout=180)
print("配置基础章：四结构完整工程，六项测试通过。")
