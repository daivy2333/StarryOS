#!/usr/bin/env python3
"""MS16 host CPU/RSS collector — samples QEMU, peer and collector PID stats.

Usage:
  python3 scripts/network-benchmark-collect.py --pid <PID> [--interval 1] [--duration 30]
  python3 scripts/network-benchmark-collect.py --self-test

Output: NDJSON to stdout, one object per sample.
"""
import sys
import os
import time
import json
import signal
import argparse

CLK_TCK = os.sysconf(os.sysconf_names['SC_CLK_TCK'])


def sample_pid(pid):
    """Sample /proc/<pid>/stat and /proc/<pid>/status for a given PID.

    Returns a sample with process identity and accounting fields.
    """
    stat_path = f'/proc/{pid}/stat'
    status_path = f'/proc/{pid}/status'

    try:
        with open(stat_path, 'r') as f:
            stat_line = f.read()
    except (FileNotFoundError, ProcessLookupError, PermissionError):
        return None

    # /proc/<pid>/stat: fields separated by space, but comm (field 2) may
    # contain spaces within parentheses
    fields = stat_line.rstrip('\n').split(') ')
    if len(fields) < 2:
        return None
    before_paren = fields[0].split(' (')
    if len(before_paren) < 2:
        return None

    rest = fields[1].split()
    # fields: [0]=pid, [1]=comm in parens, then after ')' rest[0]=state,
    # [11]=utime, [12]=stime, [20]=rss (pages)
    try:
        stat_pid = int(before_paren[0])
        state = rest[0] if rest else '?'
        utime = int(rest[11]) if len(rest) > 11 else 0
        stime = int(rest[12]) if len(rest) > 12 else 0
        rss_pages = int(rest[21]) if len(rest) > 21 else 0
        threads = int(rest[17]) if len(rest) > 17 else 0
    except (ValueError, IndexError):
        return None

    rss_kb = rss_pages * (os.sysconf(os.sysconf_names['SC_PAGESIZE']) // 1024)

    starttime = int(rest[19]) if len(rest) > 19 else -1
    return {
        'pid': stat_pid,
        'pid_starttime': starttime,
        'scope': 'process',
        'clk_tck': CLK_TCK,
        'state': state,
        'utime_ticks': utime,
        'stime_ticks': stime,
        'rss_kb': rss_kb,
        'threads': threads,
        'timestamp_ns': time.monotonic_ns(),
        'numeric_status': 'ok',
    }


def sample_continues(previous, current):
    """Return whether PID identity and cumulative counters remain monotonic."""
    if previous['pid'] != current['pid']:
        return False, 'pid_changed'
    if previous['pid_starttime'] != current['pid_starttime']:
        return False, 'pid_reused'
    if current['utime_ticks'] < previous['utime_ticks']:
        return False, 'utime_regressed'
    if current['stime_ticks'] < previous['stime_ticks']:
        return False, 'stime_regressed'
    return True, 'ok'


def collect_loop(pids, interval_s, duration_s, scopes=None):
    """Sample given PIDs at interval_s for duration_s seconds, output NDJSON."""
    if interval_s <= 0 or duration_s <= 0:
        raise ValueError('interval and duration must be positive')
    started = time.monotonic()
    deadline = started + duration_s
    sample_seq = 0
    previous = {}

    while True:
        target = started + sample_seq * interval_s
        remaining = target - time.monotonic()
        if remaining > 0:
            time.sleep(remaining)
        if time.monotonic() >= deadline:
            break
        sample_seq += 1
        for index, pid in enumerate(pids):
            sample = sample_pid(pid)
            scope = scopes[index] if scopes else 'qemu'
            record = {
                'type': 'cpu_sample',
                'sample_seq': sample_seq,
                'elapsed_s': round(time.monotonic() - (deadline - duration_s), 3),
            }
            if sample is None:
                record['pid'] = pid
                record['scope'] = scope
                record['status'] = 'gone'
                record['numeric_status'] = 'pid_gone'
            else:
                record.update(sample)
                record['scope'] = scope
                if pid in previous:
                    valid, reason = sample_continues(previous[pid], sample)
                    if not valid:
                        record['status'] = 'invalid'
                        record['numeric_status'] = reason
                        print(json.dumps(record))
                        sys.stdout.flush()
                        return 2
                previous[pid] = sample
            print(json.dumps(record))
            sys.stdout.flush()
    return 0


def run_self_test():
    """Self-test: sample own PID and verify structure."""
    pid = os.getpid()
    sample = sample_pid(pid)
    if sample is None:
        print("SELF-TEST FAIL: cannot sample own PID", file=sys.stderr)
        return 1
    required = ('pid', 'pid_starttime', 'scope', 'clk_tck', 'utime_ticks',
                'stime_ticks', 'rss_kb', 'timestamp_ns', 'numeric_status')
    for key in required:
        if key not in sample:
            print(f"SELF-TEST FAIL: missing key '{key}'", file=sys.stderr)
            return 1
    if sample['pid'] != pid:
        print(f"SELF-TEST FAIL: pid mismatch {sample['pid']} != {pid}", file=sys.stderr)
        return 1

    # Test dead PID
    dead = sample_pid(99999999)
    if dead is not None:
        print("SELF-TEST FAIL: dead PID should return None", file=sys.stderr)
        return 1

    print("SELF-TEST PASS")
    return 0


def main():
    parser = argparse.ArgumentParser(description='MS16 host CPU/RSS collector')
    parser.add_argument('--pid', type=int, nargs='+', help='PID(s) to sample')
    parser.add_argument('--scope', nargs='+', choices=['qemu', 'peer', 'collector'],
                        help='Scope for each PID (must match --pid count)')
    parser.add_argument('--interval', type=float, default=1.0, help='Sample interval (s)')
    parser.add_argument('--duration', type=float, default=30.0, help='Collection duration (s)')
    parser.add_argument('--self-test', action='store_true', help='Run self-test')
    args = parser.parse_args()

    if args.self_test:
        sys.exit(run_self_test())

    if not args.pid:
        print("Error: --pid required (or --self-test)", file=sys.stderr)
        sys.exit(1)

    if args.scope and len(args.scope) != len(args.pid):
        print("Error: --scope count must match --pid count", file=sys.stderr)
        sys.exit(1)

    if args.interval <= 0 or args.duration <= 0:
        print("Error: --interval and --duration must be positive", file=sys.stderr)
        sys.exit(1)

    # Validate PIDs exist at start before starting collection
    for pid in args.pid:
        if sample_pid(pid) is None:
            print(f"Error: PID {pid} not found at start", file=sys.stderr)
            sys.exit(1)

    sys.exit(collect_loop(args.pid, args.interval, args.duration,
                          scopes=args.scope if args.scope else None))


if __name__ == '__main__':
    main()
