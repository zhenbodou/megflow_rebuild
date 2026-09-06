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
    ("flow-derive", "token_workshop"),
    ("flow-rs", "registry_basics"),
):
    run("cargo", "run", "--manifest-path", "code/Cargo.toml", "-p", package,
        "--example", example, "--locked")

run("cargo", "run", "--manifest-path", "code/macro-labs/Cargo.toml", "-p",
    "macro-lab-app", "--locked")

with tempfile.TemporaryDirectory(prefix="megflow-macro-course-") as directory:
    temporary = Path(directory)
    for stage in (1, 2, 3):
        target = temporary / str(stage)
        run(sys.executable, "scripts/macro_checkpoint.py", "--stage", stage, "--out", target)
        run("cargo", "run", "--manifest-path", target / "Cargo.toml", "-p",
            "macro-lab-app", "--locked", "--target-dir", temporary / "target")
print("宏课程：四个示例和三个独立检查点全部通过。")
