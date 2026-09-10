#!/usr/bin/env python3
"""Build only the ten complete files printed in the environment chapter."""
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
CHAPTER = ROOT / "book/src/part0/ch02-environment.md"
EXPECTED = {
    "code/Cargo.toml",
    *(f"code/{crate}/{file}" for crate in ("flow-message", "flow-derive", "flow-rs")
      for file in ("Cargo.toml", "src/lib.rs")),
    "book/book.toml", "book/src/SUMMARY.md", "book/src/workspace.md",
}


def main():
    blocks = re.findall(
        r"<!-- course-file: ([^\n]+) -->\n```[^\n]*\n(.*?)\n```",
        CHAPTER.read_text(), re.DOTALL,
    )
    paths = [path for path, _ in blocks]
    if len(paths) != len(set(paths)) or set(paths) != EXPECTED:
        raise SystemExit(f"Chapter file inventory mismatch: {paths}")
    with tempfile.TemporaryDirectory(prefix="megflow-environment-course-") as directory:
        target = Path(directory)
        for name, content in blocks:
            path = target / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content + "\n")
        for action in ("build", "test"):
            subprocess.run(
                ["cargo", action, "--manifest-path", "code/Cargo.toml", "--workspace", "--offline"],
                cwd=target, check=True, timeout=180,
            )
        subprocess.run(["mdbook", "build", "book"], cwd=target, check=True, timeout=60)
        for name in ("index.html", "workspace.html"):
            if not (target / "book/book" / name).is_file():
                raise SystemExit(f"Missing rendered page: {name}")
    print("环境章：正文十份完整文件在空目录构建通过。")


if __name__ == "__main__":
    main()
