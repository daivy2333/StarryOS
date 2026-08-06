#!/usr/bin/env python3
"""Validate dual-endpoint MS16 NDJSON and write normalized CSV/JSON."""
import argparse
import csv
import json
import os
import statistics
import sys


SCHEMA_VERSION = 1
ROUND_FIELDS = {
    'schema_version', 'type', 'run_id', 'round_id', 'side', 'status',
    'protocol', 'direction', 'completion_point', 'duration_s',
    'tx_bytes', 'tx_packets', 'rx_bytes', 'rx_packets',
}


def load_ndjson(path):
    if not path or not os.path.isfile(path):
        raise ValueError(f'missing NDJSON: {path}')
    records = []
    with open(path, encoding='utf-8') as stream:
        for line_number, line in enumerate(stream, 1):
            if not line.strip():
                continue
            try:
                records.append(json.loads(line))
            except json.JSONDecodeError as exc:
                raise ValueError(f'{path}:{line_number}: malformed JSON') from exc
    return records


def validate_round(record, side):
    missing = ROUND_FIELDS - set(record)
    if missing:
        return False, f'missing fields: {sorted(missing)}'
    if record['schema_version'] != SCHEMA_VERSION:
        return False, 'unsupported schema_version'
    if record['type'] != 'round' or record['side'] != side:
        return False, 'type or side mismatch'
    if record['status'] not in ('valid', 'invalid'):
        return False, 'unknown status'
    return True, None


def round_index(records, side):
    result = {}
    malformed = []
    for record in records:
        if record.get('type') != 'round':
            continue
        key = (record.get('run_id'), record.get('test_id'), record.get('round_id'))
        if key in result:
            raise ValueError(f'duplicate {side} round: {key}')
        valid, reason = validate_round(record, side)
        if not valid:
            malformed.append({**record, 'status': 'invalid', 'invalid_reason': reason})
        else:
            result[key] = record
    return result, malformed


def percentile(values, fraction):
    if not values:
        return None
    ordered = sorted(values)
    index = min(len(ordered) - 1, int((len(ordered) - 1) * fraction))
    return ordered[index]


def cpu_summary(path):
    if not path:
        return {'available': False, 'reason': 'not_collected'}
    samples = [r for r in load_ndjson(path) if r.get('type') == 'cpu_sample']
    by_scope = {}
    for sample in samples:
        by_scope.setdefault(sample.get('scope', 'unknown'), []).append(sample)
    result = {'available': bool(samples), 'scopes': {}}
    for scope, values in by_scope.items():
        first, last = values[0], values[-1]
        if first.get('pid_starttime') != last.get('pid_starttime'):
            raise ValueError(f'PID identity changed for scope {scope}')
        ticks = (last.get('utime_ticks', 0) + last.get('stime_ticks', 0) -
                 first.get('utime_ticks', 0) - first.get('stime_ticks', 0))
        if ticks < 0:
            raise ValueError(f'CPU counter regressed for scope {scope}')
        elapsed_ns = last.get('timestamp_ns', 0) - first.get('timestamp_ns', 0)
        clock = last.get('clk_tck', 0)
        cpu_pct = None
        if elapsed_ns > 0 and clock > 0:
            cpu_pct = ticks / clock / (elapsed_ns / 1e9) * 100
        result['scopes'][scope] = {'samples': len(values), 'cpu_pct': cpu_pct}
    return result


