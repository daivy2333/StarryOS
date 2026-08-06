import json
import glob
import os
import subprocess
import tempfile
import unittest


ROOT = os.path.dirname(os.path.dirname(__file__))
BIN = os.path.join(ROOT, "tests", "network_benchmark-host")
REPORT = os.path.join(ROOT, "scripts", "network_benchmark_report.py")
EVIDENCE = os.path.join(ROOT, "scripts", "network_benchmark_evidence.py")
FIXTURES = os.path.join(ROOT, "tests", "fixtures", "network-benchmark")


def run(*args, timeout=30):
    return subprocess.run(
        args, cwd=ROOT, text=True, stdout=subprocess.PIPE,
        stderr=subprocess.PIPE, timeout=timeout, check=False,
    )


def records(output):
    return [json.loads(line) for line in output.splitlines() if line.strip()]


class WorkloadIntegration(unittest.TestCase):
    def test_internal_fault_matrix(self):
        proc = run(BIN, "--self-test")
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertIn("SELF-TEST PASS", proc.stdout)

    def test_profile_defaults_precede_explicit_overrides(self):
        proc = run(
            BIN, "loopback", "--profile", "quick", "--duration", "1",
            "--warmup", "0", "--seed", "9", "--print-config",
        )
        self.assertEqual(proc.returncode, 0, proc.stderr)
        config = records(proc.stdout)[0]
        self.assertEqual(config["duration_s"], 1)
        self.assertEqual(config["warmup_s"], 0)
        self.assertEqual(config["seed"], 9)

    def test_loopback_matrix_has_two_closed_ledgers(self):
        for protocol in ("tcp", "udp"):
            for direction in ("tx", "rx", "bidi"):
                for flows in (1, 2, 4, 8):
                    with self.subTest(protocol=protocol, direction=direction, flows=flows):
                        proc = run(
                            BIN, "loopback", "--protocol", protocol,
                            "--direction", direction, "--flows", str(flows),
                            "--profile", "smoke", "--duration", "1",
                        )
                        self.assertEqual(proc.returncode, 0, proc.stderr)
                        rounds = {
                            item["side"]: item for item in records(proc.stdout)
                            if item.get("type") == "round"
                        }
                        self.assertEqual(set(rounds), {"guest", "host"})
                        guest = rounds["guest"]
                        host = rounds["host"]
                        self.assertEqual(guest["status"], "valid")
                        self.assertEqual(host["status"], "valid")
                        self.assertEqual(guest["flow_count"], flows)
                        self.assertEqual(host["flow_count"], flows)
                        self.assertEqual(guest["tx_bytes"], host["rx_bytes"])
                        self.assertEqual(host["tx_bytes"], guest["rx_bytes"])
                        self.assertGreater(guest["tx_bytes"] + host["tx_bytes"], 0)
                        if protocol == "udp":
                            for endpoint in (guest, host):
                                if endpoint["tx_packets"]:
                                    self.assertEqual(
                                        endpoint["udp_offered"], endpoint["tx_packets"]
                                    )
                                    self.assertEqual(
                                        endpoint["udp_accepted"], endpoint["tx_packets"]
                                    )

    def test_internal_fault_matrix_is_exercised(self):
        proc = run(BIN, "--self-test")
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertIn(
            "faults=config-mismatch,peer-eof,timeout,cancel,udp-anomalies",
            proc.stdout,
        )

    def test_unavailable_instret_has_null_numeric_fields(self):
        proc = run(BIN, "--calibrate")
        self.assertEqual(proc.returncode, 0, proc.stderr)
        calibration = records(proc.stdout)[0]
        if calibration["instret_status"] == "unavailable":
            self.assertIsNone(calibration["instret_begin"])
            self.assertIsNone(calibration["instret_end"])
            self.assertIsNone(calibration["instret_overhead"])

    def test_manual_package_contract(self):
        matches = glob.glob(os.path.join(
            ROOT, "openspec", "changes", "archive",
            "*-ms16-qemu-polling-network-performance-baseline",
            "manual-calibration.md",
        ))
        self.assertEqual(
            len(matches), 1,
            f"expected one archived MS16 manual package, found {matches}",
        )
        path = matches[0]
        with open(path, encoding="utf-8") as stream:
            guide = stream.read()
        self.assertIn("evidence/005-runtime-readiness-closure-and-manual-handoff/", guide)
        for required in (
            "manifest.json", "README.md", "qemu-command.txt", "qemu-serial.log",
            "guest-console.log", "guest-netbench.ndjson", "host-netbench.ndjson",
            "host-cpu.ndjson", "irq-snapshots.log", "capture.pcap",
            "results.csv", "summary.json", "evidence-check.json",
        ):
            self.assertIn(required, guide)
        self.assertIn("10.0.2.2:15555", guide)
        self.assertIn("10.0.2.2:5555", guide)
        self.assertNotIn("\nmkdir -p /root/ms16\n", guide)
        self.assertNotIn("/mnt/starry-rootfs/root/ms16", guide)
        self.assertIn("ms16-usernet-terminal.log", guide)
        self.assertIn("ms16-tap-terminal.log", guide)
        self.assertIn("json.loads(line)", guide)
        self.assertIn(
            "wget -q -O /tmp/network_benchmark "
            "http://10.0.2.2:18765/network_benchmark",
            guide,
        )

    def test_report_rejects_missing_peer(self):
        guest = os.path.join(FIXTURES, "valid", "guest-netbench.ndjson")
        proc = run("python3", REPORT, "--guest", guest)
        self.assertNotEqual(proc.returncode, 0, proc.stdout)

    def test_incomparable_evidence_exits_nonzero(self):
        a = os.path.join(FIXTURES, "mismatch-a")
        with tempfile.TemporaryDirectory() as other:
            with open(os.path.join(a, "manifest.json"), encoding="utf-8") as stream:
                manifest = json.load(stream)
            manifest["payload_size"] = manifest.get("payload_size", 1400) + 1
            with open(os.path.join(other, "manifest.json"), "w", encoding="utf-8") as stream:
                json.dump(manifest, stream)
            proc = run("python3", EVIDENCE, "--compare", a, other)
            self.assertNotEqual(proc.returncode, 0, proc.stdout)

    def test_numeric_tail_is_rejected(self):
        proc = run(BIN, "loopback", "--duration", "1junk")
        self.assertNotEqual(proc.returncode, 0)


if __name__ == "__main__":
    unittest.main()
