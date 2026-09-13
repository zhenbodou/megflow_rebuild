#!/usr/bin/env python3
"""Compile the beginner trait lesson with rustc only, without later crates."""
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix="megflow-traits-course-") as directory:
    target = Path(directory)
    shutil.copyfile(ROOT / "book/labs/traits-basics/main.rs", target / "main.rs")
    subprocess.run(["rustc", "--edition=2021", "main.rs", "-o", "traits-study"],
                   cwd=target, check=True, timeout=60)
    result = subprocess.run([str(target / "traits-study")], cwd=target,
                            check=True, text=True, capture_output=True, timeout=10)
    assert result.stdout.strip() == "泛型、借用与装箱 trait 对象、Any 借用及所有权恢复：通过"
    source = (target / "main.rs").read_text()
    experiments = [
        (source.replace("// borrowed.repeat(9u32);", "borrowed.repeat(9u32);"),
         "cannot be invoked on a trait object"),
        (source.replace("    where\n        Self: Sized,\n", ""), "E0038"),
    ]
    for broken, diagnostic in experiments:
        assert broken != source, "排错实验未修改源文件"
        (target / "broken.rs").write_text(broken)
        failure = subprocess.run(
            ["rustc", "--edition=2021", "broken.rs", "-o", "broken-study"],
            cwd=target, text=True, capture_output=True, timeout=60)
        assert failure.returncode != 0, "排错实验意外编译成功"
        assert diagnostic in failure.stderr, failure.stderr
print("Ch1.2：独立标准库实验、输出断言及两种预期编译失败通过。")
