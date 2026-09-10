#!/usr/bin/env python3
"""Run registry lessons without compiling any MegFlow runtime module."""
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix="megflow-registry-course-") as directory:
    target = Path(directory)
    source = ROOT / "code/flow-rs/examples/registry_from_functions.rs"
    subprocess.run(["rustc", "--edition=2021", str(source), "-o", str(target / "manual")],
                   check=True, timeout=60)
    result = subprocess.run([str(target / "manual")], check=True, capture_output=True,
                            text=True, timeout=10)
    assert result.stdout.strip() == "注册原理通过：按名查构造器、独立实例、不同类型统一调用、未知名称报错"
    (target / "src").mkdir()
    shutil.copyfile(ROOT / "code/flow-rs/examples/registry_basics.rs", target / "src/main.rs")
    (target / "Cargo.toml").write_text('''[package]
name = "inventory-study"
version = "0.1.0"
edition = "2021"
[workspace]
[dependencies]
inventory = "0.3"
''')
    shutil.copyfile(ROOT / "code/Cargo.lock", target / "Cargo.lock")
    result = subprocess.run(["cargo", "run", "--manifest-path", str(target / "Cargo.toml"), "--offline"],
                            check=True, capture_output=True, text=True, timeout=180)
    assert result.stdout.strip() == '注册与调用通过：[("double", 6), ("increment", 4)]'
print("注册课程：标准库构造表与独立 inventory 工程通过。")
