#!/usr/bin/env python3
"""验证手写节点实作不依赖最终引擎、节点宏、注册表或图装配。"""
from pathlib import Path
import subprocess
import sys
import tempfile

root = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix='megflow-manual-node-') as directory:
    target = Path(directory)
    subprocess.run([sys.executable, str(root / 'scripts/message_checkpoint.py'), '--out', str(target / 'message')], check=True)
    (target / 'Cargo.toml').write_text('''[package]
name = "manual-node"
version = "0.1.0"
edition = "2021"
[workspace]
exclude = ["message"]
[dependencies]
flow-message = { path = "message" }
tokio = { version = "1", features = ["rt", "macros", "sync", "time"] }
''')
    (target / 'src').mkdir()
    (target / 'src/main.rs').write_text((root / 'code/flow-rs/examples/manual_actor_steps.rs').read_text())
    subprocess.run(['cargo', 'run', '--offline', '--manifest-path', str(target / 'Cargo.toml')], check=True, timeout=180)
print('手写节点：独立工程验证通过。')
