#!/usr/bin/env python3
"""MS05 automatic product Gate capture runner.

Executes the declared automatic Gate suite with literal argv, captures the
complete stdout/stderr of every subprocess into an indexed raw log, hashes
each log and writes a versioned JSON manifest under the Iteration 010
Evidence root. The manifest is the sole authority for what ran; prose
summaries are derived from it and can never replace a raw log.

Usage:
  python3 scripts/ms05_evidence_capture.py --self-test
  python3 scripts/ms05_evidence_capture.py --run automatic \
      --root <evidence-root>
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

SCHEMA_VERSION = 1

# Declared automatic Gate set: exact IDs, order and expected outcomes. A
# sequential shell expression is split into separate records; no record is
# reconstructed from prose. `repeat-100` gates produce 100 indexed child
# records with complete stdout/stderr; the parent record holds a derived
# summary only. `d1` gates carry an exact diagnostic-count contract.
GATES = [
    {"id": "host-test",
     "argv": ["make", "host-test"],
     "expected_exit": 0, "kind": "run"},
    {"id": "axnet-qemu-diagnostics",
     "argv": ["cargo", "test", "--manifest-path", "crates/axnet/Cargo.toml",
              "--locked", "--offline", "--features", "qemu-diagnostics",
              "--lib"],
     "expected_exit": 0, "kind": "run"},
    {"id": "axnet-default",
     "argv": ["cargo", "test", "--manifest-path", "crates/axnet/Cargo.toml",
              "--locked", "--offline", "--lib"],
     "expected_exit": 0, "kind": "run"},
    {"id": "axdriver-net",
     "argv": ["cargo", "test", "--manifest-path",
              "crates/axdriver_net/Cargo.toml", "--offline"],
     "expected_exit": 0, "kind": "run"},
    {"id": "axdriver-virtio",
     "argv": ["cargo", "test", "--manifest-path",
              "crates/axdriver_virtio/Cargo.toml", "--offline",
              "--features", "net"],
     "expected_exit": 0, "kind": "run"},
    {"id": "virtio-drivers",
     "argv": ["cargo", "test", "--manifest-path",
              "crates/virtio-drivers/Cargo.toml", "--offline",
              "--features", "alloc"],
     "expected_exit": 0, "kind": "run"},
    {"id": "uart-async",
     "argv": ["cargo", "test", "--manifest-path",
              "crates/uart_16550/Cargo.toml", "--offline",
              "--features", "async"],
     "expected_exit": 0, "kind": "run"},
    {"id": "ms03-harness-compile",
     "argv": ["rustc", "--edition=2024", "--test",
              "tests/ms03-irq-host-harness.rs", "-o", "/tmp/ms03-irq-host-test"],
     "expected_exit": 0, "kind": "run"},
    {"id": "ms03-harness-run",
     "argv": ["/tmp/ms03-irq-host-test"],
     "expected_exit": 0, "kind": "run"},
    {"id": "ms04-harness-compile",
     "argv": ["rustc", "--edition=2024", "--test",
              "tests/ms04-async-rx-host-harness.rs",
              "-o", "/tmp/ms04-async-rx-host-test"],
     "expected_exit": 0, "kind": "run"},
    {"id": "ms04-harness-run",
     "argv": ["/tmp/ms04-async-rx-host-test"],
     "expected_exit": 0, "kind": "run"},
    {"id": "evidence-tools-unittest",
     "argv": ["python3", "-m", "unittest", "tests.test_ms05_evidence_tools",
              "-v"],
     "expected_exit": 0, "kind": "run"},
    {"id": "capture-self-test",
     "argv": ["python3", "scripts/ms05_evidence_capture.py", "--self-test"],
     "expected_exit": 0, "kind": "run"},
    {"id": "audit-self-test",
     "argv": ["python3", "scripts/ms05_evidence_audit.py", "--self-test"],
     "expected_exit": 0, "kind": "run"},
    {"id": "race-control-100x",
     "argv": ["cargo", "test", "--manifest-path", "crates/axnet/Cargo.toml",
              "--locked", "--offline", "--features", "qemu-diagnostics",
              "--lib", "diagnostic_control_shared_path_is_bounded_"
                       "and_publishes_after_unlock"],
     "expected_exit": 0, "kind": "repeat-100"},
    {"id": "race-v3-100x",
     "argv": ["cargo", "test", "--manifest-path", "crates/axnet/Cargo.toml",
              "--locked", "--offline", "--features", "qemu-diagnostics",
              "--lib",
              "v3_shared_snapshot_path_returns_only_committed_tuples_"
              "under_control_and_tick"],
     "expected_exit": 0, "kind": "repeat-100"},
    {"id": "race-full-suite-100x",
     "argv": ["cargo", "test", "--manifest-path", "crates/axnet/Cargo.toml",
              "--locked", "--offline", "--lib"],
     "expected_exit": 0, "kind": "repeat-100"},
    {"id": "kernel-qemu-check",
     "argv": ["cargo", "check", "--offline", "-p", "starry-kernel",
              "--features", "qemu"],
     "expected_exit": 0, "kind": "run"},
    {"id": "kernel-lichee-d1-check",
     "argv": ["cargo", "check", "--offline", "-p", "starry-kernel",
              "--features", "lichee-d1"],
     "expected_exit": 101, "kind": "d1",
     "d1_counts": {"error[E0432]": 20, "error[E0433]": 5}},
    {"id": "build-image",
     "argv": ["make", "LOG=info", "build"],
     "expected_exit": 0, "kind": "run"},
    {"id": "build-ms01",
     "argv": ["/opt/musl/riscv64-linux-musl-cross/bin/"
              "riscv64-linux-musl-gcc", "-static", "-O2", "-o",
              "tests/ms01_socket_baseline", "tests/ms01_socket_baseline.c"],
     "expected_exit": 0, "kind": "run"},
    {"id": "build-payloads",
     "argv": ["make", "-B", "tests/ms02_guest_service",
              "tests/ms03_irq_probe", "tests/ms04_rx_probe",
              "tests/ms05_data_plane_probe"],
     "expected_exit": 0, "kind": "run"},
    {"id": "rustfmt-check",
     "argv": ["rustfmt", "--check", "--edition", "2024",
              "--config", "skip_children=true",
              "crates/axdriver_net/src/lib.rs",
              "crates/axdriver_virtio/src/net.rs",
              "crates/axnet/src/async_rx.rs",
              "crates/axnet/src/device/ethernet.rs",
              "crates/axnet/src/device/fixed_queue.rs",
              "crates/axnet/src/device/mod.rs",
              "crates/axnet/src/device/tests.rs",
              "crates/axnet/src/diag.rs",
              "crates/axnet/src/flush.rs",
              "crates/axnet/src/lib.rs",
              "crates/axnet/src/router.rs",
              "crates/axnet/src/service.rs",
              "crates/virtio-drivers/src/device/net/dev_raw.rs",
              "kernel/src/drivers/virtio_net_irq.rs",
              "kernel/src/drivers/virtio_net_irq_logic.rs",
              "kernel/src/syscall/fs/ctl.rs",
              "tests/ms03-irq-host-harness.rs",
              "tests/ms04-async-rx-host-harness.rs"],
     "expected_exit": 0, "kind": "run"},
    {"id": "openspec-validate-strict",
     "argv": ["openspec", "validate",
              "ms05-qemu-bounded-bidirectional-device-data-plane",
              "--strict"],
     "expected_exit": 0, "kind": "run"},
    {"id": "diff-check",
     "argv": ["git", "diff", "--check", "--",
              ".", ":(exclude)openspec/changes/ms05-qemu-bounded-"
                    "bidirectional-device-data-plane/evidence/**"],
     "expected_exit": 0, "kind": "run"},
    {"id": "diff-cached-check",
     "argv": ["git", "diff", "--cached", "--check", "--",
              ".", ":(exclude)openspec/changes/ms05-qemu-bounded-"
                    "bidirectional-device-data-plane/evidence/**"],
     "expected_exit": 0, "kind": "run"},
]

REQUIRED_GATE_IDS = [gate["id"] for gate in GATES]

# Six final artifacts: the QEMU image and the five guest payloads.
ARTIFACTS = [
    "StarryOS_riscv64-qemu-virt.bin",
    "tests/ms01_socket_baseline",
    "tests/ms02_guest_service",
    "tests/ms03_irq_probe",
    "tests/ms04_rx_probe",
    "tests/ms05_data_plane_probe",
]

# Source files whose content hash is part of the source freeze. A later edit
# invalidates every dependent record. Build artifacts (the image and guest
# payloads) are NOT frozen sources: they are artifact records rebuilt by the
# build gates.
FROZEN_SOURCES = [
    "Makefile",
    "scripts/ms05_data_plane_stimulus.py",
    "scripts/ms05_evidence_capture.py",
    "scripts/ms05_evidence_audit.py",
    "tests/ms05_data_plane_probe.c",
    "tests/ms05_data_plane_probe_test.c",
    "tests/test_ms05_evidence_tools.py",
]

# Capability-failure markers: a record is classified `env-blocked` only when
# its raw log contains one of these at the earliest failing layer and the
# command was not modified. Anything else is a product failure.
CAPABILITY_MARKERS = (
    "Operation not permitted",
    "EPERM",
    "SIGSYS",
    "Read-only file system",
    "READ-ONLY",
    "Network is unreachable",
    "TERMINAL",
    "CAPABILITY",
)

# Product-failure markers: compile/link/assert/source/audit/diff failures
# that must never be rewritten as environment blocks, regardless of position.
PRODUCT_FAILURE_MARKERS = (
    "error[",
    "error:",
    "undefined reference",
    "assertion failed",
    "FAILED",
    "panicked at",
    "fatal error",
    "trailing whitespace",
)

# Artifact producer map: an artifact record's generating_gate must match the
# declared Gate that actually built it.
ARTIFACT_PRODUCERS = {
    "tests/ms01_socket_baseline": "build-ms01",
}


def artifact_record_specs() -> list[tuple[str, list[str]]]:
    """The exact 18 artifact command records as (gate_id, argv) pairs.

    Deterministic derivation from the six artifacts and three commands
    (file/stat/sha256sum), shared by the capture runner and the audit
    validator so the set cannot drift between the two."""
    specs = []
    for artifact in ARTIFACTS:
        for argv in (["file", artifact],
                     ["stat", "--format=%n %s %Y", artifact],
                     ["sha256sum", artifact]):
            gate_id = (f"artifact-{Path(artifact).name}-"
                       f"{Path(argv[0]).name}-{len(specs)}")
            specs.append((gate_id, argv))
    return specs


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    return sha256_bytes(path.read_bytes())


def write_log(log_path: Path, raw: str, exit_code: int) -> str:
    """Write the complete stdout/stderr plus a deterministic capture-provenance
    trailer so a silently-successful subprocess still yields a non-empty,
    hash-stable raw log."""
    trailer = f"# ms05 capture exit={exit_code}\n"
    log_path.write_text(raw + trailer)
    return sha256_bytes((raw + trailer).encode())


def rfc3339_now() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.%fZ")


def earliest_failure_layer(raw: str) -> str | None:
    """Classify the earliest failing layer of a raw log.

    A product-failure marker anywhere in the log classifies the log as
    `product`: a mixed product/environment log is ambiguous and must never be
    handed off as env-blocked, even when a capability marker appears first.
    Only a log with capability markers and no product marker is `capability`.
    """
    if any(marker in raw for marker in PRODUCT_FAILURE_MARKERS):
        return "product"
    if any(marker in raw for marker in CAPABILITY_MARKERS):
        return "capability"
    return None


def classify(exit_code: int, expected_exit: int, raw: str) -> str:
    """Classify a record: pass when the exit matches; env-blocked only when
    the earliest failing layer is a capability marker and no product failure
    appears anywhere; else fail."""
    if exit_code == expected_exit:
        return "pass"
    if earliest_failure_layer(raw) == "capability":
        return "env-blocked"
    return "fail"


def classify_d1(exit_code: int, expected_exit: int, raw: str,
                counts: dict[str, int]) -> tuple[str, str | None]:
    """D1 classification: exit 101 is expected only when the raw log contains
    exactly the established diagnostic counts and no unclassified error."""
    if exit_code != expected_exit:
        if earliest_failure_layer(raw) == "capability":
            return "env-blocked", None
        return "fail", "exit mismatch"
    actual = {code: raw.count(code) for code in counts}
    if actual != counts:
        detail = "diagnostic count mismatch: " + json.dumps(
            {k: actual.get(k, 0) for k in counts})
        return "fail", detail
    other_errors = [line for line in raw.splitlines()
                    if line.startswith("error[") and
                    not any(line.startswith(code)
                            for code in counts)]
    if other_errors:
        return "fail", f"unclassified error: {other_errors[0]}"
    return "pass", None


def run_subprocess(argv: list[str], cwd: Path, root: Path, gate_id: str,
                   timeout: int, log_name: str | None = None) -> dict:
    """One capture primitive shared by normal, D1, repeat-100 and artifact
    records: run literal argv, record RFC3339 start/end, exit, complete raw
    output and its hash. Every call follows this single boundary."""
    rel_log = log_name if log_name is not None else f"{gate_id}.log"
    log_path = root / "logs" / rel_log
    log_path.parent.mkdir(parents=True, exist_ok=True)
    start = rfc3339_now()
    try:
        proc = subprocess.run(argv, cwd=str(cwd), capture_output=True,
                              text=True, timeout=timeout)
    except subprocess.TimeoutExpired as error:
        raw = f"TIMEOUT: {error}\n"
        log_sha = write_log(log_path, raw, -1)
        return {
            "start": start, "end": rfc3339_now(), "exit": -1,
            "raw": raw, "log": str(log_path.relative_to(root)),
            "log_sha256": log_sha,
        }
    raw = proc.stdout + proc.stderr
    log_sha = write_log(log_path, raw, proc.returncode)
    return {
        "start": start, "end": rfc3339_now(), "exit": proc.returncode,
        "raw": raw, "log": str(log_path.relative_to(root)),
        "log_sha256": log_sha,
    }


def run_record(gate: dict, root: Path, cwd: Path) -> dict:
    """Run one normal subprocess with literal argv."""
    result = run_subprocess(gate["argv"], cwd, root, gate["id"], 1800)
    classification = classify(result["exit"], gate["expected_exit"],
                              result["raw"])
    return {
        "gate_id": gate["id"], "argv": gate["argv"], "cwd": str(cwd),
        "start": result["start"], "end": result["end"],
        "exit": result["exit"], "classification": classification,
        "log": result["log"], "log_sha256": result["log_sha256"],
    }


def run_d1(gate: dict, root: Path, cwd: Path) -> dict:
    """D1 record: exit 101 plus exact diagnostic-count contract."""
    result = run_subprocess(gate["argv"], cwd, root, gate["id"], 1800)
    classification, detail = classify_d1(
        result["exit"], gate["expected_exit"], result["raw"],
        gate["d1_counts"])
    record = {
        "gate_id": gate["id"], "argv": gate["argv"], "cwd": str(cwd),
        "start": result["start"], "end": result["end"],
        "exit": result["exit"], "classification": classification,
        "log": result["log"], "log_sha256": result["log_sha256"],
    }
    if detail is not None:
        record["detail"] = detail
    return record


def run_repeat100(gate: dict, root: Path, cwd: Path) -> dict:
    """A 100x Gate: 100 indexed child records, each with complete raw output
    and hash; the parent record is a derived summary only."""
    gate_id = gate["id"]
    log_dir = root / "logs" / gate_id
    log_dir.mkdir(parents=True, exist_ok=True)
    parent_start = rfc3339_now()
    children = []
    all_pass = True
    summary = []
    for index in range(1, 101):
        child = run_subprocess(gate["argv"], cwd, root, gate_id, 600,
                               log_name=f"{gate_id}/{index:04d}.log")
        classification = classify(child["exit"], gate["expected_exit"],
                                  child["raw"])
        if classification != "pass":
            all_pass = False
        children.append({
            "index": index, "exit": child["exit"],
            "classification": classification,
            "log": child["log"], "log_sha256": child["log_sha256"],
        })
        summary.append(
            f"run {index}: exit={child['exit']} "
            f"classification={classification}")
    parent_log = root / "logs" / f"{gate_id}.summary.log"
    summary_sha = write_log(parent_log, "\n".join(summary) + "\n", 0)
    return {
        "gate_id": gate_id, "argv": gate["argv"], "cwd": str(cwd),
        "start": parent_start, "end": rfc3339_now(),
        "exit": 0 if all_pass else 1,
        "classification": "pass" if all_pass else "fail",
        "kind": "repeat-100", "children": children,
        "log": str(parent_log.relative_to(root)),
        "log_sha256": summary_sha,
    }


def run_artifact_records(root: Path, cwd: Path) -> list[dict]:
    """Literal file/stat/sha256sum records plus artifact identity records."""
    records = []
    artifact_records = []
    for gate_id, argv in artifact_record_specs():
        result = run_subprocess(argv, cwd, root, gate_id, 60)
        records.append({
            "gate_id": gate_id, "argv": argv, "cwd": str(cwd),
            "start": result["start"], "end": result["end"],
            "exit": result["exit"],
            "classification": "pass" if result["exit"] == 0 else "fail",
            "log": result["log"], "log_sha256": result["log_sha256"],
        })
    for artifact in ARTIFACTS:
        path = Path(artifact)
        artifact_records.append({
            "path": artifact,
            "size": path.stat().st_size if path.exists() else -1,
            "mtime": int(path.stat().st_mtime) if path.exists() else -1,
            "sha256": sha256_file(path) if path.exists() else None,
            "generating_gate": ARTIFACT_PRODUCERS.get(
                artifact, "build-image" if artifact.endswith(".bin")
                else "build-payloads"),
        })
    return records, artifact_records


def git_readonly_output(cwd: Path, argv: list[str]) -> str:
    """Deterministic read-only Git output for index/worktree identity."""
    proc = subprocess.run(argv, cwd=str(cwd), capture_output=True, text=True)
    if proc.returncode != 0:
        return ""
    return proc.stdout


def source_identity(cwd: Path) -> dict:
    """Read-only index/worktree identity: hashed git state that a later edit
    invalidates. The worktree identity hashes the unstaged diff (tracked
    bytes, not status categories) plus deterministic untracked path/content
    entries, so editing an already-modified file still invalidates the
    freeze. Never writes objects or mutates the live index/worktree."""
    index = git_readonly_output(cwd, ["git", "ls-files", "--stage"])
    diff = git_readonly_output(cwd, ["git", "diff", "--binary"])
    untracked = git_readonly_output(cwd, ["git", "ls-files", "--others",
                                          "--exclude-standard"])
    untracked_parts = []
    for rel in sorted(untracked.splitlines()):
        path = cwd / rel
        try:
            content = path.read_bytes()
        except OSError:
            content = b""
        untracked_parts.append(f"{rel}\0{sha256_bytes(content)}")
    worktree = diff + "\n" + "\n".join(untracked_parts)
    return {
        "index_identity": sha256_bytes(index.encode()),
        "worktree_identity": sha256_bytes(worktree.encode()),
    }


def freeze_source(cwd: Path) -> dict:
    """Record source identity before any Gate runs."""
    frozen = {}
    for source in FROZEN_SOURCES:
        path = cwd / source
        frozen[source] = sha256_file(path) if path.exists() else None
    head = git_readonly_output(cwd, ["git", "rev-parse", "HEAD"]).strip()
    identity = source_identity(cwd)
    return {"captured_at": rfc3339_now(), "head": head, "files": frozen,
            **identity}


def verify_frozen(root: Path, cwd: Path) -> str | None:
    """Re-hash the frozen sources and identities; return the first drifted
    field name or None."""
    manifest = json.loads((root / "manifest.json").read_text())
    frozen = manifest["source_freeze"]["files"]
    for source, recorded_hash in frozen.items():
        path = cwd / source
        actual = sha256_file(path) if path.exists() else None
        if actual != recorded_hash:
            return f"file:{source}"
    identity = source_identity(cwd)
    for key in ("index_identity", "worktree_identity"):
        if manifest["source_freeze"].get(key) != identity[key]:
            return key
    return None


def build_manifest(root: Path, cwd: Path) -> None:
    freeze = freeze_source(cwd)
    records: list[dict] = []
    for gate in GATES:
        if gate["kind"] == "repeat-100":
            records.append(run_repeat100(gate, root, cwd))
        elif gate["kind"] == "d1":
            records.append(run_d1(gate, root, cwd))
        else:
            records.append(run_record(gate, root, cwd))
    artifact_gates, artifact_records = run_artifact_records(root, cwd)
    records.extend(artifact_gates)
    manifest = {
        "schema_version": SCHEMA_VERSION,
        "root": str(root),
        "created": rfc3339_now(),
        "source_freeze": freeze,
        "records": records,
        "artifacts": artifact_records,
    }
    (root / "manifest.json").write_text(
        json.dumps(manifest, indent=2) + "\n")
    # Derived, non-authoritative artifact index: sha256 + path per artifact.
    sha_lines = []
    for entry in artifact_records:
        if entry["sha256"] is not None:
            sha_lines.append(f"{entry['sha256']}  {entry['path']}")
    (root / "artifacts.sha256").write_text("\n".join(sha_lines) + "\n")


def self_test() -> int:
    assert sha256_bytes(b"abc") == hashlib.sha256(b"abc").hexdigest()
    assert classify(0, 0, "ok") == "pass"
    assert classify(1, 0, "ok") == "fail"
    assert classify(1, 0, "Operation not permitted") == "env-blocked"
    assert classify(1, 0, "SIGSYS: bad system call") == "env-blocked"
    assert classify(1, 0, "Read-only file system") == "env-blocked"
    # equal/late/regressed classification stays a product fail, not a block
    assert classify(2, 0, "assertion failed") == "fail"

    # D1 contract: exact counts pass; wrong count and unclassified error fail.
    good = "\n".join(
        ["error[E0432]: unresolved import `axfs`"] * 20 +
        ["error[E0433]: cannot find module or crate `axfs` in this scope"] * 5)
    classification, detail = classify_d1(101, 101, good,
                                         {"error[E0432]": 20,
                                          "error[E0433]": 5})
    assert classification == "pass" and detail is None
    bad_count = good + "\nerror[E0432]: extra"
    classification, detail = classify_d1(101, 101, bad_count,
                                         {"error[E0432]": 20,
                                          "error[E0433]": 5})
    assert classification == "fail" and "diagnostic count" in (detail or "")
    unclassified = good + "\nerror[E0599]: other"
    classification, detail = classify_d1(101, 101, unclassified,
                                         {"error[E0432]": 20,
                                          "error[E0433]": 5})
    assert classification == "fail" and "unclassified" in (detail or "")
    wrong_exit = classify_d1(1, 101, good, {"error[E0432]": 20,
                                            "error[E0433]": 5})
    assert wrong_exit[0] == "fail"

    print("ms05 evidence capture self-test: schema=PASS hash=PASS "
          "classify=PASS d1=PASS")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--run", choices=["automatic"])
    parser.add_argument("--root", help="Evidence root directory")
    args = parser.parse_args()

    if args.self_test:
        return self_test()
    if args.run == "automatic":
        if not args.root:
            parser.error("--root is required with --run automatic")
        root = Path(args.root)
        root.mkdir(parents=True, exist_ok=True)
        (root / "logs").mkdir(parents=True, exist_ok=True)
        cwd = Path.cwd()
        build_manifest(root, cwd)
        print(f"ms05 evidence capture: automatic manifest written to "
              f"{root / 'manifest.json'}")
        return 0
    parser.error("--self-test or --run automatic is required")
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