def generate_summary(guest_path, host_path, require_complete=True, cpu_path=None):
    guest, malformed_guest = round_index(load_ndjson(guest_path), 'guest')
    if not host_path:
        if require_complete:
            raise ValueError('host peer NDJSON is required')
        invalid = malformed_guest + [
            {**record, 'status': 'invalid', 'invalid_reason': 'missing_peer'}
            for record in guest.values() if record.get('status') == 'invalid'
        ]
        return empty_summary(invalid)
    host, malformed_host = round_index(load_ndjson(host_path), 'host')
    if require_complete and set(guest) != set(host):
        missing_host = sorted(set(guest) - set(host), key=str)
        missing_guest = sorted(set(host) - set(guest), key=str)
        raise ValueError(f'round set mismatch: missing_host={missing_host}, missing_guest={missing_guest}')

    normalized = []
    invalid = malformed_guest + malformed_host
    rtt_samples = []
    for key in sorted(set(guest) | set(host), key=str):
        g = guest.get(key)
        h = host.get(key)
        if not g or not h:
            invalid.append({
                'run_id': key[0], 'test_id': key[1], 'round_id': key[2],
                'status': 'invalid', 'invalid_reason': 'missing_peer',
            })
            continue
        reason = None
        if g['status'] != h['status']:
            reason = 'peer_status_mismatch'
        elif g.get('protocol') != h.get('protocol'):
            reason = 'protocol_mismatch'
        elif g.get('config_fingerprint') and h.get('config_fingerprint') and \
                g['config_fingerprint'] != h['config_fingerprint']:
            reason = 'fingerprint_mismatch'
        elif g['tx_bytes'] != h['rx_bytes'] or h['tx_bytes'] != g['rx_bytes']:
            reason = 'ledger_mismatch'
        elif g['status'] == 'invalid':
            reason = g.get('invalid_reason', 'endpoint_invalid')
        direction = g['direction']
        receiver_bytes = h['rx_bytes'] if direction == 'TX' else g['rx_bytes']
        receiver_packets = h['rx_packets'] if direction == 'TX' else g['rx_packets']
        if direction == 'BIDI':
            receiver_bytes = g['rx_bytes'] + h['rx_bytes']
            receiver_packets = g['rx_packets'] + h['rx_packets']
        duration = max(float(g['duration_s']), float(h['duration_s']))
        row = {
            'run_id': key[0], 'test_id': key[1], 'round_id': key[2],
            'status': 'invalid' if reason else 'valid',
            'invalid_reason': reason or '', 'protocol': g['protocol'],
            'direction': direction, 'completion_point': 6,
            'duration_s': duration, 'receiver_bytes': receiver_bytes,
            'receiver_packets': receiver_packets,
            'goodput_mbps': receiver_bytes * 8 / duration / 1e6 if duration > 0 else 0,
            'pps': receiver_packets / duration if duration > 0 else 0,
            'udp_loss': g.get('udp_loss', 0) + h.get('udp_loss', 0),
            'udp_duplicate': g.get('udp_duplicate', 0) + h.get('udp_duplicate', 0),
            'udp_reorder': g.get('udp_reorder', 0) + h.get('udp_reorder', 0),
            'udp_corrupt': g.get('udp_corrupt', 0) + h.get('udp_corrupt', 0),
            'udp_late': g.get('udp_late', 0) + h.get('udp_late', 0),
        }
        samples = g.get('rtt_samples_us', []) + h.get('rtt_samples_us', [])
        rtt_samples.extend(samples)
        if reason:
            invalid.append(row)
        else:
            normalized.append(row)

    if require_complete and not normalized:
        raise ValueError('no valid dual-endpoint rounds')
    goodput = [row['goodput_mbps'] for row in normalized]
    pps = [row['pps'] for row in normalized]
    jitter = [abs(b - a) for a, b in zip(rtt_samples, rtt_samples[1:])]
    instret_values = []
    for record in list(guest.values()) + list(host.values()):
        if record.get('instret_status') != 'available':
            continue
        begin, end = record.get('instret_begin'), record.get('instret_end')
        overhead = record.get('instret_overhead', 0)
        bits = record.get('rx_bytes', 0) * 8
        if begin is not None and end is not None and end >= begin + overhead and bits:
            instret_values.append((end - begin - overhead) / bits)
    return {
        'schema_version': 1,
        'rounds': {'total': len(normalized) + len(invalid),
                   'valid': len(normalized), 'invalid': len(invalid)},
        'goodput_mbps': distribution(goodput),
        'pps': distribution(pps),
        'rtt_us': {'p50': percentile(rtt_samples, .50),
                   'p95': percentile(rtt_samples, .95),
                   'p99': percentile(rtt_samples, .99),
                   'max': max(rtt_samples) if rtt_samples else None},
        'delay_variation_us': statistics.median(jitter) if jitter else None,
        'cpu_efficiency': cpu_summary(cpu_path),
        'instret_instructions_per_bit': (
            statistics.median(instret_values) if instret_values else 'unavailable'),
        'udp_errors': {name: sum(row[name] for row in normalized)
                       for name in ('udp_loss', 'udp_duplicate', 'udp_reorder',
                                    'udp_corrupt', 'udp_late')},
        'valid_rounds': normalized,
        'invalid_rounds': invalid,
    }


