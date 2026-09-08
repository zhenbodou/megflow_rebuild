#!/usr/bin/env python3
"""在不含引擎的空工程中验证异步三步实验。"""
from pathlib import Path
import subprocess
import tempfile

root = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix='megflow-async-course-') as directory:
    target = Path(directory)
    (target / 'src').mkdir()
    (target / 'Cargo.toml').write_text('''[package]
name = "async-steps"
version = "0.1.0"
edition = "2021"
[workspace]
[dependencies]
tokio = { version = "1", features = ["rt", "macros", "sync"] }
futures-util = "0.3"
''')
    (target / 'src/main.rs').write_text((root / 'code/flow-rs/examples/async_steps.rs').read_text())
    subprocess.run(['cargo', 'run', '--manifest-path', str(target / 'Cargo.toml'), '--offline'], check=True, timeout=180)
print('异步三步实验：独立工程验证通过。')
