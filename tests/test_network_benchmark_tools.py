"""MS16 network benchmark tool tests — RED test suite.

Import failures = RED: scripts do not exist yet.
GREEN owners: Tasks 1.6 (collector), 1.7 (report), 1.8 (evidence).
"""
import unittest
import sys
import os
import json
import tempfile
import shutil

FIXTURE_DIR = os.path.join(os.path.dirname(__file__), 'fixtures', 'network-benchmark')

# ── RED gate: scripts must exist ──────────────────────────────────────

collector_available = False
report_available = False
evidence_available = False

try:
    sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..', 'scripts'))
    import network_benchmark_collect as collector  # noqa: E402
    collector_available = True
except ImportError:
    pass

try:
    import network_benchmark_report as report  # noqa: E402
    report_available = True
except ImportError:
    pass

try:
    import network_benchmark_evidence as evidence  # noqa: E402
    evidence_available = True
except ImportError:
    pass


class TestCollector(unittest.TestCase):
    @unittest.skipUnless(collector_available, "collector script not found (RED)")
    def test_self_test_passes(self):
        """Collector --self-test must exit 0."""
        rc = collector.run_self_test()
        self.assertEqual(rc, 0)

    @unittest.skipUnless(collector_available, "collector script not found (RED)")
    def test_sample_format(self):
        """A sample line must contain pid, utime_ticks, stime_ticks, rss_kb."""
        sample = collector.sample_pid(os.getpid())
        self.assertIsInstance(sample, dict)
        self.assertIn('pid', sample)
        self.assertIn('utime_ticks', sample)
        self.assertIn('stime_ticks', sample)
        self.assertIn('rss_kb', sample)
        self.assertIn('timestamp_ns', sample)

    @unittest.skipUnless(collector_available, "collector script not found (RED)")
    def test_dead_pid_handling(self):
        """Sampling a non-existent PID must not crash."""
        try:
            collector.sample_pid(99999999)
        except Exception:
            self.fail("sample_pid() crashed on dead PID")

    @unittest.skipUnless(collector_available, "collector script not found (RED)")
    def test_counter_regression_and_pid_reuse_rejected(self):
        first = {'pid': 7, 'pid_starttime': 10, 'utime_ticks': 20, 'stime_ticks': 5}
        reused = {'pid': 7, 'pid_starttime': 11, 'utime_ticks': 21, 'stime_ticks': 5}
        regressed = {'pid': 7, 'pid_starttime': 10, 'utime_ticks': 19, 'stime_ticks': 5}
        self.assertFalse(collector.sample_continues(first, reused)[0])
        self.assertFalse(collector.sample_continues(first, regressed)[0])

    def test_red_collector_absent(self):
        if not collector_available:
            self.fail("RED: collector script not found (expected until Task 1.6)")


class TestReport(unittest.TestCase):
    @unittest.skipUnless(report_available, "report script not found (RED)")
    def test_self_test_passes(self):
        rc = report.run_self_test()
        self.assertEqual(rc, 0)

    @unittest.skipUnless(report_available, "report script not found (RED)")
    def test_valid_fixture_summary(self):
        guest_path = os.path.join(FIXTURE_DIR, 'valid', 'guest-netbench.ndjson')
        host_path = os.path.join(FIXTURE_DIR, 'valid', 'host-netbench.ndjson')
        summary = report.generate_summary(guest_path, host_path, require_complete=False)
        self.assertIn('rounds', summary)
        self.assertEqual(summary['rounds']['valid'], 3)
        self.assertEqual(summary['rounds']['invalid'], 0)
        self.assertGreater(summary['goodput_mbps']['median'], 0)

    @unittest.skipUnless(report_available, "report script not found (RED)")
    def test_invalid_round_retained(self):
        guest_path = os.path.join(FIXTURE_DIR, 'invalid', 'host-netbench.ndjson')
        summary = report.generate_summary(guest_path, None, require_complete=False)
        self.assertGreater(summary['rounds']['invalid'], 0)

    @unittest.skipUnless(report_available, "report script not found (RED)")
    def test_malformed_json(self):
        guest_path = os.path.join(FIXTURE_DIR, 'invalid', 'guest-netbench.ndjson')
        with self.assertRaises(ValueError):
            report.generate_summary(guest_path, None, require_complete=False)

    @unittest.skipUnless(report_available, "report script not found (RED)")
    def test_missing_peer_rejected_for_complete_report(self):
        guest_path = os.path.join(FIXTURE_DIR, 'valid', 'guest-netbench.ndjson')
        with self.assertRaises(ValueError):
            report.generate_summary(guest_path, None, require_complete=True)

    @unittest.skipUnless(report_available, "report script not found (RED)")
    def test_writes_csv_and_json(self):
        guest_path = os.path.join(FIXTURE_DIR, 'valid', 'guest-netbench.ndjson')
        host_path = os.path.join(FIXTURE_DIR, 'valid', 'host-netbench.ndjson')
        summary = report.generate_summary(guest_path, host_path)
        with tempfile.TemporaryDirectory() as directory:
            csv_path = os.path.join(directory, 'results.csv')
            json_path = os.path.join(directory, 'summary.json')
            report.write_outputs(summary, csv_path, json_path)
            self.assertTrue(os.path.isfile(csv_path))
            self.assertTrue(os.path.isfile(json_path))

    @unittest.skipUnless(report_available, "report script not found (RED)")
    def test_unavailable_instret_zero_payload_is_not_zero_efficiency(self):
        guest_record = {
            "schema_version": 1, "type": "round", "run_id": 1,
            "test_id": 1, "round_id": 1, "side": "guest",
            "status": "valid", "protocol": "TCP", "direction": "RX",
            "completion_point": 6, "duration_s": 1,
            "tx_bytes": 0, "tx_packets": 0, "rx_bytes": 64,
            "rx_packets": 1, "instret_status": "unavailable",
            "instret_begin": 0, "instret_end": 0, "instret_overhead": 0,
        }
        host_record = dict(guest_record, side="host", direction="RX",
                           tx_bytes=64, tx_packets=1, rx_bytes=0, rx_packets=0)
        with tempfile.TemporaryDirectory() as directory:
            guest = os.path.join(directory, "guest.ndjson")
            host = os.path.join(directory, "host.ndjson")
            with open(guest, "w", encoding="utf-8") as stream:
                stream.write(json.dumps(guest_record) + "\n")
            with open(host, "w", encoding="utf-8") as stream:
                stream.write(json.dumps(host_record) + "\n")
            summary = report.generate_summary(guest, host)
        self.assertEqual(summary["instret_instructions_per_bit"], "unavailable")

    def test_red_report_absent(self):
        if not report_available:
            self.fail("RED: report script not found (expected until Task 1.7)")


