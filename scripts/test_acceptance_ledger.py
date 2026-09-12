#!/usr/bin/env python3
"""Regression tests for false completion claims, without reference checkout."""
import copy
from pathlib import Path
import tempfile
import unittest

from acceptance_ledger import COMMIT, CRATES, validate


class LedgerGateTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        for name in ("impl.rs", "test.rs", "chapter.md", "replay.py"):
            (self.root / name).write_text("fixture\n")
        self.snapshot = {
            "reference_commit": COMMIT, "crates": list(CRATES),
            "files": [{"path": "flow-rs/src/node/port.rs", "sha256": "abc", "lines": 20}],
        }
        self.contract = {
            "id": "close", "state": "实现中",
            "source": {"path": "flow-rs/src/node/port.rs", "sha256": "abc", "start": 1, "end": 10},
            "implementation": ["impl.rs"], "tests": ["test.rs"], "chapters": ["chapter.md"],
            "checks": ["test command"], "remaining": ["explicit close missing"],
        }

    def errors(self, contract=None):
        return validate(self.snapshot, {"reference_commit": COMMIT,
                        "contracts": [contract or self.contract]}, self.root)

    def test_in_progress_can_honestly_record_gaps(self):
        self.assertEqual(self.errors(), [])

    def test_verified_state_rejects_remaining_work_and_missing_contract(self):
        self.contract["state"] = "行为已验证"
        errors = self.errors()
        self.assertTrue(any("仍有缺口" in error for error in errors))
        self.assertTrue(any("input" in error for error in errors))

    def test_reference_digest_and_lines_must_match(self):
        self.contract["source"]["sha256"] = "tampered"
        self.assertTrue(any("摘要不匹配" in error for error in self.errors()))
        self.contract["source"]["sha256"] = "abc"
        self.contract["source"]["end"] = 21
        self.assertTrue(any("行号越界" in error for error in self.errors()))

    def test_missing_or_external_paths_are_not_evidence(self):
        for path in ("missing.rs", "../outside.rs"):
            self.contract["tests"] = [path]
            self.assertTrue(any("文件不存在" in error for error in self.errors()))

    def test_teaching_verified_requires_replay_files(self):
        contract = copy.deepcopy(self.contract)
        contract.update(state="教学已验证", remaining=[])
        for field in ("input", "output", "errors", "side_effects", "lifecycle"):
            contract[field] = "specified"
        self.assertTrue(any("独立复现" in error for error in self.errors(contract)))
        contract["replay"] = {"script": "replay.py", "complete_files": ["impl.rs", "test.rs"]}
        self.assertEqual(self.errors(contract), [])


if __name__ == "__main__":
    unittest.main()
