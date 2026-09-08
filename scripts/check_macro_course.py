#!/usr/bin/env python3
"""运行宏课完整示例，并在临时目录从零构建三个独立检查点。"""
from pathlib import Path
import subprocess
import sys
import tempfile

root = Path(__file__).resolve().parents[1]

def run(*command):
    print("+", " ".join(map(str, command)), flush=True)
    subprocess.run(list(map(str, command)), cwd=root, check=True, timeout=300)

for package, example in (
    ("flow-derive", "macro_rules_basics"),
    ("flow-derive", "macro_rules_advanced"),
    ("flow-derive", "token_workshop"),
    ("flow-derive", "node_expansion_walkthrough"),
    ("flow-rs", "registry_basics"),
    ("flow-rs", "registry_from_functions"),
):
    run("cargo", "run", "--manifest-path", "code/Cargo.toml", "-p", package,
        "--example", example, "--locked")

run("cargo", "run", "--manifest-path", "code/macro-labs/Cargo.toml", "-p",
    "macro-lab-app", "--locked")

run("cargo", "test", "--manifest-path", "code/macro-labs/Cargo.toml", "--locked")

with tempfile.TemporaryDirectory(prefix="megflow-macro-course-") as directory:
    temporary = Path(directory)
    for stage in (1, 2, 3):
        target = temporary / str(stage)
        run(sys.executable, "scripts/macro_checkpoint.py", "--stage", stage, "--out", target)
        run("cargo", "run", "--manifest-path", target / "Cargo.toml", "-p",
            "macro-lab-app", "--locked", "--target-dir", temporary / "target")
print("宏课程：七个示例、进阶宏测试和三个独立检查点全部通过。")
