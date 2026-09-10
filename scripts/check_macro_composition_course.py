#!/usr/bin/env python3
"""Exercise AST traversal and cfg combinations in independent course projects."""
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix="megflow-macro-composition-") as directory:
    target = Path(directory)
    ast = target / "ast"
    shutil.copytree(ROOT / "book/labs/ast-walk", ast)
    shutil.copyfile(ROOT / "code/Cargo.lock", ast / "Cargo.lock")
    subprocess.run(["cargo", "run", "--manifest-path", str(ast / "Cargo.toml"), "--offline"],
                   check=True, timeout=180)
    lab = target / "macro-labs"
    shutil.copytree(ROOT / "code/macro-labs", lab, ignore=shutil.ignore_patterns("target"))
    for features in ([], ["--features", "metrics"]):
        subprocess.run(["cargo", "test", "--manifest-path", str(lab / "Cargo.toml"),
                        "-p", "macro-lab-app", "--test", "composition", "--offline", *features],
                       check=True, timeout=180)
print("宏组合：AST 遍历及 metrics 开/关测试通过。")
