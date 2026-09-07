#!/usr/bin/env python3
"""导出 Ch1.3 消息层独立工程，不依赖父 workspace 或任何第三方 crate。"""
import argparse
from pathlib import Path
import shutil

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--out', type=Path, required=True)
args = parser.parse_args()
source = Path(__file__).resolve().parents[1] / 'code/flow-message'
destination = args.out.resolve()
if destination.exists():
    parser.error(f'目标已存在，拒绝覆盖：{destination}')
destination.mkdir(parents=True)
(destination / 'Cargo.toml').write_text('''[package]
name = "flow-message"
version = "0.1.0"
edition = "2021"
license = "Apache-2.0"

# 独立工作区，即使导出到另一个 workspace 内也不会隐式继承它。
[workspace]
''')
for directory in ('src', 'tests', 'examples'):
    shutil.copytree(source / directory, destination / directory)
print(f'消息层检查点已导出到 {destination}')
print('在该目录执行 cargo test --offline；无需下载依赖。')
