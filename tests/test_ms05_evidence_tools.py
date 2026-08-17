"""MS05 Evidence tool tests — capture runner and manifest/audit validator.

RED gate: import failures mean the scripts do not exist yet. GREEN owners:
Tasks 5.3 (stimulus) and 5.4 (capture/audit).
"""
import subprocess
import tempfile
import unittest
import sys
import os
from pathlib import Path

sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..', 'scripts'))

capture_available = False
audit_available = False

try:
    import ms05_evidence_capture as capture  # noqa: E402
    capture_available = True
except ImportError:
    pass

try:
    import ms05_evidence_audit as audit  # noqa: E402
    audit_available = True
except ImportError:
    pass


class TestCaptureRunner(unittest.TestCase):
    @unittest.skipUnless(capture_available, "capture script not found (RED)")
    def test_self_test_passes(self):
        self.assertEqual(capture.self_test(), 0)

    @unittest.skipUnless(capture_available, "capture script not found (RED)")
    def test_required_gate_ids_are_declared(self):
        self.assertEqual(len(capture.REQUIRED_GATE_IDS), len(capture.GATES))
        for gate in capture.GATES:
            self.assertIn(gate["id"], capture.REQUIRED_GATE_IDS)

    @unittest.skipUnless(capture_available, "capture script not found (RED)")
    def test_d1_contract_declared(self):
        d1 = [g for g in capture.GATES if g["kind"] == "d1"]
        self.assertEqual(len(d1), 1)
        self.assertEqual(d1[0]["d1_counts"],
                         {"error[E0432]": 20, "error[E0433]": 5})


class TestEvidenceAudit(unittest.TestCase):
    @unittest.skipUnless(audit_available, "audit script not found (RED)")
    def test_self_test_passes(self):
        audit.run_fixtures()

    @unittest.skipUnless(audit_available, "audit script not found (RED)")
    def test_repeat100_gates_declared(self):
        self.assertIn("race-control-100x", audit.REPEAT100_GATES)
        self.assertIn("race-v3-100x", audit.REPEAT100_GATES)
        self.assertIn("race-full-suite-100x", audit.REPEAT100_GATES)

    @unittest.skipUnless(audit_available, "audit script not found (RED)")
    def test_d1_gates_declared(self):
        self.assertIn("kernel-lichee-d1-check", audit.D1_GATES)
        self.assertEqual(audit.D1_EXPECTED,
                         {"error[E0432]": 20, "error[E0433]": 5})


class TestArtifactRecordSpec(unittest.TestCase):
    """The exact 18 artifact command records shared by capture and audit."""

    @unittest.skipUnless(capture_available, "capture script not found (RED)")
    def test_exact_18_records_are_derived(self):
        specs = capture.artifact_record_specs()
        self.assertEqual(len(specs), 18)
        self.assertEqual(len({gate_id for gate_id, _ in specs}), 18)
        commands = {"file", "stat", "sha256sum"}
        for gate_id, argv in specs:
            self.assertTrue(gate_id.startswith("artifact-"), gate_id)
            self.assertTrue(len(argv) in (2, 3), argv)
            self.assertIn(argv[0], commands)
            self.assertIn(f"-{argv[0]}-", gate_id)


class TestContentSensitiveIdentity(unittest.TestCase):
    """Worktree identity must bind tracked byte edits even when `git status`
    category text is unchanged, and relevant untracked content."""

    def _repo(self):
        tmp = tempfile.TemporaryDirectory()
        root = Path(tmp.name)
        subprocess.run(["git", "init", "-q", str(root)], check=True)
        subprocess.run(["git", "-C", str(root), "config", "user.email",
                        "t@example.com"], check=True)
        subprocess.run(["git", "-C", str(root), "config", "user.name",
                        "t"], check=True)
        target = root / "tracked.txt"
        target.write_text("one\n")
        subprocess.run(["git", "-C", str(root), "add", "tracked.txt"],
                       check=True)
        subprocess.run(["git", "-C", str(root), "commit", "-qm", "init"],
                       check=True)
        return tmp, root, target

    @unittest.skipUnless(capture_available, "capture script not found (RED)")
    def test_same_status_byte_edit_changes_identity(self):
        tmp, root, target = self._repo()
        try:
            target.write_text("two\n")
            first = capture.source_identity(root)["worktree_identity"]
            target.write_text("three\n")
            second = capture.source_identity(root)["worktree_identity"]
            self.assertNotEqual(first, second)
        finally:
            tmp.cleanup()

    @unittest.skipUnless(capture_available, "capture script not found (RED)")
    def test_untracked_content_changes_identity(self):
        tmp, root, _target = self._repo()
        try:
            extra = root / "extra.txt"
            extra.write_text("a\n")
            first = capture.source_identity(root)["worktree_identity"]
            extra.write_text("b\n")
            second = capture.source_identity(root)["worktree_identity"]
            self.assertNotEqual(first, second)
        finally:
            tmp.cleanup()


if __name__ == "__main__":
    unittest.main()