class TestEvidence(unittest.TestCase):
    @unittest.skipUnless(evidence_available, "evidence script not found (RED)")
    def test_self_test_passes(self):
        rc = evidence.run_self_test()
        self.assertEqual(rc, 0)

    @unittest.skipUnless(evidence_available, "evidence script not found (RED)")
    def test_valid_evidence_pass(self):
        evidence_dir = os.path.join(FIXTURE_DIR, 'valid')
        result = evidence.check_evidence(evidence_dir)
        self.assertTrue(result['pass'], f"Expected pass, got: {result}")

    @unittest.skipUnless(evidence_available, "evidence script not found (RED)")
    def test_missing_file_fails(self):
        evidence_dir = os.path.join(FIXTURE_DIR, 'missing-file')
        result = evidence.check_evidence(evidence_dir)
        self.assertFalse(result['pass'], "Should fail on missing files")
        self.assertGreater(len(result.get('missing', [])), 0)

    @unittest.skipUnless(evidence_available, "evidence script not found (RED)")
    def test_comparison_mismatch(self):
        dir_a = os.path.join(FIXTURE_DIR, 'mismatch-a')
        dir_b = os.path.join(FIXTURE_DIR, 'mismatch-b')
        result = evidence.compare(dir_a, dir_b)
        self.assertIn('comparable', result)
        # If treatment differs but everything else is same, should be comparable
        self.assertTrue(result.get('comparable', False),
                        f"Should be comparable, got: {result}")

    @unittest.skipUnless(evidence_available, "evidence script not found (RED)")
    def test_malformed_schema_fails(self):
        evidence_dir = os.path.join(FIXTURE_DIR, 'invalid')
        result = evidence.check_evidence(evidence_dir)
        # Invalid manifest or malformed NDJSON should cause failures
        self.assertFalse(result['pass'],
                         "Malformed evidence should fail validation")

    @unittest.skipUnless(evidence_available, "evidence script not found (RED)")
    def test_host_extra_round_rejected(self):
        guest = os.path.join(FIXTURE_DIR, 'valid', 'guest-netbench.ndjson')
        host = os.path.join(FIXTURE_DIR, 'valid', 'host-netbench.ndjson')
        with tempfile.TemporaryDirectory() as directory:
            guest_copy = os.path.join(directory, 'guest.ndjson')
            host_copy = os.path.join(directory, 'host.ndjson')
            with open(guest, encoding='utf-8') as source, open(guest_copy, 'w', encoding='utf-8') as target:
                target.write(source.read())
            with open(host, encoding='utf-8') as source, open(host_copy, 'w', encoding='utf-8') as target:
                target.write(source.read())
                target.write('{"schema_version":1,"type":"round","run_id":"extra","round_id":99,"side":"host","status":"valid","protocol":"TCP","direction":"RX","completion_point":6,"duration_s":1,"tx_bytes":0,"rx_bytes":1,"tx_packets":0,"rx_packets":1}\n')
            ok, errors = evidence.check_round_closure(guest_copy, host_copy)
            self.assertFalse(ok, errors)

    @unittest.skipUnless(evidence_available and report_available,
                         "evidence/report script not found (RED)")
    def test_drifted_summary_is_rejected(self):
        source = os.path.join(FIXTURE_DIR, "valid")
        with tempfile.TemporaryDirectory() as directory:
            for name in evidence.REQUIRED_FILES:
                shutil.copy2(os.path.join(source, name), os.path.join(directory, name))
            summary = report.generate_summary(
                os.path.join(directory, "guest-netbench.ndjson"),
                os.path.join(directory, "host-netbench.ndjson"),
                cpu_path=os.path.join(directory, "host-cpu.ndjson"),
            )
            summary["rounds"]["valid"] += 1
            with open(os.path.join(directory, "summary.json"), "w", encoding="utf-8") as stream:
                json.dump(summary, stream)
            result = evidence.check_evidence(directory)
        self.assertFalse(result["pass"], result)
        self.assertTrue(any("summary" in error for error in result["errors"]))

    def test_red_evidence_absent(self):
        if not evidence_available:
            self.fail("RED: evidence script not found (expected until Task 1.8)")


if __name__ == '__main__':
    unittest.main()
