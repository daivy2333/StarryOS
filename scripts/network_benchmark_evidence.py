#!/usr/bin/env python3
"""MS16 network benchmark Evidence checker.

Validates Evidence directory completeness, field presence, round sets,
dual-endpoint ledger closure, and A/B comparison keys.

Usage:
  python3 scripts/network-benchmark-evidence.py --dir <evidence_dir>
  python3 scripts/network-benchmark-evidence.py --compare <dir_a> <dir_b>
  python3 scripts/network-benchmark-evidence.py --self-test
"""
import sys
import os
import json
import hashlib
import argparse

import network_benchmark_report as report

REQUIRED_FILES = [
    'manifest.json',
    'guest-netbench.ndjson',
    'host-netbench.ndjson',
    'host-cpu.ndjson',
]

REQUIRED_MANIFEST_FIELDS = {
    'schema_version', 'side', 'platform', 'driver_mode',
    'protocol', 'payload_size', 'flow_count', 'duration_s',
}

COMPARISON_FIELDS = [
    'benchmark_hash', 'kernel_hash', 'rootfs_hash', 'platform',
    'backend', 'netdev', 'mtu', 'offload', 'vhost', 'qemu_version',
    'machine', 'smp', 'memory_mb', 'icount', 'affinity', 'payload_size',
    'flow_count', 'duration_s', 'seed', 'completion_point', 'queue_size',
    'socket_buffer', 'telemetry', 'log_level',
]

B0_FILES = [
    'README.md',
    'qemu-command.txt',
    'qemu-serial.log',
    'guest-console.log',
    'capture.pcap',
    'irq-snapshots.log',
    'results.csv',
    'summary.json',
    'evidence-check.json',
]


def sha256_file(path):
    """Compute SHA-256 of a file. Returns None if file missing."""
    if not os.path.isfile(path):
        return None
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for chunk in iter(lambda: f.read(65536), b''):
            h.update(chunk)
    return h.hexdigest()


def check_required_files(evidence_dir, profile='foundation'):
    """Check that required files exist. Returns (list_of_missing, list_of_found)."""
    files = REQUIRED_FILES if profile in ('foundation', 'local') else REQUIRED_FILES + B0_FILES
    missing = []
    found = []
    for fname in files:
        path = os.path.join(evidence_dir, fname)
        if os.path.isfile(path):
            found.append(fname)
        else:
            missing.append(fname)
    return missing, found


def check_manifest(evidence_dir):
    """Validate manifest.json fields. Returns (ok, errors)."""
    path = os.path.join(evidence_dir, 'manifest.json')
    if not os.path.isfile(path):
        return False, ['manifest.json not found']

    try:
        with open(path, 'r') as f:
            manifest = json.load(f)
    except (json.JSONDecodeError, IOError) as e:
        return False, [f'manifest.json parse error: {e}']

    errors = []
    for field in REQUIRED_MANIFEST_FIELDS:
        if field not in manifest:
            errors.append(f'missing manifest field: {field}')

    if manifest.get('schema_version') != 1:
        errors.append(f'unsupported schema_version: {manifest.get("schema_version")}')

    return len(errors) == 0, errors


def count_ndjson_records(path):
    """Count valid JSON lines in an NDJSON file. Returns count or -1 on error."""
    if not os.path.isfile(path):
        return -1
    count = 0
    try:
        with open(path, 'r') as f:
            for line in f:
                line = line.strip()
                if not line:
                    continue
                try:
                    json.loads(line)
                    count += 1
                except json.JSONDecodeError:
                    return -1  # malformed
    except IOError:
        return -1
    return count


