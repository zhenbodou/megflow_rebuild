#!/usr/bin/env python3
"""从全新临时目录离线验证消息层，不使用整套引擎的 workspace。"""
from pathlib import Path
import subprocess
import sys
import tempfile

root = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix='megflow-message-course-') as directory:
    red = subprocess.run(['rustc', '--edition=2021', '--test',
                          str(root / 'book/labs/envelope/red.rs'),
                          '-o', str(Path(directory) / 'red')],
                         capture_output=True, text=True, timeout=60)
    assert red.returncode != 0 and 'E0433' in red.stderr and 'Envelope' in red.stderr, red.stderr
    envelope = Path(directory) / 'envelope'
    subprocess.run([sys.executable, str(root / 'scripts/message_checkpoint.py'),
                    '--stage', 'envelope', '--out', str(envelope)], check=True)
    assert not (envelope / 'src/algo_base').exists()
    assert not (envelope / 'src/dr.rs').exists()
    # 在同一工程按书中顺序开发；中间步骤没有终点的集成测试和示例。
    import shutil
    final_source = (envelope / 'src/envelope.rs').read_text()
    final_lib = (envelope / 'src/lib.rs').read_text()
    shutil.rmtree(envelope / 'tests')
    shutil.rmtree(envelope / 'examples')
    (envelope / 'src/lib.rs').write_text((root / 'book/labs/envelope/step-lib.rs').read_text())
    for step in ('01', '02'):
        (envelope / 'src/envelope.rs').write_text((root / f'book/labs/envelope/step{step}.rs').read_text())
        subprocess.run(['cargo', 'test', '--offline', '--manifest-path', str(envelope / 'Cargo.toml')], check=True, timeout=120)
    (envelope / 'src/envelope.rs').write_text(final_source)
    (envelope / 'src/lib.rs').write_text(final_lib)
    (envelope / 'tests').mkdir()
    (envelope / 'examples').mkdir()
    for relative in ('tests/envelope_contract.rs', 'examples/first_principles.rs'):
        shutil.copyfile(root / 'code/flow-message' / relative, envelope / relative)
    subprocess.run(['cargo', 'test', '--offline', '--manifest-path', str(envelope / 'Cargo.toml')], check=True, timeout=120)
    subprocess.run(['cargo', 'run', '--offline', '--manifest-path', str(envelope / 'Cargo.toml'),
                    '--example', 'first_principles'], check=True, timeout=120)
    target = Path(directory) / 'message'
    subprocess.run([sys.executable, str(root / 'scripts/message_checkpoint.py'), '--out', str(target)], check=True)
    subprocess.run(['cargo', 'test', '--offline', '--manifest-path', str(target / 'Cargo.toml')], check=True, timeout=120)
    subprocess.run(['cargo', 'run', '--offline', '--manifest-path', str(target / 'Cargo.toml'), '--example', 'first_principles'], check=True, timeout=120)
    subprocess.run(['cargo', 'run', '--offline', '--manifest-path', str(target / 'Cargo.toml'), '--example', 'algorithm_records'], check=True, timeout=120)
print('消息层：独立工程离线构建、测试与入门示范通过。')
