#!/usr/bin/env python3
"""从全新临时目录离线验证消息层，不使用整套引擎的 workspace。"""
from pathlib import Path
import subprocess
import sys
import tempfile

root = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix='megflow-message-course-') as directory:
    target = Path(directory) / 'message'
    subprocess.run([sys.executable, str(root / 'scripts/message_checkpoint.py'), '--out', str(target)], check=True)
    subprocess.run(['cargo', 'test', '--offline', '--manifest-path', str(target / 'Cargo.toml')], check=True, timeout=120)
print('消息层：独立工程离线构建与测试通过。')