def check_round_closure(guest_path, host_path):
    """Verify exact bidirectional byte ledger: guest TX == host RX, host TX == guest RX."""
    errors = []
    host_rounds = {}
    guest_rounds = {}

    if os.path.isfile(host_path):
        with open(host_path, 'r') as f:
            for line in f:
                line = line.strip()
                if not line:
                    continue
                try:
                    rec = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if rec.get('type') == 'round':
                    key = (rec.get('run_id'), rec.get('test_id'), rec.get('round_id'))
                    host_rounds[key] = rec

    if os.path.isfile(guest_path):
        with open(guest_path, 'r') as f:
            for line in f:
                line = line.strip()
                if not line:
                    continue
                try:
                    rec = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if rec.get('type') != 'round':
                    continue
                if rec.get('status') == 'invalid':
                    continue
                key = (rec.get('run_id'), rec.get('test_id'), rec.get('round_id'))
                guest_rounds[key] = rec
                if key not in host_rounds:
                    errors.append(f'round {key} in guest but missing from host')
                else:
                    hrec = host_rounds[key]
                    if rec.get('status') != hrec.get('status'):
                        errors.append(f'round {key}: endpoint status mismatch')
                        continue
                    if (rec.get('config_fingerprint') and hrec.get('config_fingerprint')
                            and rec.get('config_fingerprint') != hrec.get('config_fingerprint')):
                        errors.append(f'round {key}: config fingerprint mismatch')
                    guest_tx = rec.get('tx_bytes', 0)
                    guest_rx = rec.get('rx_bytes', 0)
                    host_tx = hrec.get('tx_bytes', 0)
                    host_rx = hrec.get('rx_bytes', 0)

                    if guest_tx != host_rx:
                        errors.append(
                            f'round {key}: guest TX={guest_tx} != host RX={host_rx}')
                    if host_tx != guest_rx:
                        errors.append(
                            f'round {key}: host TX={host_tx} != guest RX={guest_rx}')
                    if rec.get('protocol', 'TCP') == 'UDP':
                        for endpoint, label in ((rec, 'guest'), (hrec, 'host')):
                            offered = endpoint.get('udp_offered', 0)
                            accepted = endpoint.get('udp_accepted', 0)
                            if accepted > offered:
                                errors.append(f'round {key}: {label} UDP accepted > offered')

    for key in sorted(set(host_rounds) - set(guest_rounds), key=str):
        errors.append(f'round {key} in host but missing from guest')

    return len(errors) == 0, errors


def extract_comparison_key(manifest):
    """Extract fields that form the comparison key."""
    return tuple((field, manifest.get(field)) for field in COMPARISON_FIELDS)


def compare(dir_a, dir_b):
    """Compare two Evidence directories for A/B compatibility."""
    result = {
        'comparable': True,
        'dir_a': dir_a,
        'dir_b': dir_b,
        'differences': [],
        'treatment': None,
    }

    ma_path = os.path.join(dir_a, 'manifest.json')
    mb_path = os.path.join(dir_b, 'manifest.json')

    if not os.path.isfile(ma_path):
        result['comparable'] = False
        result['differences'].append('dir_a manifest missing')
    if not os.path.isfile(mb_path):
        result['comparable'] = False
        result['differences'].append('dir_b manifest missing')

    if result['comparable']:
        with open(ma_path, 'r') as f:
            ma = json.load(f)
        with open(mb_path, 'r') as f:
            mb = json.load(f)

        # Treatment: the field that is allowed to differ
        treatment_a = ma.get('treatment')
        treatment_b = mb.get('treatment')

        if treatment_a and treatment_b and treatment_a != treatment_b:
            result['treatment'] = f'{treatment_a} -> {treatment_b}'

        key_a = extract_comparison_key(ma)
        key_b = extract_comparison_key(mb)

        if key_a != key_b:
            result['comparable'] = False
            for (fa, va), (fb, vb) in zip(key_a, key_b):
                if va != vb:
                    result['differences'].append(
                        f'{fa}: {va!r} != {vb!r}')

        if ma.get('schema_version') != mb.get('schema_version'):
            result['comparable'] = False
            result['differences'].append('schema_version mismatch')

    return result


