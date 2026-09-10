#!/usr/bin/env python3
"""在新目录编译 Broker 章：只复制该章文件及前置消息层。"""
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

root = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix="megflow-broker-course-") as directory:
    target = Path(directory)
    shutil.copytree(root / "book/labs/broker", target, dirs_exist_ok=True)
    subprocess.run([
        sys.executable, str(root / "scripts/message_checkpoint.py"),
        "--out", str(target / "message"),
    ], check=True)
    shutil.copyfile(root / "code/flow-rs/src/broker.rs", target / "src/broker.rs")
    (target / "tests").mkdir()
    shutil.copyfile(root / "code/flow-rs/tests/broker.rs", target / "tests/broker.rs")
    # 共用已提交锁文件的版本解析，允许 Cargo 为隔离工程删去未使用的包。
    shutil.copyfile(root / "code/Cargo.lock", target / "Cargo.lock")
    for arguments in (["test"], ["run", "--example", "notify"]):
        subprocess.run([
            "cargo", *arguments, "--manifest-path", str(target / "Cargo.toml"), "--offline",
        ], check=True, timeout=180)
print("Broker 章节：独立工程测试与实例通知示例通过。")
