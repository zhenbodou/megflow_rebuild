#!/usr/bin/env python3
"""只用 serde/toml，在独立工程验证配置分层实验。"""
from pathlib import Path
import subprocess
import tempfile

root = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix='megflow-config-course-') as directory:
    target = Path(directory)
    (target / 'src').mkdir()
    (target / 'Cargo.toml').write_text('''[package]
name = "config-steps"
version = "0.1.0"
edition = "2021"
[workspace]
[dependencies]
serde = { version = "1", features = ["derive"] }
toml = "0.8.23"
''')
    (target / 'src/main.rs').write_text((root / 'code/flow-rs/examples/config_steps.rs').read_text())
    subprocess.run(['cargo', 'run', '--manifest-path', str(target / 'Cargo.toml'), '--offline'], check=True, timeout=180)
print('配置分层实验：独立工程验证通过。')
