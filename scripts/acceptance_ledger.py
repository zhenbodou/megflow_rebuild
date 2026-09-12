#!/usr/bin/env python3
"""Inventory a fixed reference and reject unsupported completion claims.

Source declarations are lexical search candidates, not a Rust type resolver.
The committed snapshot lets CI validate mappings without private registry access.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[1]
COMMIT = "95f870bfefd48fa31f9cf88320de4cc177985c72"
CRATES = ("flow-rs", "flow-derive", "flow-message", "flow-plugins")
SNAPSHOT = ROOT / "acceptance/reference.json"
LEDGER = ROOT / "acceptance/contracts.json"
STATES = ("未开始", "实现中", "行为已验证", "教学已验证")


def git(reference, *args):
    return subprocess.check_output(["git", "-C", str(reference), *args])


def inventory(reference):
    tree = git(reference, "ls-tree", "-r", "--name-only", COMMIT, *CRATES).decode().splitlines()
    files = []
    for name in sorted(tree):
        if not name.endswith((".rs", ".toml")):
            continue
        data = git(reference, "show", f"{COMMIT}:{name}")
        content = data.decode("utf-8")
        # A candidate list helps locate evidence. It does not expand macros/cfg,
        # determine re-exports, or imply that every public API is already indexed.
        candidates = []
        for number, line in enumerate(content.splitlines(), 1):
            if re.search(r"\b(pub(?:\([^)]*\))?\s+|macro_rules!|(?:node|resource|opt)_register!|#\[(?:proc_macro|.*test))", line):
                candidates.append({"line": number, "text": line.strip()})
        row = {
            "path": name,
            "sha256": hashlib.sha256(data).hexdigest(),
            "lines": len(content.splitlines()),
            "candidates": candidates,
        }
        if name in (f"{crate}/Cargo.toml" for crate in CRATES):
            manifest = tomllib.loads(content)
            row["features"] = manifest.get("features", {})
            row["dependencies"] = {
                key: manifest.get(key, {})
                for key in ("dependencies", "dev-dependencies", "build-dependencies", "target")
            }
        files.append(row)
    return {"schema": 1, "reference_commit": COMMIT, "crates": list(CRATES), "files": files}


def existing_file(root, name):
    if not isinstance(name, str) or not name:
        return False
    path = (root / name).resolve()
    return path.is_relative_to(root.resolve()) and path.is_file()


def validate(snapshot, ledger, root=ROOT):
    errors = []
    if snapshot.get("reference_commit") != COMMIT or ledger.get("reference_commit") != COMMIT:
        errors.append("参照提交不匹配")
    if snapshot.get("crates") != list(CRATES):
        errors.append("四个目标 crate 不可删减")
    sources = {row["path"]: row for row in snapshot["files"]}
    ids = set()
    for contract in ledger["contracts"]:
        key = contract.get("id", "")
        if not key or key in ids:
            errors.append(f"重复或空契约 ID: {key}")
        ids.add(key)
        state = contract.get("state")
        if state not in STATES:
            errors.append(f"{key}: 非法状态 {state}")
            continue
        evidence = contract.get("source", {})
        source = sources.get(evidence.get("path"))
        if not source or evidence.get("sha256") != source["sha256"]:
            errors.append(f"{key}: 缺失固定版本源码证据或摘要不匹配")
        elif not (1 <= evidence.get("start", 0) <= evidence.get("end", 0) <= source["lines"]):
            errors.append(f"{key}: 原版证据行号越界")
        for field in ("implementation", "tests", "chapters"):
            paths = contract.get(field, [])
            for path in paths:
                if not existing_file(root, path):
                    errors.append(f"{key}: {field} 文件不存在: {path}")
        if state in ("行为已验证", "教学已验证"):
            for field in ("input", "output", "errors", "side_effects", "lifecycle"):
                if not isinstance(contract.get(field), str) or not contract[field].strip():
                    errors.append(f"{key}: 已验证状态缺少 {field} 契约")
            for field in ("implementation", "tests", "chapters", "checks"):
                if not contract.get(field):
                    errors.append(f"{key}: 已验证状态缺少 {field} 映射")
            if contract.get("remaining"):
                errors.append(f"{key}: 仍有缺口却标为已验证")
        if state == "教学已验证":
            replay = contract.get("replay", {})
            if not existing_file(root, replay.get("script")) or not replay.get("complete_files"):
                errors.append(f"{key}: 教学已验证缺少完整文件和独立复现入口")
            for path in replay.get("complete_files", []):
                if not existing_file(root, path):
                    errors.append(f"{key}: 复现文件不存在: {path}")
    return errors


def report(snapshot, ledger):
    mapped = {row["source"]["path"] for row in ledger["contracts"]}
    lines = ["# 自动验收矩阵", "", f"固定原版提交：`{COMMIT}`。", "",
             "这是文件覆盖与证据映射报告，不是完整性证明。未映射文件保留在清单中；混有绑定代码的文件仍须人工划分 Rust 行为。",
             "词法候选不等于完整 AST/API 清单。检查通过只表示记录自洽，不表示契约已经执行或语义等价。", "",
             "## 已登记契约", "", "| 契约 | 状态 | 剩余工作 |", "| --- | --- | --- |"]
    for row in ledger["contracts"]:
        lines.append(f"| {row['id']} | {row['state']} | {'；'.join(row.get('remaining', [])) or '无已登记缺口'} |")
    lines += ["", "## 原版文件覆盖", "", "| 原版文件 | 契约映射 |", "| --- | --- |"]
    for row in snapshot["files"]:
        lines.append(f"| `{row['path']}` | {'已有局部映射' if row['path'] in mapped else '待拆分契约'} |")
    chapter_map = {path for row in ledger["contracts"] for path in row.get("chapters", [])}
    lines += ["", "## 全部已写页面", "", "下表包含未进入 SUMMARY 的旧页面，避免旧章从检查范围中消失。目录文件本身不作为教学章节。", "",
              "| 页面 | 契约映射 |", "| --- | --- |"]
    for path in sorted((ROOT / "book/src").rglob("*.md")):
        if path.name in ("SUMMARY.md", "ch06-acceptance-matrix.md"):
            continue
        name = path.relative_to(ROOT).as_posix()
        lines.append(f"| `{name}` | {'已有局部映射' if name in chapter_map else '待登记；未验证'} |")
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--refresh-reference", type=Path, help="从只读原版 Git 对象重建固定清单")
    parser.add_argument("--verify-reference", type=Path, help="核对提交快照与固定原版内容一致")
    parser.add_argument("--write-report", action="store_true")
    args = parser.parse_args()
    if args.refresh_reference:
        SNAPSHOT.parent.mkdir(parents=True, exist_ok=True)
        SNAPSHOT.write_text(json.dumps(inventory(args.refresh_reference), ensure_ascii=False, indent=2) + "\n")
    snapshot = json.loads(SNAPSHOT.read_text())
    if args.verify_reference and inventory(args.verify_reference) != snapshot:
        raise SystemExit("原版快照与固定提交内容不一致")
    ledger = json.loads(LEDGER.read_text())
    errors = validate(snapshot, ledger)
    if errors:
        raise SystemExit("\n".join(errors))
    rendered = report(snapshot, ledger)
    output = ROOT / "book/src/part0/ch06-acceptance-matrix.md"
    if args.write_report:
        output.write_text(rendered)
    elif not output.exists() or output.read_text() != rendered:
        raise SystemExit("验收矩阵已过期，请运行 --write-report")
    counts = {state: sum(row["state"] == state for row in ledger["contracts"]) for state in STATES}
    print(f"原版文件 {len(snapshot['files'])}；契约状态 {counts}；记录检查通过（不等于框架完成）。")


if __name__ == "__main__":
    main()
