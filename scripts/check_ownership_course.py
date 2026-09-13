#!/usr/bin/env python3
"""Replay the first Rust lesson and its deliberate compilation failures."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
source = (ROOT / "book/labs/ownership-basics/main.rs").read_text()
with tempfile.TemporaryDirectory(prefix="megflow-ownership-") as directory:
    target = Path(directory)
    (target / "main.rs").write_text(source)
    subprocess.run(["rustc", "--edition=2021", "main.rs", "-o", "study"],
                   cwd=target, check=True, timeout=60)
    result = subprocess.run([str(target / "study")], cwd=target,
                            check=True, capture_output=True, text=True, timeout=10)
    assert result.stdout.strip() == "移动、借用、克隆、线程共享与错误分支：通过"
    for statement, code in [
        ("consume(frame);", "E0382"),
        ("needs_static(local.as_str());", "E0597"),
        ("needs_send(std::rc::Rc::new(1));", "E0277"),
    ]:
        broken = source.replace("// " + statement, statement)
        assert broken != source
        (target / "broken.rs").write_text(broken)
        result = subprocess.run(["rustc", "--edition=2021", "broken.rs"],
                                cwd=target, capture_output=True, text=True, timeout=60)
        assert result.returncode != 0 and code in result.stderr, result.stderr
print("Ch1.1：独立实验及三种预期编译失败通过。")