def distribution(values):
    if not values:
        return {'median': None, 'min': None, 'max': None}
    return {'median': statistics.median(values), 'min': min(values), 'max': max(values)}


def empty_summary(invalid):
    return {
        'schema_version': 1,
        'rounds': {'total': len(invalid), 'valid': 0, 'invalid': len(invalid)},
        'goodput_mbps': distribution([]), 'pps': distribution([]),
        'rtt_us': {'p50': None, 'p95': None, 'p99': None, 'max': None},
        'delay_variation_us': None,
        'cpu_efficiency': {'available': False, 'reason': 'not_collected'},
        'instret_instructions_per_bit': 'unavailable',
        'udp_errors': {name: 0 for name in ('loss', 'duplicate', 'reorder', 'corrupt', 'late')},
        'valid_rounds': [], 'invalid_rounds': invalid,
    }


def write_outputs(summary, csv_path, json_path):
    fields = [
        'run_id', 'test_id', 'round_id', 'status', 'invalid_reason', 'protocol',
        'direction', 'completion_point', 'duration_s', 'receiver_bytes',
        'receiver_packets', 'goodput_mbps', 'pps', 'udp_loss', 'udp_duplicate',
        'udp_reorder', 'udp_corrupt', 'udp_late',
    ]
    with open(csv_path, 'w', newline='', encoding='utf-8') as stream:
        writer = csv.DictWriter(stream, fieldnames=fields, extrasaction='ignore')
        writer.writeheader()
        writer.writerows(summary['valid_rounds'] + summary['invalid_rounds'])
    with open(json_path, 'w', encoding='utf-8') as stream:
        json.dump(summary, stream, sort_keys=True, indent=2)
        stream.write('\n')


def run_self_test():
    base = os.path.join(os.path.dirname(__file__), '..', 'tests', 'fixtures',
                        'network-benchmark', 'valid')
    try:
        summary = generate_summary(os.path.join(base, 'guest-netbench.ndjson'),
                                   os.path.join(base, 'host-netbench.ndjson'))
    except ValueError as exc:
        print(f'SELF-TEST FAIL: {exc}', file=sys.stderr)
        return 1
    if summary['rounds']['valid'] != 3 or summary['goodput_mbps']['median'] <= 0:
        print('SELF-TEST FAIL: fixture summary', file=sys.stderr)
        return 1
    print('SELF-TEST PASS')
    return 0


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--guest')
    parser.add_argument('--host')
    parser.add_argument('--cpu')
    parser.add_argument('--output-csv')
    parser.add_argument('--output-summary')
    parser.add_argument('--self-test', action='store_true')
    args = parser.parse_args()
    if args.self_test:
        return run_self_test()
    if not args.guest:
        parser.error('--guest is required')
    try:
        summary = generate_summary(args.guest, args.host, True, args.cpu)
        if args.output_csv or args.output_summary:
            if not args.output_csv or not args.output_summary:
                raise ValueError('--output-csv and --output-summary are required together')
            write_outputs(summary, args.output_csv, args.output_summary)
        else:
            print(json.dumps(summary, sort_keys=True, indent=2))
    except (OSError, ValueError) as exc:
        print(f'Error: {exc}', file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
