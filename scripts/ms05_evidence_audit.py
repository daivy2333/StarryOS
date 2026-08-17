#!/usr/bin/env python3
"""MS05 manifest/audit validator and qualification binder.

Audits the frozen Iteration 010 Evidence produced by
`ms05_evidence_capture.py`: schema, required Gate set and order, unique IDs,
parseable times, non-empty argv, exit/classification consistency, every raw
log's existence/non-emptiness/hash, 100x child completeness, source-freeze
identity, artifact identity, D1 exact diagnostics and R44 boundaries.

Negative fixtures mutate temporary copies only and must each return the exact
named error code. After the positive audit, the qualification record binds
the manifest hash, the audit-log hash and a PASS verdict; a final verifier
re-checks that binding.

Usage:
  python3 scripts/ms05_evidence_audit.py --self-test
  python3 scripts/ms05_evidence_audit.py --root <evidence-root>
  python3 scripts/ms05_evidence_audit.py --root <evidence-root> --write-qualification
  python3 scripts/ms05_evidence_audit.py --root <evidence-root> --verify-qualification
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import shutil
import sys
import tempfile
from datetime import datetime, timezone
from pathlib import Path

from ms05_evidence_capture import (ARTIFACTS, ARTIFACT_PRODUCERS, GATES,
                                   REQUIRED_GATE_IDS, earliest_failure_layer,
                                   artifact_record_specs, sha256_file)
import ms05_evidence_capture as capture

SCHEMA_VERSION = 1

REPEAT100_GATES = {"race-control-100x", "race-v3-100x",
                   "race-full-suite-100x"}
D1_GATES = {"kernel-lichee-d1-check"}
D1_EXPECTED = {"error[E0432]": 20, "error[E0433]": 5}

# Capability failure layers, in evaluation order: the earliest marker found
# in a raw log is the classification reason. Only these justify env-blocked.
R44_MARKERS = ("Operation not permitted", "EPERM", "SIGSYS",
               "Read-only file system", "READ-ONLY",
               "Network is unreachable", "TERMINAL", "CAPABILITY")


class AuditFailure(Exception):
    def __init__(self, code: str, message: str) -> None:
        self.code = code
        super().__init__(f"[{code}] {message}")


def parse_rfc3339(value: str) -> None:
    try:
        datetime.strptime(value, "%Y-%m-%dT%H:%M:%S.%fZ")
    except ValueError:
        raise AuditFailure("MISSING_TIME", f"unparseable time: {value!r}")


def parse_rfc3339_dt(value: str) -> datetime | None:
    try:
        return datetime.strptime(value, "%Y-%m-%dT%H:%M:%S.%fZ")
    except ValueError:
        return None


def load_manifest(root: Path) -> dict:
    manifest_path = root / "manifest.json"
    if not manifest_path.exists():
        raise AuditFailure("BAD_SCHEMA", "manifest.json missing")
    try:
        manifest = json.loads(manifest_path.read_text())
    except (json.JSONDecodeError, OSError) as error:
        raise AuditFailure("BAD_SCHEMA", f"manifest.json parse error: {error}")
    if manifest.get("schema_version") != SCHEMA_VERSION:
        raise AuditFailure("BAD_SCHEMA",
                           f"schema_version != {SCHEMA_VERSION}")
    for field in ("root", "created", "source_freeze", "records", "artifacts"):
        if field not in manifest:
            raise AuditFailure("BAD_SCHEMA", f"manifest missing {field}")
    return manifest


def check_log(root: Path, rel_path: str, declared_hash: str | None,
              nonempty: bool = True) -> None:
    """A referenced raw log must exist, be non-empty and match its hash."""
    log_path = root / rel_path
    if not log_path.exists():
        raise AuditFailure("MISSING_LOG", f"log missing: {rel_path}")
    raw = log_path.read_bytes()
    if nonempty and len(raw) == 0:
        raise AuditFailure("EMPTY_LOG", f"log empty: {rel_path}")
    actual = hashlib.sha256(raw).hexdigest()
    if declared_hash is not None and actual != declared_hash:
        raise AuditFailure("LOG_HASH_MISMATCH",
                           f"log hash mismatch: {rel_path}")


def audit_record(root: Path, record: dict, gate_id: str | None = None) -> None:
    rid = record.get("gate_id")
    if gate_id is not None and rid != gate_id:
        raise AuditFailure("REQUIRED_GATE_MISSING",
                           f"expected gate {gate_id}, got {rid!r}")
    if not rid or not isinstance(rid, str):
        raise AuditFailure("MISSING_ARGV", "record lacks gate_id")
    argv = record.get("argv")
    if not argv or not isinstance(argv, list) or not all(
            isinstance(a, str) and a for a in argv):
        raise AuditFailure("MISSING_ARGV", f"{rid}: non-empty argv required")
    parse_rfc3339(record.get("start", ""))
    parse_rfc3339(record.get("end", ""))
    if not isinstance(record.get("exit"), int):
        raise AuditFailure("MISSING_EXIT", f"{rid}: exit must be an int")
    classification = record.get("classification")
    if classification not in ("pass", "fail", "env-blocked"):
        raise AuditFailure("UNSUPPORTED_CLASSIFICATION",
                           f"{rid}: bad classification {classification!r}")
    log = record.get("log")
    if not log or not isinstance(log, str):
        raise AuditFailure("MISSING_LOG", f"{rid}: no log path")
    check_log(root, log, record.get("log_sha256"))
    if classification == "env-blocked":
        reason = audit_env_blocked(root / log)
        if reason is None:
            raise AuditFailure("UNSUPPORTED_CLASSIFICATION",
                               f"{rid}: env-blocked without capability marker")


def audit_env_blocked(log_path: Path) -> str | None:
    """Earliest capability-failure reason for a raw log, or None when the log
    is a product failure or an ambiguous mixed log. The classification is
    shared with the capture runner so both reject the same logs."""
    raw = log_path.read_text(errors="replace")
    if earliest_failure_layer(raw) != "capability":
        return None
    for marker in R44_MARKERS:
        if marker in raw:
            return marker
    return None


def audit_child_record(root: Path, child: dict) -> None:
    """An indexed 100x child record: no argv/time, but exit, classification,
    log and hash are mandatory."""
    if not isinstance(child.get("exit"), int):
        raise AuditFailure("MISSING_EXIT", "child exit must be an int")
    classification = child.get("classification")
    if classification not in ("pass", "fail", "env-blocked"):
        raise AuditFailure("UNSUPPORTED_CLASSIFICATION",
                           f"child bad classification {classification!r}")
    log = child.get("log")
    if not log or not isinstance(log, str):
        raise AuditFailure("MISSING_LOG", "child no log path")
    check_log(root, log, child.get("log_sha256"))


def audit_repeat100(root: Path, record: dict, gate_id: str) -> None:
    audit_record(root, record, gate_id)
    children = record.get("children")
    if not isinstance(children, list) or len(children) != 100:
        raise AuditFailure("INCOMPLETE_CHILD_SET",
                           f"{gate_id}: expected 100 children, got "
                           f"{len(children) if isinstance(children, list) else 'none'}")
    seen = set()
    for child in children:
        index = child.get("index")
        if not isinstance(index, int) or index < 1 or index > 100:
            raise AuditFailure("INCOMPLETE_CHILD_SET",
                               f"{gate_id}: bad child index {index!r}")
        if index in seen:
            raise AuditFailure("INCOMPLETE_CHILD_SET",
                               f"{gate_id}: duplicate child index {index}")
        seen.add(index)
        audit_child_record(root, child)
    if len(seen) != 100:
        raise AuditFailure("INCOMPLETE_CHILD_SET",
                           f"{gate_id}: child indexes 1..100 required")


def audit_d1(root: Path, record: dict, gate_id: str) -> None:
    audit_record(root, record, gate_id)
    log_path = root / record["log"]
    raw = log_path.read_text(errors="replace")
    if record["exit"] != 101:
        if record["classification"] == "env-blocked":
            return
        raise AuditFailure("D1_DIAGNOSTIC_COUNT",
                           f"{gate_id}: exit {record['exit']} != 101")
    actual = {code: raw.count(code) for code in D1_EXPECTED}
    if actual != D1_EXPECTED:
        raise AuditFailure("D1_DIAGNOSTIC_COUNT",
                           f"{gate_id}: count mismatch {actual}")
    for line in raw.splitlines():
        if line.startswith("error[") and not any(
                line.startswith(code) for code in D1_EXPECTED):
            raise AuditFailure("D1_UNCLASSIFIED_ERROR",
                               f"{gate_id}: {line}")


def audit_artifacts(root: Path, manifest: dict) -> None:
    artifacts = manifest.get("artifacts")
    if not isinstance(artifacts, list):
        raise AuditFailure("ARTIFACT_MISMATCH", "artifacts missing")
    recorded = {a["path"]: a for a in artifacts}
    for expected in ARTIFACTS:
        entry = recorded.get(expected)
        if entry is None:
            raise AuditFailure("ARTIFACT_MISMATCH",
                               f"artifact {expected} missing")
        path = Path(expected)
        if not path.exists():
            raise AuditFailure("ARTIFACT_MISMATCH",
                               f"artifact {expected} missing on disk")
        actual_hash = sha256_file(path)
        if entry.get("sha256") != actual_hash:
            raise AuditFailure("ARTIFACT_MISMATCH",
                               f"artifact {expected} hash mismatch")
        if entry.get("size") != path.stat().st_size:
            raise AuditFailure("ARTIFACT_MISMATCH",
                               f"artifact {expected} size mismatch")
        if entry.get("mtime") != int(path.stat().st_mtime):
            raise AuditFailure("ARTIFACT_MISMATCH",
                               f"artifact {expected} mtime mismatch")
        expected_gate = ARTIFACT_PRODUCERS.get(
            expected, "build-image" if expected.endswith(".bin")
            else "build-payloads")
        if entry.get("generating_gate") != expected_gate:
            raise AuditFailure("ARTIFACT_PRODUCER",
                               f"artifact {expected}: producer "
                               f"{entry.get('generating_gate')!r} != "
                               f"{expected_gate}")


def audit_artifact_records(root: Path, manifest: dict, cwd: Path) -> None:
    """The exact 18 artifact command records (file/stat/sha256sum per
    artifact) must all exist with the literal derived argv, pass the common
    record checks, exit 0 with `pass` classification, and carry a cwd
    consistent with the qualification root. Missing, unexpected or damaged
    artifact records fail before derived artifact entries (duplicates are
    already rejected by the global gate_id check)."""
    records = manifest["records"]
    by_id = {record.get("gate_id"): record for record in records}
    specs = artifact_record_specs()
    expected_ids = {gate_id for gate_id, _ in specs}
    for gate_id, argv in specs:
        record = by_id.get(gate_id)
        if record is None:
            raise AuditFailure("ARTIFACT_RECORD_MISSING",
                               f"artifact record missing: {gate_id}")
        if record.get("argv") != argv:
            raise AuditFailure("ARTIFACT_RECORD_ARGV",
                               f"{gate_id}: argv {record.get('argv')!r} != "
                               f"{argv!r}")
        audit_record(root, record, gate_id)
        if record.get("exit") != 0 or record.get("classification") != "pass":
            raise AuditFailure("ARTIFACT_RECORD_FAILED",
                               f"{gate_id}: exit {record.get('exit')} "
                               f"classification {record.get('classification')}")
        if record.get("cwd") != str(cwd):
            raise AuditFailure("ARTIFACT_RECORD_CWD",
                               f"{gate_id}: cwd {record.get('cwd')!r} != "
                               f"{cwd}")
    for record in records:
        rid = record.get("gate_id")
        if isinstance(rid, str) and rid.startswith("artifact-") and \
                rid not in expected_ids:
            raise AuditFailure("ARTIFACT_RECORD_UNEXPECTED",
                               f"unexpected artifact record: {rid}")


def audit_source_freeze(manifest: dict, cwd: Path) -> None:
    """A source edit after freeze invalidates dependent records. The freeze
    also binds index/worktree identity and the temporal order
    freeze <= gate start <= gate end."""
    frozen = manifest["source_freeze"].get("files", {})
    if not frozen:
        raise AuditFailure("SOURCE_AFTER_FREEZE", "source_freeze empty")
    for path, recorded_hash in frozen.items():
        file_path = cwd / path
        actual = sha256_file(file_path) if file_path.exists() else None
        if actual != recorded_hash:
            raise AuditFailure("SOURCE_AFTER_FREEZE",
                               f"source drifted after freeze: {path}")
    identity = capture.source_identity(cwd)
    for key in ("index_identity", "worktree_identity"):
        if manifest["source_freeze"].get(key) != identity[key]:
            raise AuditFailure(
                "INDEX_DRIFT" if key == "index_identity" else
                "WORKTREE_DRIFT",
                f"{key} drifted after freeze")
    captured = parse_rfc3339_dt(
        manifest["source_freeze"].get("captured_at", ""))
    if captured is None:
        raise AuditFailure("MISSING_TIME", "freeze captured_at unparseable")
    for record in manifest["records"]:
        start = parse_rfc3339_dt(record.get("start", ""))
        end = parse_rfc3339_dt(record.get("end", ""))
        if start is None or end is None:
            raise AuditFailure("MISSING_TIME",
                               f"{record.get('gate_id')}: time unparseable")
        if captured > start or start > end:
            raise AuditFailure("TIME_ORDER",
                               f"{record.get('gate_id')}: freeze <= start "
                               f"<= end violated")


def audit_manifest(root: Path, cwd: Path) -> dict:
    manifest = load_manifest(root)
    records = manifest["records"]
    by_id: dict[str, dict] = {}
    for record in records:
        rid = record.get("gate_id")
        if rid in by_id:
            raise AuditFailure("DUPLICATE_GATE_ID", f"duplicate gate {rid}")
        by_id[rid] = record
    missing = [gid for gid in REQUIRED_GATE_IDS if gid not in by_id]
    if missing:
        raise AuditFailure("REQUIRED_GATE_MISSING",
                           f"missing={missing}")
    # required gates must appear in the declared relative order (extra
    # artifact records are allowed after the required set)
    observed_order = [r.get("gate_id") for r in records
                      if r.get("gate_id") in REQUIRED_GATE_IDS]
    if observed_order != REQUIRED_GATE_IDS:
        raise AuditFailure("GATE_ORDER",
                           f"required order mismatch: {observed_order}")
    for gate in GATES:
        gate_id = gate["id"]
        record = by_id[gate_id]
        if gate_id in REPEAT100_GATES:
            audit_repeat100(root, record, gate_id)
        elif gate_id in D1_GATES:
            audit_d1(root, record, gate_id)
        else:
            audit_record(root, record, gate_id)
        if record["classification"] == "fail":
            detail = record.get("detail")
            raise AuditFailure("REQUIRED_GATE_MISSING",
                               f"gate {gate_id} failed"
                               + (f": {detail}" if detail else ""))
        if record["exit"] != gate["expected_exit"] and \
                record["classification"] != "env-blocked":
            raise AuditFailure("MISSING_EXIT",
                               f"gate {gate_id}: exit {record['exit']} != "
                               f"expected {gate['expected_exit']}")
    audit_artifact_records(root, manifest, cwd)
    audit_artifacts(root, manifest)
    audit_source_freeze(manifest, cwd)
    return manifest


def collect_env_blocked(manifest: dict) -> list[dict]:
    """Manifest record references for env-blocked gates, or []."""
    blocked = []
    for record in manifest["records"]:
        if record.get("classification") == "env-blocked":
            blocked.append({
                "gate_id": record["gate_id"],
                "exit": record["exit"],
                "reason": audit_env_blocked(
                    Path(manifest["root"]) / record["log"]),
                "argv": record["argv"],
            })
    return blocked


# ── Negative fixtures (exact-code) ────────────────────────────────────

def build_valid_fixture(root: Path) -> None:
    """A minimal-but-valid Evidence tree matching the required Gate set."""
    from ms05_evidence_capture import GATES as _GATES  # noqa: F401
    logs = root / "logs"
    logs.mkdir(parents=True, exist_ok=True)
    records = []
    for gate in GATES:
        gate_id = gate["id"]
        if gate_id in REPEAT100_GATES:
            log_dir = logs / gate_id
            log_dir.mkdir(parents=True, exist_ok=True)
            children = []
            for index in range(1, 101):
                child_log = log_dir / f"{index:04d}.log"
                child_log.write_text(f"run {index}\n")
                children.append({
                    "index": index, "exit": 0, "classification": "pass",
                    "log": f"logs/{gate_id}/{index:04d}.log",
                    "log_sha256": sha256_file(child_log),
                })
            summary = logs / f"{gate_id}.summary.log"
            summary.write_text("100 runs\n")
            records.append({
                "gate_id": gate_id, "argv": gate["argv"], "cwd": str(root),
                "start": "2026-08-15T00:00:00.000000Z",
                "end": "2026-08-15T00:01:00.000000Z", "exit": 0,
                "classification": "pass", "kind": "repeat-100",
                "children": children,
                "log": f"logs/{gate_id}.summary.log",
                "log_sha256": sha256_file(summary),
            })
        elif gate_id in D1_GATES:
            log = logs / f"{gate_id}.log"
            raw = "\n".join(
                ["error[E0432]: unresolved import `axfs`"] * 20 +
                ["error[E0433]: cannot find module or crate `axfs` in this "
                 "scope"] * 5) + "\n"
            log.write_text(raw)
            records.append({
                "gate_id": gate_id, "argv": gate["argv"], "cwd": str(root),
                "start": "2026-08-15T00:00:00.000000Z",
                "end": "2026-08-15T00:01:00.000000Z", "exit": 101,
                "classification": "pass",
                "log": f"logs/{gate_id}.log", "log_sha256": sha256_file(log),
            })
        else:
            log = logs / f"{gate_id}.log"
            log.write_text(f"{gate_id} output\n")
            records.append({
                "gate_id": gate_id, "argv": gate["argv"], "cwd": str(root),
                "start": "2026-08-15T00:00:00.000000Z",
                "end": "2026-08-15T00:01:00.000000Z", "exit": 0,
                "classification": "pass",
                "log": f"logs/{gate_id}.log", "log_sha256": sha256_file(log),
            })
    for gate_id, argv in artifact_record_specs():
        log = logs / f"{gate_id}.log"
        log.write_text(f"{argv[0]} {argv[-1]}\n")
        records.append({
            "gate_id": gate_id, "argv": argv, "cwd": str(Path.cwd()),
            "start": "2026-08-15T00:00:00.000000Z",
            "end": "2026-08-15T00:01:00.000000Z", "exit": 0,
            "classification": "pass",
            "log": f"logs/{gate_id}.log", "log_sha256": sha256_file(log),
        })
    frozen = {src: sha256_file(Path(src)) if Path(src).exists() else None
              for src in capture.FROZEN_SOURCES}
    identity = capture.source_identity(Path.cwd())
    artifacts = []
    for artifact in ARTIFACTS:
        path = Path(artifact)
        artifacts.append({
            "path": artifact,
            "size": path.stat().st_size if path.exists() else -1,
            "mtime": int(path.stat().st_mtime) if path.exists() else -1,
            "sha256": sha256_file(path) if path.exists() else None,
            "generating_gate": ARTIFACT_PRODUCERS.get(
                artifact, "build-image" if artifact.endswith(".bin")
                else "build-payloads"),
        })
    manifest = {
        "schema_version": SCHEMA_VERSION,
        "root": str(root),
        "created": "2026-08-15T00:00:00.000000Z",
        "source_freeze": {
            "captured_at": "2026-08-15T00:00:00.000000Z",
            "head": "0000000000000000000000000000000000000000",
            "files": frozen,
            "index_identity": identity["index_identity"],
            "worktree_identity": identity["worktree_identity"],
        },
        "records": records,
        "artifacts": artifacts,
    }
    (root / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")


def _sync_log_hash(root: Path, log_path: Path) -> None:
    """After a fixture mutates a raw log, update its manifest record hash so
    the audit reaches the fixture's target check instead of a stale-hash
    failure."""
    manifest_path = root / "manifest.json"
    manifest = json.loads(manifest_path.read_text())
    rel = str(log_path.relative_to(root))
    updated = False
    for record in manifest["records"]:
        if record.get("log") == rel:
            record["log_sha256"] = sha256_file(log_path)
            updated = True
    if not updated:
        raise AssertionError(f"_sync_log_hash: {rel} not in manifest")
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n")


def fixture_mutate(template: Path, mutate, emit=print) -> None:
    """Copy the pristine fixture template, apply a mutation, run the audit,
    and require the exact expected code."""
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp) / "evidence"
        shutil.copytree(template, root)
        expected = mutate(root)
        try:
            audit_manifest(root, Path.cwd())
        except AuditFailure as error:
            if error.code != expected:
                raise AssertionError(
                    f"fixture returned {error.code}, expected {expected}: "
                    f"{error}")
            emit(f"  fixture {expected}: FAILED AS EXPECTED ({error})")
            return
        raise AssertionError(f"fixture {expected}: audit did NOT fail")


def run_fixtures(emit=print) -> None:
    emit("== Negative fixtures (each must fail for its exact code) ==")
    with tempfile.TemporaryDirectory() as tmp:
        template = Path(tmp) / "evidence-template"
        build_valid_fixture(template)

        def make_missing_log(root):
            (root / "logs/host-test.log").unlink()
            return "MISSING_LOG"

        def make_empty_log(root):
            (root / "logs/host-test.log").write_bytes(b"")
            return "EMPTY_LOG"

        def make_log_hash_mismatch(root):
            (root / "logs/host-test.log").write_text("tampered\n")
            return "LOG_HASH_MISMATCH"

        def make_missing_argv(root):
            manifest = json.loads((root / "manifest.json").read_text())
            manifest["records"][0]["argv"] = []
            (root / "manifest.json").write_text(json.dumps(manifest))
            return "MISSING_ARGV"

        def make_missing_time(root):
            manifest = json.loads((root / "manifest.json").read_text())
            manifest["records"][0]["start"] = "not-a-time"
            (root / "manifest.json").write_text(json.dumps(manifest))
            return "MISSING_TIME"

        def make_missing_exit(root):
            manifest = json.loads((root / "manifest.json").read_text())
            manifest["records"][0]["exit"] = "zero"
            (root / "manifest.json").write_text(json.dumps(manifest))
            return "MISSING_EXIT"

        def make_bad_classification(root):
            manifest = json.loads((root / "manifest.json").read_text())
            manifest["records"][0]["classification"] = "maybe"
            (root / "manifest.json").write_text(json.dumps(manifest))
            return "UNSUPPORTED_CLASSIFICATION"

        def make_incomplete_children(root):
            manifest = json.loads((root / "manifest.json").read_text())
            for record in manifest["records"]:
                if record.get("gate_id") == "race-control-100x":
                    record["children"] = record["children"][:50]
            (root / "manifest.json").write_text(json.dumps(manifest))
            return "INCOMPLETE_CHILD_SET"

        def make_source_after_freeze(root):
            manifest = json.loads((root / "manifest.json").read_text())
            manifest["source_freeze"]["files"]["Makefile"] = "0" * 64
            (root / "manifest.json").write_text(json.dumps(manifest))
            return "SOURCE_AFTER_FREEZE"

        def make_artifact_mismatch(root):
            manifest = json.loads((root / "manifest.json").read_text())
            manifest["artifacts"][0]["sha256"] = "0" * 64
            (root / "manifest.json").write_text(json.dumps(manifest))
            return "ARTIFACT_MISMATCH"

        def make_d1_count_mismatch(root):
            log = root / "logs/kernel-lichee-d1-check.log"
            log.write_text(log.read_text() + "\nerror[E0432]: extra\n")
            _sync_log_hash(root, log)
            return "D1_DIAGNOSTIC_COUNT"

        def make_d1_unclassified(root):
            log = root / "logs/kernel-lichee-d1-check.log"
            log.write_text(log.read_text() + "\nerror[E0599]: other\n")
            _sync_log_hash(root, log)
            return "D1_UNCLASSIFIED_ERROR"

        def make_missing_gate(root):
            manifest = json.loads((root / "manifest.json").read_text())
            manifest["records"] = [r for r in manifest["records"]
                                   if r.get("gate_id") != "axnet-default"]
            (root / "manifest.json").write_text(json.dumps(manifest))
            return "REQUIRED_GATE_MISSING"

        def make_gate_order(root):
            manifest = json.loads((root / "manifest.json").read_text())
            manifest["records"] = manifest["records"][1:] + \
                manifest["records"][:1]
            (root / "manifest.json").write_text(json.dumps(manifest))
            return "GATE_ORDER"

        def make_index_drift(root):
            manifest = json.loads((root / "manifest.json").read_text())
            manifest["source_freeze"]["index_identity"] = "0" * 64
            (root / "manifest.json").write_text(json.dumps(manifest))
            return "INDEX_DRIFT"

        def make_worktree_drift(root):
            manifest = json.loads((root / "manifest.json").read_text())
            manifest["source_freeze"]["worktree_identity"] = "0" * 64
            (root / "manifest.json").write_text(json.dumps(manifest))
            return "WORKTREE_DRIFT"

        def make_time_order(root):
            manifest = json.loads((root / "manifest.json").read_text())
            manifest["records"][0]["start"] = "2026-08-14T23:59:59.000000Z"
            (root / "manifest.json").write_text(json.dumps(manifest))
            return "TIME_ORDER"

        def make_wrong_artifact_producer(root):
            manifest = json.loads((root / "manifest.json").read_text())
            for artifact in manifest["artifacts"]:
                if artifact["path"] == "tests/ms01_socket_baseline":
                    artifact["generating_gate"] = "build-payloads"
            (root / "manifest.json").write_text(json.dumps(manifest))
            return "ARTIFACT_PRODUCER"

        def make_product_before_capability(root):
            manifest = json.loads((root / "manifest.json").read_text())
            record = next(r for r in manifest["records"]
                          if r.get("gate_id") == "host-test")
            record["classification"] = "env-blocked"
            record["exit"] = 1
            log = root / record["log"]
            (root / "manifest.json").write_text(json.dumps(manifest))
            log.write_text("error[E0432]: unresolved import `axfs`\n"
                           "Operation not permitted\n")
            _sync_log_hash(root, log)
            return "UNSUPPORTED_CLASSIFICATION"

        def make_capability_first_ambiguous(root):
            manifest = json.loads((root / "manifest.json").read_text())
            record = next(r for r in manifest["records"]
                          if r.get("gate_id") == "host-test")
            record["classification"] = "env-blocked"
            record["exit"] = 1
            log = root / record["log"]
            (root / "manifest.json").write_text(json.dumps(manifest))
            log.write_text("Operation not permitted\n"
                           "assertion failed: tx ledger\n")
            _sync_log_hash(root, log)
            return "UNSUPPORTED_CLASSIFICATION"

        def make_artifact_record_missing(root):
            gate_id, _ = artifact_record_specs()[0]
            manifest = json.loads((root / "manifest.json").read_text())
            manifest["records"] = [r for r in manifest["records"]
                                   if r.get("gate_id") != gate_id]
            (root / "manifest.json").write_text(json.dumps(manifest))
            return "ARTIFACT_RECORD_MISSING"

        def make_artifact_record_unexpected(root):
            manifest = json.loads((root / "manifest.json").read_text())
            manifest["records"].append({
                "gate_id": "artifact-extra-file-99", "argv": ["file", "x"],
                "cwd": str(Path.cwd()),
                "start": "2026-08-15T00:00:00.000000Z",
                "end": "2026-08-15T00:01:00.000000Z", "exit": 0,
                "classification": "pass",
                "log": "logs/artifact-extra-file-99.log",
                "log_sha256": "0" * 64,
            })
            (root / "manifest.json").write_text(json.dumps(manifest))
            return "ARTIFACT_RECORD_UNEXPECTED"

        def make_artifact_record_argv(root):
            gate_id, _ = artifact_record_specs()[0]
            manifest = json.loads((root / "manifest.json").read_text())
            for record in manifest["records"]:
                if record.get("gate_id") == gate_id:
                    record["argv"] = ["sha256sum", "other"]
            (root / "manifest.json").write_text(json.dumps(manifest))
            return "ARTIFACT_RECORD_ARGV"

        def make_artifact_record_failed(root):
            gate_id, _ = artifact_record_specs()[0]
            manifest = json.loads((root / "manifest.json").read_text())
            for record in manifest["records"]:
                if record.get("gate_id") == gate_id:
                    record["exit"] = 1
                    record["classification"] = "fail"
            (root / "manifest.json").write_text(json.dumps(manifest))
            return "ARTIFACT_RECORD_FAILED"

        def make_artifact_record_duplicate(root):
            gate_id, _ = artifact_record_specs()[0]
            manifest = json.loads((root / "manifest.json").read_text())
            duplicate = next(r for r in manifest["records"]
                             if r.get("gate_id") == gate_id)
            manifest["records"].append(dict(duplicate))
            (root / "manifest.json").write_text(json.dumps(manifest))
            return "DUPLICATE_GATE_ID"

        for mutate in (
            make_missing_log, make_empty_log, make_log_hash_mismatch,
            make_missing_argv, make_missing_time, make_missing_exit,
            make_bad_classification, make_incomplete_children,
            make_source_after_freeze, make_artifact_mismatch,
            make_d1_count_mismatch, make_d1_unclassified,
            make_missing_gate, make_gate_order,
            make_index_drift, make_worktree_drift, make_time_order,
            make_wrong_artifact_producer, make_product_before_capability,
            make_capability_first_ambiguous,
            make_artifact_record_missing, make_artifact_record_unexpected,
            make_artifact_record_argv, make_artifact_record_failed,
            make_artifact_record_duplicate,
        ):
            fixture_mutate(template, mutate, emit)


def positive_audit(root: Path, cwd: Path, emit) -> dict:
    manifest = audit_manifest(root, cwd)
    blocked = collect_env_blocked(manifest)
    if blocked:
        emit(f"  env-blocked records: {len(blocked)}")
    else:
        emit("  env-blocked records: none")
    emit("  positive audit: PASS (schema, gate set/order, raw logs, 100x "
         "children, source freeze, artifacts, D1 contract)")
    return manifest


def write_qualification(root: Path, cwd: Path) -> int:
    output: list[str] = []
    emit = output.append

    run_fixtures(emit)

    emit("\n== Positive audit of Iteration 010 Evidence ==")
    manifest = positive_audit(root, cwd, emit)

    audit_log_path = root / "evidence-audit.log"
    audit_log_path.write_text("\n".join(output) + "\n")

    manifest_hash = sha256_file(root / "manifest.json")
    audit_hash = sha256_file(audit_log_path)
    qualification = {
        "schema_version": SCHEMA_VERSION,
        "manifest_sha256": manifest_hash,
        "audit_log_sha256": audit_hash,
        "verdict": "PASS",
        "created": datetime.now(timezone.utc).strftime(
            "%Y-%m-%dT%H:%M:%S.%fZ"),
    }
    (root / "qualification.json").write_text(
        json.dumps(qualification, indent=2) + "\n")

    blocked = collect_env_blocked(manifest)
    (root / "env-blocked.json").write_text(json.dumps(blocked, indent=2) + "\n")

    for line in output:
        print(line)
    print("\nms05 evidence audit: negative fixtures PASS, positive audit "
          "PASS, qualification written")
    return 0


def verify_qualification(root: Path) -> int:
    qualification_path = root / "qualification.json"
    manifest_path = root / "manifest.json"
    audit_log_path = root / "evidence-audit.log"
    for path in (qualification_path, manifest_path, audit_log_path):
        if not path.exists():
            print(f"error: {path.name} missing", file=sys.stderr)
            return 2
    qualification = json.loads(qualification_path.read_text())
    manifest_hash = sha256_file(manifest_path)
    audit_hash = sha256_file(audit_log_path)
    if qualification.get("manifest_sha256") != manifest_hash:
        print("error: manifest hash does not match qualification",
              file=sys.stderr)
        return 1
    if qualification.get("audit_log_sha256") != audit_hash:
        print("error: audit-log hash does not match qualification",
              file=sys.stderr)
        return 1
    if qualification.get("verdict") != "PASS":
        print("error: qualification verdict is not PASS", file=sys.stderr)
        return 1
    print("ms05 evidence audit: qualification binding VERIFIED "
          "(manifest + audit log + PASS)")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", help="Evidence root directory")
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--write-qualification", action="store_true")
    parser.add_argument("--verify-qualification", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        run_fixtures()
        print("\nms05 evidence audit self-test: negative fixtures PASS")
        return 0
    if not args.root:
        parser.error("--root is required")
    root = Path(args.root)
    if not root.exists():
        print(f"error: Evidence root {root} not found", file=sys.stderr)
        return 2
    if args.verify_qualification:
        return verify_qualification(root)
    if args.write_qualification:
        return write_qualification(root, Path.cwd())
    # plain audit (no qualification write)
    run_fixtures()
    print("\n== Positive audit of Iteration 010 Evidence ==")
    positive_audit(root, Path.cwd(), print)
    print("\nms05 evidence audit: negative fixtures PASS, positive audit PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