def check_evidence(evidence_dir, profile='foundation'):
    """Full evidence check. Returns dict with pass/errors/missing/warnings."""
    result = {
        'pass': True,
        'dir': evidence_dir,
        'profile': profile,
        'errors': [],
        'missing': [],
        'warnings': [],
        'hashes': {},
    }

    missing, found = check_required_files(evidence_dir, profile)
    if missing:
        result['pass'] = False
        result['missing'] = missing

    manifest_ok, manifest_errors = check_manifest(evidence_dir)
    if not manifest_ok:
        result['pass'] = False
        result['errors'].extend(manifest_errors)

    # Check NDJSON validity
    for fname in found:
        if fname.endswith('.ndjson'):
            path = os.path.join(evidence_dir, fname)
            count = count_ndjson_records(path)
            if count < 0:
                result['pass'] = False
                result['errors'].append(f'{fname} is malformed (not valid JSON per line)')
            elif count == 0:
                result['warnings'].append(f'{fname} is empty')

    # Round closure
    guest_path = os.path.join(evidence_dir, 'guest-netbench.ndjson')
    host_path = os.path.join(evidence_dir, 'host-netbench.ndjson')
    closure_ok, closure_errors = check_round_closure(guest_path, host_path)
    if not closure_ok:
        result['errors'].extend(closure_errors)
        result['pass'] = False

    summary_path = os.path.join(evidence_dir, 'summary.json')
    if os.path.isfile(summary_path):
        cpu_path = os.path.join(evidence_dir, 'host-cpu.ndjson')
        try:
            reconstructed = report.generate_summary(
                guest_path, host_path, True, cpu_path)
            with open(summary_path, encoding='utf-8') as stream:
                recorded = json.load(stream)
            if recorded != reconstructed:
                result['pass'] = False
                result['errors'].append('summary.json does not match reconstructed summary')
        except (OSError, ValueError, json.JSONDecodeError) as exc:
            result['pass'] = False
            result['errors'].append(f'summary reconstruction failed: {exc}')

    manifest_path = os.path.join(evidence_dir, 'manifest.json')
    if os.path.isfile(manifest_path):
        with open(manifest_path, 'r') as f:
            manifest = json.load(f)
        declared_hashes = manifest.get('file_hashes', {})
        if profile in ('calibration', 'b0'):
            for field in ('benchmark_hash', 'kernel_hash', 'rootfs_hash'):
                if not manifest.get(field):
                    result['pass'] = False
                    result['errors'].append(f'missing manifest field: {field}')
            if not declared_hashes:
                result['pass'] = False
                result['errors'].append('file_hashes is required')
        for fname, declared in declared_hashes.items():
            actual = sha256_file(os.path.join(evidence_dir, fname))
            if actual != declared:
                result['pass'] = False
                result['errors'].append(f'hash mismatch: {fname}')

    # Compute hashes
    for fname in found:
        path = os.path.join(evidence_dir, fname)
        result['hashes'][fname] = sha256_file(path)

    return result


def run_self_test():
    """Self-test using fixture data."""
    base = os.path.join(os.path.dirname(__file__), '..',
                        'tests', 'fixtures', 'network-benchmark')
    valid_dir = os.path.join(base, 'valid')
    missing_dir = os.path.join(base, 'missing-file')
    invalid_dir = os.path.join(base, 'invalid')
    mismatch_a = os.path.join(base, 'mismatch-a')
    mismatch_b = os.path.join(base, 'mismatch-b')

    # 1. Valid evidence should pass
    result = check_evidence(valid_dir)
    if not result['pass']:
        print(f"SELF-TEST FAIL: valid fixture should pass: {result['errors']}",
              file=sys.stderr)
        return 1

    # 2. Missing files should fail
    result = check_evidence(missing_dir)
    if result['pass']:
        print("SELF-TEST FAIL: missing-file fixture should fail", file=sys.stderr)
        return 1
    if not result['missing']:
        print("SELF-TEST FAIL: missing-file should report missing files", file=sys.stderr)
        return 1

    # 3. Malformed should fail
    result = check_evidence(invalid_dir)
    if result['pass']:
        print("SELF-TEST FAIL: invalid fixture should fail", file=sys.stderr)
        return 1

    # 4. Mismatch comparison
    result = compare(mismatch_a, mismatch_b)
    if not result.get('comparable'):
        print(f"SELF-TEST FAIL: mismatch should be comparable: {result.get('differences')}",
              file=sys.stderr)
        return 1

    print("SELF-TEST PASS")
    return 0


def main():
    parser = argparse.ArgumentParser(description='MS16 Evidence checker')
    parser.add_argument('--dir', help='Evidence directory to check')
    parser.add_argument('--profile', choices=['foundation', 'local', 'calibration', 'b0'],
                        default='foundation', help='Evidence profile')
    parser.add_argument('--compare', nargs=2, metavar=('DIR_A', 'DIR_B'),
                        help='Compare two Evidence directories')
    parser.add_argument('--self-test', action='store_true', help='Run self-test')
    args = parser.parse_args()

    if args.self_test:
        sys.exit(run_self_test())

    if args.compare:
        result = compare(args.compare[0], args.compare[1])
        print(json.dumps(result, indent=2))
        if not result['comparable']:
            sys.exit(1)
    elif args.dir:
        result = check_evidence(args.dir, args.profile)
        print(json.dumps(result, indent=2))
        if not result['pass']:
            sys.exit(1)
    else:
        print("Error: --dir, --compare, or --self-test required", file=sys.stderr)
        sys.exit(1)


if __name__ == '__main__':
    main()
