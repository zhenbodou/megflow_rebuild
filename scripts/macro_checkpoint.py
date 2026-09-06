#!/usr/bin/env python3
"""导出宏课第 1/2/3 步的独立工作区；拒绝覆盖已有目录。"""
import argparse
from pathlib import Path
import shutil

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--stage", type=int, choices=(1, 2, 3), required=True)
parser.add_argument("--out", type=Path, required=True)
args = parser.parse_args()
source = Path(__file__).resolve().parents[1] / "code/macro-labs"
destination = args.out.resolve()
if destination.exists():
    parser.error(f"目录已存在，请选择空的新路径：{destination}")

text = (source / "derive/src/lib.rs").read_text()
parts = [text.split("// ANCHOR: derive")[0]]
for name in ("derive", "attribute", "function")[:args.stage]:
    parts.append(text.split(f"// ANCHOR: {name}\n", 1)[1]
                 .split(f"// ANCHOR_END: {name}", 1)[0])
for relative in ("Cargo.toml", "Cargo.lock", "derive/Cargo.toml", "app/Cargo.toml"):
    target = destination / relative
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source / relative, target)
(destination / "derive/src").mkdir()
(destination / "derive/src/lib.rs").write_text("\n".join(parts))
(destination / "app/src").mkdir()
app = source / (f"stages/0{args.stage}-main.rs" if args.stage < 3 else "app/src/main.rs")
shutil.copyfile(app, destination / "app/src/main.rs")
print(f"已生成第 {args.stage} 步：{destination}")
print(f"下一步：cd '{destination}'，然后 cargo run -p macro-lab-app --locked")
